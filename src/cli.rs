//! The CLI boundary: turning raw process arguments into a typed [`Command`].
//!
//! This module owns nothing but parsing. It takes the argument vector *without*
//! the program name and produces exactly one [`Command`] value that is
//! structurally complete for its variant — a successfully parsed `Explain` or
//! `Edit` always carries every field its later execution will need. Argument
//! shapes that do not match a known subcommand (an unknown subcommand, a
//! missing positional, an unrecognized flag) collapse into [`Command::Usage`],
//! which carries the diagnostic the binary prints to stderr before exiting
//! non-zero. Dispatch and execution are deliberately out of scope.

use std::path::PathBuf;

/// One usage line, shown to stderr whenever parsing fails. Kept terse and
/// stable; the binary prefixes it with the specific error.
pub const USAGE: &str = "\
usage:
  mend explain <path> [--symbol <name>]
  mend edit <path> <instruction> [--apply] [--json]";

/// A fully parsed command line.
///
/// Each non-`Usage` variant is an invariant of completeness: once constructed
/// it holds every value its variant needs, so downstream dispatch never has to
/// re-validate argument shape. `Usage` is the single error channel — it carries
/// a human-readable message describing what was wrong with the arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Explain a file, optionally narrowed to a single symbol.
    Explain {
        /// The file to explain.
        path: PathBuf,
        /// An optional symbol within the file to focus on.
        symbol: Option<String>,
    },
    /// Edit a file according to a natural-language instruction.
    Edit {
        /// The file to edit.
        path: PathBuf,
        /// The instruction describing the desired change.
        instruction: String,
        /// Apply the edit to disk rather than performing a dry run.
        apply: bool,
        /// Emit machine-readable JSON output.
        json: bool,
    },
    /// The arguments did not form a valid command; `message` explains why.
    Usage {
        /// A human-readable description of the argument error.
        message: String,
    },
}

impl Command {
    /// Whether this command is a dry run — i.e. it must not write to disk.
    ///
    /// Only `Edit` can mutate the filesystem, and only when `--apply` is given;
    /// the negation of `apply` is therefore the dry-run flag. Every other
    /// variant is inherently read-only, so it dry-runs by definition.
    pub fn dry_run(&self) -> bool {
        match self {
            Command::Edit { apply, .. } => !apply,
            Command::Explain { .. } | Command::Usage { .. } => true,
        }
    }
}

/// Parse argv (already stripped of the program name) into a [`Command`].
///
/// The first argument selects the subcommand; the rest are interpreted in that
/// subcommand's grammar. Any deviation — unknown subcommand, missing positional,
/// unrecognized or malformed flag — yields [`Command::Usage`] carrying the
/// reason, never a panic.
pub fn parse(args: &[String]) -> Command {
    let (sub, rest) = match args.split_first() {
        Some((sub, rest)) => (sub.as_str(), rest),
        None => return usage("missing subcommand"),
    };

    match sub {
        "explain" => parse_explain(rest),
        "edit" => parse_edit(rest),
        other => usage(&format!("unknown subcommand `{other}`")),
    }
}

/// Parse the arguments to `explain`: one positional `<path>` and an optional
/// `--symbol <name>`.
fn parse_explain(args: &[String]) -> Command {
    let mut path: Option<PathBuf> = None;
    let mut symbol: Option<String> = None;

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--symbol" => match iter.next() {
                Some(value) => symbol = Some(value.clone()),
                None => return usage("`--symbol` requires a value"),
            },
            flag if flag.starts_with("--") => {
                return usage(&format!("unknown flag `{flag}` for `explain`"));
            }
            positional => {
                if path.is_some() {
                    return usage(&format!("unexpected argument `{positional}` for `explain`"));
                }
                path = Some(PathBuf::from(positional));
            }
        }
    }

    match path {
        Some(path) => Command::Explain { path, symbol },
        None => usage("`explain` requires a <path>"),
    }
}

/// Parse the arguments to `edit`: positional `<path>` and `<instruction>`, plus
/// the boolean flags `--apply` and `--json`.
fn parse_edit(args: &[String]) -> Command {
    let mut path: Option<PathBuf> = None;
    let mut instruction: Option<String> = None;
    let mut apply = false;
    let mut json = false;

    for arg in args {
        match arg.as_str() {
            "--apply" => apply = true,
            "--json" => json = true,
            flag if flag.starts_with("--") => {
                return usage(&format!("unknown flag `{flag}` for `edit`"));
            }
            positional => {
                if path.is_none() {
                    path = Some(PathBuf::from(positional));
                } else if instruction.is_none() {
                    instruction = Some(positional.to_string());
                } else {
                    return usage(&format!("unexpected argument `{positional}` for `edit`"));
                }
            }
        }
    }

    match (path, instruction) {
        (Some(path), Some(instruction)) => Command::Edit {
            path,
            instruction,
            apply,
            json,
        },
        (None, _) => usage("`edit` requires a <path>"),
        (Some(_), None) => usage("`edit` requires an <instruction>"),
    }
}

/// Build a [`Command::Usage`] from a reason.
fn usage(message: &str) -> Command {
    Command::Usage {
        message: message.to_string(),
    }
}
