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
