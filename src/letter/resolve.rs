//! ABOUTME: Resolves only selected letter placeholders, collecting located missing-value findings.
//! ABOUTME: Substitutions are literal text; no Markdown parsing or recursive interpolation occurs.

use super::{LETTER_SCHEMA_VERSION, Letter, LetterFields, LetterInline, LetterKind, SelectedLetter};
use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::markdown::{Finding, Sink};
use crate::model::Inline;
use std::path::Path;

/// Fills selected placeholders as literal text, retaining every original missing-use location.
///
/// Source shape, nesting bounds and UTF-8 source ranges must already be validated by the parser.
///
/// # Errors
/// Missing values produce `placeholder-unresolved` findings sorted by original byte range.
/// The existing diagnostic cap applies, with an error summary for suppressed findings.
pub fn resolve(selected: SelectedLetter, fields: &LetterFields, path: &Path) -> Result<Letter, Vec<Diagnostic>> {
    let mut sink = Sink::new(path);
    let greeting = lower(selected.greeting.content, fields, &mut sink);
    let body = selected
        .body
        .into_iter()
        .map(|piece| lower(piece.content, fields, &mut sink))
        .collect();
    let closing = lower(selected.closing.content, fields, &mut sink);
    let diagnostics = sink.finish();
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    Ok(Letter {
        schema: LETTER_SCHEMA_VERSION,
        kind: LetterKind::Letter,
        name: selected.name,
        contact: selected.contact,
        date: fields.date,
        company: fields.company.clone(),
        recipient: fields.recipient.clone(),
        greeting,
        body,
        closing,
    })
}

fn lower(nodes: Vec<LetterInline>, fields: &LetterFields, sink: &mut Sink<'_>) -> Vec<Inline> {
    let mut output = Vec::new();
    for node in nodes {
        match node {
            LetterInline::Text(text) => push_literal(&mut output, text),
            LetterInline::Strong(content) => output.push(Inline::Strong {
                content: lower(content, fields, sink),
            }),
            LetterInline::Emph(content) => output.push(Inline::Emph {
                content: lower(content, fields, sink),
            }),
            LetterInline::Link { url, content } => output.push(Inline::Link {
                url,
                content: lower(content, fields, sink),
            }),
            LetterInline::Code(text) => output.push(Inline::Code { text }),
            LetterInline::Placeholder { key, range } => match fields.get(key) {
                Some(value) => push_literal(&mut output, value.to_string()),
                None => sink.report(
                    Finding::new(
                        DiagnosticCode::PlaceholderUnresolved,
                        range,
                        format!("`{{{{{key}}}}}` has no value"),
                    )
                    .hint(format!("supply the {key} application field")),
                ),
            },
        }
    }
    output
}

fn push_literal(output: &mut Vec<Inline>, text: String) {
    if let Some(Inline::Text { text: previous }) = output.last_mut() {
        previous.push_str(&text);
    } else {
        output.push(Inline::Text { text });
    }
}
