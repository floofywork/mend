//! Frozen acceptance tests for T-06.04 — Dispatch output mode.
//!
//! The criteria describe the single Rust seam that decides what reaches the user
//! and the disk: given the applied result, the original text, the edit list, and
//! the chosen output mode, it either prints a coloured diff (dry-run, the
//! default), writes the new text to the target path (`--apply`), or prints the
//! JSON result (`--json`). Exactly one of three modes runs, and only the apply
//! mode ever touches disk. As with the other phase-4/6 frozen suites, the honest
//! way to observe the contract is to *compile and run a tiny probe program against
//! the real `mend` crate*: this test crate stays std-only and never references the
//! not-yet-existing API directly, so it always compiles. Redness comes from each
//! probe failing to *build* (the `dispatch_output` function and `OutputMode` enum
//! do not exist yet), not from this file failing to parse. Each probe is built
//! into an isolated `CARGO_TARGET_DIR` and run `--offline`, so it neither contends
//! with the outer build lock nor touches the network.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `mend::OutputMode` is an enum with exactly the three modes `DryRun`,
//!     `Apply`, and `Json` — "exactly one of three modes".
//!   - `mend::dispatch_output(path: &Path, original: &str,
//!        applied: mend::Result<String>, edits: Vec<mend::ParsedEdit>,
//!        mode: mend::OutputMode, color: bool) -> mend::Result<String>`,
//!     where `applied` is the outcome of the patch engine — the proposed text on
//!     success, or its `MendError::Patch` on failure.
//!   - DryRun returns the coloured rendered diff between `original` and the
//!     proposed text (`render_diff(compute_unified_diff(..), color)`) and writes
//!     nothing.
//!   - Apply writes the proposed text to `path` and reports success; an `applied`
//!     that is already an `Err` is propagated and nothing is written.
//!   - Json returns the emitted `JsonResult` JSON and renders no diff; it does not
//!     touch disk, so its `applied` flag is false.
//!
//!   C1: in dry-run (default) mode the coloured diff prints and no file is modified.
//!   C2: in `--apply` mode the new text is written to the target path on disk.
//!   C3: in `--json` mode the JSON result prints and no diff is rendered.
//!   C4: a patch failure under `--apply` writes nothing and returns `MendError::Patch`.

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
        "mend-dispatch-output-{tag}-{}-{nanos}",
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

/// Shared probe preamble: a target-file helper so each probe can create a real
/// on-disk file, drive `dispatch_output` against it, and read the bytes back to
/// assert what did (or did not) reach disk. Prepended to every probe's `main`.
const PROBE_PRELUDE: &str = r##"
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Create a fresh temp file containing `content`, returning its path.
fn temp_file(content: &str) -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut p = std::env::temp_dir();
    p.push(format!("mend-target-{}-{}-{}.txt", std::process::id(), nanos, n));
    std::fs::write(&p, content).expect("write target file");
    p
}

/// Read the current on-disk contents of `p`.
fn read_disk(p: &PathBuf) -> String {
    std::fs::read_to_string(p).expect("read target file")
}
"##;

/// Build and run a probe crate whose `main.rs` is `PROBE_PRELUDE` followed by
/// `main_src`, against the real `mend` crate via a path dependency. A standalone
/// `[workspace]` keeps the probe from inheriting any ambient workspace; an
/// isolated target dir keeps it off the outer build lock; `--offline` keeps it off
/// the network.
///
/// In the red state `mend` has no `dispatch_output` function and no `OutputMode`
/// enum, so the probe fails to resolve and `ran_ok` is false. Once the `mend`
/// library exposes the API the probe references, it builds, runs, and prints its
/// success sentinel. A probe that panics (e.g. on a broken invariant) exits
/// non-zero and prints no sentinel, so a panic fails the corresponding test.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_dispatch_output_probe\"\n\
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

    let full_src = format!("{PROBE_PRELUDE}\n{main_src}");
    std::fs::write(crate_dir.join("src").join("main.rs"), full_src).expect("write probe main.rs");

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

/// C1 — dry-run (the default) prints the coloured diff and modifies no file.
///
/// The probe drives `dispatch_output` in `DryRun` mode against a real on-disk
/// file and pins three things: the returned text is exactly the coloured render
/// of the unified diff between the original and proposed text
/// (`render_diff(compute_unified_diff(..), true)`), which over a real change
/// carries ANSI escapes; the file on disk is byte-for-byte unchanged; and the
/// `color` flag is forwarded, not hard-coded — an uncoloured dry run matches the
/// uncoloured render and carries no escapes. The "file unchanged" assertion is the
/// C1 invariant that a dry run never touches disk.
#[test]
fn c1_dry_run_prints_diff_and_leaves_file_unmodified() {
    let src = r##"
fn main() {
    let original = "line one\nline two\nline three\n";
    let proposed = "line one\nCHANGED\nline three\n";
    let path = temp_file(original);
    let edits = vec![mend::ParsedEdit::SearchReplace {
        search: "line two\n".to_string(),
        replace: "CHANGED\n".to_string(),
    }];

    let out = mend::dispatch_output(
        &path,
        original,
        Ok(proposed.to_string()),
        edits,
        mend::OutputMode::DryRun,
        true,
    )
    .expect("C1: dry-run mode must succeed");

    let expected = mend::render_diff(&mend::compute_unified_diff(original, proposed), true);
    assert_eq!(
        out, expected,
        "C1: dry-run must return the coloured rendered diff of original -> proposed"
    );
    assert!(
        out.contains('\u{1b}'),
        "C1: a coloured diff over a real change must carry ANSI escapes"
    );

    // The defining C1 invariant: a dry run modifies no file on disk.
    assert_eq!(
        read_disk(&path),
        original,
        "C1: dry-run must not modify the target file on disk"
    );

    // Colour is forwarded to the renderer, not hard-coded: an uncoloured dry run
    // matches the uncoloured render exactly and carries no escapes.
    let edits2 = vec![mend::ParsedEdit::SearchReplace {
        search: "line two\n".to_string(),
        replace: "CHANGED\n".to_string(),
    }];
    let plain = mend::dispatch_output(
        &path,
        original,
        Ok(proposed.to_string()),
        edits2,
        mend::OutputMode::DryRun,
        false,
    )
    .expect("C1: uncoloured dry-run must succeed");
    assert_eq!(
        plain,
        mend::render_diff(&mend::compute_unified_diff(original, proposed), false),
        "C1: dry-run must forward the colour flag to the renderer"
    );
    assert!(
        !plain.contains('\u{1b}'),
        "C1: an uncoloured dry run must carry no ANSI escapes"
    );
    assert_eq!(
        read_disk(&path),
        original,
        "C1: dry-run must not modify the target file on disk"
    );

    let _ = std::fs::remove_file(&path);
    println!("C1_OK");
}
"##;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "dispatch_output dry-run must print the coloured diff and leave the file unmodified; probe output:\n{}",
        p.output
    );
}

/// C2 — apply mode writes the new text to the target path on disk.
///
/// The probe seeds a target file with the original text, drives `dispatch_output`
/// in `Apply` mode with the proposed text as the applied result, and asserts the
/// call succeeds and the file on disk now holds exactly the proposed text. This
/// pins the one mode permitted to touch disk and that what lands there is the
/// proposed text verbatim.
#[test]
fn c2_apply_writes_proposed_to_disk() {
    let src = r##"
fn main() {
    let original = "line one\nline two\nline three\n";
    let proposed = "line one\nCHANGED\nline three\n";
    let path = temp_file(original);
    let edits = vec![mend::ParsedEdit::SearchReplace {
        search: "line two\n".to_string(),
        replace: "CHANGED\n".to_string(),
    }];

    let result = mend::dispatch_output(
        &path,
        original,
        Ok(proposed.to_string()),
        edits,
        mend::OutputMode::Apply,
        false,
    );
    assert!(
        result.is_ok(),
        "C2: apply mode must succeed when the applied result is Ok; got {:?}",
        result.err()
    );

    assert_eq!(
        read_disk(&path),
        proposed,
        "C2: apply mode must write the proposed text to the target path on disk"
    );

    let _ = std::fs::remove_file(&path);
    println!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "dispatch_output apply mode must write the proposed text to disk; probe output:\n{}",
        p.output
    );
}

/// C3 — json mode prints the JSON result and renders no diff.
///
/// The probe drives `dispatch_output` in `Json` mode with `color = true` (which
/// would otherwise colour a diff) and pins: the returned text equals the emitted
/// `JsonResult` JSON for this path/original/proposed/edits; the output carries no
/// ANSI escapes and round-trips through `parse_json` (so it is JSON, not a
/// rendered diff); the round-tripped `applied` flag is false, since json mode does
/// not write; and the target file on disk is unchanged (only `--apply` touches
/// disk).
#[test]
fn c3_json_prints_result_and_renders_no_diff() {
    let src = r##"
fn main() {
    let original = "line one\nline two\nline three\n";
    let proposed = "line one\nCHANGED\nline three\n";
    let path = temp_file(original);
    let edits = vec![mend::ParsedEdit::SearchReplace {
        search: "line two\n".to_string(),
        replace: "CHANGED\n".to_string(),
    }];

    // colour=true would colour a diff — but json mode must render no diff at all.
    let out = mend::dispatch_output(
        &path,
        original,
        Ok(proposed.to_string()),
        edits,
        mend::OutputMode::Json,
        true,
    )
    .expect("C3: json mode must succeed");

    let expected = mend::emit_json(&mend::JsonResult {
        path: path.display().to_string(),
        original: original.to_string(),
        proposed: proposed.to_string(),
        edits: vec![mend::ParsedEdit::SearchReplace {
            search: "line two\n".to_string(),
            replace: "CHANGED\n".to_string(),
        }],
        applied: false,
    });
    assert_eq!(
        out, expected,
        "C3: json mode must return the emitted JsonResult JSON"
    );

    // No diff is rendered: even with colour=true, the output carries no ANSI
    // escapes and parses back as a JsonResult.
    assert!(
        !out.contains('\u{1b}'),
        "C3: json mode must render no (coloured) diff"
    );
    let parsed = mend::parse_json(&out).expect("C3: json output must round-trip via parse_json");
    assert!(
        !parsed.applied,
        "C3: json mode does not write to disk, so its applied flag is false"
    );
    assert_eq!(
        parsed.proposed, proposed,
        "C3: the JSON must carry the proposed text"
    );

    // json mode does not touch disk.
    assert_eq!(
        read_disk(&path),
        original,
        "C3: json mode must not modify the target file on disk"
    );

    let _ = std::fs::remove_file(&path);
    println!("C3_OK");
}
"##;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "dispatch_output json mode must print the JSON result and render no diff; probe output:\n{}",
        p.output
    );
}

/// C4 — a patch failure under `--apply` writes nothing and returns
/// `MendError::Patch`.
///
/// The probe seeds a target file, then drives `dispatch_output` in `Apply` mode
/// with an `applied` result that is already an `Err(MendError::Patch(..))` — the
/// non-destructive outcome the patch engine produces when an edit cannot apply.
/// It pins both halves of the boundary: the call returns exactly the `Patch`
/// variant (not `Ok`, not another error), and the file on disk is byte-for-byte
/// the original — the failure left it untouched.
#[test]
fn c4_apply_patch_failure_writes_nothing_and_returns_patch() {
    let src = r##"
fn main() {
    let original = "line one\nline two\nline three\n";
    let path = temp_file(original);
    let edits = vec![mend::ParsedEdit::SearchReplace {
        search: "absent\n".to_string(),
        replace: "x\n".to_string(),
    }];

    // The patch engine already failed: the applied result is a Patch error.
    let applied: mend::Result<String> =
        Err(mend::MendError::Patch("search text has no unique match".to_string()));
    let result = mend::dispatch_output(
        &path,
        original,
        applied,
        edits,
        mend::OutputMode::Apply,
        false,
    );

    match result {
        Err(mend::MendError::Patch(_)) => {}
        other => panic!(
            "C4: a patch failure under --apply must return MendError::Patch, got {:?}",
            other
        ),
    }

    // Non-destructive: nothing was written, the file still holds the original.
    assert_eq!(
        read_disk(&path),
        original,
        "C4: a patch failure under --apply must write nothing to disk"
    );

    let _ = std::fs::remove_file(&path);
    println!("C4_OK");
}
"##;
    let p = run_probe("c4", src);
    assert!(
        p.ran_ok && p.output.contains("C4_OK"),
        "dispatch_output must propagate MendError::Patch and write nothing on an apply-time patch failure; probe output:\n{}",
        p.output
    );
}
