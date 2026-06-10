//! Parse single-file unified diffs into search/replace edits.
//!
//! [`parse_unified_diff`] turns an LLM response string into one
//! [`ParsedEdit::SearchReplace`] per `@@ … @@` hunk. Each line after the header is
//! classified by its leading marker, which is stripped from the stored content: a
//! leading-space line is context and feeds both halves, a `-` line is removed and
//! feeds the search side only, and a `+` line is added and feeds the replace side
//! only. The search side thus reconstructs the pre-image (context + removed) and
//! the replace side the post-image (context + added), in document order, each
//! content line carrying its trailing newline. A hunk header that opens with `@@`
//! but is not delimited by the closing ` @@` is [`MendError::Parse`]. This is the
//! single-file diff format only: no multi-file headers and no patch application.

use crate::{MendError, ParsedEdit, Result};

/// Opens (and closes) a hunk header; a header line begins with this.
const HEADER: &str = "@@";
/// Closes a well-formed hunk header; it must appear after the opening `@@`.
const HEADER_CLOSE: &str = " @@";

/// The fixed number of unchanged lines kept on each side of a change; every
/// further unchanged line is elided. This is the conventional `diff -u` default.
const CONTEXT: usize = 3;

/// Compute the single-file unified diff between `old` and `new`.
///
/// This is a pure, in-memory computation: it does no file I/O and emits no
/// colour. The result is the conventional `diff -u` hunk body — an `@@ … @@`
/// header followed by lines each prefixed by a marker (a space for an unchanged
/// context line, `-` for a removed line, `+` for an added line) and carrying its
/// trailing newline — with no `---`/`+++` file-header lines, symmetric with
/// [`parse_unified_diff`]. Unchanged lines beyond [`CONTEXT`] lines on each side
/// of the change are elided, so the header spans only the context window.
/// Identical inputs yield the empty string — no spurious hunk.
pub fn compute_unified_diff(old: &str, new: &str) -> String {
    // Identical inputs carry no change, hence no hunk.
    if old == new {
        return String::new();
    }

    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    // The maximal common prefix: lines shared from the top, in order.
    let start = old_lines
        .iter()
        .zip(new_lines.iter())
        .take_while(|(a, b)| a == b)
        .count();

    // The maximal common suffix, taken only over the lines after the prefix so
    // it can never overlap it; zip stops at the shorter remainder.
    let suffix = old_lines[start..]
        .iter()
        .rev()
        .zip(new_lines[start..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();

    // The changed spans sit between the shared prefix and suffix.
    let old_change_end = old_lines.len() - suffix;
    let new_change_end = new_lines.len() - suffix;

    // The context window: up to CONTEXT unchanged lines on each side, clamped to
    // the file so the elided regions beyond it never leak into the hunk.
    let ctx_start = start.saturating_sub(CONTEXT);
    let old_ctx_end = (old_change_end + CONTEXT).min(old_lines.len());
    let new_ctx_end = (new_change_end + CONTEXT).min(new_lines.len());

    // Header line numbers are 1-based; the shared prefix means both sides open at
    // the same line.
    let old_count = old_ctx_end - ctx_start;
    let new_count = new_ctx_end - ctx_start;
    let mut out = format!(
        "{HEADER} -{},{} +{},{} {HEADER}\n",
        ctx_start + 1,
        old_count,
        ctx_start + 1,
        new_count
    );

    // Leading context, then removed lines, then added lines, then trailing
    // context — each line carrying its marker and trailing newline.
    for line in &old_lines[ctx_start..start] {
        out.push(' ');
        out.push_str(line);
        out.push('\n');
    }
    for line in &old_lines[start..old_change_end] {
        out.push('-');
        out.push_str(line);
        out.push('\n');
    }
    for line in &new_lines[start..new_change_end] {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    for line in &old_lines[old_change_end..old_ctx_end] {
        out.push(' ');
        out.push_str(line);
        out.push('\n');
    }

    out
}

/// Extract the single-file unified diff in `response` as [`ParsedEdit`]s.
///
/// The `@@ … @@` hunk becomes one [`ParsedEdit::SearchReplace`]: context
/// (leading-space) lines feed both halves, removed (`-`) lines feed the search
/// side, and added (`+`) lines feed the replace side, in document order with each
/// content line carrying its trailing newline and its marker stripped. A header
/// that opens with `@@` but lacks the closing ` @@` delimiter yields
/// [`MendError::Parse`].
pub fn parse_unified_diff(response: &str) -> Result<Vec<ParsedEdit>> {
    let mut search = String::new();
    let mut replace = String::new();
    let mut seen_header = false;

    for line in response.lines() {
        if let Some(rest) = line.strip_prefix(HEADER) {
            if !rest.contains(HEADER_CLOSE) {
                return Err(MendError::Parse(
                    "malformed hunk header: missing closing ` @@` delimiter".to_string(),
                ));
            }
            seen_header = true;
        } else if let Some(content) = line.strip_prefix('+') {
            replace.push_str(content);
            replace.push('\n');
        } else if let Some(content) = line.strip_prefix('-') {
            search.push_str(content);
            search.push('\n');
        } else if let Some(content) = line.strip_prefix(' ') {
            search.push_str(content);
            search.push('\n');
            replace.push_str(content);
            replace.push('\n');
        }
    }

    let mut edits = Vec::new();
    if seen_header {
        edits.push(ParsedEdit::SearchReplace { search, replace });
    }
    Ok(edits)
}
