//! Frozen acceptance tests for T-07.03 — Run fix command.
//!
//! The task adds the repair subcommand entry point. Its contract is behavioural
//! (gather context → build the *fix* prompt, embedding the supplied `--error`
//! message when one is given and instructing the model to *infer* the defect when
//! it is absent → call the Completer → parse the response → route the parsed edits
//! through the output dispatcher exactly as the `edit` command does), so — exactly
//! as the T-07.01 `run_explain` and T-07.02 `run_edit` tests do — the honest way
//! to observe it is to *compile and run a tiny probe program against the real
//! `mend` crate*. Each probe drives the real entry point with a
//! [`mend::MockCompleter`], so the whole flow runs deterministically and without a
//! network.
//!
//! This test crate stays std-only and never names the not-yet-existing entry
//! point directly, so it always *compiles*. Redness comes from each probe failing
//! to *build* (the `run_fix` function does not exist yet), not from this file
//! failing to parse. Every probe is built into an isolated `CARGO_TARGET_DIR` and
//! run `--offline`, so it neither contends with the outer build lock nor touches
//! the network.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!
//!   fn run_fix(
//!       path: &Path,
//!       error: Option<&str>,
//!       config: &mend::Config,
//!       completer: &dyn mend::Completer,
//!       mode: mend::OutputMode,
//!       color: bool,
//!   ) -> mend::Result<String>
//!
//! It reads the target file, assembles its context, builds the fix prompt —
//! `fix_prompt(context, error)` when an error message is supplied, and
//! `fix_prompt(context, <inference instruction>)` when it is absent — asks the
//! Completer to complete it, parses the response into a `Vec<ParsedEdit>`, applies
//! them, and routes the outcome through `dispatch_output(path, original, applied,
//! edits, mode, color)` — returning exactly what the dispatcher returns, identical
//! to the `edit` command.
//!
//!   C1: builds the fix prompt embedding the `--error` message when provided.
//!   C2: absent `--error`, the prompt instructs the model to infer the defect from
//!       the file.
//!   C3: parsed edits route through output dispatch identically to the edit command.
//!
//! The exact inference instruction the absent-error path embeds (criterion C2) is
//! pinned here so the contract is deterministic; the green implementation must
//! emit precisely this string.
//!
//!   const INFER_INSTRUCTION: &str =
//!       "No specific error was provided; infer the defect from the file and fix it.";

use std::path::PathBuf;
use std::process::Command;

/// The exact instruction the absent-`--error` path passes to `fix_prompt`. Pinned
/// so the C2 contract is byte-deterministic and the green phase has no latitude to
/// drift the wording.
const INFER_INSTRUCTION: &str =
    "No specific error was provided; infer the defect from the file and fix it.";

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
    dir.push(format!("mend-fix-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Outcome of building and running a probe: whether it built-and-exited-0, plus
/// stdout and stderr captured *separately*.
struct Probe {
    ran_ok: bool,
    stdout: String,
    stderr: String,
}

impl Probe {
    /// stdout and stderr concatenated, for `contains` checks that don't care
    /// which stream carried the text.
    fn combined(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

/// Build and run a probe crate whose `main.rs` is `main_src`, against the real
/// `mend` crate via a path dependency. A standalone `[workspace]` keeps the probe
/// from inheriting any ambient workspace; an isolated target dir keeps it off the
/// outer build lock; `--offline` keeps it off the network. The probe runs with its
/// own crate dir as the working directory, so it can read and write a target file
/// by a bare relative path.
///
/// In the red state `mend` has no `run_fix`, so the probe fails to resolve and
/// `ran_ok` is false. Once the library defines the entry point the probe
/// references, it builds, runs, and prints its sentinel.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_fix_probe\"\n\
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

    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let ran_ok = out.status.success();

    let _ = std::fs::remove_dir_all(&crate_dir);
    let _ = std::fs::remove_dir_all(&target_dir);

    Probe {
        ran_ok,
        stdout,
        stderr,
    }
}

/// C1 — builds the fix prompt embedding the `--error` message when provided.
///
/// The probe writes a target file carrying an importable `use` and a public `fn`,
/// drives `run_fix` in `DryRun` with a `MockCompleter` and a concrete `--error`
/// message (`Some(...)`), and checks:
///   * the Completer is called exactly once — one round-trip;
///   * that one prompt equals `fix_prompt(assemble_context(structure, imports,
///     budget), error)` — so context really was gathered and the *fix* prompt with
///     that error really was built;
///   * the prompt embeds the supplied error message verbatim (the criterion's
///     literal requirement), with leading/trailing spaces and punctuation intact so
///     any trimming/escaping would be caught;
///   * the prompt embeds the file's structure (the `fn`) and imports, proving the
///     context was assembled rather than discarded.
/// A mutant that built the prompt from the inference instruction instead of the
/// error, or dropped the error, would not contain the error text and would fail.
#[test]
fn c1_with_error_embeds_error_in_fix_prompt() {
    let src = r##"
use std::path::Path;

fn main() {
    let source = "use std::collections::BTreeMap;\n\npub fn greet(name: &str) -> String { String::new() }\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("m"),
        endpoint: String::from("e"),
        token_budget: 100000,
    };

    // A concrete compiler-style error, with surrounding spaces and punctuation
    // that any non-verbatim handling would corrupt.
    let error = "  error[E0308]: mismatched types: expected `String`, found `()`  ";
    // A fenced whole-file response so the round-trip reaches Ok and we can inspect
    // the recorded prompt.
    let response = "```rust\npub fn greet() -> String { String::from(\"hi\") }\n```\n";
    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    mend::run_fix(
        Path::new("target.rs"),
        Some(error),
        &config,
        &completer,
        mend::OutputMode::DryRun,
        false,
    )
    .expect("run_fix should succeed when an error is supplied and the response parses");

    let prompts = completer.prompts();
    assert_eq!(prompts.len(), 1, "the completer is called exactly once — one round-trip");

    // The prompt is exactly fix_prompt(context, error): context gathered, fix
    // prompt built, the supplied error embedded.
    let structure = mend::extract_structure(source).expect("structure");
    let imports = mend::extract_imports(source).expect("imports");
    let context = mend::assemble_context(&structure, &imports, config.token_budget as usize);
    assert_eq!(
        prompts[0],
        mend::fix_prompt(&context, error),
        "the prompt is fix_prompt(context, error) — the --error message is embedded"
    );

    // And the error message itself is present verbatim.
    assert!(
        prompts[0].contains(error),
        "the prompt embeds the supplied --error message unchanged"
    );
    // The inference instruction must NOT be used when an error is provided.
    assert!(
        !prompts[0].contains("No specific error was provided"),
        "the inference instruction is absent when an explicit --error is given"
    );
    // Context really was assembled into the prompt.
    assert!(prompts[0].contains("greet"), "prompt embeds the file's structure (the fn)");
    assert!(
        prompts[0].contains("std::collections::BTreeMap"),
        "prompt embeds the file's imports"
    );

    eprintln!("C1_OK");
}
"##;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.combined().contains("C1_OK"),
        "fix must build the fix prompt embedding the --error message when provided; \
         probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// C2 — absent `--error`, the prompt instructs the model to infer the defect.
///
/// The probe drives `run_fix` with `error = None` and checks:
///   * the Completer is called exactly once;
///   * the prompt equals `fix_prompt(context, INFER_INSTRUCTION)` — the absent
///     error degrades to a fixed inference instruction, not a failure and not an
///     empty error;
///   * the prompt literally instructs the model to *infer the defect from the
///     file* (the criterion's behaviour);
///   * the prompt still embeds the assembled context (the `fn`), so inference is
///     grounded in the file.
/// To prove the error branch genuinely steers the prompt (rather than the two arms
/// collapsing to one), it then runs `run_fix` again with a concrete `Some(error)`
/// over the same file and asserts that prompt carries the error verbatim, omits the
/// inference instruction, and differs from the None-case prompt.
#[test]
fn c2_without_error_prompt_instructs_inference() {
    let src = r##"
use std::path::Path;

fn main() {
    let source = "use std::collections::BTreeMap;\n\npub fn greet() {}\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("m"),
        endpoint: String::from("e"),
        token_budget: 100000,
    };

    let infer_instruction = "No specific error was provided; infer the defect from the file and fix it.";
    let response = "```rust\npub fn greet() { let _ = 1; }\n```\n";

    // --- No error supplied: the prompt must instruct inference. ---
    let completer = mend::MockCompleter::new(vec![String::from(response)]);
    mend::run_fix(
        Path::new("target.rs"),
        None,
        &config,
        &completer,
        mend::OutputMode::DryRun,
        false,
    )
    .expect("run_fix should succeed (degrade to inference) when no error is supplied");

    let prompts = completer.prompts();
    assert_eq!(prompts.len(), 1, "the completer is called exactly once — one round-trip");

    let structure = mend::extract_structure(source).expect("structure");
    let imports = mend::extract_imports(source).expect("imports");
    let context = mend::assemble_context(&structure, &imports, config.token_budget as usize);

    // The absent-error prompt is exactly fix_prompt(context, INFER_INSTRUCTION).
    assert_eq!(
        prompts[0],
        mend::fix_prompt(&context, infer_instruction),
        "absent --error, the prompt is fix_prompt(context, <inference instruction>)"
    );
    // And it literally instructs the model to infer the defect from the file.
    assert!(
        prompts[0].contains("infer the defect from the file"),
        "the prompt instructs the model to infer the defect from the file"
    );
    // Inference is still grounded in the assembled context.
    assert!(prompts[0].contains("greet"), "the inference prompt still embeds the file context");

    // --- A concrete error supplied: the branch must actually diverge. ---
    let error = "error[E0277]: the trait bound is not satisfied";
    let completer2 = mend::MockCompleter::new(vec![String::from(response)]);
    mend::run_fix(
        Path::new("target.rs"),
        Some(error),
        &config,
        &completer2,
        mend::OutputMode::DryRun,
        false,
    )
    .expect("run_fix with an explicit error should succeed");
    let with_error = completer2.prompts()[0].clone();
    assert!(
        with_error.contains(error),
        "the with-error prompt carries the error verbatim"
    );
    assert!(
        !with_error.contains("infer the defect from the file"),
        "the with-error prompt does NOT carry the inference instruction"
    );
    assert!(
        with_error != prompts[0],
        "the with-error and absent-error prompts differ — the --error branch steers the prompt"
    );

    eprintln!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.combined().contains("C2_OK"),
        "absent --error, fix must instruct the model to infer the defect from the file; \
         probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
    // Sanity: the pinned inference instruction the test depends on is the one named
    // in this file's contract, so a green drift in the wording is caught here too.
    assert!(
        INFER_INSTRUCTION.contains("infer the defect from the file"),
        "the pinned inference instruction instructs inference"
    );
}

/// C3 — parsed edits route through output dispatch identically to the edit command
/// (observed in `DryRun`).
///
/// The probe drives `run_fix` in `DryRun` with a concrete error and a fenced
/// whole-file edit, then pins that the returned text equals what
/// `dispatch_output(path, original, apply_edits(original, parse_response(response)),
/// parsed, DryRun, false)` returns — exactly the routing `run_edit` performs. It
/// also checks the dry-run diff is non-empty (the edit genuinely flowed) and that
/// `DryRun` left the target file byte-for-byte unchanged. A mutant that parsed
/// nothing, dropped the edits, or skipped the dispatcher would not reproduce the
/// dispatcher's own output and would fail.
#[test]
fn c3_dryrun_routes_parsed_edits_through_dispatch() {
    let src = r##"
use std::path::Path;

fn main() {
    let source = "use std::collections::BTreeMap;\n\npub fn greet(name: &str) -> String { String::new() }\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("m"),
        endpoint: String::from("e"),
        token_budget: 100000,
    };

    let response = "```rust\npub fn greet() -> String { String::from(\"fixed\") }\n```\n";
    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    let out = mend::run_fix(
        Path::new("target.rs"),
        Some("error[E0308]: mismatched types"),
        &config,
        &completer,
        mend::OutputMode::DryRun,
        false,
    )
    .expect("run_fix DryRun should succeed when the response parses and applies");

    // The response was parsed and the parsed edits routed through dispatch: the
    // returned text equals the dispatcher's own output over those edits — the same
    // contract as the edit command.
    let edits = mend::parse_response(response).expect("parse");
    let applied = mend::apply_edits(source, &edits);
    let expected_out = mend::dispatch_output(
        Path::new("target.rs"),
        source,
        applied,
        edits,
        mend::OutputMode::DryRun,
        false,
    )
    .expect("dispatch");
    assert_eq!(
        out, expected_out,
        "fix routes the parsed edits through output dispatch (DryRun diff), identical to edit"
    );
    assert!(!out.is_empty(), "the dry-run diff is non-empty — the edit actually flowed");

    // DryRun must not touch the target file.
    let after = std::fs::read_to_string("target.rs").expect("read back target");
    assert_eq!(after, source, "DryRun must leave the target file untouched");

    eprintln!("C3_DRYRUN_OK");
}
"##;
    let p = run_probe("c3dry", src);
    assert!(
        p.ran_ok && p.combined().contains("C3_DRYRUN_OK"),
        "fix must route the parsed edits through output dispatch identically to edit (DryRun); \
         probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// C3 — the routed edits reach output dispatch end-to-end (observed in `Apply`).
///
/// The probe drives `run_fix` in `Apply` — here with `error = None`, so the
/// inference path also flows through the identical dispatch contract — with a
/// fenced whole-file edit, and pins that the dispatcher actually wrote the applied
/// edits to disk: the returned text equals `apply_edits(original,
/// parse_response(response))`, the file on disk now equals that applied text, and
/// the file genuinely changed from its original. A mutant that parsed nothing,
/// dropped the edits, or skipped the dispatcher would leave the file unchanged and
/// fail.
#[test]
fn c3_apply_routes_edits_to_disk() {
    let src = r##"
use std::path::Path;

fn main() {
    let source = "use std::collections::BTreeMap;\n\npub fn greet(name: &str) -> String { String::new() }\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("m"),
        endpoint: String::from("e"),
        token_budget: 100000,
    };

    let response = "```rust\npub fn greet() -> String { String::from(\"repaired\") }\n```\n";
    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    let out = mend::run_fix(
        Path::new("target.rs"),
        None,
        &config,
        &completer,
        mend::OutputMode::Apply,
        false,
    )
    .expect("run_fix Apply should succeed");

    // Apply returns the applied (proposed) text...
    let edits = mend::parse_response(response).expect("parse");
    let proposed = mend::apply_edits(source, &edits).expect("apply");
    assert_eq!(out, proposed, "Apply returns the applied text");

    // ...and the dispatcher wrote those routed edits to disk — the same output
    // contract the edit command uses.
    let after = std::fs::read_to_string("target.rs").expect("read back target");
    assert_eq!(after, proposed, "Apply routes edits to dispatch, which writes them to disk");
    assert!(after != source, "the file was actually changed by the routed edit");

    eprintln!("C3_APPLY_OK");
}
"##;
    let p = run_probe("c3apply", src);
    assert!(
        p.ran_ok && p.combined().contains("C3_APPLY_OK"),
        "fix must route the parsed edits through output dispatch (writing them to disk under \
         Apply); probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}
