//! Assemble the per-subcommand command prompts from an assembled context.
//!
//! This is the deterministic prompt layer: one pure builder per subcommand over a
//! shared module. Each builder takes the already-assembled file context plus its
//! own per-command parameter (a target symbol, an edit instruction, an error
//! message, or nothing) and returns the single prompt string the backend will be
//! asked to complete. No builder performs I/O, consults a clock, or calls a model;
//! each is a referentially transparent function of its inputs, so the same inputs
//! always yield byte-identical output. Invoking a [`Completer`](crate::Completer)
//! and parsing its response belong to the command tasks, not here.

/// Build the prompt for the `explain` subcommand.
///
/// The assembled `context` is always embedded. When a target `symbol` is present
/// its name is embedded too; when it is absent the symbol is handled by omission —
/// the prompt still embeds the context and is never empty or an error. A pure,
/// deterministic function of `(context, symbol)`.
pub fn explain_prompt(context: &str, symbol: Option<&str>) -> String {
    match symbol {
        Some(name) => format!(
            "Explain the `{name}` symbol, using the following file context.\n\n{context}"
        ),
        None => format!("Explain the following file context.\n\n{context}"),
    }
}

/// Build the prompt for the `edit` subcommand.
///
/// Embeds the assembled `context` and the user's `instruction` verbatim — the
/// instruction is reproduced unchanged so the backend sees exactly what the user
/// wrote. A pure, deterministic function of `(context, instruction)`.
pub fn edit_prompt(context: &str, instruction: &str) -> String {
    format!(
        "Apply the following edit instruction, using the file context below.\n\nInstruction: {instruction}\n\n{context}"
    )
}

/// Build the prompt for the `fix` subcommand.
///
/// Embeds the assembled `context` and the supplied compiler/runtime `error`
/// message. A pure, deterministic function of `(context, error)`.
pub fn fix_prompt(context: &str, error: &str) -> String {
    format!(
        "Fix the following error, using the file context below.\n\nError: {error}\n\n{context}"
    )
}

/// Build the prompt for the `test` subcommand.
///
/// Embeds the assembled `context` and requests tests for the public API only. A
/// pure, deterministic function of `context`.
pub fn test_prompt(context: &str) -> String {
    format!("Write tests for the public API only, using the following file context.\n\n{context}")
}
