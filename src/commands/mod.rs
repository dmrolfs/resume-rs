//! ABOUTME: The commands behind the CLI, as library code over injected streams so they test in process.
//! ABOUTME: `main` builds the real streams and environment, calls `run`, and turns the `Outcome` into an exit code.

mod build;
mod emit;
pub mod report;

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use crate::cli::{Cli, Command};
use crate::diagnostics::termcolor::WriteColor;

pub use self::report::BuildLine;

/// The streams a command writes to: human text and JSON lines are kept apart.
pub struct Io<'a> {
    /// JSON lines for `--format json`, and what `styles` and `licenses` print. Nothing else goes here.
    pub stdout: &'a mut dyn io::Write,
    /// Summaries and human diagnostics, like cargo and rustc.
    pub stderr: &'a mut dyn WriteColor,
}

/// What a command reads from the process, injected so tests do not touch it.
#[derive(Debug, Clone)]
pub struct Env {
    /// The current directory; paths under it are shown relative to it.
    pub cwd: PathBuf,
    /// The value of `RESUME_FONT_PATH`, if set.
    pub resume_font_path: Option<OsString>,
}

impl Env {
    /// Reads the current directory and `RESUME_FONT_PATH` from the process.
    ///
    /// # Errors
    /// The current directory cannot be read.
    pub fn detect() -> io::Result<Self> {
        Ok(Self {
            cwd: std::env::current_dir()?,
            resume_font_path: std::env::var_os("RESUME_FONT_PATH"),
        })
    }
}

/// How a run ended; usage errors never get this far, because clap exits with code 2 first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Outcome {
    /// Everything built; warnings are allowed.
    Success = 0,
    /// An error finding, a failed build, or an operational error.
    Failure = 1,
}

impl From<Outcome> for ExitCode {
    fn from(outcome: Outcome) -> Self {
        ExitCode::from(outcome as u8)
    }
}

/// Runs the command in `cli`.
pub fn run(cli: &Cli, env: &Env, io: &mut Io<'_>) -> Outcome {
    match &cli.command {
        Command::Build(args) => build::run(cli, args, env, io),
    }
}
