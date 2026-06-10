//! The `mend` binary entry point.
//!
//! This is the thin process shell around the parser: it hands the process
//! arguments (minus the program name) to [`mend::parse`] and, when they do not
//! form a valid command, prints usage to stderr and exits non-zero. Dispatching
//! and executing a successfully parsed command is a later task's concern.

use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let mend::Command::Usage { message } = mend::parse(&args) {
        eprintln!("{message}");
        eprintln!("{}", mend::cli::USAGE);
        exit(1);
    }
}
