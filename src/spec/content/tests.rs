use super::*;
use claims::{assert_err, assert_none, assert_ok, assert_some};
use pretty_assertions::assert_eq;

fn snapshot(text: &str) -> SourceText {
    SourceText::new("unsaved/position.toml".into(), SourceKind::Spec, text)
}

#[test]
fn literal_spans_from_the_actual_toml_parser_decode_without_offset_guesses() {
    #[derive(Deserialize)]
    struct Envelope {
        content: toml::Spanned<String>,
    }
    for literal in [
        "'plain'",
        "\"é \\u4e2d\"",
        "'''\nmultiline\ntext'''",
        "\"\"\"\nline \\\n   continued\"\"\"",
        "'''\r\nCRLF\r\ntext'''",
        "\"quote \\\" text\"",
    ] {
        let text = format!("# prefix é\ncontent = {literal}\n");
        let parsed: Envelope = assert_ok!(toml::from_str(&text));
        let source = snapshot(&text);
        let content = assert_ok!(SpecContent::resume(
            &source,
            ResumeFragmentKind::Paragraph,
            parsed.content.span()
        ));
        assert_eq!(content.markdown(), parsed.content.get_ref());
        assert_eq!(content.origin().span().range, parsed.content.span());
        assert_eq!(content.source(), &source);
    }
}

#[test]
fn rejects_non_spec_invalid_ranges_and_non_literal_slices() {
    let source = snapshot("'é'");
    assert!(matches!(
        assert_err!(SpecContent::letter(&source, 2..3)),
        ContentError::Range { .. }
    ));
    assert!(matches!(
        assert_err!(SpecContent::letter(&source, 0..99)),
        ContentError::Range { .. }
    ));
    assert!(matches!(
        assert_err!(SpecContent::letter(&source, Range { start: 3, end: 1 })),
        ContentError::Range { .. }
    ));
    let baseline = SourceText::new("x.md".into(), SourceKind::Markdown, "'a'");
    assert!(matches!(
        assert_err!(SpecContent::letter(&baseline, 0..3)),
        ContentError::NotSpec { .. }
    ));
    for literal in [
        "",
        "42",
        "['text']",
        "{content = 'text'}",
        "content = 'text'",
        " 'text'",
        "'text' ",
        "'text' # comment",
        "'text' # comment ending 'quote'",
        "'text' 'more'",
        "'text'\n",
        "'text'\r\n",
        "'text'\n# c",
        "'text'\nother = 1",
        "'text'\nvalue = 'b'",
        "'text'\n[t]",
        "\"bad\\q\"",
    ] {
        let source = snapshot(literal);
        assert_err!(SpecContent::letter(&source, 0..literal.len()), "{literal}");
    }
}

#[test]
fn errors_keep_the_original_snapshot_and_locate_the_whole_literal() {
    let literal = "\"é \\n{{unknown}}\"";
    let text = format!("content = {literal}\n");
    let source = snapshot(&text);
    let error = assert_err!(SpecContent::letter(&source, 10..10 + literal.len()));
    let (retained, findings) = assert_some!(error.findings());
    assert_eq!(retained.text(), text);
    let unknown = assert_some!(
        findings
            .iter()
            .find(|d| d.code == crate::diagnostics::DiagnosticCode::PlaceholderUnknown)
    );
    let span = assert_some!(unknown.span.as_ref());
    assert_eq!(span.file, source.path());
    assert_eq!(span.kind, SourceKind::Spec);
    assert_eq!(&retained.text()[span.range.clone()], literal);
    assert!(unknown.hints[0].contains("2:1"), "{:?}", unknown.hints);
    assert!(findings.iter().all(|d| d.fix.is_none()));
}

#[test]
fn warnings_are_preserved_without_edits_and_authored_text_is_not_reconstructed() {
    let literal = "'''**Label**: text  \ncontinued'''";
    let source = snapshot(literal);
    let content = assert_ok!(SpecContent::resume(
        &source,
        ResumeFragmentKind::Bullet,
        0..literal.len()
    ));
    assert_eq!(content.markdown(), "**Label**: text  \ncontinued");
    assert!(
        content
            .warnings()
            .iter()
            .any(|d| d.code == crate::diagnostics::DiagnosticCode::HardBreak)
    );
    for finding in content.warnings() {
        assert_none!(finding.fix.as_ref());
        assert_eq!(assert_some!(finding.span.as_ref()).range, 0..literal.len());
    }
}

#[test]
fn malformed_literal_errors_do_not_print_envelope_coordinates_or_unsafe_characters() {
    let literal = "\"bad\\q\u{202e}\"";
    let source = snapshot(literal);
    let error = assert_err!(SpecContent::letter(&source, 0..literal.len()));
    let shown = error.to_string();
    assert!(!shown.contains("value ="), "{shown}");
    assert!(!shown.contains("line 1"), "{shown}");
    assert!(!shown.contains('\u{202e}'), "{shown}");
    let error = crate::spec::SpecError::AdditionContent {
        source: Box::new(error),
    };
    assert_none!(error.hint());
}

#[test]
fn later_literal_findings_keep_source_positions_and_escape_decoded_controls() {
    let prefix = "# header\n\n[letter]\ncontent = ";
    let literal = "\"\\u202e{{unknown}}\"";
    let source = snapshot(&format!("{prefix}{literal}\n"));
    let error = assert_err!(SpecContent::letter(&source, prefix.len()..prefix.len() + literal.len()));
    let (snapshot, findings) = assert_some!(error.findings());
    let mut sources = crate::diagnostics::Sources::new(None);
    sources.insert_text(snapshot);
    for finding in findings {
        let span = assert_some!(finding.span.as_ref());
        let position = assert_some!(sources.position(span));
        assert_eq!((position.line, position.column), (4, 11));
        assert_eq!(&snapshot.text()[span.range.clone()], literal);
        assert!(!finding.hints[0].contains('\u{202e}'));
        assert!(finding.hints[0].contains("\\u{202e}"));
    }
}

#[test]
fn public_constructors_cannot_bypass_the_spec_size_limit() {
    let source = snapshot(&format!(
        "'text'{}",
        " ".repeat(super::super::error::MAX_FILE_BYTES as usize)
    ));
    assert!(matches!(
        assert_err!(SpecContent::letter(&source, 0..6)),
        ContentError::TooLarge { .. }
    ));
}
