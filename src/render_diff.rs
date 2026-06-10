//! Render a unified diff for human reading, optionally with colour.
//!
//! [`render_diff`] is the purely presentational counterpart to
//! [`crate::compute_unified_diff`]: it consumes a unified diff string (an
//! `@@ … @@` header followed by body lines, each prefixed by a marker — a space
//! for context, `-` for removed, `+` for added) plus a TTY signal, and returns
//! the diff styled for display. When `color` is true each line is wrapped in the
//! ANSI SGR escape matching its marker — cyan for the hunk header, red for a
//! removed line, green for an added line — while context lines stay verbatim.
//! When `color` is false (stdout is not a terminal) the diff is returned byte for
//! byte, with no escapes. The styling is additive only: stripping the ANSI codes
//! from a coloured rendering recovers the original diff text exactly, so an empty
//! diff renders the empty string in either mode. This does no diff computation
//! and no paging or width wrapping.

/// ANSI SGR escape colouring a hunk header (`@@ … @@`) cyan.
const CYAN: &str = "\x1b[36m";
/// ANSI SGR escape colouring a removed (`-`) line red.
const RED: &str = "\x1b[31m";
/// ANSI SGR escape colouring an added (`+`) line green.
const GREEN: &str = "\x1b[32m";
/// ANSI SGR escape resetting styling back to the terminal default.
const RESET: &str = "\x1b[0m";

/// Render a unified `diff` for display, colouring each line by its marker when
/// `color` is true (stdout is a TTY) and returning it verbatim otherwise.
///
/// With colour, a `@@` hunk header renders cyan, a `-` line red, and a `+` line
/// green; context lines are left unstyled. The colour codes wrap only the line
/// content, before its trailing newline, so stripping every ANSI escape recovers
/// the input exactly. An empty diff yields the empty string in either mode.
pub fn render_diff(diff: &str, color: bool) -> String {
    // Not a TTY: hand back the diff untouched, with no colour codes at all.
    if !color {
        return diff.to_string();
    }

    let mut out = String::new();
    // Each piece keeps its own trailing newline (if any), so the line structure
    // is preserved exactly. An empty diff yields no pieces and so empty output.
    for piece in diff.split_inclusive('\n') {
        // Separate the line content from its terminator so the colour reset lands
        // before the newline, leaving the newline itself untouched.
        let (content, newline) = match piece.strip_suffix('\n') {
            Some(without) => (without, "\n"),
            None => (piece, ""),
        };

        // Classify the line by its leading marker and pick its colour, if any.
        let color_code = if content.starts_with("@@") {
            Some(CYAN)
        } else if content.starts_with('-') {
            Some(RED)
        } else if content.starts_with('+') {
            Some(GREEN)
        } else {
            None
        };

        match color_code {
            Some(code) => {
                out.push_str(code);
                out.push_str(content);
                out.push_str(RESET);
            }
            None => out.push_str(content),
        }
        out.push_str(newline);
    }
    out
}
