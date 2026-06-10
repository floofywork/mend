//! The single seam that decides what reaches the user and the disk.
//!
//! Every subcommand funnels its outcome through [`dispatch_output`], so the
//! contract for "what the user sees and what lands on disk" lives in exactly one
//! place. Its inputs are the applied result (the proposed text on success or the
//! patch engine's [`MendError::Patch`] on failure), the original text, the edit
//! list, the target path, and the chosen [`OutputMode`]. Exactly one of three
//! modes runs:
//!   - [`OutputMode::DryRun`] (the default) returns the coloured rendered diff
//!     between `original` and the proposed text and writes nothing.
//!   - [`OutputMode::Apply`] writes the proposed text to `path` and is the only
//!     mode that ever touches disk; an already-failed apply is propagated and
//!     nothing is written.
//!   - [`OutputMode::Json`] returns the emitted [`JsonResult`] JSON, renders no
//!     diff, and never touches disk, so its `applied` flag is false.

use crate::{
    compute_unified_diff, emit_json, render_diff, JsonResult, ParsedEdit, Result,
};
use std::path::Path;

/// The output mode selecting what `dispatch_output` does with a result.
///
/// Exactly one of the three modes runs per invocation: render a diff for human
/// reading ([`DryRun`](OutputMode::DryRun)), write the proposed text to disk
/// ([`Apply`](OutputMode::Apply)), or emit the machine-readable JSON result
/// ([`Json`](OutputMode::Json)).
#[derive(Debug, PartialEq)]
pub enum OutputMode {
    /// Print the coloured diff and modify no file (the default).
    DryRun,
    /// Write the proposed text to the target path on disk.
    Apply,
    /// Print the JSON result and render no diff.
    Json,
}

/// Route an applied result to the chosen output mode.
///
/// `applied` is the patch engine's outcome — the proposed text on success or its
/// [`MendError::Patch`](crate::MendError::Patch) on failure. In
/// [`DryRun`](OutputMode::DryRun) the returned text is the colour-aware render of
/// the unified diff between `original` and the proposed text, and no file is
/// touched. In [`Apply`](OutputMode::Apply) the proposed text is written to
/// `path`; a failed `applied` is propagated unchanged, so a patch failure writes
/// nothing. In [`Json`](OutputMode::Json) the returned text is the emitted
/// [`JsonResult`] JSON with `applied` false, and no diff is rendered and no file
/// touched.
pub fn dispatch_output(
    path: &Path,
    original: &str,
    applied: Result<String>,
    edits: Vec<ParsedEdit>,
    mode: OutputMode,
    color: bool,
) -> Result<String> {
    match mode {
        OutputMode::DryRun => {
            let proposed = applied?;
            Ok(render_diff(&compute_unified_diff(original, &proposed), color))
        }
        OutputMode::Apply => {
            let proposed = applied?;
            std::fs::write(path, &proposed)?;
            Ok(proposed)
        }
        OutputMode::Json => {
            let proposed = applied?;
            Ok(emit_json(&JsonResult {
                path: path.display().to_string(),
                original: original.to_string(),
                proposed,
                edits,
                applied: false,
            }))
        }
    }
}
