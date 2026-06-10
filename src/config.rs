//! The settings boundary: loading [`Config`] from file, env, and defaults.
//!
//! [`Config::load`] resolves three typed fields — `model`, `endpoint`, and
//! `token_budget` — from three layers with a fixed precedence: built-in
//! defaults are overridden by a `mend.toml` in the working directory, which is
//! in turn overridden by the `MEND_MODEL`, `MEND_ENDPOINT`, and
//! `MEND_TOKEN_BUDGET` environment variables. A missing file is not an error —
//! the defaults stand. The one hard failure is a `token_budget`, from either
//! the file or the env, that is not an integer; that surfaces as
//! [`MendError::Config`].

use crate::{MendError, Result};

/// The resolved configuration feeding the deterministic layers.
///
/// Construct it with [`Config::load`], which reads the working-directory
/// `mend.toml` and the `MEND_*` environment variables over a set of defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The model identifier the completer backend should use.
    pub model: String,
    /// The endpoint URL the completer backend should talk to.
    pub endpoint: String,
    /// The per-run token budget the deterministic layers spend against.
    pub token_budget: u32,
}

impl Config {
    /// Resolve the configuration from defaults, `mend.toml`, then the `MEND_*`
    /// environment variables, in increasing order of precedence.
    ///
    /// Returns [`MendError::Config`] if a `token_budget` provided by the file or
    /// the environment is not a valid integer; a missing file is not an error.
    pub fn load() -> Result<Config> {
        let mut model = String::from("gpt-4");
        let mut endpoint = String::from("https://api.openai.com/v1");
        let mut token_budget: u32 = 4096;

        if let Ok(text) = std::fs::read_to_string("mend.toml") {
            for line in text.lines() {
                if let Some((key, value)) = line.split_once('=') {
                    let value = unquote(value.trim());
                    match key.trim() {
                        "model" => model = value.to_string(),
                        "endpoint" => endpoint = value.to_string(),
                        "token_budget" => token_budget = parse_budget(value)?,
                        _ => {}
                    }
                }
            }
        }

        if let Ok(value) = std::env::var("MEND_MODEL") {
            model = value;
        }
        if let Ok(value) = std::env::var("MEND_ENDPOINT") {
            endpoint = value;
        }
        if let Ok(value) = std::env::var("MEND_TOKEN_BUDGET") {
            token_budget = parse_budget(&value)?;
        }

        Ok(Config {
            model,
            endpoint,
            token_budget,
        })
    }
}

/// Strip one layer of surrounding double quotes from a TOML scalar, leaving a
/// bare (unquoted) value untouched.
fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
        .unwrap_or(value)
}

/// Parse a `token_budget` value, mapping any non-integer text to
/// [`MendError::Config`].
fn parse_budget(value: &str) -> Result<u32> {
    value
        .parse::<u32>()
        .map_err(|_| MendError::Config(format!("token_budget must be an integer, got: {value}")))
}
