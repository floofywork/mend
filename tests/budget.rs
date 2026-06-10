//! Frozen acceptance tests for T-02.03 — Budget context tokens.
//!
//! The criteria are about a Rust API (a function that trims a text string so its
//! estimated token count stays within a budget), so — exactly as the T-02.01
//! structure and T-02.02 imports tests do — the honest way to observe them is to
//! *compile and run a tiny probe program against the real `mend` crate*. The
//! probes pin the contract the criteria imply; this test crate stays std-only and
//! never references the not-yet-existing API directly, so it always compiles.
//! Redness comes from each probe failing to *build* (the `budget` function does
//! not exist yet), not from this file failing to parse. Each probe is built into
//! an isolated `CARGO_TARGET_DIR` and run `--offline`, so it neither contends
//! with the outer build lock nor touches the network.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `mend::budget(text: &str, n: usize) -> String`
//!   - estimation uses a FIXED chars-per-token ratio of 4 (4 characters ≈ 1
//!     token), so the estimated token count of a string is `ceil(chars / 4)`.
//!     Since `ceil(c / 4) <= n` exactly when `c <= 4 * n`, the budget invariant
//!     "estimated tokens of the output never exceed `n`" is observed as
//!     "the output is at most `4 * n` characters long".
//!   - estimation/truncation is deterministic and pure: the same `(text, n)`
//!     always yields byte-identical output, with no model or network call.
//!   - when (and only when) the text is too large to fit, the result is
//!     truncated and ends with the visible marker `...`.
//!   - a budget of `0` yields an empty string.
//!
//!   C1: `budget(text, n)` returns text whose estimated token count is at most `n`.
//!   C2: estimation uses a fixed deterministic chars-per-token ratio with no model or network call.
//!   C3: when truncation occurs the result ends with a visible truncation marker.
//!   C4: a budget of 0 returns an empty string.

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
    dir.push(format!("mend-budget-{tag}-{}-{nanos}", std::process::id()));
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
/// In the red state `mend` has no `budget`, so the probe fails to resolve and
/// `ran_ok` is false. Once the `mend` library exposes the API the probe
/// references, it builds, runs, and prints its success sentinel. A probe that
/// panics (e.g. on a broken invariant) exits non-zero and prints no sentinel, so
/// a panic fails the corresponding test.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_budget_probe\"\n\
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

/// C1 — `budget(text, n)` returns text whose estimated token count is at most
/// `n`. At the fixed ratio of 4 chars/token, `ceil(chars / 4) <= n` iff
/// `chars <= 4 * n`, so the invariant is observed as a hard ceiling on the
/// output's character length. An oversize input (40 chars) under a budget of
/// `n = 2` (an 8-char ceiling) must come back within that ceiling — and not by
/// degenerately returning nothing. An already-small input must likewise stay
/// within budget. Both sides of "needs trimming / already fits" are checked so
/// the invariant is pinned for trimmed and untrimmed inputs alike.
#[test]
fn c1_output_estimate_within_budget() {
    let src = r##"
fn main() {
    let n = 2usize; // ratio 4 -> an 8-character ceiling

    let big: String = "abcdefghij".repeat(4); // 40 chars, far over budget
    let out = mend::budget(&big, n);
    assert!(
        out.chars().count() <= n * 4,
        "C1: budgeted output must be at most 4*n = {} chars, got {} chars: {:?}",
        n * 4,
        out.chars().count(),
        out
    );
    assert!(
        !out.is_empty(),
        "C1: an oversize, positive-budget input must retain content, not vanish: {:?}",
        out
    );

    let small = "abcd"; // 4 chars, already within the 8-char ceiling
    let out_small = mend::budget(small, n);
    assert!(
        out_small.chars().count() <= n * 4,
        "C1: an already-fitting input must stay within budget, got {:?}",
        out_small
    );

    println!("C1_OK");
}
"##;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "budget must keep its output within the estimated token budget; probe output:\n{}",
        p.output
    );
}

/// C2 — estimation uses a fixed deterministic chars-per-token ratio with no
/// model or network call. Two facets are pinned. First, determinism: the same
/// `(text, n)` evaluated twice yields byte-identical output (no randomness, no
/// external lookup). Second, the ratio is exactly 4, pinned on BOTH sides of the
/// `4 * n` threshold: an input of exactly `4 * n = 8` characters fits and passes
/// through unchanged, while an input of `4 * n + 1 = 9` characters is trimmed
/// back within the ceiling. Testing both the exact-fit and the one-over case
/// fixes the ratio at 4 (anything larger would leave 9 chars untouched; anything
/// smaller would trim 8).
#[test]
fn c2_fixed_deterministic_ratio() {
    let src = r##"
fn main() {
    let n = 2usize; // ratio 4 -> threshold at 4*n = 8 chars

    // Deterministic: identical inputs always produce identical output.
    let text: String = "wxyz".repeat(10); // 40 chars
    let a = mend::budget(&text, n);
    let b = mend::budget(&text, n);
    assert_eq!(a, b, "C2: budgeting must be deterministic for identical inputs");

    // Ratio pinned at 4: exactly 4*n = 8 chars fits and is returned unchanged.
    let fits = "abcdefgh"; // 8 chars
    let out_fits = mend::budget(fits, n);
    assert_eq!(
        out_fits, fits,
        "C2: an input of exactly 4*n chars must fit and pass through unchanged, got {:?}",
        out_fits
    );

    // Ratio pinned at 4: one char past the threshold (9 chars) must be trimmed.
    let over = "abcdefghi"; // 9 chars
    let out_over = mend::budget(over, n);
    assert!(
        out_over != over,
        "C2: an input one char past 4*n must be trimmed, got {:?}",
        out_over
    );
    assert!(
        out_over.chars().count() <= n * 4,
        "C2: a trimmed output must fall within the 4*n char budget, got {:?}",
        out_over
    );

    println!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "budget must use a fixed deterministic 4-chars-per-token ratio; probe output:\n{}",
        p.output
    );
}

/// C3 — when truncation occurs the result ends with a visible truncation marker.
/// An oversize input (40 chars under an 8-char budget) is necessarily trimmed and
/// must end with the visible marker `...`. The complementary side is also pinned:
/// an input that already fits (and does not itself end in the marker) must come
/// back WITHOUT the marker, so the marker marks truncation specifically rather
/// than being appended unconditionally.
#[test]
fn c3_truncation_appends_visible_marker() {
    let src = r##"
fn main() {
    let n = 2usize; // 8-char ceiling

    let big: String = "0123456789".repeat(4); // 40 chars, must be truncated
    let truncated = mend::budget(&big, n);
    assert!(
        truncated != big,
        "C3: an oversize input must actually be truncated, got {:?}",
        truncated
    );
    assert!(
        truncated.ends_with("..."),
        "C3: a truncated result must end with the visible marker `...`, got {:?}",
        truncated
    );

    let fits = "abcd"; // 4 chars, within budget, does not end in the marker
    let kept = mend::budget(fits, n);
    assert!(
        !kept.ends_with("..."),
        "C3: a result that was not truncated must not carry the marker, got {:?}",
        kept
    );

    println!("C3_OK");
}
"##;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "a truncated result must end with a visible marker, an untruncated one must not; probe output:\n{}",
        p.output
    );
}

/// C4 — a budget of 0 returns an empty string. With `n = 0` the ceiling is 0
/// characters, so any text — empty or not — must come back empty. The
/// complementary side is pinned too: a budget of `1` over the same non-empty text
/// is NOT empty, so `0` is the genuine boundary rather than the function always
/// returning empty.
#[test]
fn c4_zero_budget_yields_empty() {
    let src = r##"
fn main() {
    let text = "some non-empty text";

    let zero = mend::budget(text, 0usize);
    assert!(
        zero.is_empty(),
        "C4: a budget of 0 must return an empty string, got {:?}",
        zero
    );

    let zero_empty = mend::budget("", 0usize);
    assert!(
        zero_empty.is_empty(),
        "C4: a budget of 0 on empty input must stay empty, got {:?}",
        zero_empty
    );

    // The other side of the zero boundary: a budget of 1 keeps content.
    let one = mend::budget(text, 1usize);
    assert!(
        !one.is_empty(),
        "C4: a non-zero budget must retain content, not return empty, got {:?}",
        one
    );

    println!("C4_OK");
}
"##;
    let p = run_probe("c4", src);
    assert!(
        p.ran_ok && p.output.contains("C4_OK"),
        "a budget of 0 must yield an empty string while a positive budget retains content; probe output:\n{}",
        p.output
    );
}
