//! The read-only `explain` subcommand entry point.
//!
//! [`run_explain`] is the thin orchestration over the deterministic layers for
//! explanation: it reads the target file, gathers its context (extracted
//! structure plus imports, joined and budget-bounded), builds the explain prompt
//! — targeting a symbol when one is given, the whole file when not — hands that
//! prompt to a [`Completer`](crate::Completer), and returns the model's response
//! text. It never edits the target file, and a Completer failure surfaces
//! unchanged as a [`MendError::Completer`](crate::MendError::Completer), which the
//! binary turns into a non-zero exit with the message on stderr.

use std::path::Path;

use crate::context::assemble_context;
use crate::imports::extract_imports;
use crate::prompt::explain_prompt;
use crate::structure::extract_structure;
use crate::{Completer, Result};

/// Run the `explain` subcommand over the file at `path`.
///
/// Reads the target file, extracts its structure and imports, assembles them into
/// a budget-bounded context, builds `explain_prompt(context, symbol)`, and returns
/// the [`Completer`](crate::Completer)'s response. When `symbol` is `Some` the
/// prompt targets that symbol; when `None` it targets the whole file. The target
/// file is only read, never written. A Completer failure is returned unchanged.
pub fn run_explain(
    path: &Path,
    symbol: Option<&str>,
    config: &crate::Config,
    completer: &dyn Completer,
) -> Result<String> {
    let source = std::fs::read_to_string(path)?;
    let structure = extract_structure(&source)?;
    let imports = extract_imports(&source)?;
    let context = assemble_context(&structure, &imports, config.token_budget as usize);
    let prompt = explain_prompt(&context, symbol);
    completer.complete(&prompt)
}
