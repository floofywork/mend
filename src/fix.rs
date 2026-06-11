//! The repair-driven `fix` subcommand entry point.
//!
//! [`run_fix`] is the thin orchestration over the deterministic layers for a
//! repair: it reads the target file, gathers its context (extracted structure
//! plus imports, joined and budget-bounded), builds the *fix* prompt — embedding
//! the supplied `--error` message when one is given, and instructing the model to
//! infer the defect from the file when it is absent — hands that prompt to a
//! [`Completer`](crate::Completer), parses the response into a list of edits,
//! applies them, and routes the outcome through
//! [`dispatch_output`](crate::dispatch_output), identically to the `edit` command.
//! A missing error message degrades to inference, not failure. Auto-detecting the
//! error via a compiler and test generation are out of scope.

use std::path::Path;

use crate::apply::apply_edits;
use crate::context::assemble_context;
use crate::dispatch::parse_response;
use crate::dispatch_output::{dispatch_output, OutputMode};
use crate::imports::extract_imports;
use crate::prompt::fix_prompt;
use crate::structure::extract_structure;
use crate::{Completer, Result};

/// The instruction the absent-`--error` path embeds in the fix prompt, telling the
/// model to infer the defect from the file rather than failing.
const INFER_INSTRUCTION: &str =
    "No specific error was provided; infer the defect from the file and fix it.";

/// Run the `fix` subcommand over the file at `path`.
///
/// Reads the target file, extracts its structure and imports, assembles them into
/// a budget-bounded context, and builds `fix_prompt(context, error)` — embedding
/// the supplied `error` message when one is given, and `fix_prompt(context,
/// <inference instruction>)` when it is absent — then asks the
/// [`Completer`](crate::Completer) to complete it. The response is parsed into
/// edits, applied to the original source, and routed through
/// [`dispatch_output`](crate::dispatch_output) under the chosen [`OutputMode`];
/// the dispatcher's return is returned verbatim, identical to the `edit` command.
pub fn run_fix(
    path: &Path,
    error: Option<&str>,
    config: &crate::Config,
    completer: &dyn Completer,
    mode: OutputMode,
    color: bool,
) -> Result<String> {
    let source = std::fs::read_to_string(path)?;
    let structure = extract_structure(&source)?;
    let imports = extract_imports(&source)?;
    let context = assemble_context(&structure, &imports, config.token_budget as usize);
    let prompt = fix_prompt(&context, error.unwrap_or(INFER_INSTRUCTION));
    let response = completer.complete(&prompt)?;
    let edits = parse_response(&response)?;
    let applied = apply_edits(&source, &edits);
    dispatch_output(path, &source, applied, edits, mode, color)
}
