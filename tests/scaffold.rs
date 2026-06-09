//! Frozen acceptance tests for T-00.01 — Scaffold Cargo binary crate.
//!
//! The criteria are stated in terms of `cargo`, so these tests observe them by
//! shelling out to `cargo` exactly as a user would. They are deliberately
//! std-only: the empty scaffold builds with zero dependencies, so its tests add
//! none either, and they never touch the network (`--offline`).
//!
//!   C1: `cargo build` exits 0 and emits a binary named `mend` under target/debug.
//!   C2: Cargo.toml declares package name `mend` with edition 2021 or later.
//!   C3: `cargo test` exits 0 on the unmodified scaffold with zero tests.

use std::path::PathBuf;
use std::process::Command;

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
    dir.push(format!(
        "mend-scaffold-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// The on-disk file name of a binary target with the given stem.
fn bin_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

/// Body of the `[package]` table, up to the next table header. Minimal,
/// dependency-free TOML slicing — enough to read `name`/`edition`.
fn package_section(toml: &str) -> Option<String> {
    let mut in_pkg = false;
    let mut out = String::new();
    for line in toml.lines() {
        let t = line.trim_start();
        if t.starts_with('[') {
            in_pkg = t.trim_end() == "[package]";
            continue;
        }
        if in_pkg {
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

/// Value of a `key = "value"` (or bare) assignment within a table body.
fn field(section: &str, key: &str) -> Option<String> {
    for line in section.lines() {
        // Drop any inline comment, then isolate `key = rest`.
        let t = line.split('#').next().unwrap_or("").trim();
        if let Some(rest) = t.strip_prefix(key) {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                let v = rest.trim().trim_matches(['"', '\'']).to_string();
                return Some(v);
            }
        }
    }
    None
}

/// C1 — a clean `cargo build` exits 0 and produces a binary literally named
/// `mend` under the debug profile directory. Built into an isolated target
/// directory so it neither contends with the outer test run's build lock nor
/// depends on prior build state.
#[test]
fn c1_cargo_build_emits_mend_debug_binary() {
    let target = unique_temp_dir("c1-target");
    let status = Command::new(cargo_bin())
        .current_dir(manifest_dir())
        .env("CARGO_TARGET_DIR", &target)
        .args(["build", "--offline"])
        .status()
        .expect("failed to spawn `cargo build`");

    let bin = target.join("debug").join(bin_name("mend"));
    let bin_exists = bin.exists();
    let _ = std::fs::remove_dir_all(&target);

    assert!(status.success(), "`cargo build` must exit 0; got {status}");
    assert!(
        bin_exists,
        "`cargo build` must emit a binary named `mend` under target/debug \
         (expected {})",
        bin.display()
    );
}

/// C2 — the manifest declares package name `mend` and an edition of 2021 or
/// later.
#[test]
fn c2_manifest_declares_mend_edition_2021_or_later() {
    let manifest = manifest_dir().join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest)
        .unwrap_or_else(|e| panic!("Cargo.toml must exist at {}: {e}", manifest.display()));

    let pkg = package_section(&text).expect("Cargo.toml must contain a [package] table");

    let name = field(&pkg, "name").expect("[package] must declare a name");
    assert_eq!(name, "mend", "package name must be `mend`, got `{name}`");

    let edition = field(&pkg, "edition").expect("[package] must declare an edition");
    let year: u32 = edition
        .parse()
        .unwrap_or_else(|_| panic!("edition must be a year like 2021, got `{edition}`"));
    assert!(year >= 2021, "edition must be 2021 or later, got {year}");
}

/// C3 — `cargo test` on the unmodified empty scaffold exits 0 and runs zero
/// tests. Gated on the real project already being the `mend` crate (so this is
/// red until the implementation exists), then verified against a freshly
/// generated pristine scaffold so the assertion stays valid as the real project
/// later grows its own tests.
#[test]
fn c3_cargo_test_green_with_zero_tests_on_empty_scaffold() {
    let manifest = manifest_dir().join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).expect("Cargo.toml must exist");
    let pkg = package_section(&text).expect("[package] table");
    assert_eq!(
        field(&pkg, "name").as_deref(),
        Some("mend"),
        "the scaffold must be the `mend` crate before its empty `cargo test` can be green"
    );

    // A pristine empty `mend` binary crate: manifest + an inert `main`, no tests.
    let dir = unique_temp_dir("c3-scaffold");
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"mend\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n[dependencies]\n",
    )
    .expect("write scaffold Cargo.toml");
    std::fs::create_dir_all(dir.join("src")).expect("create src dir");
    std::fs::write(dir.join("src").join("main.rs"), "fn main() {}\n").expect("write main.rs");

    let out = Command::new(cargo_bin())
        .current_dir(&dir)
        .args(["test", "--offline"])
        .output()
        .expect("failed to spawn `cargo test`");
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let success = out.status.success();
    let ran_zero = combined.contains("running 0 tests");
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        success,
        "`cargo test` on an empty scaffold must exit 0; output:\n{combined}"
    );
    assert!(
        ran_zero,
        "`cargo test` on an empty scaffold must run zero tests; output:\n{combined}"
    );
}
