//! Frozen acceptance tests for T-07.01 — Run explain command.
//!
//! The task adds a read-only subcommand entry point. Its contract is behavioural
//! (gather context → build the explain prompt → call the Completer → surface the
//! response, with a Completer failure becoming a non-zero exit whose message lands
//! on stderr), so — exactly as the T-01.01 `Completer` tests do — the honest way to
//! observe it is to *compile and run a tiny probe program against the real `mend`
//! crate*. Each probe drives the real entry point with a [`mend::MockCompleter`],
//! so the whole flow runs deterministically and without a network.
//!
//! This test crate stays std-only and never names the not-yet-existing entry
//! point directly, so it always *compiles*. Redness comes from each probe failing
//! to *build* (the `run_explain` function does not exist yet), not from this file
//! failing to parse. Every probe is built into an isolated `CARGO_TARGET_DIR` and
//! run `--offline`, so it neither contends with the outer build lock nor touches
//! the network.
//!
//! The probes pin the contract the criteria imply:
//!
//!   fn run_explain(
//!       path: &Path,
//!       symbol: Option<&str>,
//!       config: &mend::Config,
//!       completer: &dyn mend::Completer,
//!   ) -> mend::Result<String>
//!
//! On success it returns the model's explanation text (what the binary prints to
//! stdout); on a Completer failure it returns that failure unchanged (what the
//! binary surfaces on stderr before exiting non-zero). The prompt it hands the
//! Completer is `explain_prompt(assemble_context(structure, imports, budget),
//! symbol)` over the target file — gathered context, never an edit to the file.
//!
//!   C1: gathers context, builds the explain prompt, calls the Completer, and
//!       returns/prints the response text (and leaves the target file untouched).
//!   C2: with `--symbol` the prompt targets that symbol; absent, it targets the
//!       whole file.
//!   C3: a Completer error exits non-zero with the message on stderr.

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
    dir.push(format!("mend-explain-{tag}-{}-{nanos}", std::process::id()));
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
/// In the red state `mend` has no `run_explain`, so the probe fails to resolve and
/// `ran_ok` is false. Once the library defines the entry point the probe
/// references, it builds, runs, and prints its sentinel.
fn run_probe(tag: &str, main_src: &str) -> Probe {
    let crate_dir = unique_temp_dir(tag);
    let target_dir = unique_temp_dir(&format!("{tag}-target"));

    // Literal (single-quoted) TOML string so backslashes in the path are not
    // treated as escapes on any platform.
    let manifest = format!(
        "[package]\n\
         name = \"mend_explain_probe\"\n\
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

/// C1 — gathers context, builds the explain prompt, calls the Completer, and
/// returns/prints the response text; the target file is left untouched.
///
/// The probe writes a target file carrying both an importable `use` and a public
/// `fn`, drives `run_explain` with a `MockCompleter` scripted to return a known
/// explanation, and checks four things:
///   * the returned text (printed to stdout) is exactly the scripted completion —
///     the response is surfaced, not swallowed or rewritten;
///   * the Completer was called exactly once, with the prompt the deterministic
///     layers compose — `explain_prompt(assemble_context(structure, imports,
///     budget), None)` — so context really was gathered and the explain prompt
///     really was built (it embeds both the `fn` signature and the import path);
///   * the target file is byte-for-byte unchanged after the call (read-only).
#[test]
fn c1_gathers_context_builds_prompt_calls_completer_prints_response() {
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
    let completer = mend::MockCompleter::new(vec![String::from("THE_EXPLANATION_42")]);

    let out = mend::run_explain(Path::new("target.rs"), None, &config, &completer)
        .expect("run_explain should succeed when the completer succeeds");

    // The response text is surfaced verbatim (this is what the binary prints).
    print!("{out}");
    assert_eq!(out, "THE_EXPLANATION_42", "explain returns the completer's response verbatim");

    // The completer was called exactly once.
    let prompts = completer.prompts();
    assert_eq!(prompts.len(), 1, "the completer is called exactly once");

    // The prompt is the explain prompt over the gathered context, with no symbol.
    let structure = mend::extract_structure(source).expect("structure");
    let imports = mend::extract_imports(source).expect("imports");
    let context = mend::assemble_context(&structure, &imports, config.token_budget as usize);
    let expected = mend::explain_prompt(&context, None);
    assert_eq!(prompts[0], expected, "prompt is explain_prompt(assemble_context(...), None)");

    // Context gathering really embedded both halves: structure and imports.
    assert!(prompts[0].contains("greet"), "prompt embeds the file's structure (the fn)");
    assert!(prompts[0].contains("std::collections::BTreeMap"), "prompt embeds the file's imports");

    // Invariant: explain never edits the target file.
    let after = std::fs::read_to_string("target.rs").expect("read back target");
    assert_eq!(after, source, "explain must leave the target file untouched");

    eprintln!("C1_OK");
}
"##;
    let p = run_probe("c1", src);
    assert!(
        p.ran_ok && p.combined().contains("THE_EXPLANATION_42") && p.combined().contains("C1_OK"),
        "explain must gather context, build the explain prompt, call the completer, and \
         return the response text while leaving the file untouched; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// C2 — with `--symbol` the prompt targets that symbol; absent, it targets the
/// whole file.
///
/// The probe runs `run_explain` twice over the same file — once with
/// `Some("alpha")` and once with `None` — and pins each resulting prompt against
/// the frozen `explain_prompt` for that symbol option. Both sides of the symbol
/// boundary are checked: the symbol-targeted prompt names the symbol and equals
/// the `Some` form; the symbol-absent prompt equals the `None` form; and the two
/// differ, proving the symbol actually steers the target.
#[test]
fn c2_symbol_targets_symbol_else_whole_file() {
    let src = r##"
use std::path::Path;

fn expected(source: &str, symbol: Option<&str>) -> String {
    let structure = mend::extract_structure(source).expect("structure");
    let imports = mend::extract_imports(source).expect("imports");
    let config_budget = 100000usize;
    let context = mend::assemble_context(&structure, &imports, config_budget);
    mend::explain_prompt(&context, symbol)
}

fn main() {
    let source = "pub fn alpha() {}\npub fn beta() {}\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("m"),
        endpoint: String::from("e"),
        token_budget: 100000,
    };

    // With --symbol: the prompt targets that symbol.
    let with = mend::MockCompleter::new(vec![String::from("R1")]);
    mend::run_explain(Path::new("target.rs"), Some("alpha"), &config, &with)
        .expect("ok with symbol");
    let p_with = with.prompts()[0].clone();
    assert!(p_with.contains("alpha"), "the symbol-targeted prompt names the symbol");
    assert_eq!(p_with, expected(source, Some("alpha")),
        "with --symbol, the prompt is explain_prompt(context, Some(symbol))");

    // Absent --symbol: the prompt targets the whole file.
    let without = mend::MockCompleter::new(vec![String::from("R2")]);
    mend::run_explain(Path::new("target.rs"), None, &config, &without)
        .expect("ok without symbol");
    let p_without = without.prompts()[0].clone();
    assert_eq!(p_without, expected(source, None),
        "absent --symbol, the prompt is explain_prompt(context, None)");

    // The symbol presence actually changes the target.
    assert!(p_with != p_without, "symbol presence steers the prompt target");

    eprintln!("C2_OK");
}
"##;
    let p = run_probe("c2", src);
    assert!(
        p.ran_ok && p.combined().contains("C2_OK"),
        "with --symbol the prompt must target the symbol, and absent it must target the \
         whole file; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}

/// C3 — a Completer error exits non-zero with the message on stderr.
///
/// The probe drives `run_explain` with an *empty* mock script, so the Completer
/// fails. It asserts the call returns `Err` (a mutant that swallowed the failure
/// and returned `Ok` would print the success sentinel and exit 0, flipping this
/// test green→red and so being killed), and that the error preserves the
/// Completer's own message. It then surfaces that message on stderr and exits
/// non-zero — exactly the binary's behaviour. The outer test requires a non-zero
/// exit *and* the completer message on stderr (not stdout), with no success
/// sentinel anywhere.
#[test]
fn c3_completer_error_exits_nonzero_message_on_stderr() {
    let src = r##"
use std::path::Path;
use std::process::exit;

fn main() {
    let source = "pub fn f() {}\n";
    std::fs::write("target.rs", source).expect("write target");

    let config = mend::Config {
        model: String::from("m"),
        endpoint: String::from("e"),
        token_budget: 100000,
    };
    // An empty script makes the mock completer fail on the first call.
    let completer = mend::MockCompleter::new(vec![]);

    match mend::run_explain(Path::new("target.rs"), None, &config, &completer) {
        Ok(text) => {
            // Must NOT succeed when the completer fails.
            println!("UNEXPECTED_OK:{text}");
            exit(0);
        }
        Err(e) => {
            // The error must be the completer's, message preserved.
            let msg = e.to_string();
            assert!(msg.contains("completer error"), "surfaces a completer error: {msg}");
            // The target file must still be untouched on the error path.
            let after = std::fs::read_to_string("target.rs").expect("read back target");
            assert_eq!(after, source, "explain must not edit the file even when the completer fails");
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
        "a completer error must exit non-zero; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
    assert!(
        p.stderr.contains("completer error"),
        "the completer error message must be surfaced on stderr; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
    assert!(
        !p.combined().contains("UNEXPECTED_OK"),
        "explain must not succeed when the completer fails; probe stdout:\n{}\nstderr:\n{}",
        p.stdout,
        p.stderr
    );
}
