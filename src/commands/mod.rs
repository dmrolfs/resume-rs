//! ABOUTME: The commands behind the CLI, as library code over injected streams so they test in process.
//! ABOUTME: `main` builds the real streams and environment, calls `run`, and turns the `Outcome` into an exit code.

mod build;
mod doctor;
mod emit;
mod export;
mod export_style;
mod letter;
mod letter_pdf;
mod letter_report;
mod licenses;
pub mod report;
mod styles;
mod tags;
mod watch;

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

/// Runs the command in `cli` and flushes stdout before deciding its outcome.
pub fn run(cli: &Cli, env: &Env, io: &mut Io<'_>) -> Outcome {
    use std::io::Write;

    let mut stdout = Delivery {
        writer: io.stdout,
        failed: false,
    };
    let mut streams = Io {
        stdout: &mut stdout,
        stderr: io.stderr,
    };
    let outcome = match &cli.command {
        Command::Letter(args) => letter::run(cli, args, env, &mut streams),
        Command::Build(args) => build::run(cli, args, env, &mut streams),
        Command::Doctor(args) => doctor::run(cli, args, env, &mut streams),
        Command::Check(args) => doctor::run_check(cli, args, env, &mut streams),
        Command::Styles(args) => styles::run(cli, args, env, &mut streams),
        Command::ExportStyle(args) => export_style::run(cli, args, env, &mut streams),
        Command::Export(args) => export::run(cli, args, env, &mut streams),
        Command::Watch(args) => watch::run(cli, args, env, &mut streams),
        Command::Tags(args) => tags::run(cli, args, env, &mut streams),
        Command::Licenses => licenses::run(cli, env, &mut streams),
    };
    // Report delayed errors here; immediate write errors were already reported by Reporter.
    let already_reported = stdout.failed;
    if let Err(error) = stdout.flush()
        && error.kind() != io::ErrorKind::BrokenPipe
        && !already_reported
    {
        let _ = writeln!(io.stderr, "error: cannot write to stdout: {error}");
    }
    if stdout.failed { Outcome::Failure } else { outcome }
}

/// Records delivery failure across all reporters and files, even if a later write succeeds.
/// Reporters report immediate write errors; `run` reports only newly discovered flush failures.
struct Delivery<'a> {
    writer: &'a mut dyn io::Write,
    failed: bool,
}

impl io::Write for Delivery<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let result = self.writer.write(bytes).and_then(|written| {
            if written == 0 && !bytes.is_empty() {
                Err(io::ErrorKind::WriteZero.into())
            } else {
                Ok(written)
            }
        });
        if let Err(error) = &result {
            self.failed |= !matches!(error.kind(), io::ErrorKind::BrokenPipe | io::ErrorKind::Interrupted);
        }
        result
    }

    fn flush(&mut self) -> io::Result<()> {
        let result = self.writer.flush();
        if let Err(error) = &result {
            self.failed |= error.kind() != io::ErrorKind::BrokenPipe;
        }
        result
    }
}
