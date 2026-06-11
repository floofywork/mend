//! The test-generation `test` subcommand entry point.
//!
//! [`run_test`] is the thin orchestration over the deterministic layers for test
//! generation: it reads the target file, gathers its context (extracted *public*
//! structure plus imports, joined and budget-bounded), builds the *test* prompt,
//! hands that prompt to a [`Completer`](crate::Completer), and treats the whole
//! generated response as a single whole-file edit — the generated tests are not
//! parsed as one of the edit formats. Because the test file does not exist yet,
//! the edit is applied against an *empty* original and routed through
//! [`dispatch_output`](crate::dispatch_output), so a new test file diffs against
//! empty. Only the public API is targeted, because [`extract_structure`] keeps
//! only `pub` items. Running the generated tests and measuring coverage are out of
//! scope.

use std::path::Path;

use crate::apply::apply_edits;
use crate::context::assemble_context;
use crate::dispatch_output::{dispatch_output, OutputMode};
use crate::imports::extract_imports;
use crate::parsed_edit::ParsedEdit;
use crate::prompt::test_prompt;
use crate::structure::extract_structure;
use crate::{Completer, Result};

/// Run the `test` subcommand over the file at `path`.
///
/// Reads the target file, extracts its public structure and imports, assembles
/// them into a budget-bounded context, and builds `test_prompt(context)` — which
/// requests tests for the public API only — then asks the
/// [`Completer`](crate::Completer) to complete it. The whole response is treated
/// as a single [`ParsedEdit::WholeFile`] (the generated tests are not re-parsed),
/// applied against an *empty* original because the test file does not yet exist,
/// and routed through [`dispatch_output`](crate::dispatch_output) under the chosen
/// [`OutputMode`]; the dispatcher's return is returned verbatim.
pub fn run_test(
    path: &Path,
    config: &crate::Config,
    completer: &dyn Completer,
    mode: OutputMode,
    color: bool,
) -> Result<String> {
    let source = std::fs::read_to_string(path)?;
    let structure = extract_structure(&source)?;
    let imports = extract_imports(&source)?;
    let context = assemble_context(&structure, &imports, config.token_budget as usize);
    let prompt = test_prompt(&context);
    let response = completer.complete(&prompt)?;
    let edits = vec![ParsedEdit::WholeFile { body: response }];
    let applied = apply_edits("", &edits);
    dispatch_output(path, "", applied, edits, mode, color)
}
