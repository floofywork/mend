//! Frozen acceptance tests for T-07.05 — Route subcommand dispatch.
//!
//! This task adds the top-level wiring from a parsed [`mend::Command`] to the
//! matching subcommand handler. Its contract is purely *selection + injection*:
//! a parsed command picks exactly one of the explain/edit/fix/test handlers
//! (C1), the selected handler is handed the loaded config and the constructed
//! Completer (C2), and the dispatch is an *exhaustive* match so a new, unhandled
//! command variant is a compile error rather than a silent fall-through (C3).
//!
//! Exactly as the T-07.01..04 `run_*` tests do, the honest way to observe a
//! behavioural entry point is to *compile and run a tiny probe program against
//! the real `mend` crate*. Each probe drives the real dispatcher with a
//! [`mend::MockCompleter`], so the whole flow runs deterministically and without
//! a network. The dispatcher selects a handler; which handler ran is observed by
//! the *prompt the Completer recorded*, since every handler builds its own
//! distinct prompt (explain/edit/fix/test), and "exactly one handler" is observed
//! by the Completer being called exactly once.
//!
//! This test crate stays std-only and never names the not-yet-existing entry
//! point directly, so it always *compiles*. Redness comes from each probe failing
//! to *build* — in the red state `mend::run` and the `Command::Fix` /
//! `Command::Test` variants do not exist yet — not from this file failing to
//! parse. Every probe is built into an isolated `CARGO_TARGET_DIR` and run
//! `--offline`, so it neither contends with the outer build lock nor touches the
//! network.
//!
//! `run` is the chosen name because the crate root already has a `dispatch`
//! *module* (the response-format parser), so the top-level command dispatcher
//! takes the `run` name that sits naturally beside the `run_explain`/`run_edit`/
//! `run_fix`/`run_test` handlers it routes to.
//!
//! The probes pin the minimal, idiomatic contract the criteria imply:
//!
//!   fn run(
//!       command: mend::Command,
//!       config: &mend::Config,
//!       completer: &dyn mend::Completer,
//!   ) -> mend::Result<String>
//!
//! It matches `command` exhaustively and routes to exactly one of `run_explain`,
//! `run_edit`, `run_fix`, `run_test`, handing each the `config` and `completer`;
//! the non-handler `Usage` variant is handled explicitly (returned as an error),
//! and the match carries no `_` wildcard, so adding a new variant is a compile
//! error. The mutation/test gate observes this through the four routing probes
//! plus the exhaustiveness probe.
//!
//!   C1: the parsed CLI command selects exactly one of the explain/edit/fix/test
//!       handlers.
//!   C2: the selected handler receives the loaded config and the constructed
//!       Completer.
//!   C3: an unhandled command variant is a compile error via an exhaustive match.

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
    dir.push(format!("mend-route-{tag}-{}-{nanos}", std::process::id()));
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
/// In the red state `mend` has no `run` (and no `Command::Fix` /
/// `Command::Test`), so the probe fails to resolve and `ran_ok` is false. Once the
/// library defines the entry point the probe references, it builds, runs, and
/// prints its sentinel.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_route_probe\"\n\
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

/// C1 — the `Explain` command selects the explain handler (and no other).
///
/// The probe builds a `Command::Explain { path, symbol: Some("add") }`, dispatches
/// it with a `MockCompleter`, and observes: the Completer was called *exactly once*
/// (so exactly one handler ran), the one recorded prompt equals
/// `explain_prompt(context, Some("add"))` — and is *not* the edit or test prompt —
/// proving the explain handler (not another) ran, the dispatcher returned the raw
/// completion verbatim (explain prints text, never a diff), and the target file is
/// left untouched. A mutant routing `Explain` to a different handler would build a
/// different prompt and/or fail to return the response verbatim.
#[test]
fn c1_explain_command_routes_to_explain_handler() {
    let src = r##"
fn main() {
    let source = "use std::fmt;\n\npub fn add(a: i32, b: i32) -> i32 { a + b }\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("m"),
        endpoint: String::from("e"),
        token_budget: 100000,
    };

    let response = "EXPLANATION_TEXT_FROM_MOCK";
    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    let out = mend::run(
        mend::Command::Explain {
            path: std::path::PathBuf::from("target.rs"),
            symbol: Some(String::from("add")),
        },
        &config,
        &completer,
    )
    .expect("dispatch should route Explain to the explain handler");

    let prompts = completer.prompts();
    assert_eq!(prompts.len(), 1, "exactly one handler ran — one completer call");

    let structure = mend::extract_structure(source).expect("structure");
    let imports = mend::extract_imports(source).expect("imports");
    let context = mend::assemble_context(&structure, &imports, config.token_budget as usize);

    assert_eq!(
        prompts[0],
        mend::explain_prompt(&context, Some("add")),
        "the explain handler built the explain prompt"
    );
    assert_ne!(
        prompts[0],
        mend::edit_prompt(&context, "add"),
        "it is not the edit handler"
    );
    assert_ne!(
        prompts[0],
        mend::test_prompt(&context),
        "it is not the test handler"
    );

    assert_eq!(
        out, response,
        "explain returns the completion verbatim — never a diff or JSON"
    );

    let after = std::fs::read_to_string("target.rs").expect("read back target");
    assert_eq!(after, source, "explain leaves the target file untouched");

    eprintln!("C1_EXPLAIN_OK");
}
"##;
    let p = run_probe("c1-explain", src);
    assert!(
        p.ran_ok && p.combined().contains("C1_EXPLAIN_OK"),
        "an Explain command must select exactly the explain handler; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// C1 — the `Edit` command selects the edit handler (and no other).
///
/// The probe builds a `Command::Edit { instruction: "rename add", apply: false,
/// json: false }`, dispatches it with a fenced whole-file response (which
/// `parse_response` accepts and `apply_edits` always applies), and observes: the
/// Completer was called *exactly once*, the recorded prompt equals
/// `edit_prompt(context, "rename add")` — and is *not* the explain prompt — proving
/// the edit handler ran with the instruction passed through, and the default
/// (dry-run) mode left the file unchanged and produced a diff (not JSON). A mutant
/// routing `Edit` elsewhere would build a different prompt or mishandle the edit.
#[test]
fn c1_edit_command_routes_to_edit_handler() {
    let src = r##"
fn main() {
    let source = "use std::fmt;\n\npub fn add(a: i32, b: i32) -> i32 { a + b }\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("m"),
        endpoint: String::from("e"),
        token_budget: 100000,
    };

    // A fenced whole-file edit: `parse_response` routes it to the fenced parser and
    // it replaces the whole file, so the edit handler completes cleanly.
    let response = "```\nEDITED FILE BODY\n```\n";
    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    let out = mend::run(
        mend::Command::Edit {
            path: std::path::PathBuf::from("target.rs"),
            instruction: String::from("rename add"),
            apply: false,
            json: false,
        },
        &config,
        &completer,
    )
    .expect("dispatch should route Edit to the edit handler");

    let prompts = completer.prompts();
    assert_eq!(prompts.len(), 1, "exactly one handler ran — one completer call");

    let structure = mend::extract_structure(source).expect("structure");
    let imports = mend::extract_imports(source).expect("imports");
    let context = mend::assemble_context(&structure, &imports, config.token_budget as usize);

    assert_eq!(
        prompts[0],
        mend::edit_prompt(&context, "rename add"),
        "the edit handler built the edit prompt with the instruction"
    );
    assert_ne!(
        prompts[0],
        mend::explain_prompt(&context, None),
        "it is not the explain handler"
    );

    // Default mode is dry-run: the file is untouched and the output is a diff,
    // never written and never JSON.
    let after = std::fs::read_to_string("target.rs").expect("read back target");
    assert_eq!(after, source, "the default (dry-run) edit leaves the file unchanged");
    assert!(
        mend::parse_json(&out).is_err(),
        "the dry-run output is a diff, not a JSON result"
    );
    assert!(!out.is_empty(), "the dry-run produced a non-empty diff");

    eprintln!("C1_EDIT_OK");
}
"##;
    let p = run_probe("c1-edit", src);
    assert!(
        p.ran_ok && p.combined().contains("C1_EDIT_OK"),
        "an Edit command must select exactly the edit handler; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// C1 — the `Fix` command selects the fix handler (and no other).
///
/// The probe builds a `Command::Fix { error: Some("boom"), apply: false, json:
/// false }`, dispatches it with a fenced whole-file response, and observes: the
/// Completer was called *exactly once*, the recorded prompt equals
/// `fix_prompt(context, "boom")` — and is *not* the edit prompt for the same text —
/// proving the fix handler ran with the error message embedded, and the default
/// mode left the file unchanged. A mutant routing `Fix` to the edit handler would
/// build `edit_prompt` instead of `fix_prompt` and fail the equality.
#[test]
fn c1_fix_command_routes_to_fix_handler() {
    let src = r##"
fn main() {
    let source = "use std::fmt;\n\npub fn add(a: i32, b: i32) -> i32 { a + b }\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("m"),
        endpoint: String::from("e"),
        token_budget: 100000,
    };

    let response = "```\nFIXED FILE BODY\n```\n";
    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    let out = mend::run(
        mend::Command::Fix {
            path: std::path::PathBuf::from("target.rs"),
            error: Some(String::from("boom")),
            apply: false,
            json: false,
        },
        &config,
        &completer,
    )
    .expect("dispatch should route Fix to the fix handler");

    let prompts = completer.prompts();
    assert_eq!(prompts.len(), 1, "exactly one handler ran — one completer call");

    let structure = mend::extract_structure(source).expect("structure");
    let imports = mend::extract_imports(source).expect("imports");
    let context = mend::assemble_context(&structure, &imports, config.token_budget as usize);

    assert_eq!(
        prompts[0],
        mend::fix_prompt(&context, "boom"),
        "the fix handler built the fix prompt with the error message"
    );
    assert_ne!(
        prompts[0],
        mend::edit_prompt(&context, "boom"),
        "it is the fix handler, not the edit handler"
    );

    let after = std::fs::read_to_string("target.rs").expect("read back target");
    assert_eq!(after, source, "the default (dry-run) fix leaves the file unchanged");
    assert!(
        mend::parse_json(&out).is_err(),
        "the dry-run output is a diff, not a JSON result"
    );

    eprintln!("C1_FIX_OK");
}
"##;
    let p = run_probe("c1-fix", src);
    assert!(
        p.ran_ok && p.combined().contains("C1_FIX_OK"),
        "a Fix command must select exactly the fix handler; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// C1 — the `Test` command selects the test handler (and no other).
///
/// The probe builds a `Command::Test { apply: false, json: false }` and dispatches
/// it with a response carrying *none* of the three edit-format markers. That raw
/// response is the discriminator: only the test handler treats the whole response
/// as a single whole-file edit *without* running `parse_response`; the edit/fix
/// handlers would feed it to `parse_response`, which would reject an unmarked
/// response as `MendError::Parse` and the dispatch would fail. So a clean success,
/// with the recorded prompt equal to `test_prompt(context)` and not `edit_prompt`,
/// proves the test handler (and only it) ran.
#[test]
fn c1_test_command_routes_to_test_handler() {
    let src = r##"
fn main() {
    let source = "use std::fmt;\n\npub fn add(a: i32, b: i32) -> i32 { a + b }\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("m"),
        endpoint: String::from("e"),
        token_budget: 100000,
    };

    // Raw test code carrying NONE of the three edit-format markers, so
    // `parse_response` (used by edit/fix) would reject it. Only the test handler,
    // which wraps the whole response as a whole-file edit, can succeed here.
    let response = "#[test]\nfn adds() { assert_eq!(add(2, 2), 4); }\n";
    assert!(!response.contains("```"), "fixture: response carries no fence marker");
    assert!(!response.contains("@@ "), "fixture: response carries no hunk marker");
    assert!(!response.contains("<<<<<<< SEARCH"), "fixture: response carries no search marker");

    let completer = mend::MockCompleter::new(vec![String::from(response)]);

    let out = mend::run(
        mend::Command::Test {
            path: std::path::PathBuf::from("target.rs"),
            apply: false,
            json: false,
        },
        &config,
        &completer,
    )
    .expect("dispatch should route Test to the test handler (whole-file, no parse)");

    let prompts = completer.prompts();
    assert_eq!(prompts.len(), 1, "exactly one handler ran — one completer call");

    let structure = mend::extract_structure(source).expect("structure");
    let imports = mend::extract_imports(source).expect("imports");
    let context = mend::assemble_context(&structure, &imports, config.token_budget as usize);

    assert_eq!(
        prompts[0],
        mend::test_prompt(&context),
        "the test handler built the test prompt"
    );
    assert_ne!(
        prompts[0],
        mend::edit_prompt(&context, ""),
        "it is the test handler, not the edit handler"
    );

    let after = std::fs::read_to_string("target.rs").expect("read back target");
    assert_eq!(after, source, "the default (dry-run) test leaves the file unchanged");
    assert!(!out.is_empty(), "the test handler produced a non-empty dry-run diff");

    eprintln!("C1_TEST_OK");
}
"##;
    let p = run_probe("c1-test", src);
    assert!(
        p.ran_ok && p.combined().contains("C1_TEST_OK"),
        "a Test command must select exactly the test handler; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// C1 — the command's `--apply` / `--json` flags drive which output the selected
/// handler produces, proving the dispatcher wires each command's parameters into
/// the right handler invocation (not just the bare handler choice).
///
/// One probe runs two scenarios on the same edit handler:
///   * `Edit { apply: true, json: false }` must route to Apply mode: the applied
///     whole-file body is *written to disk* and returned verbatim.
///   * `Edit { apply: false, json: true }` must route to Json mode: the output
///     *parses back as a JsonResult* carrying the target path, and the file on disk
///     is *not* written.
/// Together with the dry-run edit/fix/test probes (neither flag → diff, no write),
/// this pins all three output modes against the command's flags.
#[test]
fn c1_output_mode_follows_command_flags() {
    let src = r##"
fn main() {
    let source = "use std::fmt;\n\npub fn add(a: i32, b: i32) -> i32 { a + b }\n";

    let config = mend::Config {
        model: String::from("m"),
        endpoint: String::from("e"),
        token_budget: 100000,
    };

    // --apply -> Apply mode: writes the applied whole-file to disk.
    std::fs::write("target.rs", source).expect("write target");
    let resp_apply = "```\nAPPLIED FILE BODY\n```\n";
    let c_apply = mend::MockCompleter::new(vec![String::from(resp_apply)]);
    let out_apply = mend::run(
        mend::Command::Edit {
            path: std::path::PathBuf::from("target.rs"),
            instruction: String::from("x"),
            apply: true,
            json: false,
        },
        &config,
        &c_apply,
    )
    .expect("dispatch edit --apply");
    let after_apply = std::fs::read_to_string("target.rs").expect("read back target");
    assert_ne!(after_apply, source, "--apply writes new contents to disk");
    assert_eq!(after_apply, out_apply, "--apply returns exactly what it wrote");
    assert!(after_apply.contains("APPLIED FILE BODY"), "--apply wrote the applied body");

    // --json -> Json mode: output parses as a JsonResult; disk is untouched.
    std::fs::write("target.rs", source).expect("rewrite target");
    let resp_json = "```\nJSON FILE BODY\n```\n";
    let c_json = mend::MockCompleter::new(vec![String::from(resp_json)]);
    let out_json = mend::run(
        mend::Command::Edit {
            path: std::path::PathBuf::from("target.rs"),
            instruction: String::from("x"),
            apply: false,
            json: true,
        },
        &config,
        &c_json,
    )
    .expect("dispatch edit --json");
    let parsed = mend::parse_json(&out_json).expect("--json output parses as a JsonResult");
    assert_eq!(parsed.path, "target.rs", "--json result carries the target path");
    let after_json = std::fs::read_to_string("target.rs").expect("read back target");
    assert_eq!(after_json, source, "--json does not write to disk");

    eprintln!("C1_MODES_OK");
}
"##;
    let p = run_probe("c1-modes", src);
    assert!(
        p.ran_ok && p.combined().contains("C1_MODES_OK"),
        "the command's --apply/--json flags must drive the selected handler's output mode; \
         probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// C2 — the selected handler receives the *loaded config*.
///
/// The probe dispatches an `Explain` command with a config whose `token_budget` is
/// a tiny `5`, and pins that the recorded prompt equals
/// `explain_prompt(assemble_context(structure, imports, 5))` — i.e. the context was
/// budgeted with *this* config's value. To prove the budget genuinely flowed (and a
/// mutant passing some default config would be caught), it also asserts that the
/// same context assembled at a large budget differs, and that the prompt does *not*
/// match the large-budget prompt. So the handler demonstrably saw the loaded
/// config, not a substitute.
#[test]
fn c2_handler_receives_loaded_config() {
    let src = r##"
fn main() {
    let source = "use std::collections::BTreeMap;\nuse std::fmt::Display;\n\npub fn alpha() {}\n\npub fn beta() {}\n\npub struct Gamma { pub x: u32 }\n";
    std::fs::write("target.rs", source).expect("write target");

    // A deliberately tiny budget so its effect on the assembled context is visible.
    let config = mend::Config {
        model: String::from("m"),
        endpoint: String::from("e"),
        token_budget: 5,
    };

    let completer = mend::MockCompleter::new(vec![String::from("R")]);

    mend::run(
        mend::Command::Explain {
            path: std::path::PathBuf::from("target.rs"),
            symbol: None,
        },
        &config,
        &completer,
    )
    .expect("dispatch explain");

    let prompts = completer.prompts();
    assert_eq!(prompts.len(), 1, "exactly one handler ran");

    let structure = mend::extract_structure(source).expect("structure");
    let imports = mend::extract_imports(source).expect("imports");

    let context_small = mend::assemble_context(&structure, &imports, config.token_budget as usize);
    assert_eq!(
        prompts[0],
        mend::explain_prompt(&context_small, None),
        "the handler assembled context with the loaded config's budget (5)"
    );

    // The budget really mattered: a large budget assembles a different context.
    let context_large = mend::assemble_context(&structure, &imports, 100000);
    assert_ne!(
        context_small, context_large,
        "budget 5 truncates differently than a large budget"
    );
    assert_ne!(
        prompts[0],
        mend::explain_prompt(&context_large, None),
        "the dispatcher passed the loaded config, not a default budget"
    );

    eprintln!("C2_CONFIG_OK");
}
"##;
    let p = run_probe("c2-config", src);
    assert!(
        p.ran_ok && p.combined().contains("C2_CONFIG_OK"),
        "the selected handler must receive the loaded config; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// C2 — the selected handler receives the *constructed Completer*.
///
/// The probe constructs a `MockCompleter` scripted with a unique sentinel response,
/// dispatches an `Explain` command (whose handler returns the completion verbatim),
/// and pins that the dispatch return equals that exact sentinel — so the very
/// completer the caller constructed was the one the handler invoked — and that the
/// constructed completer recorded exactly one prompt. A mutant that built or used a
/// different completer could not return this sentinel nor record the call.
#[test]
fn c2_handler_receives_constructed_completer() {
    let src = r##"
fn main() {
    let source = "pub fn add(a: i32, b: i32) -> i32 { a + b }\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("m"),
        endpoint: String::from("e"),
        token_budget: 100000,
    };

    let sentinel = "UNIQUE_COMPLETER_RESPONSE_4242";
    let completer = mend::MockCompleter::new(vec![String::from(sentinel)]);

    let out = mend::run(
        mend::Command::Explain {
            path: std::path::PathBuf::from("target.rs"),
            symbol: None,
        },
        &config,
        &completer,
    )
    .expect("dispatch explain");

    assert_eq!(
        out, sentinel,
        "the handler invoked the constructed completer — its scripted response flowed through"
    );
    assert_eq!(
        completer.prompts().len(),
        1,
        "the constructed completer received exactly one prompt"
    );

    eprintln!("C2_COMPLETER_OK");
}
"##;
    let p = run_probe("c2-completer", src);
    assert!(
        p.ran_ok && p.combined().contains("C2_COMPLETER_OK"),
        "the selected handler must receive the constructed Completer; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// C3 — dispatch is an *exhaustive* match over `Command`: an unhandled variant is a
/// compile error, not a silent fall-through.
///
/// The probe contains a wildcard-free `match` that names *every* `Command` variant
/// — `Explain`, `Edit`, `Fix`, `Test`, `Usage`. Because it carries no `_` arm, it
/// compiles *only* if those are exactly the variants the enum exposes; a new,
/// unhandled variant would make this match (and, identically, the dispatcher's
/// own exhaustive match) fail to compile. In the red state `Command::Fix` and
/// `Command::Test` do not exist yet, so the probe does not build at all — which is
/// the redness for this criterion.
///
/// It also dispatches the non-handler `Usage` variant and asserts an error: the
/// dispatcher handles `Usage` *explicitly* (the exhaustive match has a real `Usage`
/// arm) rather than dropping it through a catch-all, so the diagnostic is surfaced
/// rather than silently dispatched to a handler.
#[test]
fn c3_dispatch_match_is_exhaustive_over_command() {
    let src = r##"
/// A wildcard-free match naming every Command variant. It compiles only while the
/// enum's variant set is exactly these five; a new variant makes it a compile
/// error — the exact mechanism C3 requires of the dispatcher's own match.
fn name_of(command: &mend::Command) -> &'static str {
    match command {
        mend::Command::Explain { .. } => "explain",
        mend::Command::Edit { .. } => "edit",
        mend::Command::Fix { .. } => "fix",
        mend::Command::Test { .. } => "test",
        mend::Command::Usage { .. } => "usage",
    }
}

fn main() {
    // Exercise every arm so the exhaustive, wildcard-free match is real.
    assert_eq!(
        name_of(&mend::Command::Explain {
            path: std::path::PathBuf::from("a"),
            symbol: None,
        }),
        "explain"
    );
    assert_eq!(
        name_of(&mend::Command::Edit {
            path: std::path::PathBuf::from("a"),
            instruction: String::new(),
            apply: false,
            json: false,
        }),
        "edit"
    );
    assert_eq!(
        name_of(&mend::Command::Fix {
            path: std::path::PathBuf::from("a"),
            error: None,
            apply: false,
            json: false,
        }),
        "fix"
    );
    assert_eq!(
        name_of(&mend::Command::Test {
            path: std::path::PathBuf::from("a"),
            apply: false,
            json: false,
        }),
        "test"
    );
    assert_eq!(
        name_of(&mend::Command::Usage {
            message: String::from("m"),
        }),
        "usage"
    );

    // The dispatcher handles the non-handler Usage variant explicitly (exhaustive
    // match, no catch-all): it surfaces an error rather than silently dispatching.
    let config = mend::Config {
        model: String::from("m"),
        endpoint: String::from("e"),
        token_budget: 100,
    };
    let completer = mend::MockCompleter::new(vec![]);
    let result = mend::run(
        mend::Command::Usage {
            message: String::from("bad usage"),
        },
        &config,
        &completer,
    );
    assert!(
        result.is_err(),
        "Usage is handled explicitly as an error, not silently dispatched to a handler"
    );

    eprintln!("C3_OK");
}
"##;
    let p = run_probe("c3-exhaustive", src);
    assert!(
        p.ran_ok && p.combined().contains("C3_OK"),
        "dispatch must be an exhaustive, wildcard-free match over Command; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}
