//! Trim a text string so its estimated token count stays within a budget.
//!
//! [`budget`] keeps a prompt within a configured token budget deterministically,
//! the final guard before a prompt reaches a backend. Token count is estimated
//! with a fixed chars-per-token ratio — four characters per token — so no model
//! or network is ever consulted: a string of `c` characters costs `ceil(c / 4)`
//! tokens, which stays within `n` tokens exactly when `c <= 4 * n`. Output is
//! therefore capped at `4 * n` characters. A budget of `0` yields an empty
//! string; oversize input is trimmed to fit and marked with a visible `...` so a
//! cut is never silent.

/// The visible marker appended when, and only when, the text is trimmed.
const MARKER: &str = "...";

/// Trim `text` so its estimated token count is at most `n`.
///
/// At a fixed ratio of four characters per token the output is capped at `4 * n`
/// characters. Text that already fits is returned unchanged; text that does not
/// is trimmed to fit and ends with the visible marker [`MARKER`]. A budget of `0`
/// returns an empty string. The result is a pure, deterministic function of
/// `(text, n)` — no model or network call is involved.
pub fn budget(text: &str, n: usize) -> String {
    if n == 0 {
        return String::new();
    }

    let ceiling = n * 4;
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= ceiling {
        return text.to_string();
    }

    // Oversize: keep room for the marker within the character ceiling.
    let keep = ceiling - MARKER.chars().count();
    let mut out: String = chars[..keep].iter().collect();
    out.push_str(MARKER);
    out
}
