//! Parse targeted search/replace blocks into search/replace edits.
//!
//! [`parse_search_replace`] turns an LLM response string into one
//! [`ParsedEdit::SearchReplace`] per
//! `<<<<<<< SEARCH … ======= … >>>>>>> REPLACE` block, in document order. The
//! `search` half is the verbatim lines between the `<<<<<<< SEARCH` marker and the
//! `=======` divider; the `replace` half is the verbatim lines between the divider
//! and the `>>>>>>> REPLACE` marker. Each content line carries its trailing
//! newline; the three marker lines are excluded. A block missing its `=======`
//! divider is [`MendError::Parse`]. This is the targeted-edit format only: no
//! fuzzy matching, no application, and no nested-block handling.

use crate::{MendError, ParsedEdit, Result};

/// Opens a block; the search half runs from the next line to the divider.
const SEARCH_MARKER: &str = "<<<<<<< SEARCH";
/// Splits a block's search half from its replace half.
const DIVIDER: &str = "=======";
/// Closes a block; the replace half runs from the divider to this line.
const REPLACE_MARKER: &str = ">>>>>>> REPLACE";

/// Where the line scanner is within a block.
enum State {
    /// Not inside a block; only a `<<<<<<< SEARCH` marker is significant.
    Outside,
    /// Past the search marker, before the divider: lines are search content.
    Search,
    /// Past the divider, before the replace marker: lines are replace content.
    Replace,
}

/// Extract every search/replace block in `response` as a [`ParsedEdit`].
///
/// Each `<<<<<<< SEARCH … ======= … >>>>>>> REPLACE` block becomes one
/// [`ParsedEdit::SearchReplace`], split exactly on the `=======` divider, with the
/// three marker lines excluded and every content line carrying its trailing
/// newline. Blocks are returned in document order. A block whose `>>>>>>> REPLACE`
/// marker arrives before any `=======` divider yields [`MendError::Parse`].
pub fn parse_search_replace(response: &str) -> Result<Vec<ParsedEdit>> {
    let mut edits = Vec::new();
    let mut search = String::new();
    let mut replace = String::new();
    let mut state = State::Outside;

    for line in response.lines() {
        match state {
            State::Outside => {
                if line == SEARCH_MARKER {
                    state = State::Search;
                }
            }
            State::Search => {
                if line == DIVIDER {
                    state = State::Replace;
                } else if line == REPLACE_MARKER {
                    return Err(MendError::Parse(
                        "search/replace block missing ======= divider".to_string(),
                    ));
                } else {
                    search.push_str(line);
                    search.push('\n');
                }
            }
            State::Replace => {
                if line == REPLACE_MARKER {
                    edits.push(ParsedEdit::SearchReplace {
                        search: std::mem::take(&mut search),
                        replace: std::mem::take(&mut replace),
                    });
                    state = State::Outside;
                } else {
                    replace.push_str(line);
                    replace.push('\n');
                }
            }
        }
    }

    Ok(edits)
}
