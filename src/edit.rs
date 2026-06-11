//! The instruction-driven `edit` subcommand entry point.
//!
//! [`run_edit`] is the thin orchestration over the deterministic layers for an
//! instruction-driven mutation: it reads the target file, gathers its context
//! (extracted structure plus imports, joined and budget-bounded), builds the edit
//! prompt embedding the user's instruction verbatim, hands that prompt to a
//! [`Completer`](crate::Completer), parses the response into a list of edits,
//! applies them, and routes the outcome through
//! [`dispatch_output`](crate::dispatch_output). An unparseable response surfaces
//! verbatim as a [`MendError::Parse`](crate::MendError::Parse), which the binary
//! turns into a non-zero exit with the message on stderr. Error-message handling
//! and test generation are other commands' concerns.

use std::path::Path;

use crate::apply::apply_edits;
use crate::context::assemble_context;
use crate::dispatch::parse_response;
use crate::dispatch_output::{dispatch_output, OutputMode};
use crate::imports::extract_imports;
use crate::prompt::edit_prompt;
use crate::structure::extract_structure;
use crate::{Completer, Result};

/// Run the `edit` subcommand over the file at `path`.
///
/// Reads the target file, extracts its structure and imports, assembles them into
/// a budget-bounded context, builds `edit_prompt(context, instruction)` — with the
/// instruction embedded unchanged — and asks the [`Completer`](crate::Completer)
/// to complete it. The response is parsed into edits, applied to the original
/// source, and routed through [`dispatch_output`](crate::dispatch_output) under
/// the chosen [`OutputMode`]; the dispatcher's return is returned verbatim. An
/// unparseable response surfaces as [`MendError::Parse`](crate::MendError::Parse)
/// and the target file is left untouched.
pub fn run_edit(
    path: &Path,
    instruction: &str,
    config: &crate::Config,
    completer: &dyn Completer,
    mode: OutputMode,
    color: bool,
) -> Result<String> {
    let source = std::fs::read_to_string(path)?;
    let structure = extract_structure(&source)?;
    let imports = extract_imports(&source)?;
    let context = assemble_context(&structure, &imports, config.token_budget as usize);
    let prompt = edit_prompt(&context, instruction);
    let response = completer.complete(&prompt)?;
    let edits = parse_response(&response)?;
    let applied = apply_edits(&source, &edits);
    dispatch_output(path, &source, applied, edits, mode, color)
}
