//! ABOUTME: Text-first letter command, with validation and length gates before stdout delivery.
//! ABOUTME: PDF modes commit only after all checks, before text; text mode never prepares a renderer.

use super::{
    Env, Io, Outcome,
    emit::Reporter,
    letter_plan::{self, LetterPlan, PlanError},
};
use crate::{
    cli::{Cli, LetterArgs},
    diagnostics::{Diagnostic, DiagnosticCode, Severity, SourceKind, Span},
    error::{Error, ReadInputSnafu, SpecContentSnafu, TracedToChoiceSnafu},
    letter::{LetterOutput, SelectedLetter, resolve, to_plain},
    markdown::{MAX_INPUT_BYTES, parse_letter},
    model::options::OutputFormat,
    spec::ChoiceKey,
};
use snafu::{IntoError, ResultExt};
use std::io::Read;
use std::path::Path;

pub(super) fn run(cli: &Cli, args: &LetterArgs, env: &Env, io: &mut Io<'_>) -> Outcome {
    if let Err(message) = args.validate(cli.format) {
        Reporter::new(cli.format, &env.cwd, io).line(&format!("error: {message}"));
        return Outcome::Failure;
    }
    let mut reporter = Reporter::new(cli.format, &env.cwd, io);
    let plan = match planned(cli, args, env, &mut reporter) {
        Ok(Some(plan)) => plan,
        Ok(None) => return Outcome::Success,
        Err(()) => return Outcome::Failure,
    };
    match prepare(&plan, cli.format, &mut reporter) {
        Ok((letter, text)) => {
            let mode = plan.output;
            let pdf = if mode != LetterOutput::Text {
                match super::letter_pdf::render(cli, &plan, env, &letter, &mut reporter) {
                    Ok(path) => Some(path),
                    Err(()) => return Outcome::Failure,
                }
            } else {
                None
            };
            if cli.dry_run {
                if mode != LetterOutput::Pdf {
                    reporter.line(&format!(
                        "ok would print letter: {} utf16, {} words; no output written",
                        text.counts().utf16,
                        text.counts().words
                    ));
                }
                summary(&plan, cli.format, pdf.as_deref(), &mut reporter);
                return Outcome::Success;
            }
            if mode == LetterOutput::Pdf {
                summary(&plan, cli.format, pdf.as_deref(), &mut reporter);
                return Outcome::Success;
            }
            let counts = text.counts();
            // Observe flush failure here, before the shared dispatcher can treat a closed pipe as success.
            if let Err(error) = io
                .stdout
                .write_all(text.as_str().as_bytes())
                .and_then(|()| io.stdout.flush())
            {
                let _ = writeln!(io.stderr, "error: letter not fully written to stdout: {error}");
                if let Some(path) = pdf {
                    let _ = writeln!(io.stderr, "PDF remains at {}", path.display());
                }
                return Outcome::Failure;
            }
            let mut reporter = Reporter::new(cli.format, &env.cwd, io);
            reporter.line(&format!(
                "ok printed letter: {} utf16, {} scalars, {} bytes, {} words, {} line breaks",
                counts.utf16, counts.scalars, counts.bytes, counts.words, counts.line_breaks
            ));
            summary(&plan, cli.format, pdf.as_deref(), &mut reporter);
            Outcome::Success
        }
        Err(CommandFailure::Findings) => Outcome::Failure,
        Err(CommandFailure::Read(error)) => {
            reporter.error(&plan.read_error(error));
            Outcome::Failure
        }
    }
}

/// What the run is: the letter a file is, or the letter a spec says. `Ok(None)` is a run that built nothing and
/// is done (`--print-spec`).
fn planned(cli: &Cli, args: &LetterArgs, env: &Env, out: &mut Reporter<'_, '_>) -> Result<Option<LetterPlan>, ()> {
    let Some(spec_path) = &args.spec.spec else {
        let file = args.input().expect("clap requires a file without a spec");
        return Ok(Some(letter_plan::plan_file(args, file)));
    };
    let resolved = letter_plan::resolve(args, spec_path).map_err(|error| out.error(&error))?;
    if args.spec.print_spec {
        out.product(&resolved.to_toml());
        return Ok(None);
    }
    match letter_plan::plan_spec(args, spec_path, resolved, cli.format, env) {
        Ok(plan) => Ok(Some(plan)),
        Err(PlanError::Error(error)) => {
            out.error(&error);
            Err(())
        }
        Err(PlanError::Usage(usage)) => {
            out.line(&format!("error: {usage}"));
            Err(())
        }
    }
}

/// The spec's lines under the run's line, or its JSON record on stdout; nothing for a letter built from a file.
///
/// Human text goes to stderr and the JSON record to stdout, which is free only when the letter is not printed
/// there: the plan has already refused `--format json` with a letter that prints text.
fn summary(plan: &LetterPlan, format: OutputFormat, pdf: Option<&Path>, out: &mut Reporter<'_, '_>) {
    let Some(spec) = &plan.spec else { return };
    match format {
        OutputFormat::Human => {
            for line in spec.facts.lines() {
                out.line(&format!("  {line}"));
            }
        }
        OutputFormat::Json => out.record(&spec.facts.record(pdf)),
    }
}

enum CommandFailure {
    Read(Error),
    Findings,
}

fn select_letter(
    source: &crate::letter::LetterSource,
    plan: &LetterPlan,
    text: &str,
    format: crate::model::options::OutputFormat,
    out: &mut Reporter<'_, '_>,
) -> Result<SelectedLetter, CommandFailure> {
    use crate::letter::{LetterSelectError, SourceItem};
    let file = &plan.shown;
    let selection = plan.selection.clone();
    let annotations = std::iter::once(&source.greeting)
        .chain(&source.body)
        .chain(std::iter::once(&source.closing))
        .flat_map(|item| match item {
            SourceItem::Paragraph(item) => std::slice::from_ref(item),
            SourceItem::Alternates(group) => group.variants(),
        })
        .map(|item| &item.annotations);
    let unknown = crate::select::UnknownNames::from_annotations(annotations, &selection);
    let mut sources = out.sources();
    sources.insert(file, text);
    let origins = plan.spec.as_ref().map(|spec| &spec.names);
    let findings = crate::pipeline::unknown_letter_names_from(file, &unknown, origins);
    out.diagnostics(&sources, &findings.iter().collect::<Vec<_>>());
    let mut report = |report: &crate::letter::LetterReport| {
        if !super::letter_report::is_silent(report, &selection) {
            let record = super::letter_report::LetterSelectionRecord::new(
                out.shown(file).display().to_string(),
                text,
                report,
                &selection,
            );
            if format == crate::model::options::OutputFormat::Json {
                out.record(&record);
            } else {
                for line in record.lines(&selection) {
                    out.line(&line);
                }
            }
        }
    };
    match crate::letter::select(source, &selection) {
        Ok((selected, explanation)) => {
            report(&explanation);
            check_picks(plan, &explanation, out)?;
            Ok(selected)
        }
        Err(error) => {
            let (code, range) = match &error {
                LetterSelectError::EmptyBody { report: explanation } => {
                    report(explanation);
                    // A misspelled pick is the likelier cause of an empty body, and it names a line of the spec.
                    check_picks(plan, explanation, out)?;
                    (
                        DiagnosticCode::LetterBodyEmpty,
                        source
                            .body
                            .first()
                            .map(|item| match item {
                                SourceItem::Paragraph(item) => item.piece.range.clone(),
                                SourceItem::Alternates(group) => group.variants()[0].piece.range.clone(),
                            })
                            .unwrap_or(0..0),
                    )
                }
                LetterSelectError::FrameNotReady { range, .. } => (DiagnosticCode::LetterFrameNotReady, range.clone()),
                // Only a spec makes picks, so the lines that picked the draft are the spec's.
                LetterSelectError::PickedDraft { picks } => {
                    out.error(&plan.picked_draft(picks, &error));
                    return Err(CommandFailure::Findings);
                }
            };
            let finding = Diagnostic {
                severity: Severity::Error,
                code,
                message: error.to_string(),
                span: Some(Span {
                    file: file.clone(),
                    kind: SourceKind::Markdown,
                    range,
                }),
                hints: vec![code.advice().fix.to_owned()],
                fix: None,
            };
            out.diagnostics(&sources, &[&finding]);
            out.line("error: no letter printed");
            Err(CommandFailure::Findings)
        }
    }
}

fn prepare(
    plan: &LetterPlan,
    format: crate::model::options::OutputFormat,
    out: &mut Reporter<'_, '_>,
) -> Result<(crate::letter::Letter, crate::letter::PlainLetter), CommandFailure> {
    let read = &plan.content;
    let file = &plan.shown;
    // Judge the byte limit before decoding: a bounded prefix can end inside a UTF-8 character.
    let opened = std::fs::File::open(read)
        .context(ReadInputSnafu { path: read })
        .map_err(CommandFailure::Read)?;
    let mut bytes = Vec::new();
    opened
        .take(MAX_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .context(ReadInputSnafu { path: read })
        .map_err(CommandFailure::Read)?;
    if bytes.len() > MAX_INPUT_BYTES {
        let mut sources = out.sources();
        sources.insert(file, "");
        let finding = Diagnostic {
            severity: Severity::Error,
            code: DiagnosticCode::InputLimit,
            message: format!("input exceeds the {MAX_INPUT_BYTES}-byte limit"),
            span: Some(Span {
                file: file.clone(),
                kind: SourceKind::Markdown,
                range: 0..0,
            }),
            hints: vec!["shorten the letter or check the input path".into()],
            fix: None,
        };
        out.diagnostics(&sources, &[&finding]);
        return Err(CommandFailure::Findings);
    }
    let text = String::from_utf8(bytes)
        .context(crate::error::InputUtf8Snafu { path: read })
        .map_err(CommandFailure::Read)?;
    let mut sources = out.sources();
    sources.insert(file, &text);
    let parsed = parse_letter(file, &text);
    out.diagnostics(&sources, &parsed.diagnostics().iter().collect::<Vec<_>>());
    let (source, _) = parsed.into_parts();
    let Some(source) = source else {
        out.line("error: no letter printed");
        return Err(CommandFailure::Findings);
    };
    let selected = select_letter(&source, plan, &text, format, out)?;
    let letter = resolve(selected, &plan.text.fields, file).map_err(|findings| {
        out.diagnostics(&sources, &findings.iter().collect::<Vec<_>>());
        out.line("error: no letter printed");
        CommandFailure::Findings
    })?;
    let plain = to_plain(&letter, plan.text.links);
    if let Some(limit) = plan.text.limit
        && let Err(error) = limit.check(&plain)
    {
        let finding = Diagnostic {
            severity: Severity::Error,
            code: DiagnosticCode::TextLimitExceeded,
            message: error.to_string(),
            span: Some(Span {
                file: file.clone(),
                kind: SourceKind::Markdown,
                range: 0..0,
            }),
            hints: vec!["shorten or select different wording; text is never truncated".into()],
            fix: None,
        };
        out.diagnostics(&sources, &[&finding]);
        out.line("error: no letter printed");
        return Err(CommandFailure::Findings);
    }
    Ok((letter, plain))
}

/// A pick that names nothing in the letter is a mistake in the spec that wrote it, and only a spec makes picks.
fn check_picks(
    plan: &LetterPlan,
    report: &crate::letter::LetterReport,
    out: &mut Reporter<'_, '_>,
) -> Result<(), CommandFailure> {
    let Some(spec) = &plan.spec else { return Ok(()) };
    spec.resolved
        .check_letter_picks(&report.unknown_picks)
        .map_err(|source| {
            out.error(&Error::Spec {
                source: Box::new(source),
            });
            out.line("error: no letter printed");
            CommandFailure::Findings
        })
}

impl LetterPlan {
    /// A failure to read the letter, naming the `content =` line that named the file when a spec did.
    fn read_error(&self, source: Error) -> Error {
        match &self.spec {
            Some(spec) => SpecContentSnafu {
                at: spec.content_at().clone(),
            }
            .into_error(source),
            None => source,
        }
    }

    /// For a draft wording a spec picked, the same error naming the lines that picked it.
    fn picked_draft(&self, picks: &[crate::model::Alternate], error: &crate::letter::LetterSelectError) -> Error {
        let message = Error::LetterPicks {
            message: error.to_string(),
        };
        let Some(spec) = &self.spec else { return message };
        let Some(letter) = &spec.resolved.letter else {
            return message;
        };
        let at: Vec<_> = picks
            .iter()
            .filter_map(|pick| letter.origin_of(&ChoiceKey::Pick(pick.group.clone())))
            .cloned()
            .collect();
        if at.is_empty() {
            message
        } else {
            TracedToChoiceSnafu { at }.into_error(message)
        }
    }
}
