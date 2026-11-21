//! ABOUTME: Text-first letter command, with validation and length gates before stdout delivery.
//! ABOUTME: PDF modes commit only after all checks, before text; text mode never prepares a renderer.

use super::{Env, Io, Outcome, emit::Reporter};
use crate::{
    cli::{Cli, LetterArgs},
    diagnostics::{Diagnostic, DiagnosticCode, Severity, SourceKind, Span},
    error::{Error, ReadInputSnafu},
    letter::{SelectedLetter, resolve, to_plain},
    markdown::{MAX_INPUT_BYTES, parse_letter},
};
use snafu::ResultExt;
use std::io::Read;

pub(super) fn run(cli: &Cli, args: &LetterArgs, env: &Env, io: &mut Io<'_>) -> Outcome {
    if let Err(message) = args.validate(cli.format) {
        Reporter::new(cli.format, &env.cwd, io).line(&format!("error: {message}"));
        return Outcome::Failure;
    }
    let mut reporter = Reporter::new(cli.format, &env.cwd, io);
    match prepare(args, &mut reporter) {
        Ok((letter, text)) => {
            let mode = args.output_format.unwrap_or_default();
            let pdf = if mode != crate::letter::LetterOutput::Text {
                match super::letter_pdf::render(cli, args, env, &letter, &mut reporter) {
                    Ok(path) => Some(path),
                    Err(()) => return Outcome::Failure,
                }
            } else {
                None
            };
            if cli.dry_run {
                if mode != crate::letter::LetterOutput::Pdf {
                    reporter.line(&format!(
                        "ok would print letter: {} utf16, {} words; no output written",
                        text.counts().utf16,
                        text.counts().words
                    ));
                }
                return Outcome::Success;
            }
            if mode == crate::letter::LetterOutput::Pdf {
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
            Reporter::new(cli.format, &env.cwd, io).line(&format!(
                "ok printed letter: {} utf16, {} scalars, {} bytes, {} words, {} line breaks",
                counts.utf16, counts.scalars, counts.bytes, counts.words, counts.line_breaks
            ));
            Outcome::Success
        }
        Err(CommandFailure::Findings) => Outcome::Failure,
        Err(CommandFailure::Read(error)) => {
            reporter.error(&error);
            Outcome::Failure
        }
    }
}

enum CommandFailure {
    Read(Error),
    Findings,
}

fn prepare(
    args: &LetterArgs,
    out: &mut Reporter<'_, '_>,
) -> Result<(crate::letter::Letter, crate::letter::PlainLetter), CommandFailure> {
    // Judge the byte limit before decoding: a bounded prefix can end inside a UTF-8 character.
    let file = std::fs::File::open(&args.file)
        .context(ReadInputSnafu { path: &args.file })
        .map_err(CommandFailure::Read)?;
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .context(ReadInputSnafu { path: &args.file })
        .map_err(CommandFailure::Read)?;
    if bytes.len() > MAX_INPUT_BYTES {
        let mut sources = out.sources();
        sources.insert(&args.file, "");
        let finding = Diagnostic {
            severity: Severity::Error,
            code: DiagnosticCode::InputLimit,
            message: format!("input exceeds the {MAX_INPUT_BYTES}-byte limit"),
            span: Some(Span {
                file: args.file.clone(),
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
        .context(crate::error::InputUtf8Snafu { path: &args.file })
        .map_err(CommandFailure::Read)?;
    let mut sources = out.sources();
    sources.insert(&args.file, &text);
    let parsed = parse_letter(&args.file, &text);
    out.diagnostics(&sources, &parsed.diagnostics().iter().collect::<Vec<_>>());
    let (source, _) = parsed.into_parts();
    let Some(source) = source else {
        out.line("error: no letter printed");
        return Err(CommandFailure::Findings);
    };
    let settings = args.effective();
    // Until letter selection exists (rsm-vqr.2) an annotated letter cannot be built; the located error is rsm-5q1's.
    let selected = SelectedLetter::try_from(source).map_err(|refused| {
        out.line(&format!("error: {refused}"));
        out.line("error: no letter printed");
        CommandFailure::Findings
    })?;
    let letter = resolve(selected, &settings.fields, &args.file).map_err(|findings| {
        out.diagnostics(&sources, &findings.iter().collect::<Vec<_>>());
        out.line("error: no letter printed");
        CommandFailure::Findings
    })?;
    let plain = to_plain(&letter, settings.links);
    if let Some(limit) = settings.limit
        && let Err(error) = limit.check(&plain)
    {
        let finding = Diagnostic {
            severity: Severity::Error,
            code: DiagnosticCode::TextLimitExceeded,
            message: error.to_string(),
            span: Some(Span {
                file: args.file.clone(),
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
