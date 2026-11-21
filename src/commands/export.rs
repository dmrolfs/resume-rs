//! ABOUTME: `resume export`: writes the resume as `resume.json` beside a copy of its style, for tinymist or `typst watch`.
//! ABOUTME: The style files are kept when they exist, so a style being edited survives a re-export of the resume.

use std::fs;
use std::path::{Path, PathBuf};

use snafu::ResultExt;

use super::emit::Reporter;
use super::export_style::shown;
use super::{Env, Io, Outcome};
use crate::cli::{Cli, ExportArgs};
use crate::diagnostics::{Diagnostic, Summary};
use crate::error::{Error, ReadInputSnafu, SelectSnafu};
use crate::markdown::parse;
use crate::output::{Overwrite, Planned, WriteMode, Writer, Written};
use crate::select::{Selection, select};
use crate::style;
use crate::typeset::resume_json;

/// The name of the model file, which a style reads as `json("/resume.json")`.
const RESUME_JSON: &str = "resume.json";

/// Writes `resume.json` and the style files to `<DIR>`.
pub(super) fn run(cli: &Cli, args: &ExportArgs, env: &Env, io: &mut Io<'_>) -> Outcome {
    let mut out = Reporter::new(cli.format, &env.cwd, io);
    match export(cli, args, env, &mut out) {
        Ok(outcome) => outcome,
        Err(error) => {
            out.error(&error);
            Outcome::Failure
        }
    }
}

/// One file of the export, and how it may be written.
struct Target {
    path: PathBuf,
    bytes: Vec<u8>,
    overwrite: Overwrite<'static>,
}

/// What became of one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fate {
    Created,
    Replaced,
    Kept,
}

fn export(cli: &Cli, args: &ExportArgs, env: &Env, out: &mut Reporter<'_, '_>) -> Result<Outcome, Error> {
    let text = fs::read_to_string(&args.file).context(ReadInputSnafu { path: &args.file })?;
    let parsed = parse(&args.file, &text);
    let mut sources = out.sources();
    sources.insert(&args.file, &text);
    let findings: Vec<&Diagnostic> = parsed.diagnostics().iter().collect();
    out.diagnostics(&sources, &findings);
    // A preview of a resume with errors would show something the resume does not say, and there is no model.
    let Some(resume) = parsed.valid() else {
        out.line(&format!(
            "error {}  {}  nothing exported",
            shown(&args.file, &env.cwd).display(),
            Summary::of(findings.iter().copied())
        ));
        return Ok(Outcome::Failure);
    };
    // The same selection `build` makes, so the preview shows what the PDF will say: no drafts, one wording a group.
    let (resume, _) = select(resume, &Selection::default()).context(SelectSnafu)?;
    let style = style::resolve(&args.style, args.styles_dir.as_deref())?;
    let targets = targets(args, style.files()?, resume_json(&resume, true)?);

    // Every target is looked at before any is written, so a file that cannot be written stops the export with
    // nothing changed. A file that is already there and is to be kept is not a problem.
    let check = Writer::new(WriteMode::DryRun);
    let refused: Vec<Error> = targets
        .iter()
        .filter_map(|target| try_commit(&check, target).err())
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
    let mut fates = Vec::new();
    for target in &targets {
        let fate = try_commit(&writer, target)?;
        out.line(&file_line(fate, cli.dry_run, shown(&target.path, &env.cwd)));
        fates.push(fate);
    }
    out.line(&closing_line(args, &env.cwd, cli.dry_run, &fates));
    out.line(&preview_hint(args, &env.cwd));
    Ok(Outcome::Success)
}

/// The style files, then `resume.json` last, so a running preview never sees a new resume with a lib half updated.
fn targets(args: &ExportArgs, files: Vec<(PathBuf, Vec<u8>)>, json: Vec<u8>) -> Vec<Target> {
    let style_overwrite = if args.force {
        Overwrite::Replace
    } else {
        Overwrite::Refuse
    };
    let mut targets: Vec<Target> = files
        .into_iter()
        .map(|(relative, bytes)| Target {
            path: args.dir.join(relative),
            bytes,
            overwrite: style_overwrite,
        })
        .collect();
    targets.push(Target {
        path: args.dir.join(RESUME_JSON),
        bytes: json,
        overwrite: Overwrite::Replace,
    });
    targets
}

/// Commits one target, where "it is already there and is to be kept" is a result and not a failure.
fn try_commit(writer: &Writer, target: &Target) -> Result<Fate, Error> {
    match writer.commit(Planned {
        target: &target.path,
        bytes: &target.bytes,
        overwrite: target.overwrite,
    }) {
        Ok(Written::Wrote { replaced: false } | Written::WouldWrite { replaces: false, .. }) => Ok(Fate::Created),
        Ok(Written::Wrote { replaced: true } | Written::WouldWrite { replaces: true, .. }) => Ok(Fate::Replaced),
        Err(Error::OutputExists { .. }) => Ok(Fate::Kept),
        Err(error) => Err(error),
    }
}

fn file_line(fate: Fate, dry_run: bool, path: &Path) -> String {
    let word = match (fate, dry_run) {
        (Fate::Created, false) => "created",
        (Fate::Created, true) => "would create",
        (Fate::Replaced, false) => "replaced",
        (Fate::Replaced, true) => "would replace",
        (Fate::Kept, false) => "kept",
        (Fate::Kept, true) => "would keep",
    };
    format!("{word} {}", path.display())
}

/// `ok exported pragmatic to mine (5 files: 1 replaced, 4 kept)`.
fn closing_line(args: &ExportArgs, cwd: &Path, dry_run: bool, fates: &[Fate]) -> String {
    let count = |wanted| fates.iter().filter(|fate| **fate == wanted).count();
    let parts: Vec<String> = [
        (Fate::Created, "created"),
        (Fate::Replaced, "replaced"),
        (Fate::Kept, "kept"),
    ]
    .into_iter()
    .filter(|(fate, _)| count(*fate) > 0)
    .map(|(fate, word)| format!("{} {word}", count(fate)))
    .collect();
    let verb = if dry_run { "would export" } else { "exported" };
    let mut line = format!(
        "ok {verb} {} to {} ({} files: {})",
        args.style,
        shown(&args.dir, cwd).display(),
        fates.len(),
        parts.join(", ")
    );
    if count(Fate::Kept) > 0 {
        line.push_str("; use --force to refresh the style files");
    }
    line
}

/// The command that previews the export, with the paths as the user will type them.
fn preview_hint(args: &ExportArgs, cwd: &Path) -> String {
    let dir = shown(&args.dir, cwd);
    format!(
        "preview: typst watch --root {dir} {main}  (add `--input paper=a4` for A4)",
        dir = dir.display(),
        main = dir.join(args.style.as_ref() as &str).join("style.typ").display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser as _;
    use pretty_assertions::assert_eq;

    fn args(extra: &[&str]) -> ExportArgs {
        let cli = crate::cli::Cli::parse_from(
            ["resume", "export", "r.md", "preview"]
                .into_iter()
                .chain(extra.iter().copied()),
        );
        let crate::cli::Command::Export(args) = cli.command else {
            panic!("export")
        };
        args
    }

    #[test]
    fn the_closing_line_names_the_style_the_directory_and_each_kind_of_file_that_there_is() {
        let cwd = Path::new("/work");
        let line = |extra: &[&str], dry, fates: &[Fate]| closing_line(&args(extra), cwd, dry, fates);
        assert_eq!(
            line(&[], false, &[Fate::Created, Fate::Created, Fate::Created]),
            "ok exported pragmatic to preview (3 files: 3 created)"
        );
        assert_eq!(
            line(&["-s", "mono"], false, &[Fate::Kept, Fate::Kept, Fate::Replaced]),
            "ok exported mono to preview (3 files: 1 replaced, 2 kept); use --force to refresh the style files"
        );
        assert_eq!(
            line(&[], true, &[Fate::Created, Fate::Replaced, Fate::Kept]),
            "ok would export pragmatic to preview (3 files: 1 created, 1 replaced, 1 kept); use --force to refresh the style files"
        );
        assert_eq!(
            line(&[], false, &[Fate::Replaced]),
            "ok exported pragmatic to preview (1 files: 1 replaced)"
        );
    }

    #[test]
    fn each_fate_has_its_own_word_in_a_real_run_and_in_a_dry_run() {
        let path = Path::new("preview/pragmatic/theme.typ");
        let words: Vec<String> = [Fate::Created, Fate::Replaced, Fate::Kept]
            .into_iter()
            .flat_map(|fate| [file_line(fate, false, path), file_line(fate, true, path)])
            .collect();
        assert_eq!(
            words,
            [
                "created preview/pragmatic/theme.typ",
                "would create preview/pragmatic/theme.typ",
                "replaced preview/pragmatic/theme.typ",
                "would replace preview/pragmatic/theme.typ",
                "kept preview/pragmatic/theme.typ",
                "would keep preview/pragmatic/theme.typ",
            ]
        );
    }

    #[test]
    fn resume_json_is_the_last_target_and_is_the_only_one_that_is_always_replaced() {
        let args = args(&[]);
        let files = vec![
            (PathBuf::from("pragmatic/style.typ"), b"a".to_vec()),
            (PathBuf::from("lib/c.typ"), b"b".to_vec()),
        ];
        let targets = targets(&args, files, b"{}".to_vec());
        let order: Vec<&Path> = targets.iter().map(|t| t.path.as_path()).collect();
        assert_eq!(
            order,
            [
                Path::new("preview/pragmatic/style.typ"),
                Path::new("preview/lib/c.typ"),
                Path::new("preview/resume.json")
            ]
        );
        assert!(matches!(targets[0].overwrite, Overwrite::Refuse) && matches!(targets[1].overwrite, Overwrite::Refuse));
        assert!(matches!(targets[2].overwrite, Overwrite::Replace));
        let forced = self::targets(
            &self::args(&["--force"]),
            vec![(PathBuf::from("a"), Vec::new())],
            Vec::new(),
        );
        assert!(forced.iter().all(|t| matches!(t.overwrite, Overwrite::Replace)));
    }
}
