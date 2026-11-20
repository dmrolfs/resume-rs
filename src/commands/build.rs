//! ABOUTME: `resume build`: parse, typeset, check the layout, and write one PDF per input file.
//! ABOUTME: Each file gets its own summary line; a failure in one file never hides or skips the others.

use std::fs;
use std::path::Path;
use std::time::Instant;

use snafu::ResultExt;

use super::emit::Reporter;
use super::report::Built;
use super::{BuildLine, Env, Io, Outcome};
use crate::cli::{BuildArgs, Cli};
use crate::diagnostics::{Diagnostic, DiagnosticCode, Sources, Summary};
use crate::error::{Error, ReadInputSnafu};
use crate::markdown::Parsed;
use crate::model::options::OutputFormat;
use crate::output::{Overwrite, Planned, WriteMode, Writer, Written};
use crate::pipeline::{Rendered, Session, Skip};

/// The result being presented, including an operational failure after findings were computed.
struct BuildResult<'a> {
    built: Built<'a>,
    error: Option<&'a Error>,
}

impl<'a> BuildResult<'a> {
    fn failed(input: &'a Path, error: &'a Error) -> Self {
        Self {
            built: Built::NoPdf { input },
            error: Some(error),
        }
    }
}

/// Everything the files of one `build` share: the style, the fonts, and the way to write.
struct Job<'a> {
    args: &'a BuildArgs,
    format: OutputFormat,
    cwd: &'a Path,
    session: Session,
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

fn summary_with_error(findings: &[&Diagnostic], error: Option<&Error>) -> Summary {
    match error {
        Some(Error::Compile { diagnostics, .. } | Error::ExportPdf { diagnostics, .. }) => {
            Summary::of(findings.iter().copied().chain(diagnostics))
        }
        _ => {
            let mut summary = Summary::of(findings.iter().copied());
            summary.errors += usize::from(error.is_some());
            summary
        }
    }
}

impl<'a> Job<'a> {
    /// Resolves the style and builds the font set once, so an unknown style or an unreadable font directory
    /// fails the whole run before any file is touched.
    fn prepare(cli: &Cli, args: &'a BuildArgs, env: &'a Env) -> Result<Self, Error> {
        // `build` can do nothing without a style or fonts, unlike `doctor`, which reports them.
        let session = Session::open(&args.render, env.resume_font_path.as_deref()).require_ready()?;
        Ok(Self {
            args,
            format: cli.format,
            cwd: &env.cwd,
            session,
            writer: Writer::new(if cli.dry_run {
                WriteMode::DryRun
            } else {
                WriteMode::Write
            }),
        })
    }

    fn build_one(&self, file: &Path, out: &mut Reporter<'_, '_>) -> Outcome {
        let started = Instant::now();
        match self.attempt(file, started, out) {
            Ok(outcome) => outcome,
            Err(error) => self.present(
                BuildResult::failed(self.shown(file), &error),
                started,
                &[],
                &[],
                &out.sources(),
                out,
            ),
        }
    }

    fn attempt(&self, file: &Path, started: Instant, out: &mut Reporter<'_, '_>) -> Result<Outcome, Error> {
        let text = fs::read_to_string(file).context(ReadInputSnafu { path: file })?;
        let assessment = self.session.assess(file, &text, &self.args.render);
        let mut sources = out.sources();
        sources.insert(file, &text);
        let parsed = &assessment.parsed;
        let (compiled, layout) = match &assessment.rendered {
            Rendered::Skipped(Skip::FormatErrors) => return Ok(self.without_pdf(file, parsed, &sources, started, out)),
            // `prepare` required a ready session, so a setup failure cannot get here.
            Rendered::Skipped(Skip::Setup) => unreachable!("build requires a ready session before any file"),
            Rendered::Failed(error) => {
                let findings: Vec<&Diagnostic> = parsed.diagnostics().iter().collect();
                return Ok(self.present(
                    BuildResult::failed(self.shown(file), error),
                    started,
                    &findings,
                    &[],
                    &sources,
                    out,
                ));
            }
            Rendered::LayoutFailed { compiled, error } => {
                for style_file in compiled.sources() {
                    sources.insert(&style_file.path, &style_file.text);
                }
                let findings: Vec<&Diagnostic> = parsed.diagnostics().iter().chain(compiled.warnings()).collect();
                return Ok(self.present(
                    BuildResult::failed(self.shown(file), error),
                    started,
                    &findings,
                    &[],
                    &sources,
                    out,
                ));
            }
            Rendered::Laid { compiled, layout } => (compiled, layout),
        };
        for style_file in compiled.sources() {
            sources.insert(&style_file.path, &style_file.text);
        }
        let mut findings: Vec<&Diagnostic> = parsed.diagnostics().iter().chain(compiled.warnings()).collect();
        findings.extend(&layout.diagnostics);
        let target = self.args.out.join(BuildArgs::pdf_name(file));
        // Delivery can fail after diagnostics exist. Present both rather than discarding the findings.
        let written = compiled.export_pdf(&BuildArgs::pdf_stem(file)).and_then(|pdf| {
            self.writer.commit(Planned {
                target: &target,
                bytes: &pdf,
                overwrite: Overwrite::Replace,
            })
        });
        let written = match written {
            Ok(written) => written,
            Err(error) => {
                return Ok(self.present(
                    BuildResult::failed(self.shown(file), &error),
                    started,
                    &findings,
                    &layout.fonts,
                    &sources,
                    out,
                ));
            }
        };
        let path = self.shown(&target);
        let style = self.style_name();
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
        Ok(self.present(
            BuildResult { built, error: None },
            started,
            &findings,
            &layout.fonts,
            &sources,
            out,
        ))
    }

    /// The name of the style in use; the session was checked ready before any file was built.
    fn style_name(&self) -> &str {
        self.session.style().map_or("", |style| style.name().as_ref())
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
        self.present(
            BuildResult { built, error: None },
            started,
            &findings,
            &[],
            sources,
            out,
        )
    }

    /// Prints the summary line, the fonts line, and the findings, and decides the outcome from all findings,
    /// including any left out of the human output.
    fn present(
        &self,
        result: BuildResult<'_>,
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
        let summary = summary_with_error(&shown, result.error);
        out.summary(&BuildLine {
            built: result.built,
            summary,
            elapsed: started.elapsed(),
        });
        out.fonts(fonts);
        out.diagnostics(sources, &shown);
        if let Some(error) = result.error {
            out.error(error);
        }
        if result.error.is_some() || Summary::of(findings.iter().copied()).has_errors() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{Edit, Severity};
    use pretty_assertions::assert_eq;

    #[test]
    fn operational_failure_adds_one_error_without_changing_other_counts() {
        let warning = Diagnostic {
            severity: Severity::Warning,
            code: DiagnosticCode::SparseLastPage,
            message: "sparse".to_string(),
            span: None,
            hints: Vec::new(),
            fix: None,
        };
        let error = Error::ReadInput {
            path: "missing.md".into(),
            source: std::io::ErrorKind::NotFound.into(),
        };
        assert_eq!(
            summary_with_error(&[&warning], Some(&error)),
            Summary {
                errors: 1,
                warnings: 1,
                notes: 0,
                fixable: 0
            }
        );
        assert_eq!(
            summary_with_error(&[&warning], None),
            Summary {
                errors: 0,
                warnings: 1,
                notes: 0,
                fixable: 0
            }
        );
    }

    #[test]
    fn compiler_and_export_findings_contribute_every_summary_count() {
        let diagnostic = |severity| Diagnostic {
            severity,
            code: DiagnosticCode::TypstError,
            message: "finding".to_string(),
            span: None,
            hints: Vec::new(),
            fix: Some(Edit {
                range: 0..0,
                replacement: String::new(),
            }),
        };
        let prior = diagnostic(Severity::Warning);
        let diagnostics = vec![
            diagnostic(Severity::Error),
            diagnostic(Severity::Warning),
            diagnostic(Severity::Note),
        ];
        for error in [
            Error::Compile {
                diagnostics: diagnostics.clone(),
                sources: Vec::new(),
            },
            Error::ExportPdf {
                diagnostics,
                sources: Vec::new(),
            },
        ] {
            assert_eq!(
                summary_with_error(&[&prior], Some(&error)),
                Summary {
                    errors: 1,
                    warnings: 2,
                    notes: 1,
                    fixable: 4
                }
            );
        }
    }
}
