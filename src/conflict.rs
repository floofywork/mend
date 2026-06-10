//! Detect conflicts in an ordered edit set before any application.
//!
//! [`detect_conflicts`] is the validation pass that runs *before* the applier
//! touches the source. Given the source text and an ordered slice of
//! [`ParsedEdit`]s, it returns the list of [`Conflict`]s that would stop the set
//! from applying cleanly. An *empty* list is the success signal — its emptiness
//! is the invariant the applier relies on, so it can assume validated input and
//! never re-check. The pass is pure: it never mutates the source and resolves
//! nothing (resolution is out of scope).
//!
//! Two independent checks make up the pass:
//!   - **Overlap** — two edits whose matched source ranges intersect cannot both
//!     apply, since each would rewrite bytes the other claims. Ranges that merely
//!     *touch* (one ends exactly where the next begins) share no byte and are
//!     fine.
//!   - **Stale match** — an edit whose search text no longer matches uniquely
//!     once the prior edits apply in order. The earlier edits' replacements can
//!     erase or duplicate the text a later edit targets, leaving it with nothing
//!     clean to match.

use std::ops::Range;

use crate::match_exact::{match_exact, ExactMatch};
use crate::parsed_edit::ParsedEdit;

/// A reason an edit set cannot apply cleanly, named by edit position.
///
/// Conflicts identify their edits by index into the validated slice rather than
/// by value, so a caller can map a conflict straight back to the offending edit.
#[derive(Debug)]
pub enum Conflict {
    /// Two edits whose matched source ranges overlap; `first < second` are their
    /// positions in the edit slice.
    Overlap {
        /// Position of the earlier edit in the slice.
        first: usize,
        /// Position of the later edit in the slice.
        second: usize,
    },
    /// An edit whose search text no longer matches uniquely once the prior edits
    /// apply; `index` is its position in the edit slice.
    StaleMatch {
        /// Position of the stale edit in the slice.
        index: usize,
    },
}

/// Validate an ordered edit set against `source`, returning every conflict.
///
/// Returns an empty `Vec` exactly when the set applies cleanly: no two edits'
/// matched ranges overlap, and every edit's search still matches uniquely once
/// the edits before it apply in order. Never mutates `source`.
pub fn detect_conflicts(source: &str, edits: &[ParsedEdit]) -> Vec<Conflict> {
    let mut conflicts = Vec::new();

    // The matched byte range of each edit in the *original* source, or `None`
    // when it is not a search/replace edit with a unique match.
    let ranges: Vec<Option<Range<usize>>> = edits
        .iter()
        .map(|edit| match edit {
            ParsedEdit::SearchReplace { search, .. } => match match_exact(source, search) {
                ExactMatch::Unique(range) => Some(range),
                _ => None,
            },
            ParsedEdit::WholeFile { .. } => None,
        })
        .collect();

    // Overlap: every pair of edits (by ascending position) whose matched ranges
    // intersect. Two ranges intersect when the later of their starts precedes
    // the earlier of their ends; ranges that only touch leave that strict.
    for first in 0..edits.len() {
        for second in (first + 1)..edits.len() {
            if let (Some(a), Some(b)) = (&ranges[first], &ranges[second]) {
                if a.start.max(b.start) < a.end.min(b.end) {
                    conflicts.push(Conflict::Overlap { first, second });
                }
            }
        }
    }

    // Stale match: every edit whose search no longer matches uniquely once the
    // edits before it apply, in order, to a working copy of the source.
    for index in 0..edits.len() {
        if let ParsedEdit::SearchReplace { search, .. } = &edits[index] {
            let mut applied = source.to_string();
            for prior in &edits[..index] {
                if let ParsedEdit::SearchReplace { search, replace } = prior {
                    applied = apply(&applied, search, replace);
                }
            }
            match match_exact(&applied, search) {
                ExactMatch::Unique(_) => {}
                _ => conflicts.push(Conflict::StaleMatch { index }),
            }
        }
    }

    conflicts
}

/// Apply one search/replace edit to `source`, splicing `replace` over the sole
/// occurrence of `search`. If `search` does not match uniquely, `source` is
/// returned unchanged — only the unique-match case advances the working copy.
fn apply(source: &str, search: &str, replace: &str) -> String {
    match match_exact(source, search) {
        ExactMatch::Unique(range) => {
            let mut out = String::new();
            out.push_str(&source[..range.start]);
            out.push_str(replace);
            out.push_str(&source[range.end..]);
            out
        }
        _ => source.to_string(),
    }
}
