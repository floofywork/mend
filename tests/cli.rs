//! Frozen acceptance tests for T-00.03 — Define CLI parser.
//!
//! Four of the five criteria are about a Rust API (a `Command` enum produced by
//! `mend::parse`), so — exactly as the T-00.02 error tests do — the honest way
//! to observe them is to *compile and run a tiny probe program against the real
//! `mend` crate*. The probes pin the contract the criteria imply; this test
//! crate stays std-only and never references the not-yet-existing API directly,
//! so it always compiles. Redness for C1–C4 comes from the probe failing to
//! build (the parser / `Command` variants don't exist yet), not from this file
//! failing to parse. Each probe is built into an isolated `CARGO_TARGET_DIR` and
//! run `--offline`, so it neither contends with the outer build lock nor touches
//! the network.
//!
//! C5 — "an unknown subcommand exits non-zero and prints usage to stderr" — is a
//! statement about *process* behaviour (exit status + stderr), so the faithful
//! observation is to run the real `mend` binary with a bogus subcommand and read
//! its exit code and stderr, the way a shell user would. In the red state the
//! scaffold's empty `main` ignores its arguments and exits 0, so the test fails;
//! it goes green only once `main` routes a parse failure to a non-zero exit with
//! usage on stderr.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `mend::parse(&[String]) -> mend::Command`     (argv-without-program-name → typed command)
//!   - `mend::Command::Explain { path: PathBuf, symbol: Option<String>, .. }`
//!   - `mend::Command::Edit    { path: PathBuf, instruction: String, apply: bool, json: bool, .. }`
//!   - `mend::Command::dry_run(&self) -> bool`        (the negation of `--apply`, defaulting true)
//!
//!   C1: parsing `explain f.rs` yields Explain with path `f.rs` and symbol `None`.
//!   C2: parsing `explain f.rs --symbol foo` sets the symbol to `Some("foo")`.
//!   C3: parsing `edit f.rs "do x"` yields Edit holding path and instruction `"do x"`.
//!   C4: `--apply` and `--json` parse to booleans; with neither present, dry-run defaults to true.
//!   C5: an unknown subcommand exits non-zero and prints usage to stderr.

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
    dir.push(format!("mend-cli-{tag}-{}-{nanos}", std::process::id()));
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
/// In the red state `mend` exposes no parser, so the probe fails to resolve and
/// `ran_ok` is false. Once the `mend` library exposes the API the probe
/// references, it builds, runs, and prints its success sentinel.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_cli_probe\"\n\
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

/// Run the real `mend` binary with `args` (passed after `--`), in the project
/// directory but with an isolated target dir so it stays off the outer build
/// lock. Returns whether the process exited 0 and its captured stderr.
fn run_mend(tag: &str, args: &[&str]) -> (bool, String) {
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    let out = Command::new(cargo_bin())
        .current_dir(manifest_dir())
        .env("CARGO_TARGET_DIR", &target_dir)
        .args(["run", "--quiet", "--offline", "--bin", "mend", "--"])
        .args(args)
        .output()
        .expect("failed to spawn `cargo run` for mend binary");

    let success = out.status.success();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    let _ = std::fs::remove_dir_all(&target_dir);

    (success, stderr)
}

/// C1 — `explain f.rs` parses to `Explain` with the given path and no symbol.
#[test]
fn c1_explain_path_and_symbol_none() {
    let src = r#"
fn main() {
    let args: Vec<String> = ["explain", "f.rs"].iter().map(|s| s.to_string()).collect();
    match mend::parse(&args) {
        mend::Command::Explain { path, symbol, .. } => {
            assert_eq!(path, std::path::PathBuf::from("f.rs"), "explain path must be f.rs");
            assert!(symbol.is_none(), "symbol must default to None");
        }
        other => panic!("expected an Explain command, got {:?}", other),
    }
    println!("C1_OK");
}
"#;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "`explain f.rs` must parse to Explain{{ path: f.rs, symbol: None }}; probe output:\n{}",
        p.output
    );
}

/// C2 — `explain f.rs --symbol foo` sets the symbol to `Some("foo")`.
#[test]
fn c2_explain_symbol_some() {
    let src = r#"
fn main() {
    let args: Vec<String> =
        ["explain", "f.rs", "--symbol", "foo"].iter().map(|s| s.to_string()).collect();
    match mend::parse(&args) {
        mend::Command::Explain { path, symbol, .. } => {
            assert_eq!(path, std::path::PathBuf::from("f.rs"), "explain path must be f.rs");
            assert_eq!(symbol.as_deref(), Some("foo"), "--symbol foo must set Some(\"foo\")");
        }
        other => panic!("expected an Explain command, got {:?}", other),
    }
    println!("C2_OK");
}
"#;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "`explain f.rs --symbol foo` must set symbol = Some(\"foo\"); probe output:\n{}",
        p.output
    );
}

/// C3 — `edit f.rs "do x"` parses to `Edit` holding the path and instruction.
#[test]
fn c3_edit_path_and_instruction() {
    let src = r#"
fn main() {
    let args: Vec<String> =
        ["edit", "f.rs", "do x"].iter().map(|s| s.to_string()).collect();
    match mend::parse(&args) {
        mend::Command::Edit { path, instruction, .. } => {
            assert_eq!(path, std::path::PathBuf::from("f.rs"), "edit path must be f.rs");
            assert_eq!(instruction, "do x", "edit instruction must be \"do x\"");
        }
        other => panic!("expected an Edit command, got {:?}", other),
    }
    println!("C3_OK");
}
"#;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "`edit f.rs \"do x\"` must parse to Edit{{ path: f.rs, instruction: \"do x\" }}; probe output:\n{}",
        p.output
    );
}

/// C4 — `--apply` and `--json` parse to booleans; with neither present, the
/// command is a dry run (`dry_run()` is true and `apply` is false).
#[test]
fn c4_apply_json_booleans_and_dryrun_default() {
    let src = r#"
fn edit_flags(args: &[&str]) -> (bool, bool) {
    let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    match mend::parse(&owned) {
        mend::Command::Edit { apply, json, .. } => (apply, json),
        other => panic!("expected an Edit command, got {:?}", other),
    }
}

fn main() {
    // Neither flag present: dry-run defaults to true, both booleans false.
    let (apply, json) = edit_flags(&["edit", "f.rs", "do x"]);
    assert_eq!(apply, false, "apply must default to false");
    assert_eq!(json, false, "json must default to false");
    let base: Vec<String> = ["edit", "f.rs", "do x"].iter().map(|s| s.to_string()).collect();
    assert_eq!(mend::parse(&base).dry_run(), true, "dry-run must default to true");

    // --apply flips apply on and dry-run off.
    let (apply, _json) = edit_flags(&["edit", "f.rs", "do x", "--apply"]);
    assert_eq!(apply, true, "--apply must parse to apply = true");
    let applied: Vec<String> =
        ["edit", "f.rs", "do x", "--apply"].iter().map(|s| s.to_string()).collect();
    assert_eq!(mend::parse(&applied).dry_run(), false, "--apply must make dry-run false");

    // --json flips json on.
    let (_apply, json) = edit_flags(&["edit", "f.rs", "do x", "--json"]);
    assert_eq!(json, true, "--json must parse to json = true");

    println!("C4_OK");
}
"#;
    let p = run_probe("c4", src);
    assert!(
        p.ran_ok && p.output.contains("C4_OK"),
        "`--apply`/`--json` must parse to booleans and dry-run must default to true; probe output:\n{}",
        p.output
    );
}

/// C5 — an unknown subcommand makes the real `mend` binary exit non-zero and
/// print usage to stderr. Observed by running the binary as a user would.
#[test]
fn c5_unknown_subcommand_exits_nonzero_with_usage_on_stderr() {
    let (success, stderr) = run_mend("c5", &["frobnicate"]);
    assert!(
        !success,
        "an unknown subcommand must exit non-zero; stderr was:\n{stderr}"
    );
    assert!(
        stderr.to_lowercase().contains("usage"),
        "an unknown subcommand must print usage to stderr; stderr was:\n{stderr}"
    );
}
