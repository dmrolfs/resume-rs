//! ABOUTME: Command-line argument definitions for the `resume` binary, built with clap derive.
//! ABOUTME: Options and defaults are documented here; checks clap cannot express run in `Cli::try_parse_validated`.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clap::error::ErrorKind;
use clap::{Args, CommandFactory, Parser, Subcommand};

use crate::model::StyleName;
use crate::model::options::{ColorMode, FontsMode, OutputFormat, PageLimit, Paper};

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
    /// Typeset one or more Markdown resumes into PDFs.
    Build(BuildArgs),
    /// List the styles that can be chosen.
    Styles(StylesArgs),
    /// Copy a built-in style to a directory, to edit it.
    ExportStyle(ExportStyleArgs),
    /// Print the license notices of the fonts built into this program.
    Licenses,
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
    /// The Markdown resumes to build.
    #[arg(required = true, value_name = "FILE")]
    pub files: Vec<PathBuf>,

    /// The directory the PDFs are written to; it is created if it does not exist.
    #[arg(short, long, default_value = "out", value_name = "DIR")]
    pub out: PathBuf,

    #[command(flatten)]
    pub render: RenderArgs,
}

/// The options that decide how a resume is rendered, shared by the commands that render.
#[derive(Debug, Args)]
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
            Command::Build(build) => {
                build.render.validate()?;
                build.validate_outputs()
            }
            Command::Styles(_) | Command::ExportStyle(_) | Command::Licenses => Ok(()),
        }
    }
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

    /// Two inputs with the same name would write the same PDF; on macOS and Windows names that differ only in
    /// case name the same file, so case is ignored. Nothing has been written when this is checked.
    fn validate_outputs(&self) -> Result<(), clap::Error> {
        let mut seen: HashMap<String, &Path> = HashMap::new();
        for input in &self.files {
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
    use clap::error::ErrorKind;
    use pretty_assertions::assert_eq;

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
}
