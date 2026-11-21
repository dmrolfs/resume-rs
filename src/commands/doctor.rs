//! ABOUTME: `resume doctor` and `resume check`: one report per file, grouped by category, with an optional fix.
//! ABOUTME: `check` is the same runner with no fix step, so it cannot write; `--explain` prints one code's advice.

use std::fs;
use std::path::Path;

use snafu::ResultExt;

use super::emit::Reporter;
use super::{Env, Io, Outcome};
use crate::cli::{AssessArgs, BuildArgs, Cli, DoctorArgs};
use crate::diagnostics::{Diagnostic, DiagnosticCode, Sources, Summary};
use crate::doctor::fix::{self, Fixed};
use crate::doctor::report::{Facts, Group, group, role_map};
use crate::doctor::{content, setup};
use crate::error::{Error, ReadInputSnafu};
use crate::model::options::OutputFormat;
use crate::output::{Overwrite, Planned, WriteMode, Writer};
use crate::pipeline::{Assessment, Rendered, Session, Skip};
use crate::select::Selection;
use crate::typeset::fonts::FontResolution;

use super::report::FontsLine;

/// Runs `resume doctor`.
pub(super) fn run(cli: &Cli, args: &DoctorArgs, env: &Env, io: &mut Io<'_>) -> Outcome {
    if let Some(code) = args.explain {
        return explain(cli, code, env, io);
    }
    Run::new(cli, &args.assess, args.fix, env).files(&args.files, io)
}

/// Runs `resume check`: the report of `doctor`, with no fix step to reach.
pub(super) fn run_check(cli: &Cli, args: &crate::cli::CheckArgs, env: &Env, io: &mut Io<'_>) -> Outcome {
    Run::new(cli, &args.assess, false, env).files(&args.files, io)
}

// ---------------------------------------------------------------------------------------------
// --explain
// ---------------------------------------------------------------------------------------------

/// Prints the advice entry of `code` to stdout, like `rustc --explain`.
fn explain(cli: &Cli, code: DiagnosticCode, env: &Env, io: &mut Io<'_>) -> Outcome {
    let mut out = Reporter::new(cli.format, &env.cwd, io);
    out.product(&explanation(code));
    Outcome::Success
}

/// The text `--explain` prints for `code`.
fn explanation(code: DiagnosticCode) -> String {
    let advice = code.advice();
    let mut text = format!(
        "{code} ({})\n\n{}\n\nWhy: {}\n\nFix: {}\n",
        code.category(),
        advice.summary,
        advice.why,
        advice.fix
    );
    if let Some((before, after)) = advice.example {
        text.push_str("\nExample:\n");
        before.lines().for_each(|line| text.push_str(&format!("  - {line}\n")));
        after.lines().for_each(|line| text.push_str(&format!("  + {line}\n")));
    }
    text
}

// ---------------------------------------------------------------------------------------------
// The report
// ---------------------------------------------------------------------------------------------

/// What came of the fix of one file.
enum Fix {
    /// The file has nothing `--fix` can change.
    Nothing,
    /// A fix was planned and checked, and written (or in a dry run, checked); here is the fixed text.
    Done(Fixed),
    /// The fix was refused or could not be written, and has been reported. Nothing was changed.
    Refused,
}

/// Whether the report that follows is of a file that was fixed, and how the closing line should say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fixing {
    /// `check`, or `doctor` without `--fix`: the fixes that could be applied are offered.
    Off,
    /// `--fix` found nothing to change.
    NothingToFix,
    /// `--fix` was refused: the same fixes are still available, once the reason is dealt with.
    Refused,
    /// `--fix` applied its fixes; the report is of the fixed text.
    Applied,
}

impl Fixing {
    /// What the closing line says about the fixable findings that are left.
    fn how(self) -> FixableNote {
        match self {
            Self::Off => FixableNote::Offer,
            Self::NothingToFix | Self::Refused | Self::Applied => FixableNote::Left,
        }
    }
}

/// How the closing line speaks of findings that carry a fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FixableNote {
    /// "fixable with `resume doctor --fix`".
    Offer,
    /// "fixable" with no offer, because `--fix` was already given.
    Left,
}

/// What one run shares across its files.
struct Run<'a> {
    format: OutputFormat,
    cwd: &'a Path,
    args: &'a AssessArgs,
    session: Session,
    /// Present only for `doctor --fix`; `check` never has one.
    fixer: Option<Writer>,
}

impl<'a> Run<'a> {
    fn new(cli: &Cli, args: &'a AssessArgs, fix: bool, env: &'a Env) -> Self {
        Self {
            format: cli.format,
            cwd: &env.cwd,
            args,
            session: Session::open(&args.render, env.resume_font_path.as_deref()),
            fixer: fix.then(|| {
                Writer::new(if cli.dry_run {
                    WriteMode::DryRun
                } else {
                    WriteMode::Write
                })
            }),
        }
    }

    /// Reports every file in the order given, and ends with the worst outcome.
    fn files(&self, files: &[std::path::PathBuf], io: &mut Io<'_>) -> Outcome {
        let mut out = Reporter::new(self.format, self.cwd, io);
        files
            .iter()
            .map(|file| self.one(file, &mut out))
            .max()
            .unwrap_or(Outcome::Success)
    }

    fn one(&self, file: &Path, out: &mut Reporter<'_, '_>) -> Outcome {
        let text = match fs::read_to_string(file).context(ReadInputSnafu { path: file }) {
            Ok(text) => text,
            Err(error) => {
                out.error(&error);
                return Outcome::Failure;
            }
        };
        if self.format == OutputFormat::Human {
            out.report_line(&self.shown(file));
        }
        let assessment = self
            .session
            .assess(file, &text, &self.args.render, &Selection::default());
        let Some(fixer) = &self.fixer else {
            return self.report(file, &text, &assessment, Fixing::Off, out);
        };
        match self.fix(fixer, file, &text, &assessment, out) {
            Fix::Nothing => self.report(file, &text, &assessment, Fixing::NothingToFix, out),
            Fix::Refused => {
                // The report of the file as it is; the run fails because the fix was asked for and not made.
                self.report(file, &text, &assessment, Fixing::Refused, out);
                Outcome::Failure
            }
            Fix::Done(fixed) => {
                let after = self
                    .session
                    .assess(file, &fixed.text, &self.args.render, &Selection::default());
                self.report(file, &fixed.text, &after, Fixing::Applied, out)
            }
        }
    }

    /// Plans and writes the fix, and says what happened. Each failure is printed once: a plan the checks
    /// refuse as a `not fixed:` line, a write that fails as the error itself.
    fn fix(
        &self,
        writer: &Writer,
        file: &Path,
        text: &str,
        assessment: &Assessment,
        out: &mut Reporter<'_, '_>,
    ) -> Fix {
        let fixed = match fix::plan(file, text, &assessment.parsed) {
            Ok(Some(fixed)) => fixed,
            Ok(None) => return Fix::Nothing,
            Err(error) => {
                out.report_line(&format!("not fixed: {error}"));
                return Fix::Refused;
            }
        };
        let planned = Planned {
            target: file,
            bytes: fixed.text.as_bytes(),
            overwrite: Overwrite::ReplaceSource {
                expected: text.as_bytes(),
            },
        };
        if let Err(error) = writer.commit(planned) {
            out.error(&error);
            return Fix::Refused;
        }
        let verb = if writer.mode() == WriteMode::DryRun {
            "would fix"
        } else {
            "fixed"
        };
        let plural = if fixed.fixed == 1 { "" } else { "s" };
        out.report_line(&format!(
            "{verb} {} finding{plural} in {}",
            fixed.fixed,
            self.shown(file)
        ));
        out.stderr_text(&fix::unified_diff(&self.shown(file), text, &fixed.text));
        Fix::Done(fixed)
    }

    /// Prints the report of one file and returns its outcome.
    fn report(
        &self,
        file: &Path,
        text: &str,
        assessment: &Assessment,
        fixing: Fixing,
        out: &mut Reporter<'_, '_>,
    ) -> Outcome {
        let mut sources: Sources<'_> = out.sources();
        sources.insert(file, text);
        let mut findings: Vec<Diagnostic> = assessment.parsed.diagnostics().to_vec();
        findings.extend(content::lint_parsed(&assessment.parsed, file));
        let mut operational: Vec<&Error> = Vec::new();
        let mut setup_facts: Vec<String> = Vec::new();
        let mut facts = Facts::default();

        self.add_session_findings(&mut findings, &mut operational, &mut setup_facts);
        match &assessment.rendered {
            Rendered::Laid { compiled, layout } => {
                for style_file in compiled.sources() {
                    sources.insert(&style_file.path, &style_file.text);
                }
                findings.extend(compiled.warnings().iter().cloned());
                findings.extend(layout.diagnostics.iter().cloned());
                facts.roles = role_map(&layout.roles, layout.pages);
                setup_facts.push(fonts_fact(&layout.fonts));
            }
            Rendered::Failed(error) => {
                add_failure(error, &mut sources, &mut findings, &mut operational);
                setup_facts.push(match error {
                    Error::Select { .. } => "fonts: not resolved (the selection failed)".into(),
                    _ => "fonts: not resolved (the style failed)".into(),
                });
            }
            Rendered::LayoutFailed { compiled, error } => {
                for style_file in compiled.sources() {
                    sources.insert(&style_file.path, &style_file.text);
                }
                findings.extend(compiled.warnings().iter().cloned());
                add_failure(error, &mut sources, &mut findings, &mut operational);
                setup_facts.push("fonts: not resolved (the style's metadata is malformed)".into());
            }
            Rendered::Skipped(Skip::FormatErrors) => {
                setup_facts.push("fonts: not resolved (the resume has errors)".into());
            }
            Rendered::Skipped(Skip::Setup) => {}
        }
        let target = self.args.out.join(BuildArgs::pdf_name(file));
        match setup::output(&target, &self.shown(&target)) {
            setup::Output::Usable(fact) => setup_facts.push(fact),
            setup::Output::Unusable(finding) => findings.push(*finding),
        }
        facts.setup = setup_facts;

        self.print(&findings, &facts, &sources, out);
        for error in &operational {
            out.error(error);
        }
        let summary = Summary::of(findings.iter());
        if self.format == OutputFormat::Human {
            let shown_summary = Summary::of(findings.iter().filter(|d| d.code != DiagnosticCode::FontFallbackMetric));
            out.doctor_line(
                shown_summary,
                fixing.how(),
                summary.has_errors() || !operational.is_empty(),
            );
        }
        if summary.has_errors() || !operational.is_empty() {
            Outcome::Failure
        } else {
            Outcome::Success
        }
    }

    /// The style and font failures of the run, as Setup findings, and the style as a Setup fact.
    fn add_session_findings<'e>(
        &'e self,
        findings: &mut Vec<Diagnostic>,
        operational: &mut Vec<&'e Error>,
        facts: &mut Vec<String>,
    ) {
        match self.session.style() {
            Ok(style) => facts.push(format!("style {}", style.name())),
            Err(error) => push_setup(error, findings, operational),
        }
        if let Err(error) = self.session.fonts() {
            push_setup(error, findings, operational);
        }
    }

    fn print(&self, findings: &[Diagnostic], facts: &Facts, sources: &Sources<'_>, out: &mut Reporter<'_, '_>) {
        let human = self.format == OutputFormat::Human;
        let groups = group(findings, facts, human);
        if !human {
            // JSON: the same records `build` writes, in the report's order; a consumer groups by `category`.
            let all: Vec<(&Diagnostic, &[String])> = groups
                .iter()
                .flat_map(|g| g.shown.iter().map(|s| (s.diagnostic, &[][..])))
                .collect();
            out.diagnostics_with_notes(sources, &all);
            return;
        }
        for group in &groups {
            print_group(group, sources, out);
        }
    }

    /// A path as the user typed it, or relative to the current directory when it is under it.
    fn shown(&self, path: &Path) -> String {
        path.strip_prefix(self.cwd).unwrap_or(path).display().to_string()
    }
}

fn print_group(group: &Group<'_>, sources: &Sources<'_>, out: &mut Reporter<'_, '_>) {
    out.header(&group.header());
    group
        .before
        .iter()
        .for_each(|line| out.report_line(&format!("  = {line}")));
    let shown: Vec<(&Diagnostic, &[String])> = group.shown.iter().map(|s| (s.diagnostic, s.notes.as_slice())).collect();
    out.diagnostics_with_notes(sources, &shown);
    if let Some(facts) = group.facts_line() {
        out.report_line(&format!("  = {facts}"));
    }
}

/// A Setup finding for `error`, or an operational error line when it is not something advice can help with.
fn push_setup<'e>(error: &'e Error, findings: &mut Vec<Diagnostic>, operational: &mut Vec<&'e Error>) {
    match setup::finding_of(error) {
        Some(finding) => findings.push(finding),
        None => operational.push(error),
    }
}

/// A failed render: the Typst findings it carries, or a Setup finding, or an operational error.
fn add_failure<'e>(
    error: &'e Error,
    sources: &mut Sources<'e>,
    findings: &mut Vec<Diagnostic>,
    operational: &mut Vec<&'e Error>,
) {
    match error {
        Error::Compile {
            diagnostics,
            sources: files,
        }
        | Error::ExportPdf {
            diagnostics,
            sources: files,
        } => {
            files.iter().for_each(|file| sources.insert(&file.path, &file.text));
            findings.extend(diagnostics.iter().cloned());
        }
        other => push_setup(other, findings, operational),
    }
}

fn fonts_fact(fonts: &[FontResolution]) -> String {
    let line = FontsLine(fonts).to_string();
    if line.is_empty() {
        "fonts: none declared".to_string()
    } else {
        line
    }
}
