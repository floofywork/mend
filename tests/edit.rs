//! Frozen acceptance tests for T-07.02 — Run edit command.
//!
//! The task adds the instruction-driven mutation subcommand entry point. Its
//! contract is behavioural (gather context → build the *edit* prompt → call the
//! Completer → parse the response → route the parsed edits through the output
//! dispatcher; an unparseable response becomes a `MendError::Parse` and a non-zero
//! exit), so — exactly as the T-07.01 `run_explain` tests do — the honest way to
//! observe it is to *compile and run a tiny probe program against the real `mend`
//! crate*. Each probe drives the real entry point with a [`mend::MockCompleter`],
//! so the whole flow runs deterministically and without a network.
//!
//! This test crate stays std-only and never names the not-yet-existing entry
//! point directly, so it always *compiles*. Redness comes from each probe failing
//! to *build* (the `run_edit` function does not exist yet), not from this file
//! failing to parse. Every probe is built into an isolated `CARGO_TARGET_DIR` and
//! run `--offline`, so it neither contends with the outer build lock nor touches
//! the network.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!
//!   fn run_edit(
//!       path: &Path,
//!       instruction: &str,
//!       config: &mend::Config,
//!       completer: &dyn mend::Completer,
//!       mode: mend::OutputMode,
//!       color: bool,
//!   ) -> mend::Result<String>
//!
//! It reads the target file, assembles its context, builds `edit_prompt(context,
//! instruction)`, asks the Completer to complete it, parses the response into a
//! `Vec<ParsedEdit>`, applies them, and routes the outcome through
//! `dispatch_output(path, original, applied, edits, mode, color)` — returning
//! exactly what the dispatcher returns. An unparseable response surfaces verbatim
//! as the parser's `MendError::Parse`.
//!
//!   C1: gathers context, builds the edit prompt, parses the response, and routes
//!       the parsed edits through output dispatch.
//!   C2: the user instruction passes through to the prompt unchanged.
//!   C3: a parse failure surfaces as `MendError::Parse` with a non-zero exit.

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
    dir.push(format!("mend-edit-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Outcome of building and running a probe: whether it built-and-exited-0, plus
/// stdout and stderr captured *separately* (C3 must observe the error on stderr).
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
/// In the red state `mend` has no `run_edit`, so the probe fails to resolve and
/// `ran_ok` is false. Once the library defines the entry point the probe
/// references, it builds, runs, and prints its sentinel.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_edit_probe\"\n\
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

/// C1 — gathers context, builds the edit prompt, parses the response, and routes
/// the parsed edits through output dispatch (observed in `DryRun`).
///
/// The probe writes a target file carrying an importable `use` and a public `fn`,
/// drives `run_edit` in `DryRun` with a `MockCompleter` scripted to return a
/// fenced whole-file edit, and checks four things:
///   * the Completer is called exactly once, with the prompt the deterministic
///     layers compose — `edit_prompt(assemble_context(structure, imports, budget),
///     instruction)` — so context really was gathered and the *edit* prompt really
///     was built (it embeds both the `fn` signature and the import path);
///   * the returned text equals what `dispatch_output(path, original,
///     apply_edits(original, parse_response(response)), parsed, DryRun, false)`
///     returns — proving the response was parsed and the parsed edits were routed
///     through the dispatcher, not swallowed or short-circuited;
///   * that returned dry-run diff is non-empty (the edit genuinely flowed);
///   * `DryRun` left the target file byte-for-byte unchanged.
#[test]
fn c1_dryrun_routes_parsed_edits_through_dispatch() {
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

    let instruction = "rewrite greet to shout";
    // A fenced whole-file response: the model proposes new file contents.
    let response = "```rust\npub fn greet() -> String { String::from(\"HI\") }\n```\n";
    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    let out = mend::run_edit(
        Path::new("target.rs"),
        instruction,
        &config,
        &completer,
        mend::OutputMode::DryRun,
        false,
    )
    .expect("run_edit should succeed when the response parses and applies");

    // The completer was called exactly once, with the edit prompt over the
    // gathered context — so context was gathered and the edit prompt was built.
    let prompts = completer.prompts();
    assert_eq!(prompts.len(), 1, "the completer is called exactly once");
    let structure = mend::extract_structure(source).expect("structure");
    let imports = mend::extract_imports(source).expect("imports");
    let context = mend::assemble_context(&structure, &imports, config.token_budget as usize);
    let expected_prompt = mend::edit_prompt(&context, instruction);
    assert_eq!(
        prompts[0], expected_prompt,
        "prompt is edit_prompt(assemble_context(...), instruction)"
    );
    assert!(prompts[0].contains("greet"), "prompt embeds the file's structure (the fn)");
    assert!(
        prompts[0].contains("std::collections::BTreeMap"),
        "prompt embeds the file's imports"
    );

    // The response was parsed and the parsed edits routed through dispatch: the
    // returned text equals the dispatcher's own output over those edits.
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
        "edit routes the parsed edits through output dispatch (DryRun diff)"
    );
    assert!(!out.is_empty(), "the dry-run diff is non-empty — the edit actually flowed");

    // DryRun must not touch the target file.
    let after = std::fs::read_to_string("target.rs").expect("read back target");
    assert_eq!(after, source, "DryRun must leave the target file untouched");

    eprintln!("C1_DRYRUN_OK");
}
"##;
    let p = run_probe("c1dry", src);
    assert!(
        p.ran_ok && p.combined().contains("C1_DRYRUN_OK"),
        "edit must gather context, build the edit prompt, parse the response, and route the \
         parsed edits through output dispatch; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// C1 — the routed edits reach output dispatch end-to-end (observed in `Apply`).
///
/// The probe drives `run_edit` in `Apply` with a fenced whole-file edit and pins
/// that the dispatcher actually wrote the *applied* edits to disk: the returned
/// text equals `apply_edits(original, parse_response(response))`, the file on disk
/// now equals that applied text, and the file genuinely changed from its original.
/// A mutant that parsed nothing, dropped the edits, or skipped the dispatcher
/// would leave the file unchanged and fail.
#[test]
fn c1_apply_routes_edits_to_disk() {
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

    let response = "```rust\npub fn greet() -> String { String::from(\"shouted\") }\n```\n";
    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    let out = mend::run_edit(
        Path::new("target.rs"),
        "rewrite",
        &config,
        &completer,
        mend::OutputMode::Apply,
        false,
    )
    .expect("run_edit Apply should succeed");

    // Apply returns the applied (proposed) text...
    let edits = mend::parse_response(response).expect("parse");
    let proposed = mend::apply_edits(source, &edits).expect("apply");
    assert_eq!(out, proposed, "Apply returns the applied text");

    // ...and the dispatcher wrote those routed edits to disk.
    let after = std::fs::read_to_string("target.rs").expect("read back target");
    assert_eq!(after, proposed, "Apply routes edits to dispatch, which writes them to disk");
    assert!(after != source, "the file was actually changed by the routed edit");

    eprintln!("C1_APPLY_OK");
}
"##;
    let p = run_probe("c1apply", src);
    assert!(
        p.ran_ok && p.combined().contains("C1_APPLY_OK"),
        "edit must route the parsed edits through output dispatch (writing them to disk under \
         Apply); probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// C2 — the user instruction passes through to the prompt unchanged.
///
/// The probe drives `run_edit` with an instruction carrying leading/trailing
/// spaces, mixed case, and punctuation — text that any non-verbatim handling
/// (trim, lowercase, escape) would corrupt. It asserts the resulting prompt
/// contains that exact string and equals `edit_prompt(context, instruction)`. To
/// prove the instruction actually *steers* the prompt (rather than being ignored
/// or hardcoded), it then runs a second, different instruction and asserts that
/// prompt carries the other text verbatim and differs from the first.
#[test]
fn c2_instruction_passes_through_verbatim() {
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

    // A response that parses cleanly so the round-trip reaches the prompt check.
    let response = "```rust\npub fn x() {}\n```\n";

    // An instruction with leading/trailing spaces, mixed case, and punctuation:
    // anything but a verbatim passthrough would corrupt it.
    let instruction = "  Rename `greet` -> `HELLO`, & keep it Pub!  ";
    let completer = mend::MockCompleter::new(vec![String::from(response)]);
    mend::run_edit(
        Path::new("target.rs"),
        instruction,
        &config,
        &completer,
        mend::OutputMode::DryRun,
        false,
    )
    .expect("ok with first instruction");

    let prompts = completer.prompts();
    assert_eq!(prompts.len(), 1, "the completer is called exactly once");

    // The instruction reaches the prompt verbatim.
    assert!(
        prompts[0].contains(instruction),
        "the prompt embeds the instruction unchanged"
    );

    // And the whole prompt is exactly edit_prompt(context, instruction).
    let structure = mend::extract_structure(source).expect("structure");
    let imports = mend::extract_imports(source).expect("imports");
    let context = mend::assemble_context(&structure, &imports, config.token_budget as usize);
    assert_eq!(
        prompts[0],
        mend::edit_prompt(&context, instruction),
        "the prompt is edit_prompt(context, instruction) — the instruction passes through unchanged"
    );

    // A different instruction yields a different prompt carrying that other text:
    // proof the instruction actually drives the prompt rather than being ignored.
    let other = "delete everything";
    let completer2 = mend::MockCompleter::new(vec![String::from(response)]);
    mend::run_edit(
        Path::new("target.rs"),
        other,
        &config,
        &completer2,
        mend::OutputMode::DryRun,
        false,
    )
    .expect("ok with second instruction");
    let p2 = completer2.prompts()[0].clone();
    assert!(p2.contains(other), "the other instruction reaches its prompt verbatim too");
    assert!(p2 != prompts[0], "different instructions produce different prompts");

    eprintln!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.combined().contains("C2_OK"),
        "the user instruction must pass through to the prompt unchanged; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// C3 — a parse failure surfaces as `MendError::Parse` with a non-zero exit.
///
/// The probe scripts the Completer to *succeed*, returning a response that carries
/// none of the three known edit-format markers (no `<<<<<<< SEARCH`, no `@@ ` hunk,
/// no ``` fence). The Completer call therefore succeeds and the failure comes from
/// parsing. The probe asserts the call returns `Err`, that it is specifically the
/// `MendError::Parse` variant (a different variant routes to a `WRONG_VARIANT`
/// branch the outer test rejects; an `Ok` routes to `UNEXPECTED_OK`), that the
/// Display message reads as a parse error, and that the target file is untouched
/// on the failure path. It then surfaces the message on stderr and exits non-zero —
/// exactly the binary's behaviour. The outer test requires a non-zero exit *and*
/// the parse-error message on stderr, with neither sentinel present.
#[test]
fn c3_parse_failure_is_parse_error_nonzero() {
    let src = r##"
use std::path::Path;
use std::process::exit;

fn main() {
    let source = "use std::collections::BTreeMap;\n\npub fn greet() {}\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("m"),
        endpoint: String::from("e"),
        token_budget: 100000,
    };

    // The completer SUCCEEDS, returning a response that matches no known edit
    // format: the failure must come from parsing, not the completer.
    let response = "Sorry, I cannot help with that. No edits here.";
    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    match mend::run_edit(
        Path::new("target.rs"),
        "do it",
        &config,
        &completer,
        mend::OutputMode::DryRun,
        false,
    ) {
        Ok(text) => {
            // Must NOT succeed when the response cannot be parsed.
            println!("UNEXPECTED_OK:{text}");
            exit(0);
        }
        Err(e) => {
            // It must be specifically a Parse error — not Completer, Patch, or Io.
            if !matches!(&e, mend::MendError::Parse(_)) {
                eprintln!("WRONG_VARIANT:{e}");
                exit(4);
            }
            let msg = e.to_string();
            assert!(msg.contains("parse error"), "Display surfaces a parse error: {msg}");
            // The target file must be untouched on the parse-failure path.
            let after = std::fs::read_to_string("target.rs").expect("read back target");
            assert_eq!(after, source, "a parse failure must not write the target file");
            // Surface on stderr and exit non-zero (the binary's contract).
            eprintln!("{msg}");
            exit(3);
        }
    }
}
"##;
    let p = run_probe("c3", src);
    assert!(
        !p.ran_ok,
        "a parse failure must exit non-zero; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
    assert!(
        p.stderr.contains("parse error"),
        "the parse-error message must be surfaced on stderr; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
    assert!(
        !p.combined().contains("UNEXPECTED_OK"),
        "edit must not succeed when the response cannot be parsed; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
    assert!(
        !p.combined().contains("WRONG_VARIANT"),
        "the failure must be MendError::Parse specifically; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}
