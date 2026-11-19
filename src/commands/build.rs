//! ABOUTME: `resume build`: parse, typeset, check the layout, and write one PDF per input file.
//! ABOUTME: Each file gets its own summary line; a failure in one file never hides or skips the others.

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use snafu::ResultExt;

use super::emit::Reporter;
use super::report::Built;
use super::{BuildLine, Env, Io, Outcome};
use crate::cli::{BuildArgs, Cli};
use crate::diagnostics::{Diagnostic, DiagnosticCode, Sources, Summary};
use crate::error::{Error, ReadInputSnafu};
use crate::layout::LayoutReport;
use crate::markdown::{Parsed, parse};
use crate::model::options::OutputFormat;
use crate::output::{Overwrite, Planned, WriteMode, Writer, Written};
use crate::style::{self, StyleSource};
use crate::typeset::fonts::FontSet;
use crate::typeset::{RenderOptions, Typesetter};

/// Everything the files of one `build` share: the style, the fonts, and the way to write.
struct Job<'a> {
    args: &'a BuildArgs,
    format: OutputFormat,
    cwd: &'a Path,
    style: StyleSource,
    fonts: Arc<FontSet>,
    typesetter: Typesetter,
    writer: Writer,
}

/// Builds every file in `args`, in the order given, and ends with the worst outcome of them.
pub(super) fn run(cli: &Cli, args: &BuildArgs, env: &Env, io: &mut Io<'_>) -> Outcome {
    let mut out = Reporter::new(cli.format, &env.cwd, io);
    let job = match Job::prepare(cli, args, env) {
        Ok(job) => job,
        Err(error) => {
            out.error(&error);
            return Outcome::Failure;
        }
    };
    args.files
        .iter()
        .map(|file| job.build_one(file, &mut out))
        .max()
        .unwrap_or(Outcome::Success)
}

impl<'a> Job<'a> {
    /// Resolves the style and builds the font set once, so an unknown style or an unreadable font directory
    /// fails the whole run before any file is touched.
    fn prepare(cli: &Cli, args: &'a BuildArgs, env: &'a Env) -> Result<Self, Error> {
        let render = &args.render;
        let style = style::resolve(&render.style, render.styles_dir.as_deref())?;
        let builder = render
            .font_path
            .iter()
            .fold(FontSet::builder().mode(render.fonts), |builder, dir| {
                builder.font_path(dir)
            })
            .env_paths(env.resume_font_path.as_deref());
        let fonts = Arc::new(builder.build()?);
        Ok(Self {
            args,
            format: cli.format,
            cwd: &env.cwd,
            style,
            typesetter: Typesetter::new(Arc::clone(&fonts)),
            fonts,
            writer: Writer::new(if cli.dry_run {
                WriteMode::DryRun
            } else {
                WriteMode::Write
            }),
        })
    }

    fn build_one(&self, file: &Path, out: &mut Reporter<'_, '_>) -> Outcome {
        match self.attempt(file, Instant::now(), out) {
            Ok(outcome) => outcome,
            Err(error) => {
                out.error(&error);
                Outcome::Failure
            }
        }
    }

    fn attempt(&self, file: &Path, started: Instant, out: &mut Reporter<'_, '_>) -> Result<Outcome, Error> {
        let text = fs::read_to_string(file).context(ReadInputSnafu { path: file })?;
        let parsed = parse(file, &text);
        let mut sources = out.sources();
        sources.insert(file, &text);
        let Some(resume) = parsed.valid() else {
            return Ok(self.without_pdf(file, &parsed, &sources, started, out));
        };

        let compiled = self
            .typesetter
            .compile(resume, &self.style, RenderOptions::new(self.args.render.paper))?;
        let layout = LayoutReport::of(&compiled, &self.fonts, self.args.render.max_pages)?;
        let pdf = compiled.export_pdf(&BuildArgs::pdf_stem(file))?;
        let target = self.args.out.join(BuildArgs::pdf_name(file));
        let written = self.writer.commit(Planned {
            target: &target,
            bytes: &pdf,
            overwrite: Overwrite::Replace,
        })?;

        for style_file in compiled.sources() {
            sources.insert(&style_file.path, &style_file.text);
        }
        let findings: Vec<&Diagnostic> = parsed
            .diagnostics()
            .iter()
            .chain(compiled.warnings())
            .chain(&layout.diagnostics)
            .collect();
        let path = self.shown(&target);
        let style = self.style.name().as_ref() as &str;
        let pages = compiled.page_count();
        let built = match written {
            Written::Wrote { .. } => Built::Wrote { path, style, pages },
            Written::WouldWrite {
                replaces,
                ref creates_dir,
            } => Built::WouldWrite {
                path,
                style,
                pages,
                replaces,
                creates_dir: creates_dir.as_deref().map(|dir| self.shown(dir)),
            },
        };
        Ok(self.present(built, started, &findings, &layout.fonts, &sources, out))
    }

    /// The input has errors, so there is no model to typeset: say so, and show the findings.
    fn without_pdf(
        &self,
        file: &Path,
        parsed: &Parsed,
        sources: &Sources<'_>,
        started: Instant,
        out: &mut Reporter<'_, '_>,
    ) -> Outcome {
        let findings: Vec<&Diagnostic> = parsed.diagnostics().iter().collect();
        let built = Built::NoPdf {
            input: self.shown(file),
        };
        self.present(built, started, &findings, &[], sources, out)
    }

    /// Prints the summary line, the fonts line, and the findings, and decides the outcome from all findings,
    /// including any left out of the human output.
    fn present(
        &self,
        built: Built<'_>,
        started: Instant,
        findings: &[&Diagnostic],
        fonts: &[crate::typeset::fonts::FontResolution],
        sources: &Sources<'_>,
        out: &mut Reporter<'_, '_>,
    ) -> Outcome {
        // A fallback with the same widths is already named on the fonts line, so a person is not shown a note
        // for it; JSON keeps every finding.
        let shown: Vec<&Diagnostic> = findings
            .iter()
            .copied()
            .filter(|d| self.format == OutputFormat::Json || d.code != DiagnosticCode::FontFallbackMetric)
            .collect();
        out.summary(&BuildLine {
            built,
            summary: Summary::of(shown.iter().copied()),
            elapsed: started.elapsed(),
        });
        out.fonts(fonts);
        out.diagnostics(sources, &shown);
        if Summary::of(findings.iter().copied()).has_errors() {
            Outcome::Failure
        } else {
            Outcome::Success
        }
    }

    /// A path as the user typed it, or relative to the current directory when it is under it.
    fn shown<'p>(&self, path: &'p Path) -> &'p Path {
        path.strip_prefix(self.cwd).unwrap_or(path)
    }
}
