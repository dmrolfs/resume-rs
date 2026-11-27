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
        // `validate` has refused a run with neither a file nor a spec, so a file is here.
        let Some(file) = args.input() else {
            out.line("error: a letter needs a FILE or --spec");
            return Err(());
        };
        return Ok(Some(letter_plan::plan_file(args, file)));
    };
    let resolved = letter_plan::resolve(args, spec_path).map_err(|error| out.error(&error))?;
    out.spec_warnings(&resolved, args.spec.print_spec);
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
    sources: &crate::diagnostics::Sources<'_>,
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
    let origins = plan.spec.as_ref().map(|spec| &spec.names);
    let findings = crate::pipeline::unknown_letter_names_from(file, &unknown, origins);
    out.diagnostics(sources, &findings.iter().collect::<Vec<_>>());
    let mut report = |report: &crate::letter::LetterReport| {
        if !super::letter_report::is_silent(report, &selection) {
            let record = super::letter_report::LetterSelectionRecord::new(
                out.shown(file).display().to_string(),
                file,
                sources,
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
            let (code, span) = match &error {
                LetterSelectError::EmptyBody { report: explanation } => {
                    report(explanation);
                    // A misspelled pick is the likelier cause of an empty body, and it names a line of the spec.
                    check_picks(plan, explanation, out)?;
                    (
                        DiagnosticCode::LetterBodyEmpty,
                        source.body.first().map(|item| match item {
                            SourceItem::Paragraph(item) => item.piece.origin.span(),
                            SourceItem::Alternates(group) => group.variants()[0].piece.origin.span(),
                        }),
                    )
                }
                LetterSelectError::FrameNotReady { origin, .. } => {
                    (DiagnosticCode::LetterFrameNotReady, Some(origin.span()))
                }
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
                span,
                hints: vec![code.advice().fix.to_owned()],
                fix: None,
            };
            out.diagnostics(sources, &[&finding]);
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
    if let Some(spec) = &plan.spec {
        for snapshot in spec.resolved.sources() {
            sources
                .insert_checked(snapshot.path(), snapshot.text())
                .context(crate::error::ReportSourceSnafu)
                .map_err(CommandFailure::Read)?;
        }
    }
    let parsed = parse_letter(file, &text);
    out.diagnostics(&sources, &parsed.diagnostics().iter().collect::<Vec<_>>());
    let (source, _) = parsed.into_parts();
    let Some(source) = source else {
        out.line("error: no letter printed");
        return Err(CommandFailure::Findings);
    };
    let source = prepare_items(source, text.len(), plan, out)?;
    let selected = select_letter(&source, plan, &sources, format, out)?;
    let selected_origins: Vec<_> = selected.body.iter().map(|piece| piece.origin.span()).collect();
    let letter = resolve(selected, &plan.text.fields).map_err(|findings| {
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
            span: None,
            hints: {
                let mut hints = vec!["shorten or select different wording; text is never truncated".into()];
                if let Some(setting) = plan
                    .spec
                    .as_ref()
                    .and_then(|s| s.resolved.letter.as_ref())
                    .and_then(|l| l.settings.max_length.as_ref())
                {
                    hints.push(format!("spec length limit written at {}", setting.at));
                }
                for span in selected_origins.iter().take(8) {
                    if let Some(at) = sources.position(span) {
                        hints.push(format!("selected body content: {}:{}:{}", at.file, at.line, at.column));
                    }
                }
                hints
            },
            fix: None,
        };
        out.diagnostics(&sources, &[&finding]);
        out.line("error: no letter printed");
        return Err(CommandFailure::Findings);
    }
    Ok((letter, plain))
}

/// The letter with a spec's per-item changes applied, which selection, its warnings and every output read. A target
/// the letter lacks is a mistake in the file that wrote it, found before anything is selected or printed.
fn prepare_items(
    source: crate::letter::LetterSource,
    baseline_bytes: usize,
    plan: &LetterPlan,
    out: &mut Reporter<'_, '_>,
) -> Result<crate::letter::LetterSource, CommandFailure> {
    let Some(letter) = plan.spec.as_ref().and_then(|spec| spec.resolved.letter.as_ref()) else {
        return Ok(source);
    };
    crate::spec::items::assemble_letter(
        &source,
        baseline_bytes,
        &letter.additions,
        Some(letter.item_overrides()),
    )
    .map_err(|source| {
        out.error(&Error::Spec {
            source: Box::new(crate::spec::SpecError::PrepareItems {
                source: Box::new(source),
            }),
        });
        out.line("error: no letter printed");
        CommandFailure::Findings
    })
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
        let message = Error::LetterPicks { source: error.clone() };
        let Some(letter) = self.spec.as_ref().and_then(|spec| spec.resolved.letter.as_ref()) else {
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
