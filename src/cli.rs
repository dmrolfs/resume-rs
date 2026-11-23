//! ABOUTME: Command-line argument definitions for the `resume` binary, built with clap derive.
//! ABOUTME: Options and defaults are documented here; checks clap cannot express run in `Cli::try_parse_validated`.

mod letter;
mod spec;
pub use letter::{LetterArgs, LetterRenderArgs, LetterTextSettings};
pub use spec::{Given, SpecArgs};

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clap::error::ErrorKind;
use clap::{Args, CommandFactory, Parser, Subcommand};

use crate::diagnostics::DiagnosticCode;
use crate::model::options::{ColorMode, FontsMode, OutputFormat, PageLimit, Paper};
use crate::model::{StyleName, Tag, VariantName};
use crate::select::{Scope, SelectedTags, Selection};
use crate::spec::{Explicit, OverrideFlag, OverridePolicy, ScopeFlag};

/// Command-line arguments for `resume`.
#[derive(Debug, Parser)]
#[command(
    name = "resume",
    // Windows starts the binary as `resume.exe` and clap derives its display name from argv[0],
    // so pin it; otherwise help would print `Usage: resume.exe`.
    bin_name = "resume",
    version,
    about = "Typesets Markdown resumes into print-quality PDFs."
)]
pub struct Cli {
    /// Show what would be written without writing anything.
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// How findings are written: for people, or as JSON lines on stdout.
    #[arg(long, global = true, value_enum, default_value_t)]
    pub format: OutputFormat,

    /// When to color the output (NO_COLOR is respected).
    #[arg(long, global = true, value_enum, default_value_t)]
    pub color: ColorMode,

    #[command(subcommand)]
    pub command: Command,
}

/// What `resume` does.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print validated, personalized cover-letter text for a job website.
    Letter(LetterArgs),
    /// Typeset one or more Markdown resumes into PDFs.
    Build(BuildArgs),
    /// Check resumes, explain what is wrong with them and why it matters, and fix what is safe to fix.
    Doctor(DoctorArgs),
    /// The same report as `doctor`, which can never change a file; for scripts and hooks.
    Check(CheckArgs),
    /// List the styles that can be chosen.
    Styles(StylesArgs),
    /// Copy a built-in style to a directory, to edit it.
    ExportStyle(ExportStyleArgs),
    /// Write the resume as JSON with a style, to preview the style in tinymist or `typst watch`.
    Export(ExportArgs),
    /// Rebuild the PDF whenever the resume or a style file changes.
    Watch(WatchArgs),
    /// List the tags, keywords, and alternate wordings a resume's annotations offer.
    Tags(TagsArgs),
    /// Print the license notices of the fonts built into this program.
    Licenses,
}

/// Arguments of `resume tags`.
#[derive(Debug, Args)]
pub struct TagsArgs {
    /// The Markdown resume to list.
    #[arg(value_name = "FILE")]
    pub file: PathBuf,
}

/// Arguments of `resume export`.
#[derive(Debug, Args)]
pub struct ExportArgs {
    /// The Markdown resume to export.
    #[arg(value_name = "FILE")]
    pub file: PathBuf,

    /// Where to write `resume.json`, the style (`<DIR>/<NAME>/`), and what styles share (`<DIR>/lib/`).
    #[arg(value_name = "DIR")]
    pub dir: PathBuf,

    /// The style to export.
    #[arg(short, long, default_value = "pragmatic", value_name = "NAME")]
    pub style: StyleName,

    /// A directory of styles to use besides the built-in ones; a style here wins over a built-in of the same name.
    #[arg(long, value_name = "DIR")]
    pub styles_dir: Option<PathBuf>,

    /// Replace the style files that already exist; without it they are kept, so a style being edited is not
    /// overwritten. `resume.json` is always replaced.
    #[arg(long)]
    pub force: bool,

    #[command(flatten)]
    pub select: SelectArgs,
}

/// Arguments of `resume watch`.
#[derive(Debug, Args)]
pub struct WatchArgs {
    /// The Markdown resume to watch and build.
    #[arg(value_name = "FILE")]
    pub file: PathBuf,

    /// The directory the PDF is written to; it is created if it does not exist.
    #[arg(short, long, default_value = "out", value_name = "DIR")]
    pub out: PathBuf,

    #[command(flatten)]
    pub render: RenderArgs,

    #[command(flatten)]
    pub select: SelectArgs,

    /// How long to wait after the last change before rebuilding, such as `150ms` or `1s`.
    #[arg(long, default_value = "150ms", value_name = "DURATION", value_parser = parse_debounce)]
    pub debounce: std::time::Duration,

    /// Open the PDF once, after the first build that writes it.
    #[arg(long)]
    pub open: bool,
}

/// Reads a `--debounce` value with `humantime`: `150ms`, `2s`, `1m`.
fn parse_debounce(text: &str) -> Result<std::time::Duration, String> {
    text.parse::<humantime::Duration>()
        .map(Into::into)
        .map_err(|error| format!("{error}; write a duration such as `150ms` or `2s`"))
}

/// Arguments of `resume styles`.
#[derive(Debug, Args)]
pub struct StylesArgs {
    /// A directory of styles to list besides the built-in ones.
    #[arg(long, value_name = "DIR")]
    pub styles_dir: Option<PathBuf>,
}

/// Arguments of `resume export-style`.
#[derive(Debug, Args)]
pub struct ExportStyleArgs {
    /// The built-in style to copy.
    #[arg(value_name = "NAME")]
    pub name: StyleName,

    /// Where to copy it: the style goes to `<DIR>/<NAME>/` and what the styles share to `<DIR>/lib/`, so the
    /// copy can be used with `--styles-dir <DIR>`.
    #[arg(value_name = "DIR")]
    pub dir: PathBuf,

    /// Replace files that already exist; without it, nothing is written when any file exists.
    #[arg(long)]
    pub force: bool,
}

/// Arguments of `resume build`.
#[derive(Debug, Args)]
pub struct BuildArgs {
    /// The Markdown resumes to build; not needed with `--spec`.
    #[arg(required_unless_present = "spec", value_name = "FILE")]
    pub files: Vec<PathBuf>,

    /// The directory the PDFs are written to; it is created if it does not exist.
    #[arg(short, long, default_value = "out", value_name = "DIR")]
    pub out: PathBuf,

    /// Declared before `render` and `select`, which it asks about: see [`Given`].
    #[command(flatten)]
    pub spec: SpecArgs,

    #[command(flatten)]
    pub render: RenderArgs,

    #[command(flatten)]
    pub select: SelectArgs,
}

/// What `doctor` and `check` assess, shared so the two reports cannot drift apart.
#[derive(Debug, Args)]
pub struct AssessArgs {
    /// The directory `build` would write the PDFs to; it is checked, never written.
    #[arg(short, long, default_value = "out", value_name = "DIR")]
    pub out: PathBuf,

    #[command(flatten)]
    pub render: RenderArgs,

    #[command(flatten)]
    pub select: SelectArgs,
}

/// Arguments of `resume check`: no `--fix` and no `--explain`, so it cannot change a file by construction.
#[derive(Debug, Args)]
pub struct CheckArgs {
    /// The Markdown resumes to check.
    #[arg(required = true, value_name = "FILE")]
    pub files: Vec<PathBuf>,

    #[command(flatten)]
    pub assess: AssessArgs,
}

/// Arguments of `resume doctor`.
#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// The Markdown resumes to check; not needed with `--explain`.
    #[arg(required_unless_present = "explain", value_name = "FILE")]
    pub files: Vec<PathBuf>,

    #[command(flatten)]
    pub assess: AssessArgs,

    /// Apply the fixes that are safe: each keeps what the resume says and only changes how it is written.
    #[arg(long, conflicts_with = "explain")]
    pub fix: bool,

    /// Explain a finding by its code, like `rustc --explain`, and exit.
    #[arg(long, value_name = "CODE", conflicts_with_all = ["files", "fix"], value_parser = parse_code)]
    pub explain: Option<DiagnosticCode>,
}

/// Reads a `--explain` argument, and for one that is not a code names the codes it is close to.
fn parse_code(text: &str) -> Result<DiagnosticCode, String> {
    use std::str::FromStr as _;
    use strum::IntoEnumIterator as _;
    if let Ok(code) = DiagnosticCode::from_str(text) {
        return Ok(code);
    }
    let wanted = text.to_lowercase();
    let mut close: Vec<(f64, DiagnosticCode)> = DiagnosticCode::iter()
        .map(|code| {
            let name = code.to_string();
            // A prefix such as `period` matches every `period-*` code.
            let score = if name.starts_with(&wanted) {
                1.0
            } else {
                strsim::jaro_winkler(&wanted, &name)
            };
            (score, code)
        })
        .filter(|(score, _)| *score >= 0.85)
        .collect();
    close.sort_by(|a, b| b.0.total_cmp(&a.0));
    let names: Vec<String> = close.iter().take(3).map(|(_, code)| format!("`{code}`")).collect();
    Err(if names.is_empty() {
        format!("`{text}` is not a diagnostic code; every code is printed in the output of `resume check`")
    } else {
        format!(
            "`{text}` is not a diagnostic code; did you mean {}?",
            names.join(" or ")
        )
    })
}

/// What to keep of an annotated resume and which wordings to use: the tailoring options every command that
/// renders a resume shares, and that cover letters reuse (rsm-vqr.3).
///
/// A position spec (rsm-141.23) resolves to the same [`Selection`] these do, and names are checked by the same
/// model types, so a flag and a spec reject the same names.
#[derive(Debug, Clone, Default, Args)]
pub struct SelectArgs {
    /// Keep only items tagged with one of these tags, besides `@core` and untagged items; comma-separated or
    /// repeated.
    #[arg(long, value_name = "TAG", value_delimiter = ',', conflicts_with = "core_only", value_parser = parse_tag)]
    pub tags: Vec<Tag>,

    /// Keep only `@core` items.
    #[arg(long)]
    pub core_only: bool,

    /// The wordings to use where a group has them, most preferred first; comma-separated or repeated.
    #[arg(long, value_name = "NAME", value_delimiter = ',', value_parser = parse_variant)]
    pub prefer: Vec<VariantName>,

    /// Keep what is marked `@draft`, to preview the document once the drafts are ready.
    #[arg(long)]
    pub drafts: bool,
}

/// The rule every tag, group, and variant name follows, as a usage error says it.
const NAME_RULE: &str =
    "names are lowercase letters and digits, starting with a letter, with single hyphens, at most 40 characters";

fn parse_tag(text: &str) -> Result<Tag, String> {
    Tag::try_new(text).map_err(|_| format!("`{text}` is not a tag name: {NAME_RULE}"))
}

fn parse_variant(text: &str) -> Result<VariantName, String> {
    VariantName::try_new(text).map_err(|_| format!("`{text}` is not a wording name: {NAME_RULE}"))
}

impl SelectArgs {
    /// The selection these options ask for.
    #[must_use]
    pub fn selection(&self) -> Selection {
        let scope = if self.core_only {
            Scope::CoreOnly
        } else {
            SelectedTags::new(self.tags.iter().cloned().collect()).map_or(Scope::All, Scope::Tags)
        };
        Selection {
            scope,
            drafts: self.drafts,
            prefer: self.prefer.clone(),
            picks: std::collections::BTreeMap::new(),
            // Ordering is a spec's choice; there is no flag for it.
            promote: Vec::new(),
        }
    }
}

/// The options that decide how a resume is rendered, shared by the commands that render.
#[derive(Debug, Clone, Args)]
pub struct RenderArgs {
    /// The style to render with.
    #[arg(short, long, default_value = "pragmatic", value_name = "NAME")]
    pub style: StyleName,

    /// A directory of styles to use besides the built-in ones; a style here wins over a built-in of the same name.
    #[arg(long, value_name = "DIR")]
    pub styles_dir: Option<PathBuf>,

    /// The paper size.
    #[arg(long, value_enum, default_value_t)]
    pub paper: Paper,

    /// Fail when the resume needs more pages than this (1 to 20).
    #[arg(long, value_name = "N")]
    pub max_pages: Option<PageLimit>,

    /// Which fonts may be used: `auto` uses installed fonts too, `bundled` only the ones in the binary.
    #[arg(long, value_enum, default_value_t)]
    pub fonts: FontsMode,

    /// A directory of fonts to use in `--fonts auto` mode; repeatable.
    #[arg(long = "font-path", value_name = "DIR")]
    pub font_path: Vec<PathBuf>,
}

impl Cli {
    /// Parses `args` and runs the checks clap cannot express, returning the error instead of exiting.
    ///
    /// # Errors
    /// A usage error: an unknown option, a missing value, a value out of range, `--font-path` with
    /// `--fonts bundled`, or two inputs that would write the same PDF. `clap::Error::exit` prints it and
    /// exits with code 2 (or 0 for `--help` and `--version`).
    pub fn try_parse_validated<I, T>(args: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let cli = Self::try_parse_from(args)?;
        cli.validate()?;
        Ok(cli)
    }

    fn validate(&self) -> Result<(), clap::Error> {
        match &self.command {
            Command::Letter(args) => args.validate(self.format).map_err(usage_error),
            Command::Build(build) => {
                build.render.validate()?;
                build.validate_spec()?;
                build.validate_outputs()
            }
            // No PDF is written, so two inputs with the same name are fine.
            Command::Doctor(doctor) => {
                doctor.assess.render.validate()?;
                doctor.validate_fix_inputs()
            }
            Command::Check(check) => check.assess.render.validate(),
            Command::Watch(watch) => watch.validate(self.dry_run),
            Command::Styles(_)
            | Command::ExportStyle(_)
            | Command::Export(_)
            | Command::Tags(_)
            | Command::Licenses => Ok(()),
        }
    }
}

/// The suffix the letter of a file or a spec adds to its name: `acme.md` has the letter `acme-letter.pdf`.
pub(crate) const LETTER_SUFFIX: &str = "-letter";

/// Refuses a resume whose name ends in `-letter`, because its PDF would be named like the letter of another file.
///
/// `letter acme.md` and `letter --spec positions/acme.toml` write `acme-letter.pdf`, and `build acme-letter.md`
/// writes the same name; the writer replaces a file, so one would silently replace the other. Case is ignored: on
/// macOS and Windows names that differ only in case name the same file. Only `build` and `watch` write this name;
/// `doctor` and `check` write no PDF and keep working on such a file, so it can be fixed before it is renamed.
fn refuse_letter_stem(input: &Path) -> Result<(), clap::Error> {
    let stem = BuildArgs::pdf_stem(input);
    // The suffix is ASCII, so cutting its length off the end of a stem that ends in it is on a character boundary.
    let Some(base) = stem
        .to_ascii_lowercase()
        .ends_with(LETTER_SUFFIX)
        .then(|| &stem[..stem.len() - LETTER_SUFFIX.len()])
    else {
        return Ok(());
    };
    Err(usage_error(format!(
        "{} would write {stem}.pdf, which is the name of the letter built from {base}.md; rename the resume, \
         for example to {base}-resume.md",
        input.display(),
    )))
}

/// A usage error in clap's own format, so it exits with code 2 like any other.
fn usage_error(message: impl std::fmt::Display) -> clap::Error {
    Cli::command().error(ErrorKind::ArgumentConflict, message)
}

impl RenderArgs {
    /// `--fonts bundled` ignores `--font-path`, so giving both is a mistake. clap has no "conflicts with this
    /// value" rule, and a plain conflict would also reject the explicit `--fonts auto --font-path DIR`.
    fn validate(&self) -> Result<(), clap::Error> {
        if self.fonts == FontsMode::Bundled && !self.font_path.is_empty() {
            return Err(usage_error(
                "--font-path has no effect with --fonts bundled; use --fonts auto, or drop --font-path",
            ));
        }
        Ok(())
    }
}

impl WatchArgs {
    /// Watching without writing has no purpose, so `--dry-run` is a usage error; checked here because the flag is
    /// global and clap cannot reject a global flag for one command.
    fn validate(&self, dry_run: bool) -> Result<(), clap::Error> {
        if dry_run {
            return Err(usage_error(
                "--dry-run cannot be used with `watch`, which exists to write the PDF; use `build --dry-run`",
            ));
        }
        refuse_letter_stem(&self.file)?;
        self.render.validate()
    }
}

impl DoctorArgs {
    /// `--fix` plans every file from the text it read, so the same file twice would be fixed twice from the same
    /// original and the second write would fail. Compared by path as written; a link to the same file is caught
    /// when the write finds the file changed.
    fn validate_fix_inputs(&self) -> Result<(), clap::Error> {
        if !self.fix {
            return Ok(());
        }
        let mut seen: Vec<PathBuf> = Vec::new();
        for file in &self.files {
            // `components` keeps a leading `./`, which names the same file.
            let normalized: PathBuf = file
                .components()
                .filter(|part| !matches!(part, std::path::Component::CurDir))
                .collect();
            if seen.contains(&normalized) {
                return Err(usage_error(format!(
                    "{} is given twice; with --fix each file is fixed once",
                    file.display()
                )));
            }
            seen.push(normalized);
        }
        Ok(())
    }
}

impl BuildArgs {
    /// The file stem of `input`, which names the PDF built from it and is the PDF's identity.
    #[must_use]
    pub fn pdf_stem(input: &Path) -> String {
        input
            .file_stem()
            .map_or_else(|| "resume".into(), |stem| stem.to_string_lossy().into_owned())
    }

    /// The name of the PDF built from `input`.
    #[must_use]
    pub fn pdf_name(input: &Path) -> String {
        format!("{}.pdf", Self::pdf_stem(input))
    }

    /// The settings typed on the command line that choose what a spec chooses, and only those.
    ///
    /// A value that was not typed is `None`, even where the flag has a default, so a default is never taken for
    /// a choice.
    #[must_use]
    pub fn explicit(&self) -> Explicit {
        let given = &self.spec.given;
        let scope = if given.has(OverrideFlag::CoreOnly) {
            Some(ScopeFlag::CoreOnly)
        } else if given.has(OverrideFlag::Tags) {
            SelectedTags::new(self.select.tags.iter().cloned().collect()).map(ScopeFlag::Tags)
        } else {
            None
        };
        Explicit {
            style: given.has(OverrideFlag::Style).then(|| self.render.style.clone()),
            paper: given.has(OverrideFlag::Paper).then_some(self.render.paper),
            scope,
            prefer: given.has(OverrideFlag::Prefer).then(|| self.select.prefer.clone()),
        }
    }

    /// Whether settings typed beside `--spec` are let through.
    #[must_use]
    pub fn override_policy(&self) -> OverridePolicy {
        if self.spec.allow_overrides {
            OverridePolicy::Allow
        } else {
            OverridePolicy::Refuse
        }
    }

    /// What clap cannot say about `--spec`: a file beside it, and settings that it would be silently changed by.
    ///
    /// `--profiles`, `--allow-overrides`, and `--print-spec` need `--spec` by clap's own `requires`. This check
    /// stays out of clap on purpose: a `conflicts_with` between `--spec` and FILE makes clap stop enforcing
    /// those `requires` whenever a FILE is given.
    fn validate_spec(&self) -> Result<(), clap::Error> {
        if self.spec.spec.is_none() {
            return Ok(());
        }
        if let Some(file) = self.files.first() {
            return Err(usage_error(format!(
                "--spec builds the file the spec names, so {} cannot be given too; build either a spec or files",
                file.display()
            )));
        }
        let explicit = self.explicit();
        if self.spec.print_spec {
            let flags = explicit.flags();
            if !flags.is_empty() {
                let names: Vec<String> = flags.iter().map(ToString::to_string).collect();
                return Err(usage_error(format!(
                    "--print-spec shows the spec as written, so {} cannot be given with it; leave the flags out",
                    names.join(", ")
                )));
            }
            return Ok(());
        }
        explicit.admit(self.override_policy()).map(|_| ()).map_err(usage_error)
    }

    /// Two inputs with the same name would write the same PDF; on macOS and Windows names that differ only in
    /// case name the same file, so case is ignored. Nothing has been written when this is checked.
    fn validate_outputs(&self) -> Result<(), clap::Error> {
        let mut seen: HashMap<String, &Path> = HashMap::new();
        for input in &self.files {
            refuse_letter_stem(input)?;
            let name = Self::pdf_name(input);
            if let Some(first) = seen.insert(name.to_lowercase(), input) {
                return Err(usage_error(format!(
                    "{} and {} would both write {name}; rename one of them or build them separately",
                    first.display(),
                    input.display()
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};
    use clap::FromArgMatches as _;
    use clap::error::ErrorKind;
    use pretty_assertions::assert_eq;

    use crate::letter::{LengthUnit, LetterOutput, LinkStyle};
    use crate::model::options::Paper;
    use crate::spec::{LetterChoices, LetterExplicit};

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_validated(std::iter::once("resume").chain(args.iter().copied()))
    }

    fn cli(args: &[&str]) -> Cli {
        assert_ok!(parse(args))
    }

    fn build_of(cli: &Cli) -> &BuildArgs {
        match &cli.command {
            Command::Build(build) => build,
            other => panic!("expected `build`, got {other:?}"),
        }
    }

    // ========================================================================
    // Defaults and shapes
    // ========================================================================

    #[test]
    fn defaults_match_the_documented_ones() {
        let cli = cli(&["build", "a.md"]);
        let build = build_of(&cli);
        assert!(!cli.dry_run);
        assert_eq!(cli.format, OutputFormat::Human);
        assert_eq!(cli.color, ColorMode::Auto);
        assert_eq!(build.files, [PathBuf::from("a.md")]);
        assert_eq!(build.out, PathBuf::from("out"));
        assert_eq!(build.render.style.as_ref() as &str, "pragmatic");
        assert_eq!(build.render.styles_dir, None);
        assert_eq!(build.render.paper, Paper::Letter);
        assert_eq!(build.render.max_pages, None);
        assert_eq!(build.render.fonts, FontsMode::Auto);
        assert!(build.render.font_path.is_empty());
    }

    #[test]
    fn the_paper_default_is_letter_and_a4_is_accepted() {
        // `Paper` displays as `us-letter` but its argument name is `letter`: a default taken from Display
        // would not parse. This pins that both spellings the user can type are right.
        assert_eq!(
            build_of(&cli(&["build", "a.md", "--paper", "letter"])).render.paper,
            Paper::Letter
        );
        assert_eq!(
            build_of(&cli(&["build", "a.md", "--paper", "a4"])).render.paper,
            Paper::A4
        );
    }

    #[test]
    fn dry_run_is_global_so_it_can_come_before_or_after_the_command() {
        assert!(cli(&["--dry-run", "build", "a.md"]).dry_run);
        assert!(cli(&["build", "a.md", "--dry-run"]).dry_run);
    }

    #[test]
    fn format_and_color_are_global_and_take_their_names() {
        let cli = cli(&["build", "a.md", "--format", "json", "--color", "never"]);
        assert_eq!((cli.format, cli.color), (OutputFormat::Json, ColorMode::Never));
    }

    #[test]
    fn font_path_is_repeatable_and_a_style_name_is_normalized() {
        let cli = cli(&[
            "build",
            "a.md",
            "--font-path",
            "one",
            "--font-path",
            "two",
            "-s",
            "Mono",
            "-o",
            "pdfs",
        ]);
        let build = build_of(&cli);
        assert_eq!(build.render.font_path, [PathBuf::from("one"), PathBuf::from("two")]);
        assert_eq!(build.render.style.as_ref() as &str, "mono");
        assert_eq!(build.out, PathBuf::from("pdfs"));
    }

    #[test]
    fn max_pages_is_one_to_twenty() {
        let limit = |n: &str| {
            build_of(&cli(&["build", "a.md", "--max-pages", n]))
                .render
                .max_pages
                .map(u8::from)
        };
        assert_eq!(limit("1"), Some(1));
        assert_eq!(limit("20"), Some(20));
        for bad in ["0", "21", "many"] {
            let err = assert_err!(parse(&["build", "a.md", "--max-pages", bad]));
            assert_eq!(err.kind(), ErrorKind::ValueValidation, "{bad}: {err}");
        }
    }

    #[test]
    fn a_bad_style_name_is_a_usage_error() {
        let err = assert_err!(parse(&["build", "a.md", "--style", "9 bad"]));
        assert_eq!(err.kind(), ErrorKind::ValueValidation, "{err}");
    }

    #[test]
    fn build_needs_at_least_one_file() {
        let err = assert_err!(parse(&["build"]));
        assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument, "{err}");
    }

    // ========================================================================
    // What clap cannot express
    // ========================================================================

    #[test]
    fn fonts_bundled_with_font_path_is_a_usage_error() {
        let err = assert_err!(parse(&["build", "a.md", "--fonts", "bundled", "--font-path", "x"]));
        assert_eq!(err.kind(), ErrorKind::ArgumentConflict, "{err}");
        assert!(err.to_string().contains("--font-path"), "{err}");
    }

    #[test]
    fn explicit_fonts_auto_with_font_path_is_accepted() {
        assert_ok!(parse(&["build", "a.md", "--fonts", "auto", "--font-path", "x"]));
        assert_ok!(parse(&["build", "a.md", "--font-path", "x"]));
        assert_ok!(parse(&["build", "a.md", "--fonts", "bundled"]));
    }

    #[test]
    fn two_inputs_with_the_same_name_are_a_usage_error_whatever_their_case() {
        for pair in [["a/Resume.md", "b/Resume.md"], ["a/Resume.md", "b/resume.md"]] {
            let err = assert_err!(parse(&["build", pair[0], pair[1]]));
            assert_eq!(err.kind(), ErrorKind::ArgumentConflict, "{pair:?}: {err}");
            assert!(err.to_string().to_lowercase().contains("resume.pdf"), "{err}");
        }
    }

    #[test]
    fn inputs_with_different_names_are_accepted() {
        assert_ok!(parse(&["build", "a/One.md", "b/Two.md"]));
    }

    // ========================================================================
    // doctor and check
    // ========================================================================

    fn doctor_of(cli: &Cli) -> &DoctorArgs {
        match &cli.command {
            Command::Doctor(doctor) => doctor,
            other => panic!("expected `doctor`, got {other:?}"),
        }
    }

    #[test]
    fn doctor_and_check_take_files_and_the_render_options() {
        let cli = cli(&["doctor", "a.md", "b.md", "--style", "mono", "-o", "pdfs"]);
        let doctor = doctor_of(&cli);
        assert_eq!(doctor.files, [PathBuf::from("a.md"), PathBuf::from("b.md")]);
        assert_eq!(doctor.assess.out, PathBuf::from("pdfs"));
        assert_eq!(doctor.assess.render.style.as_ref() as &str, "mono");
        assert!(!doctor.fix && doctor.explain.is_none());
        let Command::Check(check) = self::cli(&["check", "a.md"]).command else {
            panic!("expected `check`")
        };
        assert_eq!(check.assess.out, PathBuf::from("out"));
    }

    #[test]
    fn check_rejects_fix_and_explain_as_usage_errors() {
        for args in [
            &["check", "a.md", "--fix"][..],
            &["check", "--explain", "period-hyphen"][..],
        ] {
            let err = assert_err!(parse(args));
            assert_eq!(err.kind(), ErrorKind::UnknownArgument, "{args:?}: {err}");
        }
    }

    #[test]
    fn check_and_doctor_need_a_file_unless_explaining() {
        assert_eq!(
            assert_err!(parse(&["check"])).kind(),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            assert_err!(parse(&["doctor"])).kind(),
            ErrorKind::MissingRequiredArgument
        );
        assert_ok!(parse(&["doctor", "--explain", "period-hyphen"]));
    }

    #[test]
    fn explain_takes_a_known_code_alone() {
        let cli = cli(&["doctor", "--explain", "period-hyphen"]);
        assert_eq!(doctor_of(&cli).explain, Some(DiagnosticCode::PeriodHyphen));
        for args in [
            &["doctor", "a.md", "--explain", "period-hyphen"][..],
            &["doctor", "--explain", "period-hyphen", "--fix"][..],
        ] {
            assert_eq!(assert_err!(parse(args)).kind(), ErrorKind::ArgumentConflict, "{args:?}");
        }
    }

    #[test]
    fn an_unknown_code_is_a_usage_error_that_suggests_the_close_ones() {
        let err = assert_err!(parse(&["doctor", "--explain", "periods-hyphen"]));
        assert_eq!(err.kind(), ErrorKind::ValueValidation);
        assert!(err.to_string().contains("did you mean `period-hyphen`"), "{err}");
        let prefix = assert_err!(parse(&["doctor", "--explain", "period"])).to_string();
        for code in ["period-hyphen", "period-unparseable", "period-end-before-start"] {
            assert!(prefix.contains(code) || code == "period-end-before-start", "{prefix}");
        }
        let far = assert_err!(parse(&["doctor", "--explain", "zzzzzz"])).to_string();
        assert!(
            far.contains("is not a diagnostic code") && !far.contains("did you mean"),
            "{far}"
        );
    }

    #[test]
    fn explain_accepts_every_code() {
        use strum::IntoEnumIterator;
        for code in DiagnosticCode::iter() {
            assert_eq!(assert_ok!(parse_code(&code.to_string())), code);
        }
    }

    #[test]
    fn fix_rejects_the_same_file_twice_but_other_commands_do_not() {
        let err = assert_err!(parse(&["doctor", "a.md", "./a.md", "--fix"]));
        assert_eq!(err.kind(), ErrorKind::ArgumentConflict, "{err}");
        assert_ok!(parse(&["doctor", "a.md", "a.md"]));
        assert_ok!(parse(&["check", "a.md", "a.md"]));
        assert_ok!(parse(&["doctor", "a/Resume.md", "b/Resume.md", "--fix"]));
    }

    #[test]
    fn doctor_and_check_share_the_font_usage_rule_but_not_the_output_name_rule() {
        for command in ["doctor", "check"] {
            let err = assert_err!(parse(&[command, "a.md", "--fonts", "bundled", "--font-path", "x"]));
            assert_eq!(err.kind(), ErrorKind::ArgumentConflict, "{command}: {err}");
            assert_ok!(parse(&[command, "a/Resume.md", "b/resume.md"]));
        }
    }

    // ========================================================================
    // styles and export-style
    // ========================================================================

    #[test]
    fn styles_takes_an_optional_styles_dir() {
        let Command::Styles(plain) = cli(&["styles"]).command else {
            panic!("expected `styles`")
        };
        assert_eq!(plain.styles_dir, None);
        let Command::Styles(with_dir) = cli(&["styles", "--styles-dir", "mine"]).command else {
            panic!("expected `styles`")
        };
        assert_eq!(with_dir.styles_dir, Some(PathBuf::from("mine")));
    }

    // ------------------------------------------------------------------------------------------
    // watch and export
    // ------------------------------------------------------------------------------------------

    fn watch_args(args: &[&str]) -> WatchArgs {
        let Command::Watch(watch) = cli(&[&["watch"], args].concat()).command else {
            panic!("expected `watch`")
        };
        watch
    }

    #[test]
    fn watch_defaults_to_150ms_and_does_not_open() {
        let watch = watch_args(&["Resume.md"]);
        assert_eq!(watch.debounce, std::time::Duration::from_millis(150));
        assert!(!watch.open);
        assert_eq!(watch.out, PathBuf::from("out"));
    }

    #[test]
    fn debounce_parses_humantime_values() {
        for (text, wanted) in [
            ("150ms", std::time::Duration::from_millis(150)),
            ("2s", std::time::Duration::from_secs(2)),
            ("1s 500ms", std::time::Duration::from_millis(1500)),
            ("0ms", std::time::Duration::ZERO),
        ] {
            assert_eq!(
                watch_args(&["Resume.md", "--debounce", text]).debounce,
                wanted,
                "{text}"
            );
        }
    }

    #[test]
    fn watch_takes_max_pages_like_build() {
        assert_eq!(
            watch_args(&["r.md", "--max-pages", "2"])
                .render
                .max_pages
                .map(PageLimit::into_inner),
            Some(2)
        );
        assert!(Cli::try_parse_validated(["resume", "watch", "r.md", "--max-pages", "0"]).is_err());
    }

    #[test]
    fn a_bad_debounce_is_a_usage_error_that_shows_an_example() {
        for text in ["fast", "150", "1 parsec", ""] {
            let error = assert_err!(Cli::try_parse_validated([
                "resume",
                "watch",
                "r.md",
                "--debounce",
                text
            ]));
            assert_eq!(error.exit_code(), 2, "{text}");
            assert!(error.to_string().contains("`150ms`"), "{text}: {error}");
        }
    }

    #[test]
    fn watch_rejects_dry_run_before_or_after_the_command() {
        for args in [
            &["resume", "--dry-run", "watch", "r.md"][..],
            &["resume", "watch", "r.md", "--dry-run"][..],
        ] {
            let error = assert_err!(Cli::try_parse_validated(args));
            assert_eq!(error.exit_code(), 2);
            assert!(
                error.to_string().contains("--dry-run cannot be used with `watch`"),
                "{error}"
            );
        }
    }

    #[test]
    fn watch_takes_one_file_and_the_render_options() {
        let watch = watch_args(&["r.md", "-s", "mono", "--paper", "a4", "--fonts", "bundled", "--open"]);
        assert_eq!(watch.file, PathBuf::from("r.md"));
        assert_eq!(watch.render.style.as_ref() as &str, "mono");
        assert!(watch.open);
        assert!(Cli::try_parse_validated(["resume", "watch"]).is_err());
        assert!(Cli::try_parse_validated(["resume", "watch", "a.md", "b.md"]).is_err());
        let error = assert_err!(Cli::try_parse_validated([
            "resume",
            "watch",
            "a.md",
            "--fonts",
            "bundled",
            "--font-path",
            "f"
        ]));
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn export_takes_a_file_a_directory_a_style_and_force() {
        let Command::Export(plain) = cli(&["export", "r.md", "preview"]).command else {
            panic!("expected `export`")
        };
        assert_eq!(plain.file, PathBuf::from("r.md"));
        assert_eq!(plain.dir, PathBuf::from("preview"));
        assert_eq!(plain.style.as_ref() as &str, "pragmatic");
        assert!(!plain.force);
        let Command::Export(more) = cli(&["export", "r.md", "d", "-s", "mono", "--styles-dir", "s", "--force"]).command
        else {
            panic!("expected `export`")
        };
        assert_eq!(more.style.as_ref() as &str, "mono");
        assert_eq!(more.styles_dir, Some(PathBuf::from("s")));
        assert!(more.force);
        for args in [
            &["export"][..],
            &["export", "r.md"][..],
            &["export", "a.md", "b.md", "d"][..],
        ] {
            let all: Vec<&str> = std::iter::once("resume").chain(args.iter().copied()).collect();
            assert_eq!(assert_err!(Cli::try_parse_validated(all)).exit_code(), 2, "{args:?}");
        }
    }

    #[test]
    fn export_style_takes_a_name_a_directory_and_force() {
        let Command::ExportStyle(plain) = cli(&["export-style", "Pragmatic", "mine"]).command else {
            panic!("expected `export-style`")
        };
        assert_eq!(plain.name.as_ref() as &str, "pragmatic");
        assert_eq!(plain.dir, PathBuf::from("mine"));
        assert!(!plain.force, "overwriting needs --force");
        let Command::ExportStyle(forced) = cli(&["export-style", "pragmatic", "mine", "--force"]).command else {
            panic!("expected `export-style`")
        };
        assert!(forced.force);
    }

    #[test]
    fn export_style_needs_a_name_and_a_directory() {
        for args in [&["export-style"][..], &["export-style", "pragmatic"][..]] {
            let err = assert_err!(parse(args));
            assert_eq!(err.kind(), ErrorKind::MissingRequiredArgument, "{args:?}: {err}");
        }
    }

    // ========================================================================
    // `build --spec`
    // ========================================================================

    fn flags_of(args: &[&str]) -> Vec<OverrideFlag> {
        build_of(&cli(args)).explicit().flags()
    }

    #[test]
    fn a_spec_needs_no_file_and_a_file_needs_no_spec() {
        let by_spec = cli(&["build", "--spec", "positions/acme.toml"]);
        let build = build_of(&by_spec);
        assert_eq!(build.spec.spec, Some(PathBuf::from("positions/acme.toml")));
        assert!(build.files.is_empty());
        assert_eq!(
            assert_err!(parse(&["build"])).kind(),
            ErrorKind::MissingRequiredArgument
        );
        assert!(build_of(&cli(&["build", "a.md"])).spec.spec.is_none());
    }

    #[test]
    fn a_spec_with_a_file_is_a_usage_error_naming_the_file() {
        let err = assert_err!(parse(&["build", "a.md", "--spec", "s.toml"]));
        assert_eq!(err.kind(), ErrorKind::ArgumentConflict);
        assert!(err.to_string().contains("a.md"), "{err}");
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn profiles_allow_overrides_and_print_spec_each_need_a_spec_even_beside_a_file() {
        // clap stops enforcing `requires` once FILE has `conflicts_with = "spec"`, so the check is ours; this
        // pins that a file beside the flag does not hide the missing spec.
        for flag in [
            &["--profiles", "p"][..],
            &["--allow-overrides"][..],
            &["--print-spec"][..],
        ] {
            for file in [&[][..], &["a.md"][..]] {
                let args: Vec<&str> = ["build"].iter().chain(file).chain(flag).copied().collect();
                let err = assert_err!(parse(&args));
                assert!(
                    matches!(err.kind(), ErrorKind::MissingRequiredArgument),
                    "{args:?}: {:?} {err}",
                    err.kind()
                );
            }
        }
    }

    #[test]
    fn a_repeated_spec_is_refused() {
        let err = assert_err!(parse(&["build", "--spec", "a.toml", "--spec", "b.toml"]));
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn a_style_typed_equal_to_the_default_is_still_given() {
        assert_eq!(flags_of(&["build", "a.md"]), []);
        assert_eq!(
            flags_of(&["build", "a.md", "--paper", "letter", "-s", "pragmatic"]),
            [OverrideFlag::Style, OverrideFlag::Paper]
        );
    }

    #[test]
    fn every_flag_that_chooses_is_detected_by_the_line_that_types_it() {
        let cases: [(&[&str], OverrideFlag); 6] = [
            (&["--tags", "ai"], OverrideFlag::Tags),
            (&["--tags=ai,platform"], OverrideFlag::Tags),
            (&["--core-only"], OverrideFlag::CoreOnly),
            (&["--prefer", "short"], OverrideFlag::Prefer),
            (&["--style", "mono"], OverrideFlag::Style),
            (&["--paper", "a4"], OverrideFlag::Paper),
        ];
        for (typed, flag) in cases {
            let args: Vec<&str> = ["build", "a.md"].iter().chain(typed).copied().collect();
            assert_eq!(flags_of(&args), [flag], "{typed:?}");
        }
    }

    #[test]
    fn the_probe_sees_typed_flags_whichever_way_the_line_is_parsed() {
        // `try_parse_from` is what library callers and tests use; `try_parse_validated` adds only checks.
        let cli = assert_ok!(Cli::try_parse_from(["resume", "build", "a.md", "--paper", "a4"]));
        assert_eq!(build_of(&cli).explicit().flags(), [OverrideFlag::Paper]);
    }

    #[test]
    fn the_probe_asks_about_ids_that_build_really_has() {
        // `value_source` panics in debug builds for an id a command lacks, and returns `None` for one that was
        // renamed: either would turn detection off silently. Every id the probe asks about must exist.
        let command = Cli::command();
        let build = command.find_subcommand("build").expect("build exists");
        let ids: Vec<&str> = build.get_arguments().map(|arg| arg.get_id().as_str()).collect();
        for id in Given::ids() {
            assert!(ids.contains(&id), "`build` has no argument `{id}`: {ids:?}");
        }
    }

    #[test]
    fn the_probe_does_not_panic_on_a_command_without_the_ids() {
        // `watch` has style, paper, tags, core-only, and prefer too, but a command with none must not panic.
        let matches = clap::Command::new("none").get_matches_from(["none"]);
        assert_eq!(assert_ok!(Given::from_arg_matches(&matches)), Given::default());
    }

    #[test]
    fn every_stray_flag_is_named_in_one_usage_error() {
        let err = assert_err!(parse(&[
            "build", "--spec", "s.toml", "--tags", "a", "--prefer", "b", "--style", "mono", "--paper", "a4"
        ]));
        assert_eq!(err.kind(), ErrorKind::ArgumentConflict);
        assert_eq!(err.exit_code(), 2);
        let text = err.to_string();
        assert!(
            text.contains("--tags, --prefer, --style and --paper cannot be used beside --spec"),
            "{text}"
        );
        assert!(text.contains("--allow-overrides"), "{text}");
    }

    #[test]
    fn core_only_beside_a_spec_is_named_too() {
        let err = assert_err!(parse(&["build", "--spec", "s.toml", "--core-only"]));
        assert!(
            err.to_string().contains("--core-only cannot be used beside --spec"),
            "{err}"
        );
    }

    #[test]
    fn allow_overrides_admits_every_choosing_flag() {
        let cli = cli(&[
            "build",
            "--spec",
            "s.toml",
            "--allow-overrides",
            "--tags",
            "a",
            "--prefer",
            "b",
            "--style",
            "mono",
            "--paper",
            "a4",
        ]);
        assert_eq!(build_of(&cli).explicit().flags().len(), 4);
        assert_eq!(build_of(&cli).override_policy(), OverridePolicy::Allow);
    }

    #[test]
    fn drafts_and_environment_flags_are_allowed_beside_a_spec_without_overrides() {
        let parsed = cli(&[
            "build",
            "--spec",
            "s.toml",
            "--drafts",
            "--fonts",
            "bundled",
            "--styles-dir",
            "d",
            "--max-pages",
            "2",
            "-o",
            "pdfs",
        ]);
        let build = build_of(&parsed);
        assert!(build.select.drafts);
        assert_eq!(build.override_policy(), OverridePolicy::Refuse);
        assert_eq!(build.explicit().flags(), []);
        let with_path = cli(&["build", "--spec", "s.toml", "--font-path", "f"]);
        assert_eq!(build_of(&with_path).render.font_path, [PathBuf::from("f")]);
    }

    #[test]
    fn print_spec_with_a_choosing_flag_is_refused_even_with_allow_overrides() {
        for extra in [&[][..], &["--allow-overrides"][..]] {
            let args: Vec<&str> = ["build", "--spec", "s.toml", "--print-spec", "--tags", "a"]
                .iter()
                .chain(extra)
                .copied()
                .collect();
            let err = assert_err!(parse(&args));
            assert_eq!(err.exit_code(), 2);
            assert!(err.to_string().contains("--print-spec"), "{err}");
            assert!(err.to_string().contains("--tags"), "{err}");
        }
        assert_ok!(parse(&["build", "--spec", "s.toml", "--print-spec"]));
    }

    #[test]
    fn profiles_is_taken_with_a_spec() {
        let cli = cli(&["build", "--spec", "s.toml", "--profiles", "p"]);
        assert_eq!(build_of(&cli).spec.profiles, Some(PathBuf::from("p")));
    }
    // ========================================================================
    // `letter --spec`
    // ========================================================================

    fn letter_of(cli: &Cli) -> &LetterArgs {
        match &cli.command {
            Command::Letter(letter) => letter,
            other => panic!("expected `letter`, got {other:?}"),
        }
    }

    #[test]
    fn a_letter_needs_a_file_or_a_spec_and_never_both() {
        let by_spec = cli(&["letter", "--spec", "positions/acme.toml"]);
        assert_eq!(
            letter_of(&by_spec).spec.spec,
            Some(PathBuf::from("positions/acme.toml"))
        );
        assert!(letter_of(&by_spec).file.is_none());
        assert_eq!(
            assert_err!(parse(&["letter"])).kind(),
            ErrorKind::MissingRequiredArgument
        );
        let both = assert_err!(parse(&["letter", "a.md", "--spec", "s.toml"]));
        assert_eq!(both.kind(), ErrorKind::ArgumentConflict);
        assert_eq!(both.exit_code(), 2);
        assert!(both.to_string().contains("a.md"), "{both}");
        assert_eq!(letter_of(&cli(&["letter", "a.md"])).input(), Some(Path::new("a.md")));
    }

    #[test]
    fn letter_profiles_allow_overrides_and_print_spec_each_need_a_spec_even_beside_a_file() {
        for flag in [
            &["--profiles", "p"][..],
            &["--allow-overrides"][..],
            &["--print-spec"][..],
        ] {
            for file in [&[][..], &["a.md"][..]] {
                let args: Vec<&str> = ["letter"].iter().chain(file).chain(flag).copied().collect();
                let err = assert_err!(parse(&args));
                assert!(
                    matches!(err.kind(), ErrorKind::MissingRequiredArgument),
                    "{args:?}: {:?} {err}",
                    err.kind()
                );
            }
        }
    }

    #[test]
    fn every_letter_flag_a_spec_can_say_is_an_override_and_the_machine_flags_are_not() {
        let overrides = [
            ("--tags", "a", OverrideFlag::Tags),
            ("--prefer", "short", OverrideFlag::Prefer),
            ("--style", "mono", OverrideFlag::Style),
            ("--paper", "a4", OverrideFlag::Paper),
            ("--output-format", "pdf", OverrideFlag::OutputFormat),
            ("--max-length", "5", OverrideFlag::MaxLength),
            ("--links", "text-only", OverrideFlag::Links),
            ("--company", "Acme", OverrideFlag::Company),
            ("--title", "Eng", OverrideFlag::Title),
            ("--recipient", "Dana", OverrideFlag::Recipient),
            ("--date", "2028-01-01", OverrideFlag::Date),
        ];
        for (flag, value, expected) in overrides {
            let parsed = cli(&["letter", "--spec", "s.toml", "--allow-overrides", flag, value]);
            assert_eq!(letter_of(&parsed).explicit().flags(), [expected], "{flag}");
            let err = assert_err!(parse(&["letter", "--spec", "s.toml", flag, value]));
            assert_eq!(err.exit_code(), 2, "{flag}");
            assert!(err.to_string().contains(flag), "{flag}: {err}");
        }
        let unit = cli(&[
            "letter",
            "--spec",
            "s.toml",
            "--allow-overrides",
            "--length-unit",
            "words",
        ]);
        assert_eq!(letter_of(&unit).explicit().flags(), [OverrideFlag::LengthUnit]);
        let core = cli(&["letter", "--spec", "s.toml", "--allow-overrides", "--core-only"]);
        assert_eq!(letter_of(&core).explicit().flags(), [OverrideFlag::CoreOnly]);
        // About the machine, or a preview: allowed beside a spec with no override.
        let machine = cli(&[
            "letter",
            "--spec",
            "s.toml",
            "--drafts",
            "--fonts",
            "bundled",
            "--styles-dir",
            "d",
            "--max-pages",
            "2",
            "-o",
            "pdfs",
        ]);
        assert!(letter_of(&machine).select.drafts);
        assert_eq!(letter_of(&machine).override_policy(), OverridePolicy::Refuse);
        assert_eq!(letter_of(&machine).explicit().flags(), []);
        let with_path = cli(&["letter", "--spec", "s.toml", "--font-path", "f"]);
        assert_eq!(letter_of(&with_path).render.font_path, [PathBuf::from("f")]);
    }

    #[test]
    fn every_stray_letter_flag_is_named_in_one_usage_error() {
        let err = assert_err!(parse(&[
            "letter",
            "--spec",
            "s.toml",
            "--tags",
            "a",
            "--links",
            "text-only",
            "--date",
            "2028-01-01",
            "--title",
            "T",
        ]));
        assert_eq!(err.kind(), ErrorKind::ArgumentConflict);
        let text = err.to_string();
        assert!(
            text.contains("--tags, --links, --title and --date cannot be used beside --spec"),
            "{text}"
        );
        assert!(text.contains("--allow-overrides"), "{text}");
    }

    #[test]
    fn letter_print_spec_with_a_choosing_flag_is_refused_even_with_allow_overrides() {
        for extra in [&[][..], &["--allow-overrides"][..]] {
            let args: Vec<&str> = ["letter", "--spec", "s.toml", "--print-spec", "--links", "text-only"]
                .iter()
                .chain(extra)
                .copied()
                .collect();
            let err = assert_err!(parse(&args));
            assert_eq!(err.exit_code(), 2);
            assert!(
                err.to_string().contains("--print-spec") && err.to_string().contains("--links"),
                "{err}"
            );
        }
        assert_ok!(parse(&["letter", "--spec", "s.toml", "--print-spec"]));
    }

    #[test]
    fn a_length_unit_beside_a_spec_may_take_the_specs_limit_but_beside_a_file_it_needs_its_own() {
        assert_ok!(parse(&[
            "letter",
            "--spec",
            "s.toml",
            "--allow-overrides",
            "--length-unit",
            "words"
        ]));
        let err = assert_err!(parse(&["letter", "a.md", "--length-unit", "words"]));
        assert!(err.to_string().contains("--length-unit requires --max-length"), "{err}");
        assert_ok!(parse(&[
            "letter",
            "a.md",
            "--max-length",
            "9",
            "--length-unit",
            "words"
        ]));
    }

    #[test]
    fn json_with_a_typed_text_output_beside_a_spec_is_a_usage_error_and_with_the_specs_own_is_the_plans() {
        let typed = assert_err!(parse(&[
            "--format",
            "json",
            "letter",
            "--spec",
            "s.toml",
            "--allow-overrides",
            "--output-format",
            "text",
        ]));
        assert!(
            typed.to_string().contains("--format json cannot share stdout"),
            "{typed}"
        );
        // The output the spec chooses is not known until the spec is read, so the plan judges it.
        assert_ok!(parse(&["--format", "json", "letter", "--spec", "s.toml"]));
    }

    #[test]
    fn no_flag_a_spec_can_say_has_a_clap_default_so_a_typed_flag_is_a_present_one() {
        // The probe `build` needs for `--style` and `--paper` is not used on `letter` because none of its flags
        // has a default. This is the guard: a default added to any of them would make an absent flag look typed
        // and silently replace a spec's value.
        let command = Cli::command();
        let letter = command.find_subcommand("letter").expect("letter exists");
        for id in [
            "tags",
            "core_only",
            "prefer",
            "style",
            "paper",
            "output_format",
            "max_length",
            "length_unit",
            "links",
            "company",
            "title",
            "recipient",
            "date",
        ] {
            let arg = letter
                .get_arguments()
                .find(|arg| arg.get_id().as_str() == id)
                .unwrap_or_else(|| panic!("`letter` has no argument `{id}`"));
            assert!(arg.get_default_values().is_empty(), "`{id}` has a default: {arg:?}");
        }
        let bare = cli(&["letter", "--spec", "s.toml"]);
        assert_eq!(letter_of(&bare).explicit(), LetterExplicit::default());
    }

    #[test]
    fn a_letter_from_a_file_keeps_every_default_it_had() {
        let bare = cli(&["letter", "a.md"]);
        let choices = LetterChoices::from_flags(&letter_of(&bare).explicit());
        assert_eq!(
            (choices.output.value, choices.links.value, choices.length_unit.value),
            (LetterOutput::Text, LinkStyle::TextAndUrl, LengthUnit::Utf16)
        );
        assert_eq!(choices.paper.value, Paper::default());
        assert_eq!(choices.style.value.as_ref() as &str, "pragmatic");
        assert!(choices.max_length.is_none());
        let render = letter_of(&bare).effective_render();
        assert_eq!(render.max_pages.map(|p| p.into_inner()), Some(1));
    }
    // ========================================================================
    // A resume cannot be named like the letter of another file
    // ========================================================================

    #[test]
    fn build_and_watch_refuse_a_resume_named_like_the_letter_of_another_file() {
        for name in [
            "acme-letter.md",
            "acme-LETTER.md",
            "ACME-Letter.md",
            "./-letter.md",
            "dir/acme-letter.md",
        ] {
            for command in ["build", "watch"] {
                let err = assert_err!(parse(&[command, name]));
                assert_eq!(err.kind(), ErrorKind::ArgumentConflict, "{command} {name}");
                assert_eq!(err.exit_code(), 2);
                let text = err.to_string();
                assert!(
                    text.contains("the name of the letter built from"),
                    "{command} {name}: {text}"
                );
                assert!(text.contains("rename the resume"), "{text}");
            }
        }
    }

    #[test]
    fn the_refusal_names_the_file_the_pdf_and_a_name_that_works() {
        let text = assert_err!(parse(&["build", "dir/acme-letter.md"])).to_string();
        assert!(
            text.contains("dir/acme-letter.md would write acme-letter.pdf"),
            "{text}"
        );
        assert!(text.contains("the letter built from acme.md"), "{text}");
        assert!(text.contains("for example to acme-resume.md"), "{text}");
        // The name it suggests is accepted.
        assert_ok!(parse(&["build", "acme-resume.md"]));
    }

    #[test]
    fn only_a_stem_that_ends_in_the_suffix_is_refused() {
        for name in [
            "letter.md",
            "my-letters.md",
            "acme-letter-2.md",
            "acme-letter.v2.md",
            "acme.md",
            "Resume.md",
        ] {
            assert_ok!(parse(&["build", name]));
            assert_ok!(parse(&["watch", name]));
        }
    }

    #[test]
    fn the_suffix_is_judged_on_the_stem_so_the_extension_does_not_hide_or_make_it() {
        assert_err!(parse(&["build", "acme-letter.markdown"]));
        assert_err!(parse(&["build", "acme-letter"]));
        assert_ok!(parse(&["build", "acme-letter.md.bak"]));
    }

    #[test]
    fn one_such_name_among_several_is_refused_and_the_error_names_it() {
        let text = assert_err!(parse(&["build", "a.md", "b-letter.md", "c.md"])).to_string();
        assert!(text.contains("b-letter.md would write b-letter.pdf"), "{text}");
    }

    #[test]
    fn doctor_and_check_keep_working_on_such_a_file_because_they_write_no_pdf() {
        for command in ["doctor", "check"] {
            assert_ok!(parse(&[command, "acme-letter.md"]));
        }
    }

    #[test]
    fn a_letter_may_be_named_with_the_suffix_because_its_own_pdf_adds_another() {
        // `cover-letter.md` as a letter writes `cover-letter-letter.pdf`, which no resume can be named.
        assert_ok!(parse(&["letter", "cover-letter.md"]));
        assert_ok!(parse(&["letter", "--spec", "positions/acme.toml"]));
    }

    #[test]
    fn a_resume_and_the_letter_of_one_name_no_longer_share_a_pdf_name() {
        // The rule's reason: the two names a resume `X-letter` and a letter `X` would write are one.
        let letter_of = |stem: &str| format!("{stem}-letter.pdf");
        let resume_of = |input: &str| BuildArgs::pdf_name(Path::new(input));
        assert_eq!(letter_of("acme"), resume_of("acme-letter.md"));
        assert!(parse(&["build", "acme-letter.md"]).is_err(), "so the resume is refused");
        assert_ne!(letter_of("acme"), resume_of("acme.md"));
    }
}
