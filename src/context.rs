//! Assemble a file's extracted structure and imports into one budget-bounded
//! context string.
//!
//! [`assemble_context`] is the join point that produces the single context blob
//! fed to prompt assembly. It takes a file's extracted structure signatures and
//! its import paths, concatenates them deterministically into one string, then
//! routes that string through the [`budget`](crate::budget::budget) so the result
//! never exceeds the configured token budget — oversize context is trimmed by the
//! budgeter rather than erroring. The function is pure: the same
//! `(structure, imports, budget)` always yields byte-identical output, with no
//! model or network call.

use crate::budget::budget;

/// Combine a file's extracted `structure` and `imports` into one budget-bounded
/// context string.
///
/// The structure signatures and import paths are joined, in order, into a single
/// string with one entry per line, then passed through the
/// [`budget`](crate::budget::budget) so the result stays within `budget` tokens
/// (at the budgeter's fixed four-characters-per-token ratio). A budget of `0`
/// yields an empty string. The result is a pure, deterministic function of
/// `(structure, imports, budget)`.
pub fn assemble_context(structure: &[String], imports: &[String], budget_tokens: usize) -> String {
    let combined = structure
        .iter()
        .chain(imports.iter())
        .cloned()
        .collect::<Vec<String>>()
        .join("\n");
    budget(&combined, budget_tokens)
}
