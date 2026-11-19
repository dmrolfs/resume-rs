//! ABOUTME: `resume licenses`: prints the license notices of the fonts built into the program.
//! ABOUTME: The SIL Open Font License asks that a distributed copy carry them, so every binary can print them.

use super::emit::Reporter;
use super::{Env, Io, Outcome};
use crate::cli::Cli;
use crate::typeset::fonts::bundled_font_notices;

/// Prints the notices to stdout; `--dry-run` has nothing to change because nothing is written.
pub(super) fn run(cli: &Cli, env: &Env, io: &mut Io<'_>) -> Outcome {
    let mut out = Reporter::new(cli.format, &env.cwd, io);
    out.product(&format!("{}\n", bundled_font_notices()));
    Outcome::Success
}
