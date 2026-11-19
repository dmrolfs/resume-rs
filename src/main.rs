//! ABOUTME: Entry point for the `resume` binary.
//! ABOUTME: Builds the real streams and environment, runs the command, and turns its outcome into an exit code.

use std::io::{self, BufWriter, IsTerminal, Write};
use std::process::ExitCode;

use resume::cli::Cli;
use resume::commands::{self, Env, Io};
use resume::diagnostics::termcolor::BufferedStandardStream;
use resume::diagnostics::{ColorEnv, resolve_color};

fn main() -> ExitCode {
    // clap prints a usage error and exits with code 2 (or prints help or the version and exits with 0).
    let cli = match Cli::try_parse_validated(std::env::args_os()) {
        Ok(cli) => cli,
        Err(error) => error.exit(),
    };
    let env = match Env::detect() {
        Ok(env) => env,
        Err(error) => {
            eprintln!("error: cannot read the current directory: {error}");
            return ExitCode::FAILURE;
        }
    };
    let color = resolve_color(cli.color, ColorEnv::detect(io::stderr().is_terminal()));
    let mut stderr = BufferedStandardStream::stderr(color);
    let mut stdout = BufWriter::new(io::stdout().lock());
    let outcome = commands::run(
        &cli,
        &env,
        &mut Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );
    // commands::run flushes stdout and includes delivery failures in the outcome.
    let _ = stderr.flush();
    outcome.into()
}
