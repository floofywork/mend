//! Frozen acceptance tests for T-02.04 — Assemble file context.
//!
//! The criteria are about a Rust API (a function that joins a file's extracted
//! structure and imports into one budget-bounded context string), so — exactly as
//! the T-02.01 structure, T-02.02 imports, and T-02.03 budget tests do — the
//! honest way to observe them is to *compile and run a tiny probe program against
//! the real `mend` crate*. The probes pin the contract the criteria imply; this
//! test crate stays std-only and never references the not-yet-existing API
//! directly, so it always compiles. Redness comes from each probe failing to
//! *build* (the `assemble_context` function does not exist yet), not from this
//! file failing to parse. Each probe is built into an isolated `CARGO_TARGET_DIR`
//! and run `--offline`, so it neither contends with the outer build lock nor
//! touches the network.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `mend::assemble_context(structure: &[String], imports: &[String], budget: usize) -> String`
//!   - the result is a single string that combines BOTH the extracted structure
//!     signatures and the import paths for one file.
//!   - the assembled string is passed through the budgeter, so (at the budgeter's
//!     fixed 4-chars-per-token ratio) it is never longer than `4 * budget`
//!     characters; a budget of `0` yields an empty string.
//!   - assembly is pure and deterministic: the same `(structure, imports, budget)`
//!     always yields byte-identical output, with no model or network call.
//!
//!   C1: combines extracted structure and imports into one deterministic context string for a file.
//!   C2: the assembled context passes through the budgeter and never exceeds the configured token budget.
//!   C3: identical inputs always produce byte-identical output.

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
    dir.push(format!("mend-context-{tag}-{}-{nanos}", std::process::id()));
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
/// In the red state `mend` has no `assemble_context`, so the probe fails to
/// resolve and `ran_ok` is false. Once the `mend` library exposes the API the
/// probe references, it builds, runs, and prints its success sentinel. A probe
/// that panics (e.g. on a broken invariant) exits non-zero and prints no
/// sentinel, so a panic fails the corresponding test.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_context_probe\"\n\
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

/// C1 — `assemble_context` combines the extracted structure and the imports into
/// a single context string for the file. The probe feeds a structure list (two
/// signatures, each tagged with a distinctive marker) and an imports list (also
/// distinctively marked), under a budget far larger than the content so nothing
/// is trimmed. Every structure marker AND the import marker must appear in the one
/// returned string — proving both halves are joined into a single blob rather than
/// either being dropped. The result is one `String`, so "one context string" is
/// pinned by the return type itself.
#[test]
fn c1_combines_structure_and_imports() {
    let src = r##"
fn main() {
    let structure: Vec<String> = vec![
        "pub fn alpha_STRUCT() -> bool".to_string(),
        "pub struct Beta_STRUCT".to_string(),
    ];
    let imports: Vec<String> = vec![
        "std::collections::HashMap_IMPORT".to_string(),
        "std::io::Read_IMPORT".to_string(),
    ];

    // A budget far larger than the content, so the budgeter trims nothing and the
    // full combination is observable.
    let ctx = mend::assemble_context(&structure, &imports, 10_000usize);

    for marker in ["alpha_STRUCT", "Beta_STRUCT"] {
        assert!(
            ctx.contains(marker),
            "C1: the extracted structure `{}` must appear in the context, got: {:?}",
            marker,
            ctx
        );
    }
    for marker in ["HashMap_IMPORT", "Read_IMPORT"] {
        assert!(
            ctx.contains(marker),
            "C1: the import `{}` must appear in the context, got: {:?}",
            marker,
            ctx
        );
    }

    println!("C1_OK");
}
"##;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "assemble_context must combine structure and imports into one string; probe output:\n{}",
        p.output
    );
}

/// C2 — the assembled context passes through the budgeter and never exceeds the
/// configured budget. The probe feeds content far larger than a small budget and
/// pins three facets. First, under a small budget of `n = 2` (the budgeter's
/// 4-chars-per-token ratio gives an 8-character ceiling) the output must be at
/// most `4 * n` characters — proving the assembled blob is actually routed through
/// the budgeter rather than returned raw. Second, a budget of `0` must yield an
/// empty string. Third, the complementary side: under a generous budget the same
/// content must come back LONGER than the small ceiling, so budgeting is a real
/// bound and not a degenerate always-empty/always-truncate.
#[test]
fn c2_assembled_context_within_budget() {
    let src = r##"
fn main() {
    // Content far larger than a small budget: each entry alone overflows it.
    let structure: Vec<String> = vec!["a".repeat(100), "b".repeat(100)];
    let imports: Vec<String> = vec!["c".repeat(100)];

    let n = 2usize; // ratio 4 -> an 8-character ceiling
    let out = mend::assemble_context(&structure, &imports, n);
    assert!(
        out.chars().count() <= n * 4,
        "C2: budgeted context must be at most 4*n = {} chars, got {}: {:?}",
        n * 4,
        out.chars().count(),
        out
    );

    // A budget of 0 yields an empty context.
    let zero = mend::assemble_context(&structure, &imports, 0usize);
    assert!(
        zero.is_empty(),
        "C2: a budget of 0 must yield an empty context, got {:?}",
        zero
    );

    // The other side of the bound: a generous budget retains content beyond the
    // small ceiling, so the budgeter is a genuine cap, not always-empty.
    let big = mend::assemble_context(&structure, &imports, 10_000usize);
    assert!(
        big.chars().count() > n * 4,
        "C2: a generous budget must retain content beyond the {}-char small ceiling, got {} chars",
        n * 4,
        big.chars().count()
    );

    println!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "the assembled context must pass through the budgeter and stay within budget; probe output:\n{}",
        p.output
    );
}

/// C3 — identical inputs always produce byte-identical output. The probe assembles
/// the same `(structure, imports, budget)` twice and asserts the two results are
/// byte-equal, on BOTH code paths: once under a generous budget (the untruncated
/// join) and once under a tight budget (the truncated path). Equality on both
/// pins determinism for the whole function, not just the part the budgeter already
/// guarantees.
#[test]
fn c3_identical_inputs_are_byte_identical() {
    let src = r##"
fn main() {
    let structure: Vec<String> = vec![
        "pub fn alpha()".to_string(),
        "pub struct Beta".to_string(),
        "pub enum Gamma".to_string(),
    ];
    let imports: Vec<String> = vec![
        "std::io::Read".to_string(),
        "std::fmt::Display".to_string(),
    ];

    // Untruncated path: a generous budget keeps the whole join.
    let a = mend::assemble_context(&structure, &imports, 10_000usize);
    let b = mend::assemble_context(&structure, &imports, 10_000usize);
    assert_eq!(
        a, b,
        "C3: identical inputs must yield byte-identical output (untruncated path)"
    );

    // Truncated path: a tight budget forces a cut, which must also be deterministic.
    let c = mend::assemble_context(&structure, &imports, 3usize);
    let d = mend::assemble_context(&structure, &imports, 3usize);
    assert_eq!(
        c, d,
        "C3: identical inputs must yield byte-identical output (truncated path)"
    );

    println!("C3_OK");
}
"##;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "identical inputs must produce byte-identical output; probe output:\n{}",
        p.output
    );
}
