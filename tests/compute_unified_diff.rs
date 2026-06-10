//! Frozen acceptance tests for T-06.01 — Compute unified diff.
//!
//! The criteria are about a Rust API (a pure function that turns an old and a new
//! text into a single-file unified diff string), so — exactly as the T-02.01
//! structure, T-02.03 budget, T-04.04 parse-unified-diff and T-05.04 apply tests
//! do — the honest way to observe them is to *compile and run a tiny probe program
//! against the real `mend` crate*. The probes pin the contract the criteria imply;
//! this test crate stays std-only and never references the not-yet-existing API
//! directly, so it always compiles. Redness comes from each probe failing to
//! *build* (the `compute_unified_diff` function does not exist yet), not from this
//! file failing to parse. Each probe is built into an isolated `CARGO_TARGET_DIR`
//! and run `--offline`, so it neither contends with the outer build lock nor
//! touches the network.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `mend::compute_unified_diff(old: &str, new: &str) -> String`
//!     — a pure, in-memory computation: it takes two texts and returns the
//!     single-file unified diff between them. It does no file I/O and emits no
//!     colour (both out of scope), so the result is just hunk(s): an `@@ … @@`
//!     header line followed by body lines, each prefixed by its marker — a space
//!     for an unchanged (context) line, `-` for a removed line, `+` for an added
//!     line — and each carrying its trailing newline. There are no `---`/`+++`
//!     file-header lines (there are no filenames), symmetric with the T-04.04
//!     parser, which consumes exactly this hunk-only format.
//!   - the diff is the conventional `diff -u` output with a FIXED context of 3
//!     lines: at most 3 unchanged lines are kept on each side of a changed region
//!     and every further unchanged line is elided, so the `@@ … @@` header spans
//!     only the context window, not the whole file.
//!   - identical inputs yield the empty string — no spurious hunk — while any
//!     difference yields a non-empty diff carrying an `@@` header.
//!
//!   C1: given old and new text, returns a unified diff with `@@` hunk headers.
//!   C2: unchanged regions are elided to a fixed context-line count (3).
//!   C3: identical inputs yield an empty diff (and differing inputs do not).

use std::path::PathBuf;
use std::process::Command;

/// The directory holding the `mend` crate's `Cargo.toml`.
fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The cargo executable cargo itself ran us with, falling back to `cargo`.
fn cargo_bin() -> String {
    option_env!("CARGO").unwrap_or("cargo").to_string()
}

/// A fresh, process-unique scratch directory under the system temp dir.
fn unique_temp_dir(tag: &str) -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "mend-compute-unified-diff-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Outcome of building and running a probe: whether it built-and-exited-0, plus
/// the combined stdout+stderr for diagnostics.
struct Probe {
    ran_ok: bool,
    output: String,
}

/// Build and run a probe crate whose `main.rs` is `main_src`, against the real
/// `mend` crate via a path dependency. A standalone `[workspace]` keeps the
/// probe from inheriting any ambient workspace; an isolated target dir keeps it
/// off the outer build lock; `--offline` keeps it off the network.
///
/// In the red state `mend` has no `compute_unified_diff` function, so the probe
/// fails to resolve and `ran_ok` is false. Once the `mend` library exposes the
/// function the probe references, it builds, runs, and prints its success
/// sentinel. A probe that panics (e.g. on a broken invariant) exits non-zero and
/// prints no sentinel, so a panic fails the corresponding test.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_compute_unified_diff_probe\"\n\
         version = \"0.0.0\"\n\
         edition = \"2021\"\n\
         \n\
         [dependencies]\n\
         mend = {{ path = '{}' }}\n\
         \n\
         [[bin]]\n\
         name = \"probe\"\n\
         path = \"src/main.rs\"\n\
         \n\
         [workspace]\n",
        manifest_dir().display()
    );
    std::fs::write(crate_dir.join("Cargo.toml"), manifest).expect("write probe Cargo.toml");
    std::fs::create_dir_all(crate_dir.join("src")).expect("create probe src dir");
    std::fs::write(crate_dir.join("src").join("main.rs"), main_src).expect("write probe main.rs");

    let out = Command::new(cargo_bin())
        .current_dir(&crate_dir)
        .env("CARGO_TARGET_DIR", &target_dir)
        .args(["run", "--quiet", "--offline", "--bin", "probe"])
        .output()
        .expect("failed to spawn `cargo run` for probe");

    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let ran_ok = out.status.success();

    let _ = std::fs::remove_dir_all(&crate_dir);
    let _ = std::fs::remove_dir_all(&target_dir);

    Probe { ran_ok, output }
}

/// C1 — given old and new text, `compute_unified_diff` returns a unified diff
/// carrying an `@@ … @@` hunk header. The probe changes a single line amid one
/// line of context on each side and asserts the full, conventional `diff -u`
/// hunk (no file-header lines): the `@@ -1,3 +1,3 @@` header, then the context
/// line (` line1`), the removed line (`-line2`), the added line (`+CHANGED`) and
/// the trailing context (` line3`), each newline-terminated. The exact-equality
/// assertion is the tight pin (a wrong header, a leaked/dropped marker, or a
/// swapped half all fail it); the explicit `@@` and non-empty assertions tie the
/// result directly to the criterion's wording and to the C3 boundary (a real
/// change is never the empty diff).
#[test]
fn c1_change_yields_hunk_header() {
    let src = r##"
fn main() {
    let old = "line1\nline2\nline3\n";
    let new = "line1\nCHANGED\nline3\n";
    let diff = mend::compute_unified_diff(old, new);

    // The diff carries an `@@ … @@` hunk header and opens with it.
    assert!(diff.contains("@@"), "C1: diff must contain an @@ hunk header, got {:?}", diff);
    assert!(
        diff.starts_with("@@ -1,3 +1,3 @@\n"),
        "C1: diff must open with the hunk header, got {:?}",
        diff
    );

    // The full single-file unified hunk for one changed line amid context.
    assert_eq!(
        diff,
        "@@ -1,3 +1,3 @@\n line1\n-line2\n+CHANGED\n line3\n",
        "C1: one single-line change yields one unified hunk with an @@ header"
    );

    // Boundary against C3: a real change is never the empty diff.
    assert!(!diff.is_empty(), "C1: a change must produce a non-empty diff");

    println!("C1_OK");
}
"##;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "compute_unified_diff must return a unified diff with an @@ hunk header; probe output:\n{}",
        p.output
    );
}

/// C2 — unchanged regions are elided to a fixed context-line count of 3. The
/// probe changes one line (the 5th of ten) and pins BOTH sides of the context
/// boundary: the diff keeps exactly the 3 unchanged lines nearest the change on
/// each side (` b ` ` c ` ` d ` before, ` f ` ` g ` ` h ` after) and elides every
/// unchanged line beyond that window (line 1 `a`, and the trailing `i`, `j` are
/// absent), so a mutant that keeps too few context lines drops a kept line and a
/// mutant that keeps too many (or never elides) leaks an elided line. The header
/// `@@ -2,7 +2,7 @@` proves the elision: it spans only the seven-line window
/// (lines 2..8), not the whole ten-line file. The exact-equality assertion pins
/// the entire elided hunk.
#[test]
fn c2_unchanged_regions_elided_to_fixed_context() {
    let src = r##"
fn main() {
    let old = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n";
    let new = "a\nb\nc\nd\nE\nf\ng\nh\ni\nj\n";
    let diff = mend::compute_unified_diff(old, new);

    // The whole elided hunk: 3 context lines each side of the single change,
    // header spanning only the window (lines 2..8), not the full file.
    assert_eq!(
        diff,
        "@@ -2,7 +2,7 @@\n b\n c\n d\n-e\n+E\n f\n g\n h\n",
        "C2: unchanged lines beyond the 3-line context window are elided"
    );

    // Exactly 3 lines of context are KEPT on each side of the change.
    assert!(
        diff.contains(" b\n") && diff.contains(" c\n") && diff.contains(" d\n"),
        "C2: 3 lines of leading context must be kept, got {:?}",
        diff
    );
    assert!(
        diff.contains(" f\n") && diff.contains(" g\n") && diff.contains(" h\n"),
        "C2: 3 lines of trailing context must be kept, got {:?}",
        diff
    );

    // Every unchanged line beyond that 3-line window is ELIDED (absent).
    assert!(
        !diff.contains(" a\n"),
        "C2: the unchanged line before the context window must be elided, got {:?}",
        diff
    );
    assert!(
        !diff.contains(" i\n") && !diff.contains(" j\n"),
        "C2: unchanged lines after the context window must be elided, got {:?}",
        diff
    );

    // The header reflects the elided window, not the whole 10-line file.
    assert!(
        diff.starts_with("@@ -2,7 +2,7 @@\n"),
        "C2: the @@ header must span only the context window, got {:?}",
        diff
    );

    println!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "compute_unified_diff must elide unchanged regions to a fixed 3-line context; probe output:\n{}",
        p.output
    );
}

/// C3 — identical inputs yield an empty diff, and differing inputs do not. The
/// probe pins both sides of the "is there a difference" boundary: passing the same
/// multi-line text as both old and new must return exactly the empty string (no
/// spurious hunk), while a one-line change between two otherwise-identical texts
/// must return a non-empty diff carrying an `@@` header. Asserting both sides kills
/// a mutant that always returns empty (it would fail the differing case) and one
/// that always emits a hunk (it would fail the identical case).
#[test]
fn c3_identical_inputs_yield_empty_diff() {
    let src = r##"
fn main() {
    // Identical inputs → empty diff (no spurious hunks).
    let same = "x\ny\nz\n";
    let empty = mend::compute_unified_diff(same, same);
    assert_eq!(empty, "", "C3: identical inputs must yield an empty diff, got {:?}", empty);

    // Boundary: the moment the inputs differ, the diff is non-empty and has an @@ header.
    let changed = mend::compute_unified_diff("x\ny\nz\n", "x\nY\nz\n");
    assert!(!changed.is_empty(), "C3: differing inputs must yield a non-empty diff");
    assert!(
        changed.contains("@@"),
        "C3: a non-empty diff must carry an @@ header, got {:?}",
        changed
    );

    println!("C3_OK");
}
"##;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "compute_unified_diff must return empty for identical inputs and non-empty for differing ones; probe output:\n{}",
        p.output
    );
}
