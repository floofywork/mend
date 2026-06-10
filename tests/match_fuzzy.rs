//! Frozen acceptance tests for T-05.02 — Match fuzzy context.
//!
//! The criteria are about a Rust API (the *tolerant* half of the edit matcher:
//! the fallback used when exact matching fails). Exactly as the T-05.01
//! `match_exact` tests do, the honest way to observe the criteria is to *compile
//! and run a tiny probe program against the real `mend` crate*. The probes pin
//! the contract the criteria imply; this test crate stays std-only and never
//! references the not-yet-existing API directly, so it always compiles. Redness
//! comes from each probe failing to *build* (`match_fuzzy` / `FuzzyMatch` do not
//! exist yet), not from this file failing to parse. Each probe is built into an
//! isolated `CARGO_TARGET_DIR` and run `--offline`, so it neither contends with
//! the outer build lock nor touches the network.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `mend::match_fuzzy(source: &str, search: &str) -> mend::FuzzyMatch`
//!   - `FuzzyMatch::Match { range: std::ops::Range<usize>, score: f64 }` carries
//!     the matched source byte range plus a similarity score, and
//!     `FuzzyMatch::NoMatch` signals rejection — *in-band*, never errored or
//!     panicked.
//!   - matching is **whitespace-insensitive per line**: the search and a
//!     same-length window of source lines are compared after trimming each
//!     line's leading/trailing whitespace, so a region differing only in
//!     indentation still matches.
//!   - the similarity `score` lies in `[0, 1]` and equals the fraction of search
//!     lines that match (after trimming) in the best-aligned source window:
//!     all lines matching → `1.0`, two of three → `2/3`, one of three → `1/3`.
//!   - a **fixed similarity threshold** gates acceptance: a best window scoring
//!     below it is rejected as `NoMatch`; an accepted `Match` always scores at
//!     or above the threshold. The tests pin the threshold band `(1/3, 2/3]` by
//!     asserting a `2/3` candidate is accepted and a `1/3` candidate rejected,
//!     so both sides of the threshold comparison are exercised.
//!
//!   C1: matches search text ignoring per-line leading/trailing whitespace differences.
//!   C2: returns the matched range plus a similarity score in the range [0,1].
//!   C3: candidate matches below a fixed similarity threshold are rejected as no-match.

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
    dir.push(format!("mend-match-fuzzy-{tag}-{}-{nanos}", std::process::id()));
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
/// In the red state `mend` has no `match_fuzzy` / `FuzzyMatch`, so the probe
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
         name = \"mend_match_fuzzy_probe\"\n\
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

/// C1 — `match_fuzzy` matches ignoring per-line leading/trailing whitespace. The
/// probe feeds a two-line search whose indentation (and trailing spaces) differ
/// from the source region it should match. It asserts the result is a `Match`
/// whose range covers the *source* region verbatim — with the source's original
/// whitespace, byte-for-byte — and whose score is a perfect `1.0` because every
/// line is equal after trimming. The explicit `assert_ne!` against the raw
/// search text pins that this is whitespace-*insensitive* matching, not exact
/// equality (which is T-05.01's job): the source slice keeps its indentation
/// while the search did not.
#[test]
fn c1_whitespace_insensitive_match_covers_source_region() {
    let src = r##"
fn main() {
    let source = "fn f() {\n    if cond {\n        do_thing();\n    }\n}\n";
    // Search lines differ from the source only in leading/trailing whitespace.
    let search = "if cond {\n  do_thing();  \n";
    match mend::match_fuzzy(source, search) {
        mend::FuzzyMatch::Match { range, score } => {
            assert_eq!(
                &source[range.clone()],
                "    if cond {\n        do_thing();\n",
                "C1: the match range must cover the source region (with its own whitespace) whose lines equal the search after trimming"
            );
            assert!(
                (score - 1.0).abs() < 1e-9,
                "C1: every line equal after trimming must score a perfect 1.0, got {}",
                score
            );
            assert_ne!(
                &source[range.clone()],
                search,
                "C1: matching is whitespace-insensitive, so the matched source slice differs from the search verbatim"
            );
        }
        other => panic!(
            "C1: a region differing only in per-line whitespace must still match, got {:?}",
            other
        ),
    }
    println!("C1_OK");
}
"##;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "match_fuzzy must match ignoring per-line leading/trailing whitespace; probe output:\n{}",
        p.output
    );
}

/// C2 — a `Match` returns the matched range *plus* a similarity score in `[0,1]`.
/// The probe constructs a three-line search whose best-aligned source window
/// matches two of its three lines (after trimming), so the score is the partial
/// value `2/3`. It asserts: the score lies within `[0, 1]`; it is strictly
/// *inside* `(0, 1)` and equal to `2/3` — proving the score reflects similarity
/// rather than being pinned to a constant `1.0`; and the `Match` carries the
/// source range of that candidate window alongside the score.
#[test]
fn c2_match_carries_range_and_fractional_score_in_unit_interval() {
    let src = r##"
fn main() {
    // The middle three lines match two of the three search lines after trimming.
    let source = "head\n  alpha\n  XXXX\n  gamma\n tail\n";
    let search = "alpha\nbeta\ngamma\n";
    match mend::match_fuzzy(source, search) {
        mend::FuzzyMatch::Match { range, score } => {
            assert!(
                score >= 0.0 && score <= 1.0,
                "C2: the similarity score must lie in [0, 1], got {}",
                score
            );
            assert!(
                score > 0.0 && score < 1.0,
                "C2: a partial (2-of-3) match must score strictly inside (0, 1), got {}",
                score
            );
            assert!(
                (score - (2.0 / 3.0)).abs() < 1e-9,
                "C2: two of three lines matching must score 2/3, got {}",
                score
            );
            assert_eq!(
                &source[range.clone()],
                "  alpha\n  XXXX\n  gamma\n",
                "C2: the Match must carry the byte range of the best-aligned candidate window"
            );
        }
        other => panic!(
            "C2: a 2-of-3 candidate is above threshold and must be a Match carrying range and score, got {:?}",
            other
        ),
    }
    println!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "match_fuzzy must return the matched range plus a similarity score in [0,1]; probe output:\n{}",
        p.output
    );
}

/// C3 — candidates below the fixed threshold are rejected as no-match. The probe
/// uses the same three-line search twice. Against a source whose best window
/// matches only *one* of three lines (score `1/3`), the result must be exactly
/// `NoMatch` — the below-threshold candidate is rejected, not forced. Against a
/// source whose best window matches *two* of three lines (score `2/3`), the
/// result must be a `Match` whose score is at or above the threshold, pinning
/// the invariant that any accepted match scores `>=` the threshold. Together the
/// two halves bracket the threshold within `(1/3, 2/3]`, exercising both sides
/// of the threshold comparison so a flipped or shifted comparison cannot survive.
#[test]
fn c3_below_threshold_rejected_above_threshold_accepted() {
    let src = r##"
fn main() {
    let search = "alpha\nbeta\ngamma\n";

    // Best window matches only 1 of 3 lines (score 1/3) → below threshold → NoMatch.
    let lonely = "p\n  alpha\n  q\n  r\n s\n";
    match mend::match_fuzzy(lonely, search) {
        mend::FuzzyMatch::NoMatch => {}
        other => panic!(
            "C3: a 1-of-3 (below-threshold) candidate must be rejected as NoMatch, got {:?}",
            other
        ),
    }

    // Boundary counterpart: best window matches 2 of 3 lines (score 2/3) →
    // at/above threshold → accepted, and the accepted score is >= the threshold.
    let ok = "p\n  alpha\n  XXXX\n  gamma\n s\n";
    match mend::match_fuzzy(ok, search) {
        mend::FuzzyMatch::Match { range, score } => {
            assert!(
                score >= 2.0 / 3.0 - 1e-9,
                "C3: an accepted match must score at or above the threshold, got {}",
                score
            );
            let _ = range;
        }
        other => panic!(
            "C3: a 2-of-3 candidate is at/above threshold and must be accepted, got {:?}",
            other
        ),
    }

    println!("C3_OK");
}
"##;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "match_fuzzy must reject below-threshold candidates as no-match and accept at/above threshold; probe output:\n{}",
        p.output
    );
}
