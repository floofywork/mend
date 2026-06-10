//! Locate an edit's search text in the source by whitespace-tolerant line matching.
//!
//! [`match_fuzzy`] is the *tolerant* half of the edit matcher: the fallback used
//! when [`match_exact`](crate::match_exact) fails. Given the source text and an
//! edit's search text, it slides a same-length window over the source lines and
//! compares each line to the search after trimming both sides' leading and
//! trailing whitespace, so a region differing only in indentation still matches.
//!
//! The best-aligned window yields a similarity `score` in `[0, 1]` — the fraction
//! of search lines that match (after trimming) — alongside the byte range of that
//! window in the source. A fixed threshold gates acceptance: a best window scoring
//! below it is rejected as [`FuzzyMatch::NoMatch`] rather than forced, so any
//! accepted [`FuzzyMatch::Match`] scores at or above the threshold. Rejection is
//! signalled *in-band* — returned as a variant, never errored or panicked — and
//! the source is never mutated.

use std::ops::Range;

/// The fixed similarity threshold gating acceptance. A best window must score at
/// or above this fraction of matching (trimmed) lines to be accepted; anything
/// below is rejected as [`FuzzyMatch::NoMatch`].
const THRESHOLD: f64 = 2.0 / 3.0;

/// The outcome of a whitespace-tolerant match.
///
/// Either the best-aligned window scored at or above [`THRESHOLD`], yielding a
/// [`Match`](FuzzyMatch::Match) with its byte range and similarity score, or no
/// window cleared the bar, yielding [`NoMatch`](FuzzyMatch::NoMatch). Both are
/// ordinary outcomes returned in-band, not errors.
#[derive(Debug)]
pub enum FuzzyMatch {
    /// The best window cleared the threshold; carries its byte range in the
    /// source and a similarity `score` in `[0, 1]` (`score >= THRESHOLD`).
    Match {
        /// The byte range of the matched window in the source.
        range: Range<usize>,
        /// The fraction of search lines matching after trimming.
        score: f64,
    },
    /// No window cleared the threshold, so the candidate is rejected.
    NoMatch,
}

/// The trimmed content of each line of `text`, in order.
fn line_trims(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').map(|line| line.trim()).collect()
}

/// Each line of `source` as `(start, end, trimmed)`, where `start..end` is the
/// line's byte range (including its trailing newline) and `trimmed` is its
/// content with leading and trailing whitespace removed.
fn line_spans(source: &str) -> Vec<(usize, usize, &str)> {
    let mut spans = Vec::new();
    let mut start = 0;
    for line in source.split_inclusive('\n') {
        let end = start + line.len();
        spans.push((start, end, line.trim()));
        start = end;
    }
    spans
}

/// Locate `search` in `source` by whitespace-tolerant per-line comparison.
///
/// Returns [`FuzzyMatch::Match`] with the byte range and similarity score of the
/// best-aligned source window when it scores at or above [`THRESHOLD`], or
/// [`FuzzyMatch::NoMatch`] when no window clears the bar. Never panics and never
/// mutates `source`.
pub fn match_fuzzy(source: &str, search: &str) -> FuzzyMatch {
    let search_lines = line_trims(search);
    if search_lines.is_empty() {
        return FuzzyMatch::NoMatch;
    }
    let n = search_lines.len();
    let spans = line_spans(source);

    let best = spans
        .windows(n)
        .map(|window| {
            let matches = (0..n)
                .filter(|&k| window[k].2 == search_lines[k])
                .count();
            (matches, window[0].0, window.last().unwrap().1)
        })
        .max_by_key(|&(matches, _, _)| matches);

    match best {
        Some((matches, start, end)) => {
            let score = matches as f64 / n as f64;
            if score >= THRESHOLD {
                FuzzyMatch::Match { range: start..end, score }
            } else {
                FuzzyMatch::NoMatch
            }
        }
        None => FuzzyMatch::NoMatch,
    }
}
