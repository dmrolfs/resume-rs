//! ABOUTME: `resume export-style`: copies a built-in style and the files it shares into a directory to edit.
//! ABOUTME: Refuses to overwrite unless `--force`, and looks at every file first so a refusal writes nothing.

use std::path::{Path, PathBuf};

use super::emit::Reporter;
use super::{Env, Io, Outcome};
use crate::cli::{Cli, ExportStyleArgs};
use crate::error::Error;
use crate::output::{Overwrite, Planned, WriteMode, Writer, Written};
use crate::style;

/// Copies the style and the shared `lib/` to `<DIR>/<NAME>/` and `<DIR>/lib/`.
pub(super) fn run(cli: &Cli, args: &ExportStyleArgs, env: &Env, io: &mut Io<'_>) -> Outcome {
    let mut out = Reporter::new(cli.format, &env.cwd, io);
    match export(cli, args, env, &mut out) {
        Ok(outcome) => outcome,
        Err(error) => {
            out.error(&error);
            Outcome::Failure
        }
    }
}

fn export(cli: &Cli, args: &ExportStyleArgs, env: &Env, out: &mut Reporter<'_, '_>) -> Result<Outcome, Error> {
    let overwrite = if args.force {
        Overwrite::Replace
    } else {
        Overwrite::Refuse
    };
    let targets: Vec<(PathBuf, &[u8])> = style::builtin_files(&args.name)?
        .into_iter()
        .map(|(relative, bytes)| (args.dir.join(relative), bytes))
        .collect();

    // Every target is looked at before any is written, so one existing file stops the whole export and a
    // refusal writes nothing. A failure while writing (a full disk) stops at the first file that fails, after
    // the `created` lines of the ones written. The same check runs in a dry run.
    let check = Writer::new(WriteMode::DryRun);
    let refused: Vec<Error> = targets
        .iter()
        .filter_map(|(target, bytes)| {
            check
                .commit(Planned {
                    target,
                    bytes,
                    overwrite,
                })
                .err()
        })
        .collect();
    if !refused.is_empty() {
        refused.iter().for_each(|error| out.error(error));
        return Ok(Outcome::Failure);
    }

    let writer = Writer::new(if cli.dry_run {
        WriteMode::DryRun
    } else {
        WriteMode::Write
    });
    for (target, bytes) in &targets {
        let written = writer.commit(Planned {
            target,
            bytes,
            overwrite,
        })?;
        out.line(&file_line(&written, shown(target, &env.cwd)));
    }
    let verb = if cli.dry_run { "would export" } else { "exported" };
    out.line(&format!(
        "ok {verb} {} to {} ({} files)",
        args.name,
        shown(&args.dir, &env.cwd).display(),
        targets.len()
    ));
    Ok(Outcome::Success)
}

/// What happened to one file, or in a dry run what would.
fn file_line(written: &Written, path: &Path) -> String {
    let what = match written {
        Written::Wrote { replaced: false } => "created",
        Written::Wrote { replaced: true } => "replaced",
        Written::WouldWrite { replaces: false, .. } => "would create",
        Written::WouldWrite { replaces: true, .. } => "would replace",
    };
    format!("{what} {}", path.display())
}

/// A path relative to the current directory when it is under it.
pub(super) fn shown<'p>(path: &'p Path, cwd: &Path) -> &'p Path {
    path.strip_prefix(cwd).unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn each_outcome_has_its_own_word() {
        let path = Path::new("mine/pragmatic/theme.typ");
        let line = |written: Written| file_line(&written, path);
        assert_eq!(
            line(Written::Wrote { replaced: false }),
            "created mine/pragmatic/theme.typ"
        );
        assert_eq!(
            line(Written::Wrote { replaced: true }),
            "replaced mine/pragmatic/theme.typ"
        );
        let would = |replaces| Written::WouldWrite {
            replaces,
            creates_dir: None,
        };
        assert_eq!(line(would(false)), "would create mine/pragmatic/theme.typ");
        assert_eq!(line(would(true)), "would replace mine/pragmatic/theme.typ");
    }
}
