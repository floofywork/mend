//! Frozen acceptance tests for T-05.03 — Detect conflicts.
//!
//! The criteria describe a Rust API: the validation pass that runs before any
//! application. Given source text and an ordered `Vec<ParsedEdit>`, it returns a
//! list of detected conflicts — an *empty* list being the success signal that
//! the edit set applies cleanly. So — exactly as the T-05.01 `match_exact` and
//! T-04.03 search/replace tests do — the honest way to observe the criteria is
//! to *compile and run a tiny probe program against the real `mend` crate*. The
//! probes pin the contract the criteria imply; this test crate stays std-only
//! and never references the not-yet-existing API directly, so it always
//! compiles. Redness comes from each probe failing to *build*
//! (`detect_conflicts` / `Conflict` do not exist yet), not from this file
//! failing to parse. Each probe is built into an isolated `CARGO_TARGET_DIR`
//! and run `--offline`, so it neither contends with the outer build lock nor
//! touches the network.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `mend::detect_conflicts(source: &str, edits: &[ParsedEdit]) -> Vec<mend::Conflict>`
//!     — a pure validation pass that never mutates the source and signals
//!     outcomes *in-band* (an empty `Vec` is "clean", not an error).
//!   - `mend::Conflict` is an enum that derives `Debug`, with:
//!       - `Overlap { first: usize, second: usize }` — two edits (by their
//!         position in the slice, `first < second`) whose matched source ranges
//!         overlap.
//!       - `StaleMatch { index: usize }` — an edit (by its position) whose
//!         search text no longer matches cleanly once the prior edits apply.
//!   - the overlap test pins BOTH sides of the boundary: ranges that intersect
//!     are flagged, ranges that merely *touch* (`end == start`) are not — so a
//!     `<`→`<=` mutant in the overlap predicate cannot survive, and a disjoint
//!     clean set kills an `&&`→`||` mutant.
//!
//!   C1: flags two edits whose matched source ranges overlap as a conflict.
//!   C2: flags an edit whose search text no longer matches after a prior edit applies as a conflict.
//!   C3: a conflict-free edit set returns an empty conflict list.

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
    dir.push(format!("mend-conflict-{tag}-{}-{nanos}", std::process::id()));
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
/// In the red state `mend` has no `detect_conflicts` / `Conflict`, so the probe
/// fails to resolve and `ran_ok` is false. Once the `mend` library exposes the
/// API the probe references, it builds, runs, and prints its success sentinel. A
/// probe that panics (e.g. on a criterion violation) exits non-zero and prints
/// no sentinel, so a violation fails the corresponding test.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_conflict_probe\"\n\
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

/// C1 — two edits whose matched source ranges overlap are flagged as a conflict.
///
/// The probe feeds a three-line source and two search/replace edits whose search
/// texts each span two adjacent lines, so their matched byte ranges intersect on
/// the shared middle line. It asserts the result contains an `Overlap` conflict
/// naming edit 0 and edit 1 (pinning the indices and their order, `first < second`),
/// and that the list is therefore non-empty.
///
/// It then pins the *other* side of the overlap boundary: two edits whose ranges
/// merely *touch* (the first ends exactly where the second begins) must NOT be
/// reported as overlapping. This kills a `<`→`<=` mutant in the overlap
/// predicate — under `<=` a touching pair would be wrongly flagged.
#[test]
fn c1_overlapping_ranges_are_flagged() {
    let src = r##"
fn has_overlap(cs: &[mend::Conflict], a: usize, b: usize) -> bool {
    cs.iter().any(|c| matches!(c, mend::Conflict::Overlap { first, second } if *first == a && *second == b))
}

fn any_overlap(cs: &[mend::Conflict]) -> bool {
    cs.iter().any(|c| matches!(c, mend::Conflict::Overlap { .. }))
}

fn main() {
    // "AAA\n" = 4 bytes; edit 0 spans bytes 0..8, edit 1 spans bytes 4..12,
    // so they overlap on the shared middle line "BBB\n".
    let source = "AAA\nBBB\nCCC\n";
    let overlapping = vec![
        mend::ParsedEdit::SearchReplace { search: "AAA\nBBB\n".to_string(), replace: "X\n".to_string() },
        mend::ParsedEdit::SearchReplace { search: "BBB\nCCC\n".to_string(), replace: "Y\n".to_string() },
    ];
    let conflicts = mend::detect_conflicts(source, &overlapping);
    assert!(
        has_overlap(&conflicts, 0, 1),
        "C1: edits whose matched ranges overlap must be flagged as an Overlap conflict between edit 0 and edit 1; got {:?}",
        conflicts
    );
    assert!(
        !conflicts.is_empty(),
        "C1: overlapping edits must produce a non-empty conflict list; got {:?}",
        conflicts
    );

    // Boundary: ranges that merely touch (edit 0 ends at byte 4, edit 1 starts
    // at byte 4) share no byte and must NOT be reported as overlapping.
    let touching_source = "AAA\nBBB\n";
    let touching = vec![
        mend::ParsedEdit::SearchReplace { search: "AAA\n".to_string(), replace: "X\n".to_string() },
        mend::ParsedEdit::SearchReplace { search: "BBB\n".to_string(), replace: "Y\n".to_string() },
    ];
    let touching_conflicts = mend::detect_conflicts(touching_source, &touching);
    assert!(
        !any_overlap(&touching_conflicts),
        "C1: adjacent (touching) ranges must NOT be flagged as overlapping; got {:?}",
        touching_conflicts
    );

    println!("C1_OK");
}
"##;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "detect_conflicts must flag overlapping matched ranges and leave touching ranges clean; probe output:\n{}",
        p.output
    );
}

/// C2 — an edit whose search no longer matches after a prior edit applies is
/// flagged as a conflict.
///
/// The probe builds a source where edit 0 and edit 1 match *disjoint* original
/// ranges (so there is no overlap), but edit 0's replacement rewrites one line
/// to duplicate the line edit 1 targets. After edit 0 applies, edit 1's search
/// text is no longer a clean unique match, so the set cannot apply cleanly. The
/// probe asserts a `StaleMatch` conflict at index 1, and asserts the list
/// contains no `Overlap` — proving this is detected by the post-application
/// match check, not by range overlap.
///
/// It then pins the other side: the same two-edit shape, but with edit 0's
/// replacement chosen so it leaves edit 1's search intact, must yield no
/// conflict at all. The only difference between the two cases is edit 0's
/// replacement text, isolating the stale-match check — so a mutant that always
/// (or never) reports a stale match cannot survive.
#[test]
fn c2_stale_match_after_prior_edit_is_flagged() {
    let src = r##"
fn has_stale(cs: &[mend::Conflict], i: usize) -> bool {
    cs.iter().any(|c| matches!(c, mend::Conflict::StaleMatch { index } if *index == i))
}

fn any_overlap(cs: &[mend::Conflict]) -> bool {
    cs.iter().any(|c| matches!(c, mend::Conflict::Overlap { .. }))
}

fn main() {
    // edit 0 ("bar\n" -> "foo\n") and edit 1 ("foo\n") match disjoint original
    // ranges (bytes 8..12 and 0..4). But once edit 0 applies, the source holds
    // "foo\n" twice, so edit 1's search no longer matches cleanly.
    let source = "foo\nMID\nbar\n";
    let stale = vec![
        mend::ParsedEdit::SearchReplace { search: "bar\n".to_string(), replace: "foo\n".to_string() },
        mend::ParsedEdit::SearchReplace { search: "foo\n".to_string(), replace: "qux\n".to_string() },
    ];
    let conflicts = mend::detect_conflicts(source, &stale);
    assert!(
        has_stale(&conflicts, 1),
        "C2: an edit whose search no longer matches after a prior edit applies must be a StaleMatch conflict at index 1; got {:?}",
        conflicts
    );
    assert!(
        !any_overlap(&conflicts),
        "C2: the edits' original ranges are disjoint, so this must be a stale-match, not an overlap; got {:?}",
        conflicts
    );

    // Boundary counterpart: edit 0's replacement ("BAZ\n") does not collide with
    // edit 1's search, so edit 1 still matches cleanly and there is no conflict.
    let safe = vec![
        mend::ParsedEdit::SearchReplace { search: "bar\n".to_string(), replace: "BAZ\n".to_string() },
        mend::ParsedEdit::SearchReplace { search: "foo\n".to_string(), replace: "qux\n".to_string() },
    ];
    let safe_conflicts = mend::detect_conflicts(source, &safe);
    assert!(
        safe_conflicts.is_empty(),
        "C2: when a prior edit leaves a later edit's search intact, there is no stale-match conflict; got {:?}",
        safe_conflicts
    );

    println!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "detect_conflicts must flag an edit whose search goes stale after a prior edit applies, and stay clean otherwise; probe output:\n{}",
        p.output
    );
}

/// C3 — a conflict-free edit set returns an empty conflict list.
///
/// The probe feeds two edits that match *disjoint, non-touching* original ranges
/// (the first and last of three lines) and whose searches each still match
/// uniquely after the other applies. With neither an overlap nor a stale match,
/// the validation pass must return an empty `Vec` — the success signal that the
/// set applies cleanly. The disjoint ranges also kill an `&&`→`||` mutant in the
/// overlap predicate: under `||` one of the two disjoint comparisons is still
/// true, which would wrongly flag an overlap and make the list non-empty.
#[test]
fn c3_clean_set_returns_empty_list() {
    let src = r##"
fn main() {
    // edit 0 -> bytes 0..6 ("alpha\n"), edit 1 -> bytes 11..17 ("gamma\n"):
    // disjoint with a gap between them, and each still unique after the other.
    let source = "alpha\nbeta\ngamma\n";
    let clean = vec![
        mend::ParsedEdit::SearchReplace { search: "alpha\n".to_string(), replace: "ALPHA\n".to_string() },
        mend::ParsedEdit::SearchReplace { search: "gamma\n".to_string(), replace: "GAMMA\n".to_string() },
    ];
    let conflicts = mend::detect_conflicts(source, &clean);
    assert!(
        conflicts.is_empty(),
        "C3: a conflict-free edit set must return an empty conflict list; got {:?}",
        conflicts
    );

    println!("C3_OK");
}
"##;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "detect_conflicts must return an empty conflict list for a conflict-free edit set; probe output:\n{}",
        p.output
    );
}
