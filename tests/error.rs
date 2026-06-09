//! Frozen acceptance tests for T-00.02 — Define error type.
//!
//! The criteria are about a Rust API (`MendError`, its trait impls, the
//! `Result` alias), so the honest way to observe them is to *compile and run a
//! tiny probe program against the real `mend` crate* — exactly as a downstream
//! task would `use mend::{MendError, Result}`. These tests are deliberately
//! std-only and never reference the not-yet-existing API directly, so the test
//! crate itself always compiles; redness comes from the probe failing to build
//! (the `mend` library / variants don't exist yet), not from this file failing
//! to parse. Each probe is built into an isolated `CARGO_TARGET_DIR` and run
//! `--offline`, so it neither contends with the outer build lock nor touches
//! the network. This mirrors the std-only, shell-out style of the T-00.01
//! scaffold tests.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `MendError::Io(std::io::Error)`            (the one in-scope conversion source)
//!   - `MendError::Config(String)`
//!   - `MendError::Parse(String)`
//!   - `MendError::Patch(String)`
//!   - `MendError::Completer(String)`
//!
//!   C1: `MendError` implements `std::error::Error` and `std::fmt::Display`.
//!   C2: distinct variants exist for Io, Config, Parse, Patch, Completer.
//!   C3: every variant's `Display` renders a non-empty message.
//!   C4: `mend::Result<T>` is the alias `std::result::Result<T, MendError>`.

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
    dir.push(format!("mend-error-{tag}-{}-{nanos}", std::process::id()));
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
/// In the red state `mend` has no library target, so the probe fails to resolve
/// and `ran_ok` is false. Once the `mend` library exposes the API the probe
/// references, it builds, runs, and prints its success sentinel.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_error_probe\"\n\
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

/// C1 — `MendError` satisfies both `std::error::Error` and `std::fmt::Display`.
/// The generic bound only type-checks if both impls resolve; the coercion to
/// `&dyn std::error::Error` and the `Display` formatting reinforce it at use.
#[test]
fn c1_menderror_implements_error_and_display() {
    let src = r#"
fn requires_error_and_display<E: std::error::Error + std::fmt::Display>(_: &E) {}

fn main() {
    let e = mend::MendError::Config("boom".to_string());
    requires_error_and_display(&e);
    let _as_error: &dyn std::error::Error = &e;
    let rendered: String = format!("{}", e);
    let _ = rendered;
    println!("C1_OK");
}
"#;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "MendError must implement std::error::Error and std::fmt::Display; probe output:\n{}",
        p.output
    );
}

/// C2 — distinct variants exist for Io, Config, Parse, Patch, and Completer.
/// The exhaustive `match` (no wildcard arm) proves these are the variants, and
/// the five distinct discriminant codes prove they are distinct.
#[test]
fn c2_menderror_has_distinct_variants() {
    let src = r#"
fn code(e: &mend::MendError) -> u8 {
    use mend::MendError::*;
    match e {
        Io(_) => 0,
        Config(_) => 1,
        Parse(_) => 2,
        Patch(_) => 3,
        Completer(_) => 4,
    }
}

fn main() {
    let errs = [
        mend::MendError::Io(std::io::Error::new(std::io::ErrorKind::Other, "io")),
        mend::MendError::Config("config".to_string()),
        mend::MendError::Parse("parse".to_string()),
        mend::MendError::Patch("patch".to_string()),
        mend::MendError::Completer("completer".to_string()),
    ];
    let mut seen = std::collections::BTreeSet::new();
    for e in &errs {
        seen.insert(code(e));
    }
    assert_eq!(seen.len(), 5, "Io/Config/Parse/Patch/Completer must be five distinct variants");
    println!("C2_OK");
}
"#;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "MendError must have distinct Io/Config/Parse/Patch/Completer variants; probe output:\n{}",
        p.output
    );
}

/// C3 — every variant renders a non-empty `Display` message.
#[test]
fn c3_every_variant_display_is_non_empty() {
    let src = r#"
fn main() {
    let errs: [mend::MendError; 5] = [
        mend::MendError::Io(std::io::Error::new(std::io::ErrorKind::Other, "io")),
        mend::MendError::Config("config".to_string()),
        mend::MendError::Parse("parse".to_string()),
        mend::MendError::Patch("patch".to_string()),
        mend::MendError::Completer("completer".to_string()),
    ];
    for e in &errs {
        let rendered = format!("{}", e);
        assert!(
            !rendered.trim().is_empty(),
            "Display must be non-empty for variant {:?}",
            e
        );
    }
    println!("C3_OK");
}
"#;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "every MendError variant's Display must render a non-empty message; probe output:\n{}",
        p.output
    );
}

/// C4 — `mend::Result<T>` is exactly `std::result::Result<T, mend::MendError>`.
/// Assigning a `mend::Result<i32>` value into the fully-spelled type only
/// type-checks if the alias is defined that way.
#[test]
fn c4_result_alias_is_exported() {
    let src = r#"
fn ok() -> mend::Result<i32> {
    Ok(7)
}

fn err() -> mend::Result<i32> {
    Err(mend::MendError::Parse("nope".to_string()))
}

fn main() {
    let proof: std::result::Result<i32, mend::MendError> = ok();
    assert_eq!(proof.unwrap(), 7);
    assert!(err().is_err());
    let _unit: mend::Result<()> = Ok(());
    println!("C4_OK");
}
"#;
    let p = run_probe("c4", src);
    assert!(
        p.ran_ok && p.output.contains("C4_OK"),
        "mend::Result<T> must be exported as Result<T, MendError>; probe output:\n{}",
        p.output
    );
}
