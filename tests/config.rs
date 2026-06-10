//! Frozen acceptance tests for T-01.03 — Load configuration.
//!
//! The criteria are about a Rust API (`mend::Config` and its `load` associated
//! function), so — exactly as the T-00.02 error, T-00.03 CLI and T-01.01
//! completer tests do — the honest way to observe them is to *compile and run a
//! tiny probe program against the real `mend` crate*. The probes pin the
//! contract the criteria imply; this test crate stays std-only and never
//! references the not-yet-existing API directly, so it always compiles. Redness
//! comes from each probe failing to *build* (the `Config` type does not exist
//! yet), not from this file failing to parse. Each probe is built into an
//! isolated `CARGO_TARGET_DIR` and run `--offline`, so it neither contends with
//! the outer build lock nor touches the network.
//!
//! Crucially, `Config::load` reads from *ambient* inputs — a `mend.toml` in the
//! working directory and three environment variables — so every probe is run
//! with a controlled current directory (the probe crate's own scratch dir,
//! where we optionally drop a `mend.toml`) and an explicitly scrubbed, then
//! repopulated, set of the three `MEND_*` env vars. That makes each observation
//! hermetic: the file and the env are exactly what the criterion describes,
//! nothing inherited from the host.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `mend::Config::load() -> mend::Result<mend::Config>`
//!   - public fields `model: String`, `endpoint: String`, and an integer
//!     `token_budget` (compared against integer literals, so the exact width is
//!     left to the implementation).
//!   - precedence env > file > default, observed field-by-field with distinct
//!     values so a dropped, swapped, or copied override is caught.
//!
//!   C1: `Config::load` reads model, endpoint, token_budget from `mend.toml`.
//!   C2: `MEND_MODEL`/`MEND_ENDPOINT`/`MEND_TOKEN_BUDGET` override file values.
//!   C3: a missing `mend.toml` yields populated defaults without error.
//!   C4: a non-integer `token_budget` (file or env) yields `MendError::Config`.

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
    dir.push(format!("mend-config-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// The three environment variables `Config::load` consults, scrubbed before
/// every probe so the host environment can never leak into an observation.
const MEND_ENV_VARS: [&str; 3] = ["MEND_MODEL", "MEND_ENDPOINT", "MEND_TOKEN_BUDGET"];

/// Outcome of building and running a probe: whether it built-and-exited-0, plus
/// the combined stdout+stderr for diagnostics.
struct Probe {
    ran_ok: bool,
    output: String,
}

/// Build and run a probe crate whose `main.rs` is `main_src`, against the real
/// `mend` crate via a path dependency. The probe runs with its current
/// directory set to its own scratch dir; if `toml` is `Some`, that text is
/// written to `mend.toml` there first, so `Config::load`'s working-directory
/// lookup sees exactly it (and nothing when `None`). The three `MEND_*` vars are
/// removed from the child environment, then each pair in `envs` is set, so the
/// env the probe observes is precisely what the criterion specifies.
///
/// A standalone `[workspace]` keeps the probe from inheriting any ambient
/// workspace; an isolated target dir keeps it off the outer build lock;
/// `--offline` keeps it off the network. In the red state `mend` has no
/// `Config` type, so the probe fails to resolve and `ran_ok` is false. Once the
/// `mend` library exposes the API the probe references, it builds, runs, and
/// prints its success sentinel.
fn run_probe(tag: &str, main_src: &str, toml: Option<&str>, envs: &[(&str, &str)]) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_config_probe\"\n\
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

    // The ambient configuration file `Config::load` reads from the working
    // directory — present and exactly `text`, or absent entirely.
    if let Some(text) = toml {
        std::fs::write(crate_dir.join("mend.toml"), text).expect("write probe mend.toml");
    }

    let mut cmd = Command::new(cargo_bin());
    cmd.current_dir(&crate_dir)
        .env("CARGO_TARGET_DIR", &target_dir);
    for var in MEND_ENV_VARS {
        cmd.env_remove(var);
    }
    for (key, value) in envs {
        cmd.env(key, value);
    }

    let out = cmd
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

/// A valid `mend.toml` with values deliberately distinct from any plausible
/// default, so a probe that reads it can tell file values apart from defaults.
const VALID_FILE: &str = "\
model = \"file-model\"
endpoint = \"https://file.endpoint\"
token_budget = 4242
";

/// C1 — `Config::load` reads `model`, `endpoint`, and `token_budget` from the
/// working-directory `mend.toml`. With no env vars set, each field must equal
/// the file's value exactly; the distinct values pin all three fields and, by
/// differing from defaults, prove the file was actually read.
#[test]
fn c1_loads_fields_from_file() {
    let src = r#"
fn main() {
    let cfg = mend::Config::load().expect("load should succeed with a valid mend.toml");
    assert_eq!(cfg.model, "file-model", "model must come from mend.toml");
    assert_eq!(cfg.endpoint, "https://file.endpoint", "endpoint must come from mend.toml");
    assert_eq!(cfg.token_budget, 4242, "token_budget must come from mend.toml");
    println!("C1_OK");
}
"#;
    let p = run_probe("c1", src, Some(VALID_FILE), &[]);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "Config::load must read model/endpoint/token_budget from mend.toml; probe output:\n{}",
        p.output
    );
}

/// C2 — each `MEND_*` env var overrides the corresponding file value. The file
/// supplies one set of values and the env another; every field must end up with
/// its env value. Distinct per-field values mean a dropped, copied, or swapped
/// override is caught, and pairing this with C1 (env unset → file wins) pins
/// both sides of the env-vs-file precedence for all three fields.
#[test]
fn c2_env_overrides_file() {
    let src = r#"
fn main() {
    let cfg = mend::Config::load().expect("load should succeed with file + env present");
    assert_eq!(cfg.model, "env-model", "MEND_MODEL must override the file's model");
    assert_eq!(cfg.endpoint, "https://env.endpoint", "MEND_ENDPOINT must override the file's endpoint");
    assert_eq!(cfg.token_budget, 9999, "MEND_TOKEN_BUDGET must override the file's token_budget");
    println!("C2_OK");
}
"#;
    let envs = [
        ("MEND_MODEL", "env-model"),
        ("MEND_ENDPOINT", "https://env.endpoint"),
        ("MEND_TOKEN_BUDGET", "9999"),
    ];
    let p = run_probe("c2", src, Some(VALID_FILE), &envs);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "MEND_* env vars must override the corresponding mend.toml values; probe output:\n{}",
        p.output
    );
}

/// C3 — a missing `mend.toml`, with no env vars, yields populated defaults and
/// no error. `load` must return `Ok`, and every field must be populated: model
/// and endpoint non-empty, token_budget non-zero. This pins the
/// missing-file-is-not-an-error branch and that defaults are real values, not
/// blanks.
#[test]
fn c3_missing_file_yields_defaults() {
    let src = r#"
fn main() {
    let cfg = mend::Config::load().expect("a missing mend.toml must yield defaults, not an error");
    assert!(!cfg.model.trim().is_empty(), "default model must be populated");
    assert!(!cfg.endpoint.trim().is_empty(), "default endpoint must be populated");
    assert!(cfg.token_budget > 0, "default token_budget must be populated (non-zero)");
    println!("C3_OK");
}
"#;
    let p = run_probe("c3", src, None, &[]);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "a missing mend.toml must yield populated defaults without error; probe output:\n{}",
        p.output
    );
}

/// C4 (file) — a non-integer `token_budget` in `mend.toml` yields
/// `MendError::Config`. The probe matches the error variant exactly: it must be
/// `Config`, not some other error and not `Ok`.
#[test]
fn c4_non_integer_budget_in_file_is_config_error() {
    let bad_file = "\
model = \"m\"
endpoint = \"e\"
token_budget = \"not-an-integer\"
";
    let src = r#"
fn main() {
    match mend::Config::load() {
        Err(mend::MendError::Config(_)) => println!("C4_FILE_OK"),
        Err(e) => panic!("expected MendError::Config for a non-integer file token_budget, got: {}", e),
        Ok(_) => panic!("expected MendError::Config for a non-integer file token_budget, but load succeeded"),
    }
}
"#;
    let p = run_probe("c4-file", src, Some(bad_file), &[]);
    assert!(
        p.ran_ok && p.output.contains("C4_FILE_OK"),
        "a non-integer token_budget in mend.toml must yield MendError::Config; probe output:\n{}",
        p.output
    );
}

/// C4 (env) — a non-integer `MEND_TOKEN_BUDGET` yields `MendError::Config`, even
/// when the file's own `token_budget` is a perfectly valid integer. This pins
/// that the malformed-integer check applies to the env override too, and that
/// the failure surfaces as `Config` rather than being silently ignored.
#[test]
fn c4_non_integer_budget_in_env_is_config_error() {
    let src = r#"
fn main() {
    match mend::Config::load() {
        Err(mend::MendError::Config(_)) => println!("C4_ENV_OK"),
        Err(e) => panic!("expected MendError::Config for a non-integer env token_budget, got: {}", e),
        Ok(_) => panic!("expected MendError::Config for a non-integer env token_budget, but load succeeded"),
    }
}
"#;
    let envs = [("MEND_TOKEN_BUDGET", "not-an-integer")];
    let p = run_probe("c4-env", src, Some(VALID_FILE), &envs);
    assert!(
        p.ran_ok && p.output.contains("C4_ENV_OK"),
        "a non-integer MEND_TOKEN_BUDGET must yield MendError::Config; probe output:\n{}",
        p.output
    );
}
