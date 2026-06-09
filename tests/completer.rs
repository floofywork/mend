//! Frozen acceptance tests for T-01.01 — Define Completer trait.
//!
//! The criteria are about a Rust API (a `Completer` trait with one method, its
//! object-safety, and the borrowing in its signature), so — exactly as the
//! T-00.02 error and T-00.03 CLI tests do — the honest way to observe them is to
//! *compile and run a tiny probe program against the real `mend` crate*. The
//! probes pin the contract the criteria imply; this test crate stays std-only
//! and never references the not-yet-existing API directly, so it always
//! compiles. Redness comes from each probe failing to *build* (the `Completer`
//! trait does not exist yet), not from this file failing to parse. Each probe is
//! built into an isolated `CARGO_TARGET_DIR` and run `--offline`, so it neither
//! contends with the outer build lock nor touches the network.
//!
//! Crucially, every probe pins the contract *purely at the type level*: it names
//! the trait, takes `&dyn Completer` / `Box<dyn Completer>`, and calls the
//! method through those references — it never defines a concrete type that
//! `impl Completer`. This keeps the tests faithful to the task's out-of-scope
//! rule ("No real or mock implementation of the trait"): the green state is a
//! bare trait *definition*, with no implementation anywhere.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `trait Completer { fn complete(&self, prompt: &str) -> mend::Result<String>; }`
//!   - `Box<dyn Completer>` is a nameable type (the trait is object-safe).
//!   - the `prompt` parameter is `&str` (borrowed), so its owner outlives the call.
//!
//!   C1: a trait `Completer` exposes `fn complete(&self, prompt: &str) -> Result<String>`.
//!   C2: `Box<dyn Completer>` compiles, proving the trait is object-safe.
//!   C3: the prompt is borrowed (`&str`), not consumed, by the signature.

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
    dir.push(format!("mend-completer-{tag}-{}-{nanos}", std::process::id()));
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
/// In the red state `mend` has no `Completer` trait, so the probe fails to
/// resolve and `ran_ok` is false. Once the `mend` library defines the trait the
/// probe references, it builds, runs, and prints its success sentinel.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_completer_probe\"\n\
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

/// C1 — a trait `Completer` exposes `fn complete(&self, prompt: &str) -> Result<String>`.
///
/// The free function calls `complete` through a shared `&dyn Completer`, which
/// only type-checks if the method takes `&self`, accepts a `&str` argument, and
/// returns `mend::Result<String>` — the return type is pinned by the ascription
/// on the function's own signature. No concrete implementor is defined.
#[test]
fn c1_completer_trait_has_complete_signature() {
    let src = r#"
// Pins receiver (`&self`), argument (`&str`), and return (`mend::Result<String>`)
// by calling the method through a trait object and ascribing the return type.
fn call_complete(c: &dyn mend::Completer, prompt: &str) -> mend::Result<String> {
    c.complete(prompt)
}

fn main() {
    // Reference the function so it is fully checked, without constructing any
    // implementor of the trait.
    let _f: fn(&dyn mend::Completer, &str) -> mend::Result<String> = call_complete;
    println!("C1_OK");
}
"#;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "trait Completer must expose `fn complete(&self, prompt: &str) -> Result<String>`; probe output:\n{}",
        p.output
    );
}

/// C2 — `Box<dyn Completer>` compiles, proving the trait is object-safe.
///
/// Naming `Box<dyn Completer>` as a type alias and as a parameter type is itself
/// the proof: an object-unsafe trait cannot form a `dyn Completer`. No value is
/// constructed, so no implementor is needed.
#[test]
fn c2_completer_is_object_safe() {
    let src = r#"
// These type-level uses only resolve if `Completer` is object-safe.
type BoxedCompleter = Box<dyn mend::Completer>;

fn accepts_boxed(_: BoxedCompleter) {}

fn returns_ref(c: &dyn mend::Completer) -> &dyn mend::Completer {
    c
}

fn main() {
    let _f: fn(Box<dyn mend::Completer>) = accepts_boxed;
    let _g: fn(&dyn mend::Completer) -> &dyn mend::Completer = returns_ref;
    println!("C2_OK");
}
"#;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "`Box<dyn Completer>` must compile, proving Completer is object-safe; probe output:\n{}",
        p.output
    );
}

/// C3 — the prompt is borrowed (`&str`), not consumed, by the signature.
///
/// The probe builds an owned `String`, passes a borrow of it into `complete`,
/// and then keeps using the original owner *after* the call. If the signature
/// consumed the prompt (took `String` by value), the owner would have moved and
/// the subsequent uses would fail to compile. That they compile proves the
/// prompt is borrowed. The method is reached through `&dyn Completer`, so still
/// no implementor is defined.
#[test]
fn c3_prompt_is_borrowed_not_consumed() {
    let src = r#"
fn prompt_outlives_call(c: &dyn mend::Completer) {
    let owner: String = String::from("a prompt that must survive the call");

    // Passing `&owner` (a `&String`) deref-coerces to `&str`: the parameter is
    // a borrow, not an owned `String`.
    let _first = c.complete(&owner);

    // The owner is still fully usable here; it was not moved into `complete`.
    assert!(owner.len() > 0);

    // And it can be borrowed again for a second, independent call.
    let _second = c.complete(owner.as_str());
    assert!(owner.contains("survive"));
}

fn main() {
    let _f: fn(&dyn mend::Completer) = prompt_outlives_call;
    println!("C3_OK");
}
"#;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "the `prompt` parameter must be a borrowed `&str`, leaving its owner usable after the call; probe output:\n{}",
        p.output
    );
}
