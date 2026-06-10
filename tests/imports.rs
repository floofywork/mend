//! Frozen acceptance tests for T-02.02 — Extract imports.
//!
//! The criteria are about a Rust API (a function that turns Rust source text
//! into a flat list of fully-qualified import path strings), so — exactly as the
//! T-02.01 structure tests do — the honest way to observe them is to *compile and
//! run a tiny probe program against the real `mend` crate*. The probes pin the
//! contract the criteria imply; this test crate stays std-only and never
//! references the not-yet-existing API directly, so it always compiles. Redness
//! comes from each probe failing to *build* (the `extract_imports` function does
//! not exist yet), not from this file failing to parse. Each probe is built into
//! an isolated `CARGO_TARGET_DIR` and run `--offline`, so it neither contends
//! with the outer build lock nor touches the network.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `mend::extract_imports(source: &str) -> mend::Result<Vec<String>>`
//!   - the returned `Vec<String>` is a flat list of fully-qualified `use` path
//!     strings, one per declared import.
//!   - a grouped `use a::{b, c}` expands to the two concrete paths `a::b` and
//!     `a::c`; no returned string is a raw group.
//!   - a file with no imports yields an empty list, not an error.
//!
//!   C1: returns every `use` path declared in the file as strings.
//!   C2: a grouped `use a::{b, c}` expands to `a::b` and `a::c`.
//!   C3: a file with no imports yields an empty list.

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
    dir.push(format!("mend-imports-{tag}-{}-{nanos}", std::process::id()));
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
/// In the red state `mend` has no `extract_imports`, so the probe fails to
/// resolve and `ran_ok` is false. Once the `mend` library exposes the API the
/// probe references, it builds, runs, and prints its success sentinel. A probe
/// that panics (e.g. on unexpected output) exits non-zero and prints no
/// sentinel, so a panic fails the corresponding test.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_imports_probe\"\n\
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

/// C1 — `extract_imports` returns every `use` path declared in the file as
/// strings. Two distinct `use` declarations are present, one a simple path and
/// one a multi-segment path; both fully-qualified paths must appear in the
/// result and the exact count of two guards against a dropped or duplicated
/// import. Comparison is whitespace-insensitive so the contract pins the path
/// structure, not a particular spacing.
#[test]
fn c1_returns_every_use_path() {
    let src = r##"
fn main() {
    let source = r#"
use std::fmt;
use std::collections::HashMap;

pub fn alpha() {}
"#;
    let imports = mend::extract_imports(source)
        .expect("valid Rust source must extract without error");
    let norm: Vec<String> = imports
        .iter()
        .map(|s| s.chars().filter(|c| !c.is_whitespace()).collect())
        .collect();
    assert!(
        norm.iter().any(|s| s == "std::fmt"),
        "must return the `std::fmt` use path, got: {:?}",
        imports
    );
    assert!(
        norm.iter().any(|s| s == "std::collections::HashMap"),
        "must return the `std::collections::HashMap` use path, got: {:?}",
        imports
    );
    assert_eq!(
        imports.len(),
        2,
        "exactly the two declared use paths must be returned, got: {:?}",
        imports
    );
    println!("C1_OK");
}
"##;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "extract_imports must return every declared use path; probe output:\n{}",
        p.output
    );
}

/// C2 — a grouped `use a::{b, c}` expands to the two concrete paths `a::b` and
/// `a::c`. The only declaration is the grouped use, so both expanded paths must
/// be present and the count must be exactly two — pinning that the group expands
/// to *both* members, no more and no fewer. No returned string may contain a
/// brace, proving each is a single concrete path rather than the raw group.
#[test]
fn c2_grouped_use_expands_to_each_path() {
    let src = r##"
fn main() {
    let source = r#"
use a::{b, c};
"#;
    let imports = mend::extract_imports(source)
        .expect("valid Rust source must extract without error");
    let norm: Vec<String> = imports
        .iter()
        .map(|s| s.chars().filter(|c| !c.is_whitespace()).collect())
        .collect();
    assert!(
        norm.iter().any(|s| s == "a::b"),
        "grouped use must expand to `a::b`, got: {:?}",
        imports
    );
    assert!(
        norm.iter().any(|s| s == "a::c"),
        "grouped use must expand to `a::c`, got: {:?}",
        imports
    );
    assert_eq!(
        imports.len(),
        2,
        "the grouped use must expand to exactly two paths, got: {:?}",
        imports
    );
    let joined = imports.join("\n");
    assert!(
        !joined.contains('{') && !joined.contains('}'),
        "each returned string must be a single concrete path, not a raw group, got: {:?}",
        imports
    );
    println!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "a grouped use must expand to one concrete path per member; probe output:\n{}",
        p.output
    );
}

/// C3 — a file with no imports yields an empty list. The source declares items
/// but no `use` at all; extraction must return `Ok` (not `Err`) and the list
/// must be empty.
#[test]
fn c3_no_imports_yields_empty_list() {
    let src = r##"
fn main() {
    let source = r#"
pub fn alpha() {}
pub struct Beta;
"#;
    let imports = mend::extract_imports(source)
        .expect("a file with no imports must yield an empty list, not an error");
    assert!(
        imports.is_empty(),
        "a file with no imports must yield an empty list, got: {:?}",
        imports
    );
    println!("C3_OK");
}
"##;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "a file with no imports must yield an empty list without error; probe output:\n{}",
        p.output
    );
}
