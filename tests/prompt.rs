//! Frozen acceptance tests for T-03.01 — Assemble command prompts.
//!
//! The criteria are about a Rust API (one pure prompt builder per subcommand over
//! a shared module, each taking the assembled context plus its per-command
//! parameter and returning a prompt `String`), so — exactly as the T-02.0x tests
//! do — the honest way to observe them is to *compile and run a tiny probe program
//! against the real `mend` crate*. The probes pin the contract the criteria imply;
//! this test crate stays std-only and never references the not-yet-existing API
//! directly, so it always compiles. Redness comes from each probe failing to
//! *build* (the prompt builders do not exist yet), not from this file failing to
//! parse. Each probe is built into an isolated `CARGO_TARGET_DIR` and run
//! `--offline`, so it neither contends with the outer build lock nor touches the
//! network.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply — four pure
//! builders over a shared module:
//!   - `mend::explain_prompt(context: &str, symbol: Option<&str>) -> String`
//!   - `mend::edit_prompt(context: &str, instruction: &str) -> String`
//!   - `mend::fix_prompt(context: &str, error: &str) -> String`
//!   - `mend::test_prompt(context: &str) -> String`
//!
//! Each builder embeds the assembled context. The explain builder additionally
//! embeds the target symbol name *when present* and omits it (without error) when
//! absent. The edit builder embeds the user instruction verbatim; the fix builder
//! embeds the supplied error message; the test builder requests tests for the
//! public API only. Every builder is a pure, deterministic function of its inputs
//! with no I/O: the same inputs always yield byte-identical output.
//!
//!   C1: the explain prompt embeds the file context and, when present, the target symbol name.
//!   C2: the edit prompt embeds the context and the user instruction verbatim.
//!   C3: the fix prompt embeds the context and the supplied error message.
//!   C4: the test prompt embeds the context and requests tests for the public API only.
//!   C5: every prompt builder is a pure function of its inputs with no I/O.

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
    dir.push(format!("mend-prompt-{tag}-{}-{nanos}", std::process::id()));
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
/// In the red state `mend` has no prompt builders, so the probe fails to resolve
/// and `ran_ok` is false. Once the `mend` library exposes the API the probe
/// references, it builds, runs, and prints its success sentinel. A probe that
/// panics (e.g. on a broken invariant) exits non-zero and prints no sentinel, so
/// a panic fails the corresponding test.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_prompt_probe\"\n\
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

/// C1 (present-symbol side) — the explain prompt embeds the file context AND, when
/// a target symbol is present, the symbol name. The probe builds the prompt with a
/// distinctively-marked context and `Some("ZZ_SYMBOL")`, then asserts BOTH markers
/// appear in the one returned string. This pins the `Some(_)` branch: a builder
/// that dropped the symbol (or the context) would fail here.
#[test]
fn c1_explain_embeds_context_and_present_symbol() {
    let src = r##"
fn main() {
    let context = "STRUCT_CTX_MARKER\nIMPORT_CTX_MARKER";
    let out = mend::explain_prompt(context, Some("ZZ_SYMBOL"));

    assert!(
        out.contains("STRUCT_CTX_MARKER") && out.contains("IMPORT_CTX_MARKER"),
        "C1: the explain prompt must embed the file context, got: {:?}",
        out
    );
    assert!(
        out.contains("ZZ_SYMBOL"),
        "C1: the explain prompt must embed the target symbol name when present, got: {:?}",
        out
    );

    println!("C1_PRESENT_OK");
}
"##;
    let p = run_probe("c1-present", src);
    assert!(
        p.ran_ok && p.output.contains("C1_PRESENT_OK"),
        "explain_prompt must embed context and the present symbol name; probe output:\n{}",
        p.output
    );
}

/// C1 (absent-symbol side) — when no symbol is supplied the explain prompt still
/// embeds the context, omits the symbol (no error), and is non-empty. The probe
/// builds the prompt twice, once with `Some("ZZ_SYMBOL")` and once with `None`, and
/// asserts: the `None` prompt still contains the context, does NOT contain the
/// symbol marker, is non-empty, and DIFFERS from the `Some` prompt. The difference
/// proves the symbol genuinely affects output (killing a builder that ignores it),
/// while the absence pins the `None` branch as omission rather than error.
#[test]
fn c1_explain_omits_symbol_when_absent() {
    let src = r##"
fn main() {
    let context = "STRUCT_CTX_MARKER\nIMPORT_CTX_MARKER";
    let with = mend::explain_prompt(context, Some("ZZ_SYMBOL"));
    let without = mend::explain_prompt(context, None);

    assert!(
        without.contains("STRUCT_CTX_MARKER") && without.contains("IMPORT_CTX_MARKER"),
        "C1: the explain prompt must embed the context even with no symbol, got: {:?}",
        without
    );
    assert!(
        !without.contains("ZZ_SYMBOL"),
        "C1: with no symbol the prompt must omit the symbol name, got: {:?}",
        without
    );
    assert!(
        !without.is_empty(),
        "C1: an absent symbol is handled by omission, not an empty/error prompt, got: {:?}",
        without
    );
    assert!(
        with != without,
        "C1: the presence of a symbol must change the prompt; with={:?} without={:?}",
        with,
        without
    );

    println!("C1_ABSENT_OK");
}
"##;
    let p = run_probe("c1-absent", src);
    assert!(
        p.ran_ok && p.output.contains("C1_ABSENT_OK"),
        "explain_prompt must omit an absent symbol without error and still embed context; probe output:\n{}",
        p.output
    );
}

/// C2 — the edit prompt embeds the context AND the user instruction verbatim. The
/// probe uses a distinctively-marked context and a multi-word instruction with
/// internal punctuation, then asserts the prompt contains the context marker and
/// the *entire* instruction string unchanged (a single `contains` of the whole
/// instruction pins "verbatim" — any rewording, escaping, or truncation breaks it).
#[test]
fn c2_edit_embeds_context_and_instruction_verbatim() {
    let src = r##"
fn main() {
    let context = "EDIT_CTX_MARKER\nmore context here";
    let instruction = "Rename `count` to `total`, and KEEP_VERBATIM_XYZ preserve all formatting.";
    let out = mend::edit_prompt(context, instruction);

    assert!(
        out.contains("EDIT_CTX_MARKER"),
        "C2: the edit prompt must embed the file context, got: {:?}",
        out
    );
    assert!(
        out.contains(instruction),
        "C2: the edit prompt must embed the user instruction verbatim, got: {:?}",
        out
    );

    println!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.output.contains("C2_OK"),
        "edit_prompt must embed context and the instruction verbatim; probe output:\n{}",
        p.output
    );
}

/// C3 — the fix prompt embeds the context AND the supplied error message. The probe
/// uses a distinctively-marked context and an error string with a distinctive
/// marker, then asserts both appear in the returned prompt.
#[test]
fn c3_fix_embeds_context_and_error() {
    let src = r##"
fn main() {
    let context = "FIX_CTX_MARKER\nmore context here";
    let error = "error[E0599]: no method named `frobnicate_ERR_MARKER` found";
    let out = mend::fix_prompt(context, error);

    assert!(
        out.contains("FIX_CTX_MARKER"),
        "C3: the fix prompt must embed the file context, got: {:?}",
        out
    );
    assert!(
        out.contains(error),
        "C3: the fix prompt must embed the supplied error message, got: {:?}",
        out
    );

    println!("C3_OK");
}
"##;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.output.contains("C3_OK"),
        "fix_prompt must embed context and the supplied error message; probe output:\n{}",
        p.output
    );
}

/// C4 — the test prompt embeds the context AND requests tests for the public API
/// only. The probe asserts the prompt contains the distinctively-marked context and
/// the literal phrase `public API`, which is how the "public API only" instruction
/// is surfaced to the backend.
#[test]
fn c4_test_embeds_context_and_requests_public_api() {
    let src = r##"
fn main() {
    let context = "TEST_CTX_MARKER\nmore context here";
    let out = mend::test_prompt(context);

    assert!(
        out.contains("TEST_CTX_MARKER"),
        "C4: the test prompt must embed the file context, got: {:?}",
        out
    );
    assert!(
        out.contains("public API"),
        "C4: the test prompt must request tests for the public API only, got: {:?}",
        out
    );

    println!("C4_OK");
}
"##;
    let p = run_probe("c4", src);
    assert!(
        p.ran_ok && p.output.contains("C4_OK"),
        "test_prompt must embed context and request tests for the public API only; probe output:\n{}",
        p.output
    );
}

/// C5 — every prompt builder is a pure, deterministic function of its inputs with
/// no I/O. The probe calls each of the four builders twice with identical inputs
/// and asserts byte-identical output every time — covering BOTH the `Some` and
/// `None` explain paths. Byte-equality across repeated calls is the observable
/// proxy for referential transparency: a builder that consulted a clock, the
/// filesystem, the environment, or any other ambient state could not guarantee it.
#[test]
fn c5_builders_are_pure_and_deterministic() {
    let src = r##"
fn main() {
    let context = "PURE_CTX_MARKER\nmore context here";

    let e1 = mend::explain_prompt(context, Some("SymName"));
    let e2 = mend::explain_prompt(context, Some("SymName"));
    assert_eq!(e1, e2, "C5: explain_prompt (Some) must be deterministic");

    let en1 = mend::explain_prompt(context, None);
    let en2 = mend::explain_prompt(context, None);
    assert_eq!(en1, en2, "C5: explain_prompt (None) must be deterministic");

    let ed1 = mend::edit_prompt(context, "do the thing");
    let ed2 = mend::edit_prompt(context, "do the thing");
    assert_eq!(ed1, ed2, "C5: edit_prompt must be deterministic");

    let f1 = mend::fix_prompt(context, "some error");
    let f2 = mend::fix_prompt(context, "some error");
    assert_eq!(f1, f2, "C5: fix_prompt must be deterministic");

    let t1 = mend::test_prompt(context);
    let t2 = mend::test_prompt(context);
    assert_eq!(t1, t2, "C5: test_prompt must be deterministic");

    println!("C5_OK");
}
"##;
    let p = run_probe("c5", src);
    assert!(
        p.ran_ok && p.output.contains("C5_OK"),
        "every prompt builder must be pure and deterministic; probe output:\n{}",
        p.output
    );
}
