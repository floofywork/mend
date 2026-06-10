//! Apply a validated, ordered edit set to source text, in memory.
//!
//! [`apply_edits`] is the engine that turns a source string and an ordered slice
//! of [`ParsedEdit`]s into the rewritten text. It is pure: it never writes to
//! disk and never renders a diff (both out of scope). Application is
//! *all-or-nothing* — the edits are folded over a working copy in order, and the
//! first edit that cannot apply aborts the whole pass with [`MendError::Patch`],
//! so the caller's source binding is never partially rewritten.
//!
//! Two edit shapes are handled:
//!   - **Whole-file** — the edit's `body` becomes the entire result, discarding
//!     whatever came before it.
//!   - **Search/replace** — the sole occurrence of `search` in the working copy
//!     is spliced out for `replace`. An absent search text (whether genuinely
//!     missing or made stale by an earlier edit's rewrite) is a conflict and
//!     aborts with `Patch`.

use crate::match_exact::{match_exact, ExactMatch};
use crate::parsed_edit::ParsedEdit;
use crate::{MendError, Result};

/// Apply an ordered edit set to `source`, returning the rewritten text.
///
/// The edits are applied in order to a working copy of `source`. A whole-file
/// edit replaces the working copy with its body; a search/replace edit splices
/// its `replace` over the unique occurrence of its `search`. If any
/// search/replace edit has no unique match against the working copy at its turn
/// — a no-match or a conflict surfacing as a stale match — the whole application
/// aborts with [`MendError::Patch`] and `source` is left untouched. On success
/// the fully rewritten text is returned.
pub fn apply_edits(source: &str, edits: &[ParsedEdit]) -> Result<String> {
    let mut result = source.to_string();
    for edit in edits {
        match edit {
            ParsedEdit::WholeFile { body } => {
                result = body.clone();
            }
            ParsedEdit::SearchReplace { search, replace } => match match_exact(&result, search) {
                ExactMatch::Unique(range) => {
                    let mut out = String::new();
                    out.push_str(&result[..range.start]);
                    out.push_str(replace);
                    out.push_str(&result[range.end..]);
                    result = out;
                }
                _ => {
                    return Err(MendError::Patch(format!(
                        "search text has no unique match: {search:?}"
                    )))
                }
            },
        }
    }
    Ok(result)
}
