//! Frozen acceptance tests for T-05.01 — Match exact context.
//!
//! The criteria are about a Rust API (the precise half of the edit matcher: a
//! function that, given source text and an edit's search text, locates the
//! unique line range equal to the search text, or signals absence or
//! multiplicity). So — exactly as the T-04.03 search/replace and T-02.01
//! structure tests do — the honest way to observe the criteria is to *compile
//! and run a tiny probe program against the real `mend` crate*. The probes pin
//! the contract the criteria imply; this test crate stays std-only and never
//! references the not-yet-existing API directly, so it always compiles. Redness
//! comes from each probe failing to *build* (`match_exact` / `ExactMatch` do not
//! exist yet), not from this file failing to parse. Each probe is built into an
//! isolated `CARGO_TARGET_DIR` and run `--offline`, so it neither contends with
//! the outer build lock nor touches the network.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `mend::match_exact(source: &str, search: &str) -> mend::ExactMatch`
//!   - `ExactMatch::Unique(std::ops::Range<usize>)` carries the matched byte
//!     range, with the invariant `&source[range] == search`. Absence is
//!     `ExactMatch::NoMatch` and multiplicity is `ExactMatch::Ambiguous` —
//!     *signals*, returned in-band rather than errored or panicked (the function
//!     returns `ExactMatch`, not `Result`).
//!   - occurrence count partitions the outcome: `0 → NoMatch`, `1 → Unique`,
//!     `>= 2 → Ambiguous`. Both boundaries (0|1 and 1|2) are pinned so a flipped
//!     count comparison cannot survive.
//!
//!   C1: locates the unique line range in the source equal to an edit's search text.
//!   C2: search text absent from the source returns a no-match signal, never a panic.
//!   C3: search text occurring more than once returns an ambiguity signal.

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
    dir.push(format!("mend-match-exact-{tag}-{}-{nanos}", std::process::id()));
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
/// In the red state `mend` has no `match_exact` / `ExactMatch`, so the probe
/// fails to resolve and `ran_ok` is false. Once the `mend` library exposes the
/// API the probe references, it builds, runs, and prints its success sentinel. A
/// probe that panics (e.g. on a never-panic violation) exits non-zero and prints
/// no sentinel, so a panic fails the corresponding test.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_match_exact_probe\"\n\
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

/// C1 — `match_exact` locates the unique line range equal to the search text.
/// The probe feeds a three-line source and a search text equal to its middle
/// line, then asserts the result is `Unique` carrying the byte range of that
/// line. The exact start/end (`6..11`) pin the offset against an off-by-one
/// mutant, and `&source[range] == search` pins the byte-equality invariant —
/// the returned range must reproduce the search text verbatim, not some
/// neighbouring or shifted slice.
#[test]
fn c1_unique_match_returns_byte_equal_range() {
    let src = r##"
fn main() {
    let source = "alpha\nbeta\ngamma\n";
    let search = "beta\n";
    match mend::match_exact(source, search) {
        mend::ExactMatch::Unique(r) => {
            assert_eq!(r.start, 6, "C1: the unique match must start at the search text's byte offset");
            assert_eq!(r.end, 11, "C1: the unique match must end at the search text's byte offset");
            assert_eq!(
                &source[r.start..r.end],
                search,
                "C1: the returned range must be byte-equal to the search text"
            );
        }
        other => panic!("C1: a search text present exactly once must be Unique, got {:?}", other),
    }
    println!("C1_OK");
}
"##;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "match_exact must locate the unique line range byte-equal to the search text; probe output:\n{}",
        p.output
    );
}

/// C2 — search text absent from the source returns a no-match signal, never a
/// panic. The probe asks for a line that does not occur and asserts the result
/// is exactly `NoMatch` (not `Unique`, not `Ambiguous`). It then pins the 0|1
/// boundary with a counterpart that *is* present and must therefore be `Unique`,
/// not `NoMatch` — so a mutant that always reports no-match cannot survive.
/// Because the only success path prints the sentinel and any panic aborts the
/// probe before it, this also pins the never-panic guarantee.
#[test]
fn c2_absent_search_returns_no_match() {
    let src = r##"
fn main() {
    let source = "alpha\nbeta\ngamma\n";

    // Absent (zero occurrences) → NoMatch, signalled in-band, never panicked.
    match mend::match_exact(source, "delta\n") {
        mend::ExactMatch::NoMatch => {}
        other => panic!("C2: a search text absent from the source must be NoMatch, got {:?}", other),
    }

    // Boundary counterpart: one occurrence must be Unique, not NoMatch.
    match mend::match_exact(source, "beta\n") {
        mend::ExactMatch::Unique(_) => {}
        other => panic!("C2: a search text present once must be Unique, not NoMatch, got {:?}", other),
    }

    println!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "match_exact must signal NoMatch for absent search text and never panic; probe output:\n{}",
        p.output
    );
}

/// C3 — search text occurring more than once returns an ambiguity signal. The
/// probe feeds a source in which the search line appears twice and asserts the
/// result is exactly `Ambiguous`. It then pins the 1|2 boundary with a
/// counterpart in which the same line appears exactly once and must therefore be
/// `Unique`, not `Ambiguous` — so a mutant that flips the multiplicity
/// comparison (treating one occurrence as ambiguous, or two as unique) cannot
/// survive.
#[test]
fn c3_repeated_search_returns_ambiguous() {
    let src = r##"
fn main() {
    // Two occurrences → Ambiguous.
    let dup = "dup\nmid\ndup\n";
    match mend::match_exact(dup, "dup\n") {
        mend::ExactMatch::Ambiguous => {}
        other => panic!("C3: a search text occurring more than once must be Ambiguous, got {:?}", other),
    }

    // Boundary counterpart: exactly one occurrence must be Unique, not Ambiguous.
    let single = "dup\nmid\n";
    match mend::match_exact(single, "dup\n") {
        mend::ExactMatch::Unique(_) => {}
        other => panic!("C3: a search text occurring exactly once must be Unique, not Ambiguous, got {:?}", other),
    }

    println!("C3_OK");
}
"##;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "match_exact must signal Ambiguous when the search text occurs more than once; probe output:\n{}",
        p.output
    );
}
