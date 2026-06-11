//! Frozen acceptance tests for T-08.01 — Drive mock end-to-end tests.
//!
//! This is the full-stack acceptance gate: it proves the deterministic layers
//! (parse → dispatch → handler → output) *compose* into a working CLI when driven
//! by scripted [`mend::MockCompleter`] responses, with assertions over the three
//! output modes — the on-disk file after `--apply` (C1), the unchanged file plus
//! emitted diff in the default dry-run (C2), and the JSON structure under `--json`
//! (C3) — and with *no* network I/O anywhere (C4), since the completer is the
//! in-process mock.
//!
//! Exactly as the T-07.x `run_*` / `run` tests do, the honest way to observe a
//! behavioural entry point is to *compile and run a tiny probe program against the
//! real `mend` crate*. Each probe drives the real *assembled* CLI with a
//! `MockCompleter`, so the whole flow runs deterministically and without a
//! network. This test crate stays std-only and never names the not-yet-existing
//! entry point directly, so it always *compiles*; redness comes from each probe
//! failing to *build* — in the red state `mend::run_cli` does not exist, and the
//! parser does not yet route the `fix` / `test` subcommands from argv — not from
//! this file failing to parse. Every probe is built into an isolated
//! `CARGO_TARGET_DIR` and run `--offline`, so it neither contends with the outer
//! build lock nor touches the network.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply — a single
//! *assembled CLI* entry point that turns a raw argument vector into a result by
//! parsing it and dispatching the parsed command, with the [`mend::Completer`]
//! injected (so tests inject the mock and production injects a real backend):
//!
//!   fn run_cli(
//!       args: &[String],
//!       config: &mend::Config,
//!       completer: &dyn mend::Completer,
//!   ) -> mend::Result<String>
//!
//! It composes the already-verified layers: `run(parse(args), config, completer)`.
//! For every subcommand to be reachable end-to-end *from argv* (so none is left
//! without an end-to-end test), the parser must route all four subcommands; the
//! minimal grammar the gate exercises is:
//!
//!   explain <path>
//!   edit    <path> <instruction> [--apply] [--json]
//!   fix     <path> [--apply]
//!   test    <path> [--apply]
//!
//! Nothing beyond what these probes exercise is required of the parser additions —
//! per the project constitution, the green phase should add the *minimal* routing
//! that satisfies these frozen tests and no more.
//!
//!   C1: an end-to-end `edit --apply` run writes the applied text to the on-disk
//!       file (and returns it).
//!   C2: an end-to-end dry-run leaves the target file byte-for-byte unchanged and
//!       emits a diff (not JSON).
//!   C3: an end-to-end `--json` run emits JSON matching the expected `JsonResult`
//!       structure and writes nothing.
//!   C4: the run succeeds using only the in-process `MockCompleter`, with a bogus
//!       endpoint that any real network call would fail on — proving no network
//!       I/O occurs.

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
    dir.push(format!("mend-e2e-{tag}-{}-{nanos}", std::process::id()));
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
/// In the red state `mend` has no `run_cli` (and the parser does not route the
/// `fix` / `test` subcommands), so the probe fails to resolve and `ran_ok` is
/// false. Once the library defines the assembled entry point and routes every
/// subcommand, the probe builds, runs, and prints its sentinel.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_e2e_probe\"\n\
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

/// C1 — an end-to-end `edit --apply` run writes the applied text to disk.
///
/// The probe assembles the raw argument vector `["edit", "target.rs", <instr>,
/// "--apply"]`, scripts the `MockCompleter` with a fenced whole-file response, and
/// drives the *assembled* CLI (`run_cli`). It then proves the full stack composed:
/// the returned text equals `apply_edits(source, parse_response(response))` (the
/// response was parsed and applied), the file on disk now equals that applied text
/// (Apply wrote it), the file genuinely changed from its original, the applied
/// body's sentinel is present on disk, and the completer was called exactly once.
/// A mutant anywhere in the parse→dispatch→apply→write chain breaks one of these.
#[test]
fn c1_edit_apply_writes_disk() {
    let src = r##"
fn main() {
    let source = "use std::fmt;\n\npub fn add(a: i32, b: i32) -> i32 { a + b }\n";
    std::fs::write("target.rs", source).expect("write target");

    // A bogus endpoint: it is never contacted because the mock serves the call.
    let config = mend::Config {
        model: String::from("test-model"),
        endpoint: String::from("http://nonexistent.invalid:0/v1"),
        token_budget: 100000,
    };

    // A fenced whole-file edit: `parse_response` routes it to the fenced parser.
    let response = "```\nWHOLE_FILE_BODY_SENTINEL\n```\n";
    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    let args: Vec<String> = vec![
        String::from("edit"),
        String::from("target.rs"),
        String::from("rewrite add"),
        String::from("--apply"),
    ];
    let out = mend::run_cli(&args, &config, &completer)
        .expect("run_cli edit --apply should succeed end-to-end");

    let edits = mend::parse_response(response).expect("parse");
    let proposed = mend::apply_edits(source, &edits).expect("apply");

    assert_eq!(out, proposed, "edit --apply returns the applied text");
    let after = std::fs::read_to_string("target.rs").expect("read back target");
    assert_eq!(after, proposed, "edit --apply writes the applied text to disk");
    assert_ne!(after, source, "the on-disk file was actually changed");
    assert!(
        after.contains("WHOLE_FILE_BODY_SENTINEL"),
        "the applied body landed on disk"
    );
    assert_eq!(
        completer.prompts().len(),
        1,
        "exactly one completion was requested through the assembled CLI"
    );

    eprintln!("E2E_C1_APPLY_OK");
}
"##;
    let p = run_probe("c1-apply", src);
    assert!(
        p.ran_ok && p.combined().contains("E2E_C1_APPLY_OK"),
        "an end-to-end edit --apply must write the applied text to disk; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// C2 — an end-to-end dry-run leaves the target file unchanged and emits a diff.
///
/// The probe assembles `["edit", "target.rs", <instr>]` — neither `--apply` nor
/// `--json`, so the default dry-run mode runs — and drives `run_cli`. It asserts:
/// the file on disk is byte-for-byte unchanged (dry-run touches nothing), the
/// output is non-empty and is *not* a JSON result (it is a human diff), the
/// proposed body's sentinel appears in the rendered diff (a diff really was
/// emitted), and the output equals exactly what `dispatch_output(.., DryRun, false)`
/// produces over the parsed edits (the deterministic layers composed). The
/// unchanged-vs-diff pair pins both sides of the dry-run boundary.
#[test]
fn c2_edit_dryrun_unchanged_emits_diff() {
    let src = r##"
use std::path::Path;

fn main() {
    let source = "use std::fmt;\n\npub fn add(a: i32, b: i32) -> i32 { a + b }\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("test-model"),
        endpoint: String::from("http://nonexistent.invalid:0/v1"),
        token_budget: 100000,
    };

    let response = "```\nWHOLE_FILE_BODY_SENTINEL\n```\n";
    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    // No --apply and no --json: the default dry-run mode.
    let args: Vec<String> = vec![
        String::from("edit"),
        String::from("target.rs"),
        String::from("rewrite add"),
    ];
    let out = mend::run_cli(&args, &config, &completer)
        .expect("run_cli edit dry-run should succeed end-to-end");

    let after = std::fs::read_to_string("target.rs").expect("read back target");
    assert_eq!(after, source, "the dry-run leaves the target file unchanged");

    assert!(!out.is_empty(), "the dry-run emits a non-empty diff");
    assert!(
        mend::parse_json(&out).is_err(),
        "the dry-run output is a diff, not a JSON result"
    );
    assert!(
        out.contains("WHOLE_FILE_BODY_SENTINEL"),
        "the emitted diff shows the proposed body"
    );

    // The emitted diff is exactly what the output dispatcher renders for DryRun.
    let edits = mend::parse_response(response).expect("parse");
    let applied = mend::apply_edits(source, &edits);
    let expected = mend::dispatch_output(
        Path::new("target.rs"),
        source,
        applied,
        edits,
        mend::OutputMode::DryRun,
        false,
    )
    .expect("dispatch");
    assert_eq!(out, expected, "the dry-run emits exactly the rendered diff");

    eprintln!("E2E_C2_DRYRUN_OK");
}
"##;
    let p = run_probe("c2-dryrun", src);
    assert!(
        p.ran_ok && p.combined().contains("E2E_C2_DRYRUN_OK"),
        "an end-to-end dry-run must leave the file unchanged and emit a diff; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// C3 — an end-to-end `--json` run emits JSON matching the expected structure.
///
/// The probe assembles `["edit", "target.rs", <instr>, "--json"]` and drives
/// `run_cli`. It pins the emitted JSON *exactly* by rebuilding the expected
/// `JsonResult` from the same deterministic layers — path `target.rs`, the
/// original source, the applied proposed text, the parsed edits, and `applied:
/// false` (Json mode never writes) — and asserting `emit_json` of that struct
/// equals the output. It further asserts the output parses back (round-trips) with
/// `applied == false`, and that the target file on disk is unchanged (Json mode
/// touches nothing). A mutant in the JSON shape, the applied flag, or the mode
/// routing fails the exact-match.
#[test]
fn c3_edit_json_matches_expected_structure() {
    let src = r##"
fn main() {
    let source = "use std::fmt;\n\npub fn add(a: i32, b: i32) -> i32 { a + b }\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("test-model"),
        endpoint: String::from("http://nonexistent.invalid:0/v1"),
        token_budget: 100000,
    };

    let response = "```\nWHOLE_FILE_BODY_SENTINEL\n```\n";
    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    let args: Vec<String> = vec![
        String::from("edit"),
        String::from("target.rs"),
        String::from("rewrite add"),
        String::from("--json"),
    ];
    let out = mend::run_cli(&args, &config, &completer)
        .expect("run_cli edit --json should succeed end-to-end");

    let edits = mend::parse_response(response).expect("parse");
    let proposed = mend::apply_edits(source, &edits).expect("apply");
    let expected = mend::emit_json(&mend::JsonResult {
        path: String::from("target.rs"),
        original: String::from(source),
        proposed,
        edits,
        applied: false,
    });
    assert_eq!(
        out, expected,
        "--json emits JSON matching the expected JsonResult structure"
    );

    // It round-trips, and the applied flag is false (Json mode never writes).
    let parsed = mend::parse_json(&out).expect("--json output parses back");
    assert_eq!(parsed.path, "target.rs", "the JSON carries the target path");
    assert_eq!(parsed.original, source, "the JSON carries the original text");
    assert_eq!(parsed.applied, false, "Json mode reports applied = false");

    let after = std::fs::read_to_string("target.rs").expect("read back target");
    assert_eq!(after, source, "--json does not write to disk");

    eprintln!("E2E_C3_JSON_OK");
}
"##;
    let p = run_probe("c3-json", src);
    assert!(
        p.ran_ok && p.combined().contains("E2E_C3_JSON_OK"),
        "an end-to-end --json run must emit the expected JSON structure; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// C4 — the end-to-end run performs no network I/O.
///
/// Absence of network I/O is proved *deterministically*: the config's `endpoint`
/// is a bogus address that any genuine network call would fail (or hang) on, yet
/// the run *succeeds* — which is only possible if the completion was served
/// entirely by the in-process `MockCompleter`, never by reaching out to the
/// endpoint. The probe asserts the run succeeds, that the mock recorded exactly one
/// prompt (so the mock — not a network backend — produced the completion), and that
/// the applied result reached disk. A second mock response is left *unconsumed*,
/// confirming the script (not a socket) is the sole source of completions.
#[test]
fn c4_no_network_mockcompleter_serves_run() {
    let src = r##"
fn main() {
    let source = "use std::fmt;\n\npub fn add(a: i32, b: i32) -> i32 { a + b }\n";
    std::fs::write("target.rs", source).expect("write target");

    // A deliberately unroutable endpoint: any real network call would fail here.
    let config = mend::Config {
        model: String::from("test-model"),
        endpoint: String::from("http://nonexistent.invalid:0/v1"),
        token_budget: 100000,
    };

    let response = "```\nNO_NETWORK_BODY\n```\n";
    // Two scripted responses; only the first should be consumed by the one call.
    let completer =
        mend::MockCompleter::new(vec![String::from(response), String::from("UNUSED")]);

    let args: Vec<String> = vec![
        String::from("edit"),
        String::from("target.rs"),
        String::from("x"),
        String::from("--apply"),
    ];
    let out = mend::run_cli(&args, &config, &completer)
        .expect("run_cli must succeed using only the in-process MockCompleter — no network I/O");

    // The mock served the completion: exactly one prompt was recorded. Had the run
    // tried to reach the bogus endpoint, it could not have succeeded.
    assert_eq!(
        completer.prompts().len(),
        1,
        "the in-process MockCompleter served the completion (no network completer ran)"
    );

    let edits = mend::parse_response(response).expect("parse");
    let proposed = mend::apply_edits(source, &edits).expect("apply");
    assert_eq!(out, proposed, "the mock-scripted edit flowed through end-to-end");
    let after = std::fs::read_to_string("target.rs").expect("read back target");
    assert_eq!(after, proposed, "the applied result reached disk without any network");

    eprintln!("E2E_C4_NO_NETWORK_OK");
}
"##;
    let p = run_probe("c4-nonet", src);
    assert!(
        p.ran_ok && p.combined().contains("E2E_C4_NO_NETWORK_OK"),
        "the end-to-end run must succeed via the in-process mock alone (no network I/O); \
         probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// Coverage of the `explain` subcommand end-to-end (no subcommand left untested).
///
/// The probe drives `run_cli(["explain", "target.rs"], ..)` with a sentinel mock
/// response and asserts the read-only explain path: the assembled CLI returns the
/// completion verbatim, the target file is left unchanged, and the completer was
/// called exactly once.
#[test]
fn e2e_explain_returns_completion_unchanged_file() {
    let src = r##"
fn main() {
    let source = "use std::fmt;\n\npub fn add(a: i32, b: i32) -> i32 { a + b }\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("test-model"),
        endpoint: String::from("http://nonexistent.invalid:0/v1"),
        token_budget: 100000,
    };

    let response = "EXPLANATION_SENTINEL";
    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    let args: Vec<String> = vec![String::from("explain"), String::from("target.rs")];
    let out = mend::run_cli(&args, &config, &completer)
        .expect("run_cli explain should succeed end-to-end");

    assert_eq!(out, response, "explain returns the completion verbatim");
    let after = std::fs::read_to_string("target.rs").expect("read back target");
    assert_eq!(after, source, "explain leaves the target file unchanged");
    assert_eq!(completer.prompts().len(), 1, "exactly one completion was requested");

    eprintln!("E2E_EXPLAIN_OK");
}
"##;
    let p = run_probe("explain", src);
    assert!(
        p.ran_ok && p.combined().contains("E2E_EXPLAIN_OK"),
        "the explain subcommand must run end-to-end from argv; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// Coverage of the `fix` subcommand end-to-end, under `--apply`.
///
/// The probe drives `run_cli(["fix", "target.rs", "--apply"], ..)` with a fenced
/// whole-file response and asserts the fix path composed: the returned text equals
/// the applied edit, the file on disk equals it, and the file genuinely changed.
/// Paired with the dry-run fix test, this pins both sides of the `fix` `--apply`
/// boundary the parser routing must honour.
#[test]
fn e2e_fix_apply_writes_disk() {
    let src = r##"
fn main() {
    let source = "use std::fmt;\n\npub fn add(a: i32, b: i32) -> i32 { a + b }\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("test-model"),
        endpoint: String::from("http://nonexistent.invalid:0/v1"),
        token_budget: 100000,
    };

    let response = "```\nFIXED_BODY_SENTINEL\n```\n";
    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    let args: Vec<String> = vec![
        String::from("fix"),
        String::from("target.rs"),
        String::from("--apply"),
    ];
    let out = mend::run_cli(&args, &config, &completer)
        .expect("run_cli fix --apply should succeed end-to-end");

    let edits = mend::parse_response(response).expect("parse");
    let proposed = mend::apply_edits(source, &edits).expect("apply");
    assert_eq!(out, proposed, "fix --apply returns the applied text");
    let after = std::fs::read_to_string("target.rs").expect("read back target");
    assert_eq!(after, proposed, "fix --apply writes the applied text to disk");
    assert_ne!(after, source, "the on-disk file was actually changed by fix");

    eprintln!("E2E_FIX_APPLY_OK");
}
"##;
    let p = run_probe("fix-apply", src);
    assert!(
        p.ran_ok && p.combined().contains("E2E_FIX_APPLY_OK"),
        "the fix subcommand must run end-to-end and write to disk under --apply; \
         probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// Coverage of the `fix` subcommand end-to-end, in the default dry-run.
///
/// The probe drives `run_cli(["fix", "target.rs"], ..)` and asserts the file is
/// left unchanged and a non-JSON diff is emitted showing the proposed body — the
/// dry-run side of the `fix` `--apply` boundary.
#[test]
fn e2e_fix_dryrun_unchanged_emits_diff() {
    let src = r##"
fn main() {
    let source = "use std::fmt;\n\npub fn add(a: i32, b: i32) -> i32 { a + b }\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("test-model"),
        endpoint: String::from("http://nonexistent.invalid:0/v1"),
        token_budget: 100000,
    };

    let response = "```\nFIXED_BODY_SENTINEL\n```\n";
    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    let args: Vec<String> = vec![String::from("fix"), String::from("target.rs")];
    let out = mend::run_cli(&args, &config, &completer)
        .expect("run_cli fix dry-run should succeed end-to-end");

    let after = std::fs::read_to_string("target.rs").expect("read back target");
    assert_eq!(after, source, "the dry-run fix leaves the target file unchanged");
    assert!(!out.is_empty(), "the dry-run fix emits a non-empty diff");
    assert!(
        mend::parse_json(&out).is_err(),
        "the dry-run fix output is a diff, not a JSON result"
    );
    assert!(
        out.contains("FIXED_BODY_SENTINEL"),
        "the emitted diff shows the proposed body"
    );

    eprintln!("E2E_FIX_DRYRUN_OK");
}
"##;
    let p = run_probe("fix-dryrun", src);
    assert!(
        p.ran_ok && p.combined().contains("E2E_FIX_DRYRUN_OK"),
        "the fix subcommand must run end-to-end in dry-run, leaving the file unchanged; \
         probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// Coverage of the `test` subcommand end-to-end, under `--apply`.
///
/// The `test` handler wraps the whole response as one whole-file edit (no
/// `parse_response`) applied against an empty original, so the probe scripts a raw
/// test-code response with no edit-format markers. It drives `run_cli(["test",
/// "target.rs", "--apply"], ..)` and asserts the generated body is returned and
/// written to disk, changing the file. Paired with the dry-run test, this pins both
/// sides of the `test` `--apply` boundary.
#[test]
fn e2e_test_apply_writes_disk() {
    let src = r##"
fn main() {
    let source = "use std::fmt;\n\npub fn add(a: i32, b: i32) -> i32 { a + b }\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("test-model"),
        endpoint: String::from("http://nonexistent.invalid:0/v1"),
        token_budget: 100000,
    };

    // Raw generated test code carrying none of the edit-format markers: the test
    // handler treats the whole response as a single whole-file edit.
    let response = "#[test]\nfn generated_case() { assert_eq!(add(2, 2), 4); }\n";
    assert!(!response.contains("```"), "fixture carries no fence marker");
    assert!(!response.contains("@@ "), "fixture carries no hunk marker");
    assert!(!response.contains("<<<<<<< SEARCH"), "fixture carries no search marker");

    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    let args: Vec<String> = vec![
        String::from("test"),
        String::from("target.rs"),
        String::from("--apply"),
    ];
    let out = mend::run_cli(&args, &config, &completer)
        .expect("run_cli test --apply should succeed end-to-end");

    assert_eq!(out, response, "test --apply returns the generated body");
    let after = std::fs::read_to_string("target.rs").expect("read back target");
    assert_eq!(after, response, "test --apply writes the generated tests to disk");
    assert_ne!(after, source, "the on-disk file was actually changed by test");

    eprintln!("E2E_TEST_APPLY_OK");
}
"##;
    let p = run_probe("test-apply", src);
    assert!(
        p.ran_ok && p.combined().contains("E2E_TEST_APPLY_OK"),
        "the test subcommand must run end-to-end and write to disk under --apply; \
         probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// Coverage of the `test` subcommand end-to-end, in the default dry-run.
///
/// The probe drives `run_cli(["test", "target.rs"], ..)` and asserts the target
/// file is unchanged and a non-empty diff (against the empty new-file baseline)
/// showing the generated tests is emitted — the dry-run side of the `test`
/// `--apply` boundary.
#[test]
fn e2e_test_dryrun_unchanged_emits_diff() {
    let src = r##"
fn main() {
    let source = "use std::fmt;\n\npub fn add(a: i32, b: i32) -> i32 { a + b }\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("test-model"),
        endpoint: String::from("http://nonexistent.invalid:0/v1"),
        token_budget: 100000,
    };

    let response = "#[test]\nfn generated_case() { assert_eq!(add(2, 2), 4); }\n";
    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    let args: Vec<String> = vec![String::from("test"), String::from("target.rs")];
    let out = mend::run_cli(&args, &config, &completer)
        .expect("run_cli test dry-run should succeed end-to-end");

    let after = std::fs::read_to_string("target.rs").expect("read back target");
    assert_eq!(after, source, "the dry-run test leaves the target file unchanged");
    assert!(!out.is_empty(), "the dry-run test emits a non-empty diff");
    assert!(
        out.contains("generated_case"),
        "the emitted diff shows the generated tests"
    );

    eprintln!("E2E_TEST_DRYRUN_OK");
}
"##;
    let p = run_probe("test-dryrun", src);
    assert!(
        p.ran_ok && p.combined().contains("E2E_TEST_DRYRUN_OK"),
        "the test subcommand must run end-to-end in dry-run, leaving the file unchanged; \
         probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}
