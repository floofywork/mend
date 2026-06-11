//! Frozen acceptance tests for T-08.03 — Gate live integration test.
//!
//! The deliverable here is not a Rust *type* but a build-and-test arrangement:
//! a manually-run integration test that exercises the real `HttpCompleter`
//! against a real endpoint, fenced behind an opt-in Cargo feature so that the
//! default `cargo test` run never compiles it and therefore never touches the
//! network. The criteria are accordingly observed two ways:
//!
//!   - structurally, by reading `Cargo.toml` and the project's own test/source
//!     files (the `live` feature must be declared and off by default; a
//!     `#[cfg(feature = "live")]` test must drive `HttpCompleter`); and
//!   - behaviourally, by shelling out to `cargo` exactly as a user would and
//!     observing that the live test compiles only when `--features live` is
//!     passed and is absent from the default test run.
//!
//! Every `cargo` invocation builds into an isolated `CARGO_TARGET_DIR` and runs
//! `--offline`, so it neither contends with the outer build lock nor reaches the
//! crate network; and we only ever *list* the live test (`-- --list`), never run
//! it, so these acceptance tests themselves stay network-free.
//!
//!   C1: a `#[cfg(feature = "live")]` test exercises `HttpCompleter` against a
//!       real endpoint.
//!   C2: the live test is excluded from the default `cargo test` run.
//!   C3: the `live` feature is declared in Cargo.toml and is off by default.

use std::path::PathBuf;
use std::process::Command;

/// This acceptance-test file's own name, excluded from source scans so its own
/// string literals never masquerade as the implementation under test.
const SELF_FILE: &str = "live_gate.rs";

/// The directory holding the project's `Cargo.toml`.
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
    dir.push(format!("mend-livegate-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Body of a named `[table]`, up to the next table header. Minimal,
/// dependency-free TOML slicing — enough to read the `[features]` table.
fn table_section(toml: &str, table: &str) -> Option<String> {
    let header = format!("[{table}]");
    let mut in_table = false;
    let mut out = String::new();
    for line in toml.lines() {
        let t = line.trim_start();
        if t.starts_with('[') {
            in_table = t.trim_end() == header;
            continue;
        }
        if in_table {
            out.push_str(line);
            out.push('\n');
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// The raw right-hand side of a `key = ...` assignment within a table body, with
/// any inline comment stripped. `None` if the key is not present.
fn feature_value(section: &str, key: &str) -> Option<String> {
    for line in section.lines() {
        let t = line.split('#').next().unwrap_or("").trim();
        if let Some(rest) = t.strip_prefix(key) {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                return Some(rest.trim().to_string());
            }
        }
    }
    None
}

/// Whether the comma-separated array literal `value` (e.g. `["a", "b"]`)
/// contains the token `name` as one of its string entries.
fn array_contains(value: &str, name: &str) -> bool {
    value
        .trim_matches(['[', ']'])
        .split(',')
        .map(|e| e.trim().trim_matches(['"', '\'']))
        .any(|e| e == name)
}

/// Read every `*.rs` file under `dir` (non-recursively), skipping this very
/// acceptance-test file, and return their concatenated contents tagged by name.
fn rust_sources(dir: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let path = manifest_dir().join(dir);
    let entries = match std::fs::read_dir(&path) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        if !name.ends_with(".rs") || name == SELF_FILE {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&p) {
            out.push((name, text));
        }
    }
    out
}

/// Outcome of a `cargo test -- --list`: whether it succeeded, plus the set of
/// test function names it listed.
struct Listing {
    ok: bool,
    names: Vec<String>,
    output: String,
}

/// Run `cargo test [extra...] --offline -- --list` against the real project in an
/// isolated target dir, parsing the `name: test` lines libtest prints.
fn list_tests(extra: &[&str]) -> Listing {
    let target = unique_temp_dir("list-target");
    let mut args: Vec<String> = vec!["test".into(), "--offline".into()];
    for e in extra {
        args.push((*e).to_string());
    }
    args.push("--".into());
    args.push("--list".into());

    let out = Command::new(cargo_bin())
        .current_dir(manifest_dir())
        .env("CARGO_TARGET_DIR", &target)
        .args(&args)
        .output()
        .expect("failed to spawn `cargo test -- --list`");

    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let ok = out.status.success();
    let mut names = Vec::new();
    for line in output.lines() {
        let t = line.trim();
        if let Some(name) = t.strip_suffix(": test") {
            names.push(name.to_string());
        }
    }
    let _ = std::fs::remove_dir_all(&target);
    Listing { ok, names, output }
}

/// C1 — a `#[cfg(feature = "live")]` test exercises `HttpCompleter` against a
/// real endpoint.
///
/// Observed two ways that must both hold. Structurally: some project source file
/// (other than this one) gates a `#[test]` behind `#[cfg(feature = "live")]` and
/// drives `HttpCompleter` — i.e. the test that talks to the real backend exists
/// and is feature-fenced. Behaviourally: the whole test suite compiles under
/// `--features live`, proving that gated test builds against the real
/// `HttpCompleter` API rather than merely appearing in a comment. In the red
/// state neither holds: the feature does not exist (so the `--features live`
/// build errors) and no gated test references `HttpCompleter`.
#[test]
fn c1_live_test_gated_exercises_httpcompleter() {
    // Structural: find a feature-fenced test that drives HttpCompleter.
    let mut found_gated = false;
    for (_name, text) in rust_sources("tests").into_iter().chain(rust_sources("src")) {
        let squashed: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        let gated = squashed.contains("#[cfg(feature=\"live\")]");
        if gated && text.contains("HttpCompleter") && text.contains("#[test]") {
            found_gated = true;
            break;
        }
    }
    assert!(
        found_gated,
        "a #[cfg(feature = \"live\")] #[test] that exercises `HttpCompleter` must exist \
         in the project's tests/ or src/"
    );

    // Behavioural: the gated test must actually compile against the real API.
    let target = unique_temp_dir("c1-target");
    let out = Command::new(cargo_bin())
        .current_dir(manifest_dir())
        .env("CARGO_TARGET_DIR", &target)
        .args(["test", "--no-run", "--offline", "--features", "live"])
        .output()
        .expect("failed to spawn `cargo test --no-run --features live`");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let built = out.status.success();
    let _ = std::fs::remove_dir_all(&target);

    assert!(
        built,
        "`cargo test --no-run --features live` must compile the gated live test; output:\n{combined}"
    );
}

/// C2 — the live test is excluded from the default `cargo test` run.
///
/// The default test run and the `--features live` run are each enumerated with
/// `-- --list` (which builds but never executes, so no network is touched). The
/// live-gated test is precisely the set difference: a test present only when the
/// feature is on. That difference must be non-empty (the gated test exists) and
/// none of its members may appear in the default listing (it is excluded by
/// default). In the red state the `--features live` listing fails to build, so
/// there is no such test and the assertion fails.
#[test]
fn c2_live_test_excluded_from_default_run() {
    let default_run = list_tests(&[]);
    assert!(
        default_run.ok,
        "the default `cargo test -- --list` must succeed; output:\n{}",
        default_run.output
    );

    let live_run = list_tests(&["--features", "live"]);
    assert!(
        live_run.ok,
        "`cargo test --features live -- --list` must succeed once the live feature exists; output:\n{}",
        live_run.output
    );

    let gated: Vec<&String> = live_run
        .names
        .iter()
        .filter(|n| !default_run.names.contains(n))
        .collect();

    assert!(
        !gated.is_empty(),
        "at least one test must be gated behind the live feature (present with \
         --features live, absent by default); default listed {:?}, live listed {:?}",
        default_run.names,
        live_run.names
    );
    for name in &gated {
        assert!(
            !default_run.names.contains(name),
            "the live-gated test `{name}` must be excluded from the default `cargo test` run"
        );
    }
}

/// C3 (declared) — the `live` feature is declared in `Cargo.toml`.
///
/// Reads the `[features]` table and requires a `live` key. In the red state
/// there is no `[features]` table at all, so this fails.
#[test]
fn c3_live_feature_declared_in_cargo_toml() {
    let manifest = manifest_dir().join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest)
        .unwrap_or_else(|e| panic!("Cargo.toml must exist at {}: {e}", manifest.display()));

    let features =
        table_section(&text, "features").expect("Cargo.toml must contain a [features] table");
    assert!(
        feature_value(&features, "live").is_some(),
        "the `live` feature must be declared in the [features] table"
    );
}

/// C3 (off by default) — the declared `live` feature is not enabled by default.
///
/// Requires the feature to be declared (the same precondition as the criterion)
/// and then requires that any `default = [...]` list does not include `live`. A
/// missing `default` key means nothing is on by default, which also satisfies
/// "off by default"; a `default` that lists `live` violates it. In the red state
/// the feature is not declared, so the precondition already fails.
#[test]
fn c3_live_feature_off_by_default() {
    let manifest = manifest_dir().join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest)
        .unwrap_or_else(|e| panic!("Cargo.toml must exist at {}: {e}", manifest.display()));

    let features =
        table_section(&text, "features").expect("Cargo.toml must contain a [features] table");
    assert!(
        feature_value(&features, "live").is_some(),
        "the `live` feature must be declared before it can be off by default"
    );

    if let Some(default) = feature_value(&features, "default") {
        assert!(
            !array_contains(&default, "live"),
            "the `live` feature must be OFF by default, but default = {default} enables it"
        );
    }
}
