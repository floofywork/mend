//! Frozen acceptance tests for T-01.02 — Implement mock Completer.
//!
//! The criteria are about a Rust API and its *runtime* behaviour: a
//! `MockCompleter` that replays scripted responses in FIFO order, errors when
//! the script is exhausted, records the prompts it receives, and implements the
//! `Completer` trait. Exactly as the T-00.02 error, T-00.03 CLI, and T-01.01
//! trait tests do, the honest way to observe this is to *compile and run a tiny
//! probe program against the real `mend` crate*. The probes pin the contract the
//! criteria imply; this test crate stays std-only and never references the
//! not-yet-existing API directly, so it always compiles. Redness comes from each
//! probe failing to *build* (the `MockCompleter` type does not exist yet), not
//! from this file failing to parse. Each probe is built into an isolated
//! `CARGO_TARGET_DIR` and run `--offline`, so it neither contends with the outer
//! build lock nor touches the network — matching the task's "no network access
//! of any kind" fence.
//!
//! Unlike the bare-trait task, this task *does* construct the type, so the probes
//! exercise behaviour at runtime: they build a `MockCompleter`, call `complete`,
//! and assert on the values returned, the error variant on exhaustion, and the
//! recorded prompt log. A probe that builds but whose runtime assertions fail
//! exits non-zero, so `ran_ok` is false and the test fails just as loudly as a
//! build failure would.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!   - `mend::MockCompleter::new(Vec<String>) -> MockCompleter`   (scripted responses)
//!   - `complete(&self, &str) -> mend::Result<String>`            (the Completer method)
//!   - `MockCompleter::prompts(&self) -> Vec<String>`             (the recording accessor)
//!   - `MockCompleter: mend::Completer`                           (trait impl + object safety)
//!
//!   C1: successive `complete` calls return the scripted responses in FIFO order.
//!   C2: a `complete` call past the end of the script returns `MendError::Completer`.
//!   C3: every received prompt is recorded, in order, behind a `prompts()` accessor.
//!   C4: `MockCompleter` implements `Completer`.

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
    dir.push(format!("mend-mockcompleter-{tag}-{}-{nanos}", std::process::id()));
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
/// In the red state `mend` has no `MockCompleter`, so the probe fails to resolve
/// and `ran_ok` is false. Once the `mend` library defines the type the probe
/// references, it builds, runs its runtime assertions, and prints its success
/// sentinel.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_mockcompleter_probe\"\n\
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

/// C1 — successive `complete` calls return the scripted responses in FIFO order.
///
/// The probe scripts three distinct responses, calls `complete` three times, and
/// asserts the returned values match the script in first-in-first-out order. This
/// only passes if `new` stores the responses and `complete` hands them out one
/// per call, oldest first.
#[test]
fn c1_responses_replayed_in_fifo_order() {
    let src = r#"
fn main() {
    let m = mend::MockCompleter::new(vec![
        String::from("first"),
        String::from("second"),
        String::from("third"),
    ]);

    let a = m.complete("p1").expect("1st scripted call should succeed");
    let b = m.complete("p2").expect("2nd scripted call should succeed");
    let c = m.complete("p3").expect("3rd scripted call should succeed");

    assert_eq!(a, "first", "1st call must return the 1st scripted response");
    assert_eq!(b, "second", "2nd call must return the 2nd scripted response");
    assert_eq!(c, "third", "3rd call must return the 3rd scripted response");

    println!("C1_OK");
}
"#;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.output.contains("C1_OK"),
        "MockCompleter::new(responses) must replay them in FIFO order across complete calls; probe output:\n{}",
        p.output
    );
}

/// C2 — a `complete` call past the end of the script returns `MendError::Completer`.
///
/// The probe scripts a single response, consumes it, then calls `complete` once
/// more. Exhaustion must be a deterministic `MendError::Completer` value, not a
/// panic and not any other variant: the probe pattern-matches the exact variant
/// and panics (exiting non-zero) on anything else.
#[test]
fn c2_exhausting_the_script_returns_completer_error() {
    let src = r#"
fn main() {
    let m = mend::MockCompleter::new(vec![String::from("only")]);

    let _ = m.complete("p1").expect("the one scripted call should succeed");

    match m.complete("p2") {
        Err(mend::MendError::Completer(_)) => {}
        other => panic!(
            "calling complete past the script must return MendError::Completer, got {:?}",
            other
        ),
    }

    println!("C2_OK");
}
"#;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "exhausting the script must return MendError::Completer (deterministically, not a panic); probe output:\n{}",
        p.output
    );
}

/// C3 — every received prompt is recorded, in order, behind a `prompts()` accessor.
///
/// The probe makes three calls with distinct prompts and then reads the recorded
/// log via the accessor, asserting it equals the prompts in the order they were
/// received. This pins both that the prompts are recorded and that an accessor
/// exposes them for assertions.
#[test]
fn c3_received_prompts_are_recorded_in_order() {
    let src = r#"
fn main() {
    let m = mend::MockCompleter::new(vec![
        String::from("r1"),
        String::from("r2"),
        String::from("r3"),
    ]);

    let _ = m.complete("alpha");
    let _ = m.complete("beta");
    let _ = m.complete("gamma");

    let recorded: Vec<String> = m.prompts();
    assert_eq!(
        recorded,
        vec![
            String::from("alpha"),
            String::from("beta"),
            String::from("gamma"),
        ],
        "the mock must record every received prompt, in order, via its accessor"
    );

    println!("C3_OK");
}
"#;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "every received prompt must be recorded in order and exposed via a prompts() accessor; probe output:\n{}",
        p.output
    );
}

/// C4 — `MockCompleter` implements `Completer`.
///
/// Two independent proofs: a generic function bounded by `mend::Completer`
/// accepts a `&MockCompleter` (static dispatch), and a `MockCompleter` coerces
/// into a `Box<dyn mend::Completer>` and is callable through it (object-safe
/// dynamic dispatch). Either alone would fail to compile if the impl were
/// missing.
#[test]
fn c4_mock_completer_implements_completer() {
    let src = r#"
// Static proof: this only type-checks if MockCompleter satisfies the bound.
fn assert_is_completer<C: mend::Completer>(_: &C) {}

fn main() {
    let m = mend::MockCompleter::new(vec![String::from("x")]);
    assert_is_completer(&m);

    // Dynamic proof: coerce into a trait object and call through it.
    let boxed: Box<dyn mend::Completer> =
        Box::new(mend::MockCompleter::new(vec![String::from("y")]));
    let out = boxed.complete("p").expect("call through the trait object should succeed");
    assert_eq!(out, "y", "the trait-object call must replay the scripted response");

    println!("C4_OK");
}
"#;
    let p = run_probe("c4", src);
    assert!(
        p.ran_ok && p.output.contains("C4_OK"),
        "MockCompleter must implement the Completer trait (static and dynamic dispatch); probe output:\n{}",
        p.output
    );
}
