//! The top-level subcommand dispatcher: from a parsed [`Command`] to a handler.
//!
//! [`run`] is pure selection and injection. It matches a parsed [`Command`]
//! exhaustively and routes to exactly one of the `run_explain` / `run_edit` /
//! `run_fix` / `run_test` handlers, handing each the loaded [`Config`] and the
//! constructed [`Completer`] so the chosen handler does the work. The
//! filesystem-touching commands carry `--apply` / `--json` flags, which
//! [`output_mode`] turns into the [`OutputMode`] the handler runs under. The
//! non-handler [`Command::Usage`] variant is surfaced as an error rather than
//! dispatched to a handler. The match carries no `_` wildcard, so a new,
//! unhandled [`Command`] variant is a compile error rather than a silent
//! fall-through. Handler logic itself lives in the handler modules, not here.

use crate::cli::Command;
use crate::dispatch_output::OutputMode;
use crate::edit::run_edit;
use crate::explain::run_explain;
use crate::fix::run_fix;
use crate::test_cmd::run_test;
use crate::{Completer, Config, MendError, Result};

/// Translate a command's `--apply` / `--json` flags into an [`OutputMode`].
///
/// `--apply` selects [`OutputMode::Apply`] (write to disk), `--json` selects
/// [`OutputMode::Json`] (machine-readable result), and neither selects the
/// default [`OutputMode::DryRun`] (render a diff, touch nothing).
fn output_mode(apply: bool, json: bool) -> OutputMode {
    if apply {
        OutputMode::Apply
    } else if json {
        OutputMode::Json
    } else {
        OutputMode::DryRun
    }
}

/// Route a parsed [`Command`] to its handler, returning the handler's output.
///
/// Matches `command` exhaustively and invokes exactly one handler, handing it
/// the loaded `config` and the constructed `completer`; the `--apply` / `--json`
/// flags choose the handler's [`OutputMode`]. [`Command::Usage`] is handled
/// explicitly as an error. The wildcard-free match makes a new [`Command`]
/// variant a compile error.
pub fn run(command: Command, config: &Config, completer: &dyn Completer) -> Result<String> {
    match command {
        Command::Explain { path, symbol } => {
            run_explain(&path, symbol.as_deref(), config, completer)
        }
        Command::Edit {
            path,
            instruction,
            apply,
            json,
        } => run_edit(
            &path,
            &instruction,
            config,
            completer,
            output_mode(apply, json),
            false,
        ),
        Command::Fix {
            path,
            error,
            apply,
            json,
        } => run_fix(
            &path,
            error.as_deref(),
            config,
            completer,
            output_mode(apply, json),
            false,
        ),
        Command::Test { path, apply, json } => {
            run_test(&path, config, completer, output_mode(apply, json), false)
        }
        Command::Usage { message } => Err(MendError::Config(message)),
    }
}
