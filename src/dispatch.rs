//! Detect which edit format an LLM response is in and route it to its parser.
//!
//! [`parse_response`] inspects a raw response for each of the three known formats'
//! markers and hands the whole response to the single matching single-purpose
//! parser, returning that parser's `Vec<ParsedEdit>`. Selection follows a fixed
//! first-match precedence — search/replace before unified-diff before fenced — so
//! identical responses always route identically. A response carrying none of the
//! three markers is [`MendError::Parse`]. This is dispatch only: the parsers own
//! their own formats and errors.

use crate::{
    parse_fenced, parse_search_replace, parse_unified_diff, MendError, ParsedEdit, Result,
};

/// The search/replace block opener; its presence selects the search/replace parser.
const SEARCH_MARKER: &str = "<<<<<<< SEARCH";

/// A unified-diff hunk marker; its presence selects the unified-diff parser.
const HUNK_MARKER: &str = "@@ ";

/// The fenced-block delimiter; its presence selects the fenced parser.
const FENCE_MARKER: &str = "```";

/// Auto-detect `response`'s format and route it to the matching parser.
///
/// The three known formats are tried in a fixed precedence: a `<<<<<<< SEARCH`
/// marker routes to [`parse_search_replace`], else a `@@ ` hunk marker routes to
/// [`parse_unified_diff`], else a ```` ``` ```` fence routes to [`parse_fenced`].
/// The selected parser's `Vec<ParsedEdit>` is returned verbatim. A response
/// matching none of the three is [`MendError::Parse`].
pub fn parse_response(response: &str) -> Result<Vec<ParsedEdit>> {
    if response.contains(SEARCH_MARKER) {
        parse_search_replace(response)
    } else if response.contains(HUNK_MARKER) {
        parse_unified_diff(response)
    } else if response.contains(FENCE_MARKER) {
        parse_fenced(response)
    } else {
        Err(MendError::Parse(
            "response matches no known edit format".to_string(),
        ))
    }
}
