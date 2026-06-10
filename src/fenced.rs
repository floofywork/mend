//! Parse triple-backtick fenced code blocks into whole-file edits.
//!
//! [`parse_fenced`] turns an LLM response string into one [`ParsedEdit::WholeFile`]
//! per ```` ``` ````-fenced block, in document order. The opening fence's language
//! tag is dropped; the body is the verbatim inner lines, each terminated by a
//! newline. An opening fence with no matching closing fence is
//! [`MendError::Parse`]. This is the whole-file format only: no search/replace or
//! diff handling, and no validation of the language tag.

use crate::{MendError, ParsedEdit, Result};

/// The fence delimiter: a line opening or closing a block begins with this.
const FENCE: &str = "```";

/// Extract every fenced code block in `response` as a whole-file [`ParsedEdit`].
///
/// Each ```` ``` ````-delimited block becomes one [`ParsedEdit::WholeFile`] whose
/// `body` is the verbatim text between the opening and closing fence lines, with
/// the language tag on the opening fence removed and every content line carrying
/// its trailing newline. Blocks are returned in document order. An opening fence
/// that is never closed yields [`MendError::Parse`].
pub fn parse_fenced(response: &str) -> Result<Vec<ParsedEdit>> {
    let mut edits = Vec::new();
    let mut body = String::new();
    let mut in_block = false;

    for line in response.lines() {
        if line.starts_with(FENCE) {
            if in_block {
                edits.push(ParsedEdit::WholeFile {
                    body: std::mem::take(&mut body),
                });
                in_block = false;
            } else {
                in_block = true;
            }
        } else if in_block {
            body.push_str(line);
            body.push('\n');
        }
    }

    if in_block {
        return Err(MendError::Parse(
            "unterminated fenced code block".to_string(),
        ));
    }

    Ok(edits)
}
