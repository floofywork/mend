//! Frozen acceptance tests for T-07.04 — Run test command.
//!
//! The task adds the test-generation subcommand entry point. Its contract is
//! behavioural (gather context from the target file's *public* API → build the
//! *test* prompt → call the Completer → treat the generated tests as a single
//! whole-file edit → route that edit through the output dispatcher against an
//! *empty* original, because the test file does not exist yet), so — exactly as
//! the T-07.01 `run_explain`, T-07.02 `run_edit`, and T-07.03 `run_fix` tests do
//! — the honest way to observe it is to *compile and run a tiny probe program
//! against the real `mend` crate*. Each probe drives the real entry point with a
//! [`mend::MockCompleter`], so the whole flow runs deterministically and without a
//! network. Running the generated tests is explicitly out of scope, so the probes
//! never compile or execute the produced test code — they only observe the
//! generation/routing contract.
//!
//! This test crate stays std-only and never names the not-yet-existing entry
//! point directly, so it always *compiles*. Redness comes from each probe failing
//! to *build* (the `run_test` function does not exist yet), not from this file
//! failing to parse. Every probe is built into an isolated `CARGO_TARGET_DIR` and
//! run `--offline`, so it neither contends with the outer build lock nor touches
//! the network.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!
//!   fn run_test(
//!       path: &Path,
//!       config: &mend::Config,
//!       completer: &dyn mend::Completer,
//!       mode: mend::OutputMode,
//!       color: bool,
//!   ) -> mend::Result<String>
//!
//! It reads the target file, extracts its *public* structure and imports, assembles
//! them into a budget-bounded context, builds `test_prompt(context)`, asks the
//! Completer to complete it, wraps the whole response as a single
//! `ParsedEdit::WholeFile { body: response }`, and routes that edit through
//! `dispatch_output(path, "", apply_edits("", edits), edits, mode, color)` — the
//! original is the empty string, so a not-yet-existing test file diffs against
//! empty.
//!
//!   C1: builds the test prompt and treats the generated tests as a whole-file
//!       ParsedEdit.
//!   C2: output routes through dispatch, defaulting to a diff against an empty
//!       target for a new test file.
//!   C3: generated tests target only public-API symbols from the extracted
//!       structure.

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
    dir.push(format!("mend-test-{tag}-{}-{nanos}", std::process::id()));
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
/// In the red state `mend` has no `run_test`, so the probe fails to resolve and
/// `ran_ok` is false. Once the library defines the entry point the probe
/// references, it builds, runs, and prints its sentinel.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_test_probe\"\n\
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

/// C1 — builds the test prompt and treats the generated tests as a whole-file
/// ParsedEdit.
///
/// The probe writes a target file carrying an importable `use` and a public `fn`,
/// drives `run_test` in `Json` mode with a `MockCompleter` whose scripted response
/// is *raw test code carrying none of the three edit-format markers* (no
/// ```` ``` ````, no `@@ ` hunk, no `<<<<<<< SEARCH`). That raw response is the
/// discriminator: if the command routed the response through `parse_response`
/// (like `edit`/`fix`), an unmarked response would be a `MendError::Parse` and the
/// call would fail; treating it directly as a single whole-file edit is the only
/// way the call succeeds. It then checks:
///   * the Completer is called exactly once — one round-trip;
///   * that one prompt equals `test_prompt(assemble_context(structure, imports,
///     budget))` — so context really was gathered and the *test* prompt really was
///     built from it;
///   * the emitted `JsonResult` carries exactly one edit, a
///     `ParsedEdit::WholeFile { body: <response> }` — the whole generated test
///     text became a single whole-file edit, verbatim;
///   * the proposed text equals the response verbatim — the whole file is the
///     generated tests, with no parsing/splicing applied.
/// A mutant that parsed the response, wrapped only part of it, or used a different
/// prompt builder would not reproduce these and would fail.
#[test]
fn c1_builds_test_prompt_and_whole_file_edit() {
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

    // Raw generated test code carrying NONE of the three edit-format markers, so
    // `parse_response` would reject it. The command must treat it as a single
    // whole-file edit instead.
    let response = "#[test]\nfn greets() { assert_eq!(greet(\"x\"), String::new()); }\n";
    assert!(!response.contains("```"), "fixture: response carries no fence marker");
    assert!(!response.contains("@@ "), "fixture: response carries no hunk marker");
    assert!(!response.contains("<<<<<<< SEARCH"), "fixture: response carries no search marker");

    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    let out = mend::run_test(
        Path::new("target.rs"),
        &config,
        &completer,
        mend::OutputMode::Json,
        false,
    )
    .expect("run_test should succeed and treat an unmarked response as a whole-file edit");

    let prompts = completer.prompts();
    assert_eq!(prompts.len(), 1, "the completer is called exactly once — one round-trip");

    // The prompt is exactly test_prompt(context): context gathered from structure
    // and imports, test prompt built from it.
    let structure = mend::extract_structure(source).expect("structure");
    let imports = mend::extract_imports(source).expect("imports");
    let context = mend::assemble_context(&structure, &imports, config.token_budget as usize);
    assert_eq!(
        prompts[0],
        mend::test_prompt(&context),
        "the prompt is test_prompt(context) — the test prompt is built from the assembled context"
    );

    // The generated tests are a single whole-file edit carrying the response verbatim.
    let result = mend::parse_json(&out).expect("emitted JSON parses back");
    assert_eq!(
        result.edits,
        vec![mend::ParsedEdit::WholeFile { body: String::from(response) }],
        "the generated tests are exactly one whole-file edit whose body is the response"
    );
    assert_eq!(
        result.proposed, response,
        "the proposed file is the generated tests verbatim — no parsing or splicing"
    );

    eprintln!("C1_OK");
}
"##;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.combined().contains("C1_OK"),
        "the test command must build the test prompt and wrap the generated tests as a single \
         whole-file ParsedEdit; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// C2 — output routes through dispatch, defaulting to a diff against an empty
/// target for a new test file (observed in the default `DryRun` mode).
///
/// The probe drives `run_test` in `DryRun` (the default) over a *non-empty* source
/// file, with an unmarked whole-file response, and pins that the returned text
/// equals what `dispatch_output(path, "", apply_edits("", edits), edits, DryRun,
/// false)` returns for `edits = [WholeFile { body: response }]` — i.e. the diff is
/// computed against an *empty* original, as for a not-yet-existing test file, even
/// though the file on disk is non-empty. To prove the empty base is genuine (and
/// not just the actual file contents), it also asserts the output *differs* from
/// the same dispatch taken against the real source text. It further checks the
/// diff is non-empty (the generated tests actually flowed) and that `DryRun` left
/// the target file byte-for-byte unchanged (generation never runs or writes).
/// A mutant that diffed against the file's real contents, skipped the dispatcher,
/// or wrote to disk would fail.
#[test]
fn c2_dryrun_routes_whole_file_against_empty() {
    let src = r##"
use std::path::Path;

fn main() {
    // A deliberately NON-empty source file: if the command diffed against the
    // file's real contents rather than empty, the diff would carry deletions and
    // differ from the empty-base diff.
    let source = "use std::fmt;\n\npub fn add(a: i32, b: i32) -> i32 { a + b }\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("m"),
        endpoint: String::from("e"),
        token_budget: 100000,
    };

    let response = "#[test]\nfn adds() { assert_eq!(add(2, 2), 4); }\n";
    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    let out = mend::run_test(
        Path::new("target.rs"),
        &config,
        &completer,
        mend::OutputMode::DryRun,
        false,
    )
    .expect("run_test DryRun should succeed");

    // The output is exactly the dispatcher's own render of a whole-file edit
    // diffed against an EMPTY original — the new-test-file default.
    let edits_empty = vec![mend::ParsedEdit::WholeFile { body: String::from(response) }];
    let applied_empty = mend::apply_edits("", &edits_empty);
    let expected_empty = mend::dispatch_output(
        Path::new("target.rs"),
        "",
        applied_empty,
        edits_empty,
        mend::OutputMode::DryRun,
        false,
    )
    .expect("dispatch against empty");
    assert_eq!(
        out, expected_empty,
        "DryRun routes the whole-file edit through dispatch, diffing against an empty target"
    );

    // The base really is empty, not the file's real contents: against the actual
    // source the dispatcher would produce a different (deletion-bearing) diff.
    let edits_src = vec![mend::ParsedEdit::WholeFile { body: String::from(response) }];
    let applied_src = mend::apply_edits(source, &edits_src);
    let against_source = mend::dispatch_output(
        Path::new("target.rs"),
        source,
        applied_src,
        edits_src,
        mend::OutputMode::DryRun,
        false,
    )
    .expect("dispatch against source");
    assert!(
        out != against_source,
        "the diff base is the empty string, not the file's real contents"
    );

    assert!(!out.is_empty(), "the dry-run diff is non-empty — the generated tests actually flowed");

    // DryRun must not touch the target file (and the tests are never run).
    let after = std::fs::read_to_string("target.rs").expect("read back target");
    assert_eq!(after, source, "DryRun must leave the target file untouched");

    eprintln!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.combined().contains("C2_OK"),
        "the test command must route its whole-file edit through output dispatch, defaulting to a \
         diff against an empty target; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// C2 (Apply arm) — the whole-file edit reaches dispatch end-to-end and is written
/// as the generated tests against an empty base.
///
/// The probe drives `run_test` in `Apply` with an unmarked whole-file response and
/// pins that the dispatcher actually wrote the generated tests to the target path:
/// the returned text equals `apply_edits("", [WholeFile { body: response }])` (the
/// whole response), the file on disk now equals that text, and — because the edit
/// is whole-file against an empty base — the written contents are the response
/// verbatim with the prior contents fully discarded. A mutant that skipped the
/// dispatcher, wrapped only part of the response, or diffed/merged against the old
/// contents would not reproduce this.
#[test]
fn c2_apply_writes_whole_file_tests() {
    let src = r##"
use std::path::Path;

fn main() {
    let source = "use std::fmt;\n\npub fn add(a: i32, b: i32) -> i32 { a + b }\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("m"),
        endpoint: String::from("e"),
        token_budget: 100000,
    };

    let response = "#[test]\nfn adds() { assert_eq!(add(2, 2), 4); }\n";
    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    let out = mend::run_test(
        Path::new("target.rs"),
        &config,
        &completer,
        mend::OutputMode::Apply,
        false,
    )
    .expect("run_test Apply should succeed");

    // Apply returns the applied whole-file text: the generated tests verbatim.
    let edits = vec![mend::ParsedEdit::WholeFile { body: String::from(response) }];
    let proposed = mend::apply_edits("", &edits).expect("apply whole-file against empty");
    assert_eq!(out, proposed, "Apply returns the applied whole-file text");
    assert_eq!(out, response, "the whole-file body is the generated tests verbatim");

    // ...and the dispatcher wrote those generated tests to disk.
    let after = std::fs::read_to_string("target.rs").expect("read back target");
    assert_eq!(after, response, "Apply routes the whole-file edit to dispatch, which writes the tests");

    eprintln!("C2_APPLY_OK");
}
"##;
    let p = run_probe("c2apply", src);
    assert!(
        p.ran_ok && p.combined().contains("C2_APPLY_OK"),
        "the test command must route its whole-file edit through output dispatch (writing the \
         generated tests under Apply); probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// C3 — generated tests target only public-API symbols from the extracted
/// structure.
///
/// The probe writes a source file mixing a *public* `fn` and `struct` with a
/// *private* `fn` and `struct`, drives `run_test`, and inspects the prompt the
/// Completer received. It asserts the prompt is exactly `test_prompt(context)` for
/// the context assembled from `extract_structure` (public-only) and
/// `extract_imports`, then pins the public/private split directly: the public
/// symbols' names appear in the prompt while the private symbols' names do not.
/// Because `extract_structure` keeps only `pub` items, a generation grounded in
/// that structure can only target the public API — so a mutant that fed the model
/// the raw source, or otherwise leaked the private items, would put `secret_helper`
/// or `Hidden` into the prompt and fail.
#[test]
fn c3_targets_only_public_api_symbols() {
    let src = r##"
use std::path::Path;

fn main() {
    let source = "use std::collections::BTreeMap;\n\npub fn public_greet() -> String { String::new() }\n\nfn secret_helper() -> u32 { 7 }\n\npub struct Visible { pub n: u32 }\n\nstruct Hidden { n: u32 }\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("m"),
        endpoint: String::from("e"),
        token_budget: 100000,
    };

    let response = "#[test]\nfn t() { let _ = public_greet(); }\n";
    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    mend::run_test(
        Path::new("target.rs"),
        &config,
        &completer,
        mend::OutputMode::DryRun,
        false,
    )
    .expect("run_test should succeed");

    let prompts = completer.prompts();
    assert_eq!(prompts.len(), 1, "the completer is called exactly once");

    // The prompt is exactly test_prompt over the PUBLIC structure plus imports.
    let structure = mend::extract_structure(source).expect("structure");
    let imports = mend::extract_imports(source).expect("imports");
    let context = mend::assemble_context(&structure, &imports, config.token_budget as usize);
    assert_eq!(
        prompts[0],
        mend::test_prompt(&context),
        "the prompt is test_prompt(context) built from the extracted (public) structure"
    );

    // Public-API symbols ARE targeted...
    assert!(
        prompts[0].contains("public_greet"),
        "the public fn is targeted in the prompt"
    );
    assert!(
        prompts[0].contains("Visible"),
        "the public struct is targeted in the prompt"
    );
    // ...and private symbols are NOT — only the public API is targeted.
    assert!(
        !prompts[0].contains("secret_helper"),
        "the private fn must not be targeted"
    );
    assert!(
        !prompts[0].contains("Hidden"),
        "the private struct must not be targeted"
    );

    eprintln!("C3_OK");
}
"##;
    let p = run_probe("c3", src);
    assert!(
        p.ran_ok && p.combined().contains("C3_OK"),
        "the test command must target only public-API symbols from the extracted structure; \
         probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}
