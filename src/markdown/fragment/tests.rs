use super::*;
use crate::markdown::{MAX_INPUT_BYTES, parse, parse_letter};
use crate::model::{Block, RoleBlock};
use claims::{assert_err, assert_ok, assert_some};
use pretty_assertions::assert_eq;

#[test]
fn resume_leaves_match_baseline_lowering() {
    let text = "**Migration:** Delivered *safer* `Rust` &amp; [tools](https://example.com).\nContinued.";
    let bullet = assert_ok!(resume_fragment(ResumeFragmentKind::Bullet, text));
    let paragraph = assert_ok!(resume_fragment(ResumeFragmentKind::Paragraph, text));
    let blurb = assert_ok!(resume_fragment(ResumeFragmentKind::Blurb, text));
    let baseline = format!(
        "# Ada\n\nHere\n\n## S\n\n{text}\n\n### Acme · Engineer · 2020 – Present\n\n{text}\n\n- {}\n",
        text.replace('\n', "\n  ")
    );
    let parsed = parse("baseline.md", &baseline);
    let model = assert_some!(parsed.valid(), "{:?}", parsed.diagnostics());
    let Block::Paragraph { content, .. } = &model.sections[0].blocks[0] else {
        panic!("paragraph")
    };
    assert_eq!(paragraph.value, ResumeFragment::Paragraph(content.clone()));
    let Block::Role(role) = &model.sections[0].blocks[1] else {
        panic!("role")
    };
    assert_eq!(blurb.value, ResumeFragment::Blurb(assert_some!(role.blurb.clone())));
    let RoleBlock::Bullets { items } = &role.body[0] else {
        panic!("bullets")
    };
    assert_eq!(
        bullet.value,
        ResumeFragment::Bullet {
            label: items[0].label.clone(),
            content: items[0].content.clone()
        }
    );
}

#[test]
fn letter_fragment_matches_baseline_including_placeholder_coordinates() {
    let text = "For **{{company}}**, [{{title}}](https://example.com) and *{{recipient}}*.";
    let lowered = assert_ok!(letter_paragraph(text));
    let prefix = "# Ada\n\nHere\n\n## Greeting\n\nHello,\n\n## Body\n\n";
    let parsed = parse_letter("letter.md", &format!("{prefix}{text}\n\n## Closing\n\nThanks,\n"));
    let letter = assert_some!(parsed.valid(), "{:?}", parsed.diagnostics());
    let mut expected = assert_some!(letter.body[0].paragraph()).content.clone();
    fn shift(nodes: &mut [LetterInline], offset: usize) {
        for node in nodes {
            match node {
                LetterInline::Placeholder { range, .. } => {
                    range.start -= offset;
                    range.end -= offset;
                }
                LetterInline::Strong(c) | LetterInline::Emph(c) | LetterInline::Link { content: c, .. } => {
                    shift(c, offset)
                }
                _ => {}
            }
        }
    }
    shift(&mut expected, prefix.len());
    assert_eq!(lowered.value, expected);
}

#[test]
fn rejects_shapes_that_baseline_lowering_could_discard_or_flatten() {
    for text in [
        "",
        " \n\t",
        "<!-- comment -->",
        "# Heading",
        "one\n\ntwo",
        "- bullet",
        "1. numbered",
        "- outer\n  - inner",
        "> quote",
        "```\ncode\n```",
        "---",
        "<div class=\"page-break\"></div>",
        "text ![photo](photo.png)",
        "text <b>html</b>",
        "text\n\n[id]: https://example.com",
        "[link][id]\n\n[id]: https://example.com",
        "text\n\n| a | b |\n| - | - |\n| c | d |",
    ] {
        for kind in [
            ResumeFragmentKind::Bullet,
            ResumeFragmentKind::Paragraph,
            ResumeFragmentKind::Blurb,
        ] {
            assert_err!(resume_fragment(kind, text), "{text:?}");
        }
        assert_err!(letter_paragraph(text), "{text:?}");
    }
}

#[test]
fn annotations_are_rejected_at_every_inline_position() {
    for text in [
        "<!-- @core -->\ntext",
        "text <!-- #tag -->",
        "text <!-- &id --> rest",
        "**text <!-- =group.default -->**",
        "text <!-- ordinary --><!-- @verify -->",
        "text <!-- @core",
        "text\n\n<!-- @core -->",
    ] {
        assert_err!(resume_fragment(ResumeFragmentKind::Bullet, text), "{text:?}");
        assert_err!(letter_paragraph(text), "{text:?}");
    }
}

#[test]
fn ordinary_comments_are_allowed_without_hiding_extra_blocks() {
    for text in [
        "text <!-- ordinary --> continues",
        "<!-- ordinary -->\n\ntext",
        "text\n\n<!-- ordinary -->",
    ] {
        assert_ok!(resume_fragment(ResumeFragmentKind::Paragraph, text));
        assert_ok!(letter_paragraph(text));
    }
    assert_err!(resume_fragment(
        ResumeFragmentKind::Paragraph,
        "text\n\n<!-- ordinary -->\n\nmore"
    ));
    assert_err!(letter_paragraph("<!-- ordinary --> <script>bad</script>\n\ntext"));
}

#[test]
fn rejects_unsafe_characters_even_when_markdown_decodes_them() {
    for text in [
        "nul\0text",
        "escape\u{1b}text",
        "bidi\u{202e}text",
        "entity &#x202e; text",
        "entity &#x1b; text",
        "[link](https://example.com/\u{202e})",
    ] {
        assert_err!(resume_fragment(ResumeFragmentKind::Paragraph, text), "{text:?}");
        assert_err!(letter_paragraph(text), "{text:?}");
    }
    assert_ok!(resume_fragment(ResumeFragmentKind::Paragraph, "é\t中\r\ntext"));
}

#[test]
fn rejects_invalid_links_and_uses_letter_placeholder_rules() {
    for text in ["[bad](relative)", "[bad](javascript:alert)", "[missing][unknown]"] {
        assert_err!(resume_fragment(ResumeFragmentKind::Paragraph, text), "{text:?}");
        assert_err!(letter_paragraph(text), "{text:?}");
    }
    for text in [
        "{{unknown}}",
        "{{company",
        "`{{company}}`",
        "[text](https://example.com/{{company}})",
        "{{com**pany}}**",
        "\\{{company}}",
    ] {
        assert_err!(letter_paragraph(text), "{text:?}");
    }
    assert_ok!(resume_fragment(ResumeFragmentKind::Paragraph, "Literal {{company}}"));
}

#[test]
fn warnings_keep_decoded_ranges_but_cannot_carry_edits() {
    let text = "é **Label**: text  \r\ncontinued";
    let lowered = assert_ok!(resume_fragment(ResumeFragmentKind::Bullet, text));
    assert!(
        lowered
            .warnings
            .iter()
            .any(|issue| issue.code == DiagnosticCode::HardBreak)
    );
    assert!(
        lowered
            .warnings
            .iter()
            .any(|issue| issue.code == DiagnosticCode::MidSentenceBold)
    );
    for issue in lowered.warnings {
        assert!(text.get(issue.decoded).is_some());
        assert_ne!(issue.severity, Severity::Error);
    }
}

#[test]
fn input_depth_and_diagnostic_limits_still_reject_instead_of_truncating() {
    let too_big = "a".repeat(MAX_INPUT_BYTES + 1);
    let issues = assert_err!(letter_paragraph(&too_big));
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].code, DiagnosticCode::InputLimit);
    let nested = format!("{}text{}", "*a **b ".repeat(20), "** c*".repeat(20));
    let issues = assert_err!(resume_fragment(ResumeFragmentKind::Paragraph, &nested));
    assert!(issues.iter().any(|issue| issue.code == DiagnosticCode::InputLimit));
    let images = "![x](x.png) ".repeat(600);
    let issues = assert_err!(resume_fragment(ResumeFragmentKind::Paragraph, &images));
    assert!(issues.len() <= super::super::MAX_DIAGNOSTICS + 1);
    assert!(issues.iter().any(|issue| issue.severity == Severity::Error));
}

#[test]
fn bom_cannot_hide_a_reference_definition() {
    let definition = "[id]: https://example.com";
    let text = format!("\u{feff}{definition}\n\ntext");
    for kind in [
        ResumeFragmentKind::Bullet,
        ResumeFragmentKind::Paragraph,
        ResumeFragmentKind::Blurb,
    ] {
        let issues = assert_err!(resume_fragment(kind, &text));
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == DiagnosticCode::SpecFragmentShape
                    && issue.decoded == (3..3 + definition.len()))
        );
    }
    let issues = assert_err!(letter_paragraph(&text));
    assert!(issues.iter().any(|issue| issue.decoded == (3..3 + definition.len())));
    assert_ok!(letter_paragraph("\u{feff}text"));
}

#[test]
fn placeholder_ranges_index_decoded_text_with_entities_bom_and_line_endings() {
    for prefix in ["é ", "\u{feff}é ", "é\r\n", "é\r", "é\n"] {
        let placeholder = "&#123;&#123;company}}";
        let text = format!("{prefix}{placeholder}");
        let lowered = assert_ok!(letter_paragraph(&text));
        let range = assert_some!(lowered.value.iter().find_map(|node| match node {
            LetterInline::Placeholder { range, .. } => Some(range.clone()),
            _ => None,
        }));
        assert_eq!(range, prefix.len()..text.len());
        assert_eq!(&text[range], placeholder);
        let text = format!("{prefix}{{{{unknown}}}}");
        let issues = assert_err!(letter_paragraph(&text));
        let issue = assert_some!(
            issues
                .iter()
                .find(|issue| issue.code == DiagnosticCode::PlaceholderUnknown)
        );
        assert_eq!(issue.decoded, prefix.len()..text.len());
    }
}

#[test]
fn warnings_before_a_suppressed_error_cannot_make_a_fragment_valid() {
    let text = format!("{}[bad](relative)", "text  \n".repeat(510));
    let issues = assert_err!(resume_fragment(ResumeFragmentKind::Paragraph, &text));
    assert_eq!(issues.len(), super::super::MAX_DIAGNOSTICS + 1);
    let last = assert_some!(issues.last());
    assert_eq!(last.code, DiagnosticCode::InputLimit);
    assert_eq!(last.severity, Severity::Error);
}

#[test]
fn both_apis_accept_the_size_boundary_and_reject_one_byte_over() {
    let mut text = "a".repeat(MAX_INPUT_BYTES);
    assert_ok!(resume_fragment(ResumeFragmentKind::Paragraph, &text));
    assert_ok!(letter_paragraph(&text));
    text.push('a');
    for issues in [
        assert_err!(resume_fragment(ResumeFragmentKind::Paragraph, &text)),
        assert_err!(letter_paragraph(&text)),
    ] {
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, DiagnosticCode::InputLimit);
    }
}

#[test]
fn comments_preserve_surrounding_visible_text_and_nested_annotations_fail() {
    let parsed = assert_ok!(resume_fragment(
        ResumeFragmentKind::Paragraph,
        "before <!-- ordinary --> after"
    ));
    assert_eq!(
        parsed.value,
        ResumeFragment::Paragraph(vec![Inline::Text {
            text: "before after".into()
        }])
    );
    let parsed = assert_ok!(letter_paragraph("before <!-- ordinary --> after"));
    assert_eq!(parsed.value, vec![LetterInline::Text("before  after".into())]);
    for text in [
        "[text <!-- @core -->](https://example.com)",
        "*text <!-- &id -->*",
        "<!-- ordinary --><!-- @core -->\n\ntext",
    ] {
        assert_err!(resume_fragment(ResumeFragmentKind::Paragraph, text));
        assert_err!(letter_paragraph(text));
    }
    for text in [
        "[text](https://example.com \"{{company}}\")",
        "[text](https://example.com/&#123;&#123;company}})",
    ] {
        let issues = assert_err!(letter_paragraph(text));
        assert!(
            issues
                .iter()
                .any(|issue| issue.code == DiagnosticCode::PlaceholderPosition)
        );
    }
}
