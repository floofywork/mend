//! Locate an edit's search text in the source by exact line-range equality.
//!
//! [`match_exact`] is the precise half of the edit matcher. Given the source
//! text and an edit's search text, it counts the occurrences of the search text
//! in the source and partitions the outcome: zero occurrences is
//! [`ExactMatch::NoMatch`], exactly one is [`ExactMatch::Unique`] carrying the
//! matched byte range, and two or more is [`ExactMatch::Ambiguous`]. Absence and
//! multiplicity are *signalled in-band* — returned as variants, never errored or
//! panicked. The returned range is byte-equal to the search text
//! (`&source[range] == search`). Matching is exact: no whitespace tolerance, no
//! fuzzy logic (that is a sibling task's job), and the source is never mutated.

use std::ops::Range;

/// The outcome of locating a search text in the source.
///
/// The occurrence count alone decides the variant: `0 → NoMatch`,
/// `1 → Unique`, `>= 2 → Ambiguous`. The signals are returned in-band rather
/// than via [`Result`](crate::Result), because absence and multiplicity are
/// ordinary outcomes of matching, not errors.
#[derive(Debug)]
pub enum ExactMatch {
    /// The search text occurs exactly once; carries its byte range in the
    /// source, with the invariant `&source[range] == search`.
    Unique(Range<usize>),
    /// The search text does not occur in the source.
    NoMatch,
    /// The search text occurs more than once, so no single range is meant.
    Ambiguous,
}

/// Locate `search` in `source` by exact equality.
///
/// Returns [`ExactMatch::Unique`] with the byte range of the sole occurrence,
/// [`ExactMatch::NoMatch`] if `search` is absent, or [`ExactMatch::Ambiguous`]
/// if it occurs more than once. Never panics and never mutates `source`.
pub fn match_exact(source: &str, search: &str) -> ExactMatch {
    let mut occurrences = source.match_indices(search);
    let first = match occurrences.next() {
        Some((start, _)) => start,
        None => return ExactMatch::NoMatch,
    };
    if occurrences.next().is_some() {
        return ExactMatch::Ambiguous;
    }
    ExactMatch::Unique(first..first + search.len())
}
