//! ABOUTME: `resume build`: parse, typeset, check the layout, and write one PDF per input file.
//! ABOUTME: Each file gets its own summary line; a failure in one file never hides or skips the others.

use std::path::{Path, PathBuf};
use std::time::Instant;

use snafu::{IntoError as _, ResultExt as _};

use super::build_plan::{self, PdfStem, SpecPlan};
use super::emit::Reporter;
use super::report::Built;
use super::spec_report::SpecFacts;
use super::{BuildLine, Env, Io, Outcome};
use crate::cli::{BuildArgs, Cli, RenderArgs, SelectArgs};
use crate::diagnostics::{Diagnostic, DiagnosticCode, Sources, Summary};
use crate::error::{Error, SpecContentSnafu, SpecSnafu, TracedToChoiceSnafu};
use crate::markdown::Parsed;
use crate::model::options::OutputFormat;
use crate::output::{Overwrite, Planned, WriteMode, Writer, Written};
use crate::pipeline::{Assessment, NameOrigins, RenderSettings, Rendered, Selected, Session, Skip};
use crate::select::{CorePages, Selection, SelectionRecord, SelectionSummary};
use crate::spec::{ChoiceKey, Located, ResolvedSpec};

/// The result being presented, including an operational failure after findings were computed.
struct BuildResult<'a> {
    built: Built<'a>,
    error: Option<&'a Error>,
    /// What the selection did, printed under the build line; `None` when there was nothing to select from.
    selection: Option<Shown<'a>>,
}

/// A selection summary paired with the origins of its prepared input nodes.
struct Shown<'a> {
    summary: &'a SelectionSummary,
    origins: &'a [crate::diagnostics::ItemOrigin],
    /// The input, as shown to people.
    input: &'a Path,
}

impl<'a> BuildResult<'a> {
    fn failed(input: &'a Path, error: &'a Error) -> Self {
        Self {
            built: Built::NoPdf { input },
            error: Some(error),
            selection: None,
        }
    }

    fn with_selection(mut self, selection: Option<Shown<'a>>) -> Self {
        self.selection = selection;
        self
    }
}

/// Everything the files of one `build` share: the style, the fonts, and the way to write.
///
/// `watch` builds with the same job again and again, so what one file's build came to is returned, not only
/// printed.
pub(super) struct Job<'a> {
    out_dir: &'a Path,
    render: RenderSettings,
    selection: Selection,
    format: OutputFormat,
    cwd: &'a Path,
    session: Session,
    writer: Writer,
    /// Set for a build from a spec: what to report about it, and what it chose.
    spec: Option<SpecRun>,
}

/// What a build from a spec knows beyond what a build of a file does: what to report about the spec, the resolved
/// spec to check the content against, and where each name was written.
struct SpecRun {
    facts: SpecFacts,
    resolved: ResolvedSpec,
    names: NameOrigins,
    /// Where the spec chose the style being built with; `None` when a flag or the default chose it.
    style_at: Option<Located>,
}

impl SpecRun {
    /// A failure to read the content, naming the `content =` line that named the file.
    fn read_error(&self, source: Error) -> Error {
        SpecContentSnafu {
            at: self.facts.content_at().clone(),
        }
        .into_error(source)
    }

    /// A style the spec chose that cannot be found, naming the line that chose it; any other error as it was.
    ///
    /// A style a flag chose is not traced to the spec, and neither are problems with the styles directory, which
    /// the spec does not name.
    fn style_error(&self, error: Error) -> Error {
        match &self.style_at {
            Some(at) if matches!(error, Error::UnknownStyle { .. } | Error::StyleMissingMain { .. }) => {
                TracedToChoiceSnafu { at: vec![at.clone()] }.into_error(error)
            }
            _ => error,
        }
    }

    /// A pick that names nothing in the content is a mistake in the file that wrote it.
    fn check_picks(&self, selected: Option<&Selected>) -> Result<(), Error> {
        let Some(selected) = selected else { return Ok(()) };
        self.resolved
            .check_picks(&selected.report.unknown_picks)
            .context(SpecSnafu)
    }

    /// For a draft wording a spec picked, the same error naming the lines that picked it; `None` for any other.
    fn attribute(&self, error: &Error) -> Option<Error> {
        let Error::Select { source } = error else {
            return None;
        };
        use crate::select::SelectError;
        let keys: Vec<_> = match source {
            SelectError::PickedDraft { picks } => picks.iter().map(|p| ChoiceKey::Pick(p.group.clone())).collect(),
            SelectError::UnknownGroups { groups } => groups.iter().cloned().map(ChoiceKey::Group).collect(),
            SelectError::UnavailableGroups { groups } => {
                groups.iter().map(|g| ChoiceKey::Group(g.group.clone())).collect()
            }
        };
        let at: Vec<Located> = keys
            .iter()
            .filter_map(|key| self.resolved.origin_of(key))
            .cloned()
            .collect();
        (!at.is_empty()).then(|| TracedToChoiceSnafu { at }.into_error(Error::Select { source: source.clone() }))
    }
}

/// What one build reads and what it names its PDF.
pub(super) struct Input<'p> {
    /// The resume to read.
    read: &'p Path,
    /// The resume as findings and errors name it: the same file, written the way a person would.
    shown: PathBuf,
    /// The PDF's name and identity.
    stem: PdfStem,
}

impl<'p> Input<'p> {
    /// A resume built as itself: the PDF is named after the file.
    pub(super) fn plain(file: &'p Path) -> Self {
        Self {
            read: file,
            shown: file.to_path_buf(),
            stem: PdfStem::of_input(file),
        }
    }

    /// The resume a spec names, read from `read` and shown as a person would write it: without the `..` that led
    /// there, and relative to `cwd` when it is under it. The shown name is never opened.
    fn of_spec(read: &'p Path, stem: PdfStem, cwd: &Path) -> Self {
        Self {
            read,
            shown: super::spec_report::shown_path(cwd, read),
            stem,
        }
    }
}

/// What building one file came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BuiltFile {
    /// How the file's build ended, by its findings.
    pub outcome: Outcome,
    /// The PDF that was written; `None` when none was, including in a dry run. A layout error still writes
    /// the PDF, so this can be `Some` while `outcome` is `Failure`.
    pub wrote: Option<PathBuf>,
}

/// Builds every file in `args`, in the order given, and ends with the worst outcome of them.
pub(super) fn run(cli: &Cli, args: &BuildArgs, env: &Env, io: &mut Io<'_>) -> Outcome {
    let mut out = Reporter::new(cli.format, &env.cwd, io);
    if let Some(spec) = &args.spec.spec {
        return run_spec(cli, args, spec, env, &mut out);
    }
    let job = match Job::prepare(cli, args, env) {
        Ok(job) => job,
        Err(error) => {
            out.error(&error);
            return Outcome::Failure;
        }
    };
    args.files
        .iter()
        .map(|file| job.build_one(file, &mut out).outcome)
        .max()
        .unwrap_or(Outcome::Success)
}

/// Builds from the spec at `spec_path`: the resume it names, with the choices it and its profiles make.
fn run_spec(cli: &Cli, args: &BuildArgs, spec_path: &Path, env: &Env, out: &mut Reporter<'_, '_>) -> Outcome {
    let resolved = match build_plan::resolve(args, spec_path) {
        Ok(resolved) => resolved,
        Err(error) => {
            out.error(&error);
            return Outcome::Failure;
        }
    };
    if args.spec.print_spec {
        out.spec_warnings(&resolved, true);
        out.product(&resolved.to_toml());
        return Outcome::Success;
    }
    let plan = match build_plan::plan(args, spec_path, resolved, env) {
        Ok(plan) => plan,
        Err(error) => {
            out.error(&error);
            return Outcome::Failure;
        }
    };
    let SpecPlan {
        content,
        stem,
        selection,
        render,
        facts,
        names,
        resolved,
        style_at,
    } = plan;
    let run = SpecRun {
        facts,
        resolved,
        names,
        style_at,
    };
    let job = match Job::open_with(cli, &args.out, render, selection, env) {
        Ok(job) => job.with_spec(run),
        Err(error) => {
            out.error(&run.style_error(error));
            return Outcome::Failure;
        }
    };
    job.build_input(&Input::of_spec(&content, stem, &env.cwd), out).outcome
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

impl BuiltFile {
    fn unwritten(outcome: Outcome) -> Self {
        Self { outcome, wrote: None }
    }
}

impl<'a> Job<'a> {
    /// Resolves the style and builds the font set once, so an unknown style or an unreadable font directory
    /// fails the whole run before any file is touched.
    fn prepare(cli: &Cli, args: &'a BuildArgs, env: &'a Env) -> Result<Self, Error> {
        Self::open(cli, &args.out, &args.render, &args.select, env)
    }

    /// The job for building into `out_dir` with `render` and `select`, as `build` and `watch` both do.
    pub(super) fn open(
        cli: &Cli,
        out_dir: &'a Path,
        render: &RenderArgs,
        select: &SelectArgs,
        env: &'a Env,
    ) -> Result<Self, Error> {
        Self::open_with(cli, out_dir, RenderSettings::from(render), select.selection(), env)
    }

    /// [`Self::open`] for a selection that does not come from `SelectArgs`, such as a spec's.
    fn open_with(
        cli: &Cli,
        out_dir: &'a Path,
        render: RenderSettings,
        selection: Selection,
        env: &'a Env,
    ) -> Result<Self, Error> {
        // `build` can do nothing without a style or fonts, unlike `doctor`, which reports them.
        let session = Session::open(&render, env.resume_font_path.as_deref()).require_ready()?;
        Ok(Self {
            out_dir,
            render,
            selection,
            format: cli.format,
            cwd: &env.cwd,
            session,
            writer: Writer::new(if cli.dry_run {
                WriteMode::DryRun
            } else {
                WriteMode::Write
            }),
            spec: None,
        })
    }

    /// This job builds from a spec.
    fn with_spec(mut self, spec: SpecRun) -> Self {
        self.spec = Some(spec);
        self
    }

    pub(super) fn build_one(&self, file: &Path, out: &mut Reporter<'_, '_>) -> BuiltFile {
        self.build_input(&Input::plain(file), out)
    }

    /// Builds one resume: reads `input.read` and names the PDF after `input.stem`.
    fn build_input(&self, input: &Input<'_>, out: &mut Reporter<'_, '_>) -> BuiltFile {
        let started = Instant::now();
        match self.attempt(input, started, out) {
            Ok(built) => built,
            Err(error) => BuiltFile::unwritten(self.present(
                BuildResult::failed(self.shown(&input.shown), &error),
                started,
                &[],
                &[],
                &out.sources(),
                out,
            )),
        }
    }

    fn attempt(&self, input: &Input<'_>, started: Instant, out: &mut Reporter<'_, '_>) -> Result<BuiltFile, Error> {
        let file = input.shown.as_path();
        let text = self.read_content(input)?;
        let no_names = NameOrigins::default();
        let names = self.spec.as_ref().map_or(&no_names, |spec| &spec.names);
        let overrides = self.spec.as_ref().map(|spec| spec.resolved.item_overrides());
        let additions = self
            .spec
            .as_ref()
            .map_or(&[][..], |spec| spec.resolved.additions.as_slice());
        let assessment = self.session.assess_edits(
            file,
            &text,
            &self.render,
            &self.selection,
            names,
            crate::pipeline::ResumeEdits { overrides, additions },
        );
        let mut sources = out.sources();
        sources.insert(file, &text);
        if let Some(spec) = &self.spec {
            for snapshot in spec.resolved.sources() {
                sources
                    .insert_checked(snapshot.path(), snapshot.text())
                    .context(crate::error::ReportSourceSnafu)?;
            }
        }
        let parsed = &assessment.parsed;
        let (summary, core_error) = self.summarize(&assessment);
        // What every outcome shows: the parse's findings, then the selection's own warnings.
        let early: Vec<&Diagnostic> = parsed
            .diagnostics()
            .iter()
            .chain(self.spec.iter().flat_map(|spec| spec.resolved.content_warnings()))
            .chain(assessment.selected.iter().flat_map(|s| &s.findings))
            .collect();
        let shown = || {
            summary.as_ref().map(|summary| Shown {
                summary,
                origins: &assessment
                    .selected
                    .as_ref()
                    .expect("summary requires a selection")
                    .origins,
                input: file,
            })
        };
        // A pick that names nothing is a mistake in the file that wrote it: no PDF is made, and the findings found so
        // far are still shown.
        if let Some(spec) = &self.spec
            && let Err(error) = spec.check_picks(assessment.selected.as_ref())
        {
            return Ok(BuiltFile::unwritten(self.present(
                BuildResult::failed(self.shown(file), &error).with_selection(shown()),
                started,
                &early,
                &[],
                &sources,
                out,
            )));
        }
        let (compiled, layout) = match &assessment.rendered {
            Rendered::Skipped(Skip::FormatErrors) => {
                return Ok(BuiltFile::unwritten(
                    self.without_pdf(file, parsed, &sources, started, out),
                ));
            }
            // `prepare` required a ready session, so a setup failure cannot get here.
            Rendered::Skipped(Skip::Setup) => unreachable!("build requires a ready session before any file"),
            Rendered::Failed(error) => {
                let findings = early.clone();
                let attributed = self.spec.as_ref().and_then(|spec| spec.attribute(error));
                return Ok(BuiltFile::unwritten(self.present(
                    BuildResult::failed(self.shown(file), attributed.as_ref().unwrap_or(error)).with_selection(shown()),
                    started,
                    &findings,
                    &[],
                    &sources,
                    out,
                )));
            }
            Rendered::LayoutFailed { compiled, error } => {
                for style_file in compiled.sources() {
                    sources
                        .insert_checked(&style_file.path, &style_file.text)
                        .context(crate::error::ReportSourceSnafu)?;
                }
                let findings: Vec<&Diagnostic> = early.iter().copied().chain(compiled.warnings()).collect();
                return Ok(BuiltFile::unwritten(self.present(
                    BuildResult::failed(self.shown(file), error).with_selection(shown()),
                    started,
                    &findings,
                    &[],
                    &sources,
                    out,
                )));
            }
            Rendered::Laid { compiled, layout } => (compiled, layout),
        };
        for style_file in compiled.sources() {
            sources
                .insert_checked(&style_file.path, &style_file.text)
                .context(crate::error::ReportSourceSnafu)?;
        }
        let mut findings: Vec<&Diagnostic> = early.clone();
        findings.extend(compiled.warnings());
        findings.extend(&layout.diagnostics);
        let target = self.out_dir.join(input.stem.file_name());
        let protected = crate::output::protect_inputs(
            &target,
            std::iter::once(input.read).chain(
                self.spec
                    .iter()
                    .flat_map(|s| s.resolved.sources().iter().map(|s| s.path())),
            ),
        );
        // Delivery can fail after diagnostics exist. Present both rather than discarding the findings.
        let written = protected
            .and_then(|()| compiled.export_pdf(input.stem.as_str()))
            .and_then(|pdf| {
                self.writer.commit(Planned {
                    target: &target,
                    bytes: &pdf,
                    overwrite: Overwrite::Replace,
                })
            });
        let written = match written {
            Ok(written) => written,
            Err(error) => {
                return Ok(BuiltFile::unwritten(self.present(
                    BuildResult::failed(self.shown(file), &error).with_selection(shown()),
                    started,
                    &findings,
                    &layout.fonts,
                    &sources,
                    out,
                )));
            }
        };
        let built = self.built_line(&written, &target, compiled.page_count());
        let outcome = self.present(
            BuildResult {
                built,
                error: core_error.as_ref(),
                selection: shown(),
            },
            started,
            &findings,
            &layout.fonts,
            &sources,
            out,
        );
        Ok(BuiltFile {
            outcome,
            wrote: (self.writer.mode() == WriteMode::Write).then_some(target),
        })
    }

    /// The selection's summary, with the core-only page count when the summary has something to say.
    ///
    /// The count is a second compile; its error is returned apart, to fail the run after the main PDF is written.
    fn summarize(&self, assessment: &Assessment) -> (Option<SelectionSummary>, Option<Error>) {
        let mut summary = assessment.selected.as_ref().map(|s| s.summary.clone());
        let mut core_error = None;
        // The prepared copy, not the parse: the core-only count must see the same effective priorities the build did.
        if let (Some(summary), Some(prepared)) = (summary.as_mut(), assessment.prepared.as_ref())
            && !summary.is_silent()
        {
            summary.core = Some(
                match self
                    .session
                    .core_pages(prepared.resume(), &self.selection, &self.render)
                {
                    Ok(core) => core,
                    Err(error) => {
                        core_error = Some(error);
                        CorePages::Failed
                    }
                },
            );
        }
        (summary, core_error)
    }

    /// What the build line says about a PDF that was written, or would be in a dry run.
    fn built_line<'t>(&'t self, written: &'t Written, target: &'t Path, pages: usize) -> Built<'t> {
        let (path, style) = (self.shown(target), self.style_name());
        match written {
            Written::Wrote { .. } => Built::Wrote { path, style, pages },
            Written::WouldWrite { replaces, creates_dir } => Built::WouldWrite {
                replaces: *replaces,
                path,
                style,
                pages,
                creates_dir: creates_dir.as_deref().map(|dir| self.shown(dir)),
            },
        }
    }

    /// Reads the resume, naming the spec's `content =` line when the spec is why this file is read.
    fn read_content(&self, input: &Input<'_>) -> Result<String, Error> {
        crate::input::read_text(input.read, &input.shown).map_err(|source| match &self.spec {
            Some(spec) => spec.read_error(source),
            None => source,
        })
    }

    /// Resolves the style again before a build, for `watch` with a styles directory: a style made after the
    /// watch started, or one removed, is seen. The fonts are kept, and so is the previous style when this fails.
    ///
    /// # Errors
    /// The style can no longer be resolved, as it could at the start.
    pub(super) fn refresh_style(&mut self) -> Result<(), Error> {
        self.session.resolve_style_again(&self.render)
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
            BuildResult {
                built,
                error: None,
                selection: None,
            },
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
        self.spec_summary(&result.built, out);
        out.fonts(fonts);
        if let Some(selection) = &result.selection {
            self.selection_summary(selection, sources, out);
        }
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

    /// The spec's lines under the build line, or its JSON record on stdout; nothing for a build of a file.
    ///
    /// Printed before any finding, so what was built from which spec and which file is the first thing said.
    fn spec_summary(&self, built: &Built<'_>, out: &mut Reporter<'_, '_>) {
        let Some(spec) = &self.spec else { return };
        match self.format {
            OutputFormat::Human => {
                for line in spec.facts.lines() {
                    out.line(&format!("  {line}"));
                }
            }
            OutputFormat::Json => {
                let pdf = match built {
                    Built::Wrote { path, .. } | Built::WouldWrite { path, .. } => Some(*path),
                    Built::NoPdf { .. } => None,
                };
                out.record(&spec.facts.record(pdf));
            }
        }
    }

    /// The selection's lines under the build line, or its JSON record on stdout; nothing when it is silent.
    fn selection_summary(&self, selection: &Shown<'_>, sources: &Sources<'_>, out: &mut Reporter<'_, '_>) {
        let location_of = |index| Some(report_location(selection.origins, selection.input, sources, index));
        match self.format {
            OutputFormat::Human => {
                for line in selection.summary.lines(&location_of) {
                    out.line(&format!("  {line}"));
                }
            }
            OutputFormat::Json if !selection.summary.is_silent() => {
                let input = self.shown(selection.input).display().to_string();
                out.record(&SelectionRecord::new(selection.summary, &input, &location_of));
            }
            OutputFormat::Json => {}
        }
    }

    /// A path as the user typed it, or relative to the current directory when it is under it.
    fn shown<'p>(&self, path: &'p Path) -> &'p Path {
        path.strip_prefix(self.cwd).unwrap_or(path)
    }
}

fn report_location(
    origins: &[crate::diagnostics::ItemOrigin],
    input: &Path,
    sources: &Sources<'_>,
    index: usize,
) -> crate::select::ReportLocation {
    let span = origins
        .get(index)
        .expect("report indexes prepared input origins")
        .span();
    crate::select::ReportLocation {
        same_input: span.file == input,
        position: sources
            .position(&span)
            .expect("prepared input origins have registered sources"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{Edit, Severity};
    use pretty_assertions::assert_eq;

    #[test]
    fn report_locations_use_source_identity_and_registered_snapshot_coordinates() {
        use crate::diagnostics::{ItemOrigin, SourceKind, SourceText};
        use claims::assert_some;
        let base = std::env::temp_dir().join("resume-origin-test");
        let input = base.join("resume.md");
        let spec = SourceText::new(base.join("position.toml"), SourceKind::Spec, "# é\ncontent = 'words'\n");
        let origins = [
            ItemOrigin::Baseline {
                file: input.clone(),
                range: 2..3,
            },
            assert_some!(ItemOrigin::spec(&spec, 15..22)),
        ];
        let mut sources = Sources::new(Some(base));
        sources.insert(&input, "# A");
        sources.insert_text(&spec);
        let baseline = report_location(&origins, &input, &sources, 0);
        assert!(baseline.same_input);
        assert_eq!(
            (
                baseline.position.file.as_str(),
                baseline.position.line,
                baseline.position.column
            ),
            ("resume.md", 1, 3)
        );
        let added = report_location(&origins, &input, &sources, 1);
        assert!(!added.same_input);
        assert_eq!(
            (added.position.file.as_str(), added.position.line, added.position.column),
            ("position.toml", 2, 11)
        );
    }

    #[test]
    #[should_panic(expected = "prepared input origins have registered sources")]
    fn missing_report_source_is_not_silently_replaced_with_a_baseline_line() {
        let origins = [crate::diagnostics::ItemOrigin::SpecContent {
            file: "missing.toml".into(),
            content: 0..1,
        }];
        report_location(&origins, Path::new("resume.md"), &Sources::new(None), 0);
    }

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
