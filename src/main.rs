//! ABOUTME: Entry point for the `resume` binary.
//! ABOUTME: Parses arguments and dispatches to the library; clap handles help, version, and usage errors.

use clap::Parser;
use resume::cli::Cli;

fn main() {
    let _cli = Cli::parse();
}
