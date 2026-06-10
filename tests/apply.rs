//! Frozen acceptance tests for T-05.04 — Apply edits.
//!
//! The criteria describe a Rust API: the in-memory engine that turns a validated,
//! ordered `Vec<ParsedEdit>` into the rewritten file text. So — exactly as the
//! T-05.03 conflict, T-05.01 `match_exact` and T-04.03 search/replace tests do —
//! the honest way to observe the criteria is to *compile and run a tiny probe
//! program against the real `mend` crate*. The probes pin the contract the
//! criteria imply; this test crate stays std-only and never references the
//! not-yet-existing API directly, so it always compiles. Redness comes from each
//! probe failing to *build* (`apply_edits` does not exist yet), not from this
//! file failing to parse. Each probe is built into an isolated
//! `CARGO_TARGET_DIR` and run `--offline`, so it neither contends with the outer
//! build lock nor touches the network.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `mend::apply_edits(source: &str, edits: &[ParsedEdit]) -> mend::Result<String>`
//!     — a pure, in-memory transformation. On success it returns the rewritten
//!     text; it never writes to disk and never renders a diff (both out of scope).
//!   - C1: applying an ordered set of search/replace edits returns the source with
//!     *every* edit spliced in, in order — pinned by an exact-string assertion on
//!     the full result, so a splice that drops, mis-bounds, or skips an edit fails.
//!   - C2: a single whole-file `ParsedEdit` makes its `body` the *entire* result,
//!     discarding the original source — pinned by asserting the result equals the
//!     body exactly and retains nothing of the original.
//!   - C3: a conflicting edit set (overlap) and a no-match edit each abort with
//!     `MendError::Patch`; application is all-or-nothing, so an edit list whose
//!     first edit would apply but whose second cannot must still return `Patch`
//!     (never a partially-applied `Ok`), leaving the source text unchanged.
//!
//!   C1: applies an ordered `Vec<ParsedEdit>` to source text and returns the new text.
//!   C2: a whole-file ParsedEdit replaces the entire source.
//!   C3: a conflicting or no-match edit yields `MendError::Patch` and leaves the source unchanged.

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
    dir.push(format!("mend-apply-{tag}-{}-{nanos}", std::process::id()));
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
/// In the red state `mend` has no `apply_edits`, so the probe fails to resolve
/// and `ran_ok` is false. Once the `mend` library exposes the API the probe
/// references, it builds, runs, and prints its success sentinel. A probe that
/// panics (e.g. on a criterion violation) exits non-zero and prints no sentinel,
/// so a violation fails the corresponding test.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_apply_probe\"\n\
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

/// C1 — applies an ordered `Vec<ParsedEdit>` to source text and returns the new
/// text.
///
/// The probe feeds a three-line source and two ordered search/replace edits whose
/// matched ranges are disjoint (the first line and the last line). It asserts the
/// returned text equals the source with *both* substitutions spliced in — an
/// exact full-string assertion, so any splice that drops an edit, applies only
/// one, mis-bounds the cut, or corrupts the untouched middle line fails.
///
/// It then pins a single-edit case: one search/replace edit returns exactly the
/// source with that one occurrence replaced and nothing else touched. Together
/// the two cases prove the engine applies *all* of an ordered edit list and
/// returns the rewritten text, never the original.
#[test]
fn c1_applies_ordered_edits_and_returns_new_text() {
    let src = r##"
fn main() {
    // Two disjoint, ordered edits over a three-line source: line 1 and line 3.
    let source = "one\ntwo\nthree\n";
    let edits = vec![
        mend::ParsedEdit::SearchReplace { search: "one\n".to_string(), replace: "ONE\n".to_string() },
        mend::ParsedEdit::SearchReplace { search: "three\n".to_string(), replace: "THREE\n".to_string() },
    ];
    let result = mend::apply_edits(source, &edits).expect("clean edit set must apply");
    assert_eq!(
        result, "ONE\ntwo\nTHREE\n",
        "C1: both ordered edits must be spliced into the returned text; got {:?}",
        result
    );
    // The engine returns the *new* text, not the untouched source.
    assert_ne!(
        result, source,
        "C1: applying edits must change the text, not echo the source; got {:?}",
        result
    );

    // Single-edit case: exactly one occurrence replaced, the rest untouched.
    let one = vec![
        mend::ParsedEdit::SearchReplace { search: "two\n".to_string(), replace: "TWO\n".to_string() },
    ];
    let one_result = mend::apply_edits(source, &one).expect("single clean edit must apply");
    assert_eq!(
        one_result, "one\nTWO\nthree\n",
        "C1: a single edit must replace only its match and leave the rest intact; got {:?}",
        one_result
    );

    println!("C1_OK");
}
"##;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "apply_edits must apply an ordered edit list and return the rewritten text; probe output:\n{}",
        p.output
    );
}

/// C2 — a whole-file `ParsedEdit` replaces the entire source.
///
/// The probe feeds a multi-line source and a single `WholeFile` edit whose `body`
/// shares no text with the source. It asserts the result equals the body exactly
/// — proving the whole-file edit *replaces* rather than splices — and that none
/// of the original source survives in the result. A second case uses an empty
/// body to pin that a whole-file edit can empty the file entirely (the result is
/// the body verbatim, even when that is `""`), so an implementation that falls
/// back to the source on an empty body fails.
#[test]
fn c2_whole_file_replaces_entire_source() {
    let src = r##"
fn main() {
    let source = "old line 1\nold line 2\nold line 3\n";
    let body = "brand new contents\n";
    let edits = vec![
        mend::ParsedEdit::WholeFile { body: body.to_string() },
    ];
    let result = mend::apply_edits(source, &edits).expect("whole-file edit must apply");
    assert_eq!(
        result, body,
        "C2: a whole-file edit must make its body the entire result; got {:?}",
        result
    );
    assert!(
        !result.contains("old line"),
        "C2: a whole-file edit must replace the source entirely, leaving none of it; got {:?}",
        result
    );

    // A whole-file edit with an empty body empties the file: the body is taken
    // verbatim, not ignored in favour of the source.
    let empty = vec![
        mend::ParsedEdit::WholeFile { body: String::new() },
    ];
    let empty_result = mend::apply_edits(source, &empty).expect("empty whole-file edit must apply");
    assert_eq!(
        empty_result, "",
        "C2: a whole-file edit with an empty body must yield empty text, not the source; got {:?}",
        empty_result
    );

    println!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "apply_edits must let a whole-file edit replace the entire source; probe output:\n{}",
        p.output
    );
}

/// C3 — a conflicting or no-match edit yields `MendError::Patch` and leaves the
/// source unchanged.
///
/// The probe exercises three failure shapes, each of which must return
/// `Err(MendError::Patch(_))`:
///   - an *overlapping* edit set (two edits whose matched ranges intersect) — a
///     conflict;
///   - a single search/replace edit whose search text is *absent* — a no-match;
///   - a transactional case: a two-edit list whose first edit would apply
///     cleanly but whose second edit has no match. Because application is
///     all-or-nothing, this must still abort with `Patch` — never return a
///     partially-applied `Ok` — and the original `source` binding is unchanged.
///
/// The probe also confirms the *positive* counterpart inline (a clean set of the
/// same shape returns `Ok`), so a mutant that always returns `Patch` cannot
/// survive: only the conflicting/no-match inputs error, the clean one does not.
#[test]
fn c3_conflict_or_no_match_aborts_with_patch_unchanged() {
    let src = r##"
fn is_patch(r: &mend::Result<String>) -> bool {
    matches!(r, Err(mend::MendError::Patch(_)))
}

fn main() {
    // Overlap conflict: both searches span the shared middle line, so their
    // matched ranges intersect and the set cannot apply.
    let overlap_source = "AAA\nBBB\nCCC\n";
    let overlapping = vec![
        mend::ParsedEdit::SearchReplace { search: "AAA\nBBB\n".to_string(), replace: "X\n".to_string() },
        mend::ParsedEdit::SearchReplace { search: "BBB\nCCC\n".to_string(), replace: "Y\n".to_string() },
    ];
    let overlap_result = mend::apply_edits(overlap_source, &overlapping);
    assert!(
        is_patch(&overlap_result),
        "C3: a conflicting (overlapping) edit set must yield MendError::Patch; got {:?}",
        overlap_result
    );

    // No-match: the search text is absent from the source.
    let nomatch_source = "hello\nworld\n";
    let nomatch = vec![
        mend::ParsedEdit::SearchReplace { search: "absent\n".to_string(), replace: "Z\n".to_string() },
    ];
    let nomatch_result = mend::apply_edits(nomatch_source, &nomatch);
    assert!(
        is_patch(&nomatch_result),
        "C3: an edit whose search has no match must yield MendError::Patch; got {:?}",
        nomatch_result
    );

    // Transactional / all-or-nothing: edit 0 ("hello\n") matches, but edit 1
    // ("missing\n") does not. The whole application must abort with Patch — never
    // a partially-applied Ok — and leave the source text unchanged.
    let txn_source = "hello\nworld\n";
    let partial = vec![
        mend::ParsedEdit::SearchReplace { search: "hello\n".to_string(), replace: "HELLO\n".to_string() },
        mend::ParsedEdit::SearchReplace { search: "missing\n".to_string(), replace: "Q\n".to_string() },
    ];
    let partial_result = mend::apply_edits(txn_source, &partial);
    assert!(
        is_patch(&partial_result),
        "C3: when any edit cannot apply, the whole set must abort with Patch, not partially succeed; got {:?}",
        partial_result
    );
    assert_eq!(
        txn_source, "hello\nworld\n",
        "C3: a failed application must leave the source text unchanged"
    );

    // Positive counterpart: the same two-edit shape, but with a second edit that
    // *does* match, applies cleanly and returns Ok — so erroring is driven by the
    // failure, not unconditional.
    let clean = vec![
        mend::ParsedEdit::SearchReplace { search: "hello\n".to_string(), replace: "HELLO\n".to_string() },
        mend::ParsedEdit::SearchReplace { search: "world\n".to_string(), replace: "WORLD\n".to_string() },
    ];
    let clean_result = mend::apply_edits(txn_source, &clean);
    assert!(
        !is_patch(&clean_result),
        "C3: a clean edit set of the same shape must NOT error; got {:?}",
        clean_result
    );
    assert_eq!(
        clean_result.expect("clean set applies"),
        "HELLO\nWORLD\n",
        "C3: the clean counterpart must apply both edits"
    );

    println!("C3_OK");
}
"##;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "apply_edits must abort with MendError::Patch on conflict or no-match, leaving the source unchanged; probe output:\n{}",
        p.output
    );
}
