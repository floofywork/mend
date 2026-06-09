//! The `mend` binary entry point.
//!
//! Its only job at this phase is the CLI boundary: hand the process arguments
//! (minus the program name) to [`mend::parse`], and turn an unparsable command
//! line into a non-zero exit with usage on stderr. Dispatch and execution of a
//! successfully parsed command are a later concern, so a valid command simply
//! exits 0.

use std::process::ExitCode;

use mend::{cli::USAGE, parse, Command};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    match parse(&args) {
        Command::Usage { message } => {
            eprintln!("error: {message}");
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
        _ => ExitCode::SUCCESS,
    }
}
