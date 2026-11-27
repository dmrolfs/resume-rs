//! ABOUTME: Resolves only selected letter placeholders, collecting source-owned missing-value findings.
//! ABOUTME: Substitutions are literal text; one diagnostic budget covers all source files.

use super::{LETTER_SCHEMA_VERSION, Letter, LetterFields, LetterInline, LetterKind, SelectedLetter};
use crate::diagnostics::{Collector, Diagnostic, DiagnosticCode, ItemOrigin};
use crate::model::Inline;

/// Fills selected placeholders as literal text, retaining each piece's source ownership.
///
/// Source shape, nesting bounds and UTF-8 source ranges must already be validated by the parser.
/// Missing values are ordered within consecutive source runs, with one document-wide diagnostic cap.
pub fn resolve(selected: SelectedLetter, fields: &LetterFields) -> Result<Letter, Vec<Diagnostic>> {
    let mut sink = Collector::new(crate::markdown::MAX_DIAGNOSTICS);
    let greeting = lower(selected.greeting.content, &selected.greeting.origin, fields, &mut sink);
    let body = selected
        .body
        .into_iter()
        .map(|piece| lower(piece.content, &piece.origin, fields, &mut sink))
        .collect();
    let closing = lower(selected.closing.content, &selected.closing.origin, fields, &mut sink);
    let diagnostics = sink.finish(None);
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

fn lower(nodes: Vec<LetterInline>, origin: &ItemOrigin, fields: &LetterFields, sink: &mut Collector) -> Vec<Inline> {
    let mut output = Vec::new();
    for node in nodes {
        match node {
            LetterInline::Text(text) => push_literal(&mut output, text),
            LetterInline::Strong(content) => output.push(Inline::Strong {
                content: lower(content, origin, fields, sink),
            }),
            LetterInline::Emph(content) => output.push(Inline::Emph {
                content: lower(content, origin, fields, sink),
            }),
            LetterInline::Link { url, content } => output.push(Inline::Link {
                url,
                content: lower(content, origin, fields, sink),
            }),
            LetterInline::Code(text) => output.push(Inline::Code { text }),
            LetterInline::Placeholder { key, range } => match fields.get(key) {
                Some(value) => push_literal(&mut output, value.to_string()),
                None => sink.push(Diagnostic {
                    severity: DiagnosticCode::PlaceholderUnresolved.default_severity(),
                    code: DiagnosticCode::PlaceholderUnresolved,
                    span: Some(origin.inline_span(range)),
                    message: format!("`{{{{{key}}}}}` has no value"),
                    hints: vec![format!("supply the {key} application field")],
                    fix: None,
                }),
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
