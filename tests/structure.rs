//! Frozen acceptance tests for T-02.01 — Extract file structure.
//!
//! The criteria are about a Rust API (a function that turns Rust source text
//! into a list of public-item signatures), so — exactly as the T-00.02 error,
//! T-00.03 CLI, T-01.01 completer and T-01.03 config tests do — the honest way
//! to observe them is to *compile and run a tiny probe program against the real
//! `mend` crate*. The probes pin the contract the criteria imply; this test
//! crate stays std-only and never references the not-yet-existing API directly,
//! so it always compiles. Redness comes from each probe failing to *build* (the
//! `extract_structure` function does not exist yet), not from this file failing
//! to parse. Each probe is built into an isolated `CARGO_TARGET_DIR` and run
//! `--offline`, so it neither contends with the outer build lock nor touches the
//! network.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `mend::extract_structure(source: &str) -> mend::Result<Vec<String>>`
//!   - the returned `Vec<String>` is an ordered list of signature strings, one
//!     per top-level public `fn`/`struct`/`enum`/`trait`.
//!   - a signature carries the item's declaration (e.g. a fn keeps its return
//!     type) but never its body.
//!   - empty input is valid and yields an empty list; delimiter-broken input
//!     that cannot parse yields `MendError::Parse` rather than panicking.
//!
//!   C1: given Rust source, returns the signatures of every public fn/struct/enum/trait.
//!   C2: items not marked `pub` are excluded from the returned structure.
//!   C3: an empty file yields an empty structure list, not an error.
//!   C4: source that fails to parse yields `MendError::Parse`, never a panic.

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
    dir.push(format!("mend-structure-{tag}-{}-{nanos}", std::process::id()));
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
/// In the red state `mend` has no `extract_structure`, so the probe fails to
/// resolve and `ran_ok` is false. Once the `mend` library exposes the API the
/// probe references, it builds, runs, and prints its success sentinel. A probe
/// that panics (e.g. on unparseable input) exits non-zero and prints no
/// sentinel, so a panic fails the corresponding test.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_structure_probe\"\n\
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

/// C1 — given Rust source, `extract_structure` returns the signatures of every
/// top-level public `fn`, `struct`, `enum`, and `trait`. One public item of
/// each kind is present, so the result must hold exactly four signatures, one
/// mentioning each item's name. The function's signature must additionally
/// retain its return type (`bool`) — proving a *signature*, not a bare name —
/// while no signature may contain the distinctive body token (`xyzzy`), proving
/// bodies are excluded (signatures only).
#[test]
fn c1_extracts_public_item_signatures() {
    let src = r##"
fn main() {
    let source = r#"
pub fn alpha(value: i32) -> bool { let marker_xyzzy = value > 0; marker_xyzzy }
pub struct Beta;
pub enum Gamma { A, B }
pub trait Delta { fn method(&self); }
"#;
    let sigs = mend::extract_structure(source)
        .expect("valid Rust source must extract without error");
    assert_eq!(
        sigs.len(),
        4,
        "exactly the four public items must be extracted, got: {:?}",
        sigs
    );
    for name in ["alpha", "Beta", "Gamma", "Delta"] {
        assert!(
            sigs.iter().any(|s| s.contains(name)),
            "a signature for `{}` must be present, got: {:?}",
            name,
            sigs
        );
    }
    assert!(
        sigs.iter().any(|s| s.contains("alpha") && s.contains("bool")),
        "the fn signature must retain its return type, got: {:?}",
        sigs
    );
    let joined = sigs.join("\n");
    assert!(
        !joined.contains("xyzzy"),
        "signatures must exclude item bodies, got: {:?}",
        sigs
    );
    println!("C1_OK");
}
"##;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "extract_structure must return a signature for every public fn/struct/enum/trait; probe output:\n{}",
        p.output
    );
}

/// C2 — items not marked `pub` are excluded. The source pairs a public and a
/// non-pub item of each kind. Every public name must appear in the output and
/// every non-pub name must be absent, pinning both sides of the visibility
/// filter for all four kinds; the exact count of four guards against a leaked
/// private item or a dropped public one.
#[test]
fn c2_excludes_non_pub_items() {
    let src = r##"
fn main() {
    let source = r#"
pub fn pub_fn() {}
fn priv_fn() {}
pub struct PubStruct;
struct PrivStruct;
pub enum PubEnum { V }
enum PrivEnum { V }
pub trait PubTrait {}
trait PrivTrait {}
"#;
    let sigs = mend::extract_structure(source)
        .expect("valid Rust source must extract without error");
    for name in ["pub_fn", "PubStruct", "PubEnum", "PubTrait"] {
        assert!(
            sigs.iter().any(|s| s.contains(name)),
            "public `{}` must be included, got: {:?}",
            name,
            sigs
        );
    }
    let joined = sigs.join("\n");
    for name in ["priv_fn", "PrivStruct", "PrivEnum", "PrivTrait"] {
        assert!(
            !joined.contains(name),
            "non-pub `{}` must be excluded, got: {:?}",
            name,
            sigs
        );
    }
    assert_eq!(
        sigs.len(),
        4,
        "exactly the four public items must remain after filtering, got: {:?}",
        sigs
    );
    println!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "extract_structure must exclude items not marked pub; probe output:\n{}",
        p.output
    );
}

/// C3 — an empty file yields an empty structure list, not an error. `load` of
/// the empty string must return `Ok` (not `Err`) and the list must be empty.
#[test]
fn c3_empty_file_yields_empty_list() {
    let src = r##"
fn main() {
    let sigs = mend::extract_structure("")
        .expect("an empty file must yield an empty list, not an error");
    assert!(
        sigs.is_empty(),
        "an empty file must yield an empty structure list, got: {:?}",
        sigs
    );
    println!("C3_OK");
}
"##;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "an empty file must yield an empty structure list without error; probe output:\n{}",
        p.output
    );
}

/// C4 — source that fails to parse yields `MendError::Parse`, never a panic.
/// The input opens a public fn but leaves both its parameter list and its body
/// brace unterminated, so no delimiter-tracking parser can complete it. The
/// probe matches the error variant exactly: it must be `Parse`, not some other
/// error and not `Ok`. Because the match prints its sentinel only on the `Parse`
/// arm and a panic would abort the probe before any sentinel, this also pins the
/// never-panic guarantee.
#[test]
fn c4_unparseable_source_is_parse_error() {
    let src = r##"
fn main() {
    let broken = "pub fn foo( {";
    match mend::extract_structure(broken) {
        Err(mend::MendError::Parse(_)) => println!("C4_OK"),
        Err(e) => panic!("expected MendError::Parse for unparseable source, got: {}", e),
        Ok(sigs) => panic!(
            "expected MendError::Parse for unparseable source, but extraction succeeded: {:?}",
            sigs
        ),
    }
}
"##;
    let p = run_probe("c4", src);
    assert!(
        p.ran_ok && p.output.contains("C4_OK"),
        "unparseable source must yield MendError::Parse and never panic; probe output:\n{}",
        p.output
    );
}
