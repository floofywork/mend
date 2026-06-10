//! The parsed-edit model — the shared currency every response parser emits and
//! the patch engine consumes.
//!
//! Each response format (whole-file or search/replace) reduces to a
//! [`Vec<ParsedEdit>`], so the parser and applier tasks share one contract. This
//! module defines the value type only: no parsing, no patch application.

/// A single edit recovered from a model response.
///
/// An edit is either a targeted search/replace pair or a whole-file body. A
/// multi-hunk response is a [`Vec<ParsedEdit>`], with order preserved.
#[derive(Debug, PartialEq)]
pub enum ParsedEdit {
    /// A targeted edit: replace the first occurrence of `search` with `replace`.
    SearchReplace {
        /// The text to find.
        search: String,
        /// The text to substitute in its place.
        replace: String,
    },
    /// A whole-file replacement: `body` becomes the file's entire new contents.
    WholeFile {
        /// The complete new file contents.
        body: String,
    },
}
