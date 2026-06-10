//! The `mend` crate's shared error vocabulary.
//!
//! A single error type, [`MendError`], lets every deterministic layer surface
//! failure uniformly rather than panicking, and the [`Result`] alias spares
//! every fallible function from re-spelling the error type. Mapping concrete
//! failures into variants is the job of the owning tasks; this module only
//! defines the taxonomy.

use std::fmt;

pub mod cli;
pub mod config;
pub mod imports;
pub mod mock;
pub mod structure;

pub use cli::{parse, Command};
pub use config::Config;
pub use imports::extract_imports;
pub use mock::MockCompleter;
pub use structure::extract_structure;

/// The unified error type for the whole `mend` system.
///
/// Each variant names the layer that produced the failure. `Io` wraps the one
/// in-scope third-party source, [`std::io::Error`]; the remaining variants carry
/// a human-readable message describing what went wrong.
#[derive(Debug)]
pub enum MendError {
    /// An I/O failure, e.g. reading or writing a file.
    Io(std::io::Error),
    /// A configuration was missing, malformed, or invalid.
    Config(String),
    /// Input could not be parsed.
    Parse(String),
    /// A patch could not be produced or applied.
    Patch(String),
    /// A completer failed to produce a result.
    Completer(String),
}

impl fmt::Display for MendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MendError::Io(e) => write!(f, "I/O error: {e}"),
            MendError::Config(msg) => write!(f, "configuration error: {msg}"),
            MendError::Parse(msg) => write!(f, "parse error: {msg}"),
            MendError::Patch(msg) => write!(f, "patch error: {msg}"),
            MendError::Completer(msg) => write!(f, "completer error: {msg}"),
        }
    }
}

impl std::error::Error for MendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MendError::Io(e) => Some(e),
            MendError::Config(_)
            | MendError::Parse(_)
            | MendError::Patch(_)
            | MendError::Completer(_) => None,
        }
    }
}

impl From<std::io::Error> for MendError {
    fn from(e: std::io::Error) -> Self {
        MendError::Io(e)
    }
}

/// The project-wide result alias: every fallible function returns this.
pub type Result<T> = std::result::Result<T, MendError>;

/// The single seam that isolates non-determinism behind the system.
///
/// A `Completer` turns a borrowed prompt into a completion, collapsing every
/// backend failure into [`MendError::Completer`] via the [`Result`] return.
/// Every layer above depends on this abstraction, never on a concrete backend,
/// so the deterministic layers stay deterministic and testable. Concrete
/// implementations live in separate tasks; this is the definition only.
///
/// The trait is object-safe: `Box<dyn Completer>` and `&dyn Completer` are
/// nameable, so callers can hold a backend behind a trait object. The prompt is
/// borrowed (`&str`), not consumed, leaving its owner usable after the call.
pub trait Completer {
    /// Produce a completion for the given borrowed `prompt`.
    ///
    /// Returns the completion text, or [`MendError::Completer`] if the backend
    /// failed to produce one.
    fn complete(&self, prompt: &str) -> Result<String>;
}
