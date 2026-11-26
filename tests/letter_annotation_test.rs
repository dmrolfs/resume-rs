//! ABOUTME: Public contract for annotations in cover letters: shared grammar, section-bound groups, frame rules.
//! ABOUTME: Synthetic letters only; the parser keeps every wording and never chooses between them.
use claims::{assert_err, assert_ok, assert_some};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use resume::letter::{Item, LetterInline, LetterSource, SelectedLetter, SourceItem};
use resume::markdown::parse_letter;
use resume::model::{Annotations, Priority, Status, Tag};
use std::collections::BTreeSet;

/// A letter whose three sections hold the given text, each written as it would be in a file.
fn letter(greeting: &str, body: &str, closing: &str) -> String {
    format!("# Alex Example\n\n## Greeting\n\n{greeting}\n\n## Body\n\n{body}\n\n## Closing\n\n{closing}\n")
}
fn body(text: &str) -> String {
    letter("Dear Hiring Team,", text, "Sincerely,")
}
fn codes(input: &str) -> Vec<String> {
    parse_letter("letter.md", input)
        .diagnostics()
        .iter()
        .map(|d| d.code.to_string())
        .collect()
}
/// How many findings carry a code of the shared annotation family.
fn annotation_findings(input: &str) -> usize {
    codes(input)
        .iter()
        .filter(|c| c.starts_with("annotation-") || c.starts_with("alternate-"))
        .count()
}
fn valid(input: &str) -> LetterSource {
    let parsed = parse_letter("letter.md", input);
    assert_some!(parsed.valid(), "{:?}", parsed.diagnostics()).clone()
}
fn only_code(input: &str, code: &str) {
    assert_eq!(codes(input), vec![code.to_string()], "{input}");
}
fn variants(item: &SourceItem) -> Vec<&Item> {
    match item {
        SourceItem::Paragraph(item) => vec![item],
        SourceItem::Alternates(group) => group.variants().iter().collect(),
    }
}
fn text_of(item: &Item) -> String {
    item.piece
        .content
        .iter()
        .filter_map(|n| match n {
            LetterInline::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_trailing_comment_and_a_comment_line_above_say_the_same_thing() {
    let trailing = valid(&body("I lead platform work. <!-- @core #ai -->"));
    let above = valid(&body("<!-- @core #ai -->\nI lead platform work."));
    let expected = Annotations {
        priority: Some(Priority::Core),
        tags: BTreeSet::from([assert_ok!(Tag::try_new("ai"))]),
        ..Annotations::default()
    };
    for source in [&trailing, &above] {
        let [SourceItem::Paragraph(item)] = source.body.as_slice() else {
            panic!("one plain paragraph expected: {source:?}")
        };
        assert_eq!(item.annotations, expected);
        assert_eq!(text_of(item).trim(), "I lead platform work.");
    }
}

#[test]
fn the_annotation_range_is_the_comment_and_the_piece_range_is_the_paragraph() {
    for (text, comment) in [
        ("Words here. <!-- @extra -->", "<!-- @extra -->"),
        ("<!-- @extra -->\nWords here.", "<!-- @extra -->"),
        ("Words here. <!--\n@extra\n-->", "<!--\n@extra\n-->"),
    ] {
        let input = body(text);
        let source = valid(&input);
        let item = &variants(&source.body[0])[0];
        let found = assert_some!(item.annotation_range.clone());
        assert_eq!(&input[found], comment, "{text}");
        assert!(input[item.piece.range.clone()].contains("Words here."), "{text}");
    }
}

#[test]
fn an_ordinary_comment_is_still_ignored_and_marks_nothing() {
    let source = valid(&body("Words here. <!-- a note for me -->"));
    let item = &variants(&source.body[0])[0];
    assert!(item.annotations.is_empty());
    assert_eq!(item.annotation_range, None);
}

#[test]
fn ordinary_comments_are_ignored_wherever_they_stand() {
    for input in [
        body("<!-- a note -->\nWords here."),
        body("Words here.\n\n<!-- a note -->\n\nMore words."),
        letter("Hi. <!-- a note -->", "Body.", "Bye."),
        letter("<!-- a note -->\nHi.", "Body.", "<!-- a note -->\nBye."),
        body("Body.").replace("## Body", "<!-- a note -->\n## Body"),
        body("Body.").replace("# Alex Example", "# Alex Example <!-- a note -->"),
    ] {
        let source = valid(&input);
        for item in std::iter::once(&source.greeting)
            .chain(&source.body)
            .chain(std::iter::once(&source.closing))
        {
            assert!(
                variants(item)
                    .iter()
                    .all(|v| v.annotations.is_empty() && v.annotation_range.is_none()),
                "{input}"
            );
        }
    }
}

#[test]
fn placeholder_spans_survive_annotation_and_grouping() {
    let input = body(
        "<!-- =pitch.default @core -->\nApplying for {{title}} at {{company}}.\n\n<!-- =pitch.short @core -->\nI want {{title}}.",
    );
    let source = valid(&input);
    let [SourceItem::Alternates(group)] = source.body.as_slice() else {
        panic!("one group expected")
    };
    for variant in group.variants() {
        for node in &variant.piece.content {
            if let LetterInline::Placeholder { key, range } = node {
                assert_eq!(&input[range.clone()], format!("{{{{{key}}}}}"));
            }
        }
    }
    let spans = group
        .variants()
        .iter()
        .flat_map(|v| &v.piece.content)
        .filter(|n| matches!(n, LetterInline::Placeholder { .. }))
        .count();
    assert_eq!(spans, 3);
}

/// Each placeholder of `piece` as (key, the original text its range covers), with the paragraph's own start.
fn placeholders(input: &str, item: &Item) -> Vec<(String, String, usize)> {
    item.piece
        .content
        .iter()
        .filter_map(|n| match n {
            LetterInline::Placeholder { key, range } => Some((
                key.to_string(),
                input[range.clone()].to_string(),
                range.start - item.piece.range.start,
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn a_grouped_paragraph_has_the_same_text_and_placeholder_spans_as_the_plain_one() {
    let paragraph = "é &amp; ‘q’ Applying for {{title}} at &#123;&#123;company&#125;&#125;.";
    for ending in ["\n", "\r\n"] {
        for bom in ["", "\u{feff}"] {
            let plain_input = format!("{bom}{}", body(paragraph)).replace('\n', ending);
            let grouped_input = format!(
                "{bom}{}",
                body(&format!(
                    "{paragraph} <!-- =p.default @core -->\n\nSecond. <!-- =p.short @core -->"
                ))
            )
            .replace('\n', ending);
            let plain = valid(&plain_input);
            let grouped = valid(&grouped_input);
            let SourceItem::Alternates(group) = &grouped.body[0] else {
                panic!("a group expected")
            };
            let first = &group.variants()[0];
            let plain_item = variants(&plain.body[0])[0];
            assert_eq!(
                placeholders(&grouped_input, first),
                placeholders(&plain_input, plain_item),
                "{ending:?} {bom:?}"
            );
            assert_eq!(
                text_of(first).trim_end(),
                text_of(plain_item).trim_end(),
                "{ending:?} {bom:?}"
            );
            assert_eq!(placeholders(&plain_input, plain_item).len(), 2);
        }
    }
}

#[test]
fn a_group_keeps_every_wording_in_authored_order_even_when_default_is_not_first() {
    let source = valid(&body(
        "Short. <!-- =evidence.short @core -->\n\nDefault. <!-- =evidence.default @core -->\n\nLong. <!-- =evidence.long @core -->\n\nAfter.",
    ));
    assert_eq!(source.body.len(), 2);
    let SourceItem::Alternates(group) = &source.body[0] else {
        panic!("a group expected")
    };
    let names: Vec<String> = group
        .variants()
        .iter()
        .map(|v| assert_some!(v.annotations.alternate.clone()).variant.to_string())
        .collect();
    assert_eq!(names, ["short", "default", "long"]);
    assert_eq!(text_of(variants(&source.body[1])[0]), "After.");
}

#[test]
fn drafts_and_every_variant_are_kept_because_parsing_chooses_nothing() {
    let source = valid(&body(
        "Wording one. <!-- =a.default -->\n\nWording two. <!-- =a.other @draft -->\n\nOwn draft. <!-- @draft -->",
    ));
    let kept: Vec<&Item> = source.body.iter().flat_map(variants).collect();
    let statuses: Vec<Option<Status>> = kept.iter().map(|i| i.annotations.status).collect();
    assert_eq!(statuses, [None, Some(Status::Draft), Some(Status::Draft)]);
}

#[test]
fn no_paragraph_is_lost_between_the_text_and_the_model() {
    let input = letter(
        "Dear all, <!-- =hello.default -->\n\nHi all, <!-- =hello.informal -->",
        "One. <!-- @core -->\n\nTwo. <!-- =t.default -->\n\nThree. <!-- =t.b -->\n\nFour.",
        "Regards, <!-- =bye.default -->\n\nBest, <!-- =bye.short -->",
    );
    let source = valid(&input);
    let total: usize = std::iter::once(&source.greeting)
        .chain(&source.body)
        .chain(std::iter::once(&source.closing))
        .map(|i| variants(i).len())
        .sum();
    assert_eq!(total, 8);
}

#[test]
fn greeting_and_closing_hold_one_paragraph_or_one_group() {
    let source = valid(&letter(
        "Dear Hiring Team, <!-- =hello.default -->\n\nHello team, <!-- =hello.warm -->",
        "Body.",
        "Sincerely, <!-- =bye.default -->\n\nBest, <!-- =bye.short -->",
    ));
    assert_eq!(variants(&source.greeting).len(), 2);
    assert!(matches!(source.greeting, SourceItem::Alternates(_)));
    assert!(matches!(source.closing, SourceItem::Alternates(_)));
    let single = valid(&letter("Hello. <!-- =hello.default -->", "Body.", "Bye."));
    assert!(
        matches!(single.greeting, SourceItem::Alternates(_)),
        "a lone default is still a group"
    );
}

#[test]
fn two_unrelated_paragraphs_in_a_frame_are_still_an_error_and_never_dropped() {
    for input in [
        letter("One.\n\nTwo.", "Body.", "Bye."),
        letter("One. <!-- =a.default -->\n\nTwo.", "Body.", "Bye."),
        letter("Hi.", "Body.", "One. <!-- =a.default -->\n\nTwo. <!-- =b.default -->"),
    ] {
        assert!(
            codes(&input).contains(&"letter-section-shape".to_string()),
            "{input}: {:?}",
            codes(&input)
        );
    }
}

#[test]
fn a_frame_takes_alternates_only_and_nothing_else() {
    for annotation in ["@core", "@extra", "@draft", "@verify", "#ai", "@core #ai"] {
        for (greeting, closing) in [
            (format!("Hi. <!-- {annotation} -->"), "Bye.".to_string()),
            (format!("Hi. <!-- =a.default {annotation} -->"), "Bye.".to_string()),
            ("Hi.".to_string(), format!("<!-- {annotation} -->\nBye.")),
        ] {
            let input = letter(&greeting, "Body.", &closing);
            only_code(&input, "annotation-misplaced");
        }
    }
    valid(&letter("Hi. <!-- =a.default -->", "Body.", "Bye. <!-- =b.default -->"));
}

#[test]
fn a_body_paragraph_may_carry_priority_status_and_tags_but_the_group_must_agree() {
    valid(&body("One. <!-- @extra @verify #a #b -->"));
    only_code(
        &body("One. <!-- =x.default @core -->\n\nTwo. <!-- =x.short -->"),
        "alternate-priority-conflict",
    );
    valid(&body("One. <!-- =x.default @core -->\n\nTwo. <!-- =x.short @core -->"));
    valid(&body("One. <!-- =x.default -->\n\nTwo. <!-- =x.short -->"));
}

#[test]
fn group_rules_are_the_resume_rules_and_use_its_codes() {
    for (text, code) in [
        (
            "A. <!-- =x.short -->\n\nB. <!-- =x.long -->",
            "alternate-missing-default",
        ),
        (
            "A. <!-- =x.default -->\n\nB. <!-- =x.default -->",
            "alternate-variant-repeated",
        ),
        (
            "A. <!-- =x.default -->\n\nMiddle.\n\nB. <!-- =x.short -->",
            "alternate-not-adjacent",
        ),
        (
            "A. <!-- =x.default -->\n\nMiddle.\n\nB. <!-- =x.default -->",
            "alternate-group-reused",
        ),
        (
            "A. <!-- =x.default -->\n\nB. <!-- =y.default -->\n\nC. <!-- =x.short -->",
            "alternate-not-adjacent",
        ),
    ] {
        only_code(&body(text), code);
    }
}

/// The source text under each finding of `code`.
fn spans_of(input: &str, code: &str) -> Vec<String> {
    parse_letter("letter.md", input)
        .diagnostics()
        .iter()
        .filter(|d| d.code.to_string() == code)
        .map(|d| input[assert_some!(d.span.as_ref()).range.clone()].to_string())
        .collect()
}

#[test]
fn group_errors_point_at_the_comment_that_is_wrong() {
    let cases = [
        (
            "A. <!-- =x.default -->\n\nB. <!-- =x.default -->",
            "alternate-variant-repeated",
            "<!-- =x.default -->",
        ),
        (
            "A. <!-- =x.default -->\n\nM.\n\nB. <!-- =x.short -->",
            "alternate-not-adjacent",
            "<!-- =x.short -->",
        ),
        (
            "A. <!-- =x.default -->\n\nM.\n\nB. <!-- =x.default -->",
            "alternate-group-reused",
            "<!-- =x.default -->",
        ),
        (
            "A. <!-- =x.short -->\n\nB. <!-- =x.long -->",
            "alternate-missing-default",
            "<!-- =x.short -->",
        ),
        (
            "A. <!-- =x.default @core -->\n\nB. <!-- =x.s -->",
            "alternate-priority-conflict",
            "<!-- =x.s -->",
        ),
    ];
    for (text, code, comment) in cases {
        let input = body(text);
        assert_eq!(spans_of(&input, code), vec![comment.to_string()], "{code}");
    }
    // Across sections the later group, the closing's, is the one named.
    let input = letter("Hi.", "A. <!-- =x.default -->", "B. <!-- =x.default -->");
    let at = assert_some!(input.rfind("<!-- =x.default -->"));
    let parsed = parse_letter("letter.md", &input);
    let found = assert_some!(
        parsed
            .diagnostics()
            .iter()
            .find(|d| d.code.to_string() == "alternate-group-reused")
    );
    assert_eq!(assert_some!(found.span.as_ref()).range.start, at);
}

#[test]
fn a_group_cannot_cross_sections_and_names_are_unique_across_the_document() {
    // The body's last paragraph and the closing are different sections: not one group.
    only_code(
        &letter("Hi.", "A. <!-- =x.default -->", "B. <!-- =x.short -->"),
        "alternate-not-adjacent",
    );
    // The same name starting a group in two sections is a reused name.
    only_code(
        &letter("Hi.", "A. <!-- =x.default -->", "B. <!-- =x.default -->"),
        "alternate-group-reused",
    );
    // Greeting and closing sections also count, with a different name each.
    valid(&letter(
        "Hi. <!-- =g.default -->",
        "A. <!-- =x.default -->",
        "B. <!-- =c.default -->",
    ));
}

#[test]
fn the_shared_grammar_reports_its_own_codes_inside_a_letter() {
    for (text, code) in [
        ("A. <!-- @corr -->", "annotation-unknown-keyword"),
        ("A. <!-- #Bad -->", "annotation-malformed"),
        ("A. <!-- @core @extra -->", "annotation-conflict"),
        ("<!-- @core -->\n<!-- @extra -->\nA.", "annotation-duplicate"),
        // Without the comment line, `B.` would join the line above it, so the line may not stand there.
        ("A. <!-- @core -->\n<!-- @extra -->\nB.", "annotation-misplaced"),
        ("<!-- @core -->\nA. <!-- @extra -->", "annotation-duplicate"),
    ] {
        only_code(&body(text), code);
    }
}

#[test]
fn a_comment_line_that_marks_nothing_is_reported_where_it_stands() {
    only_code(&body("<!-- @core -->\n\nA."), "annotation-detached");
    only_code(&body("A.\n\n<!-- @core -->\n\n<!-- #ai -->\nB."), "annotation-detached");
    let at_end = "# Alex Example\n\n## Greeting\n\nHi.\n\n## Body\n\nA.\n\n## Closing\n\nBye.\n\n<!-- @core -->\n";
    only_code(at_end, "annotation-without-target");
}

#[test]
fn nothing_but_a_paragraph_takes_an_annotation() {
    let ok = body("A.");
    for (input, what) in [
        (
            ok.replace("Alex Example", "Alex Example <!-- @core -->"),
            "name trailing",
        ),
        (ok.replace("# Alex", "<!-- @core -->\n# Alex"), "name leading"),
        (ok.replace("## Body", "## Body <!-- @core -->"), "heading trailing"),
        (ok.replace("## Body", "<!-- @core -->\n## Body"), "heading leading"),
        (
            ok.replace("## Greeting", "alex@example.invalid <!-- @core -->\n\n## Greeting"),
            "contact trailing",
        ),
        (
            ok.replace("## Greeting", "<!-- @core -->\nalex@example.invalid\n\n## Greeting"),
            "contact leading",
        ),
        (ok.replace("A.", "A.\n\n<!-- #ai -->\n> quoted"), "above a quote"),
    ] {
        assert!(
            codes(&input).contains(&"annotation-misplaced".to_string()),
            "{what}: {:?}",
            codes(&input)
        );
        assert_eq!(annotation_findings(&input), 1, "{what}: {:?}", codes(&input));
    }
}

#[test]
fn an_annotation_in_the_middle_of_a_paragraph_or_inside_formatting_is_misplaced_once() {
    for text in [
        "A <!-- @core --> b.",
        "**A <!-- @core -->** b.",
        "[a <!-- @core -->](https://example.invalid) b.",
    ] {
        only_code(&body(text), "annotation-misplaced");
    }
}

#[test]
fn an_annotation_where_the_tree_cannot_show_it_fails_once_and_never_publishes() {
    for text in [
        "> A. <!-- @core -->",
        "- A. <!-- @core -->",
        "| a |\n|---|\n| A. <!-- @core --> |",
        "> <!--\n> @core\n> -->",
    ] {
        let input = body(text);
        let parsed = parse_letter("letter.md", &input);
        assert!(parsed.has_errors(), "{text}: {:?}", codes(&input));
        assert!(parsed.valid().is_none(), "{text}");
        assert_eq!(annotation_findings(&input), 1, "{text}: {:?}", codes(&input));
    }
}

#[test]
fn a_comment_shape_inside_link_metadata_is_not_a_comment_and_loses_nothing() {
    // Not a reference definition (no quoted title), so it is a paragraph that ends in a real annotation.
    let paragraph = valid(&body("[x]: https://example.invalid <!-- @core -->\n\nA."));
    assert_eq!(paragraph.body.len(), 2);
    // A title is metadata of the definition, not a comment: nothing is annotated and the next paragraph survives.
    let titled = valid(&body("[x]: https://example.invalid \"<!-- @core -->\"\n\nA."));
    assert_eq!(titled.body.len(), 1);
    assert!(variants(&titled.body[0])[0].annotations.is_empty());
}

#[test]
fn every_line_ending_and_a_bom_give_the_same_one_finding_for_a_hidden_annotation() {
    for ending in ["\n", "\r\n", "\r"] {
        for bom in ["", "\u{feff}"] {
            for text in ["> A. <!-- @core -->", "- A. <!-- @core -->", "A. <!-- @core --> b."] {
                let input = format!("{bom}{}", body(text)).replace('\n', ending);
                assert_eq!(
                    annotation_findings(&input),
                    1,
                    "{text:?} {ending:?} {bom:?}: {:?}",
                    codes(&input)
                );
            }
            let ok = format!("{bom}{}", body("A. <!-- @core -->")).replace('\n', ending);
            valid(&ok);
        }
    }
}

#[test]
fn an_unclosed_annotation_is_malformed_once_and_a_literal_one_is_just_text() {
    only_code(&body("A. <!-- @core"), "annotation-malformed");
    assert_eq!(codes(&body("A. <!-- @core")).len(), 1);
    for text in [r"A \<!-- @core -->", "A `<!-- @core -->`", "A &lt;!-- @core --&gt;"] {
        let source = valid(&body(text));
        assert!(variants(&source.body[0])[0].annotations.is_empty(), "{text}");
    }
}

#[test]
fn a_repeated_section_heading_does_not_rejoin_a_group_across_the_gap() {
    // The second Greeting is an error of its own; its paragraph must not join the first section's group, so each
    // lone variant is a group missing its `default` or sitting apart from it.
    let input = "# A\n\n## Greeting\n\nHi. <!-- =g.default -->\n\n## Greeting\n\nHey. <!-- =g.short -->\n\n## Body\n\nB.\n\n## Closing\n\nBye.\n";
    assert_eq!(
        codes(input),
        [
            "letter-duplicate-section",
            "letter-section-order",
            "letter-section-shape",
            "alternate-not-adjacent"
        ]
    );
    // The apart variant is the second one, which proves it did not rejoin the group before the gap.
    assert_eq!(spans_of(input, "alternate-not-adjacent"), ["<!-- =g.short -->"]);
}

#[test]
fn an_annotated_source_is_refused_by_the_unannotated_bridge_and_names_the_comment() {
    let plain = valid(&body("Plain."));
    assert_ok!(SelectedLetter::try_from(plain));
    for (text, comment) in [
        ("Draft. <!-- @draft -->", "<!-- @draft -->"),
        ("Core. <!-- @core -->", "<!-- @core -->"),
        ("Alone. <!-- =x.default -->", "<!-- =x.default -->"),
    ] {
        let input = body(text);
        let refused = assert_err!(SelectedLetter::try_from(valid(&input)));
        assert_eq!(&input[refused.range], comment, "{text}");
    }
    for input in [
        letter("Hi. <!-- =g.default -->", "A.", "Bye."),
        letter("Hi.", "A.", "Bye. <!-- =g.default -->"),
    ] {
        let refused = assert_err!(SelectedLetter::try_from(valid(&input)));
        assert_eq!(&input[refused.range], "<!-- =g.default -->");
    }
    // A group names its first wording's comment, and the earliest annotated section is the one named.
    let input = letter(
        "Hi. <!-- =g.default -->",
        "A. <!-- =b.default -->\n\nB. <!-- =b.two -->",
        "Bye.",
    );
    assert_eq!(
        &input[assert_err!(SelectedLetter::try_from(valid(&input))).range],
        "<!-- =g.default -->"
    );
    let input = body("A. <!-- =b.default -->\n\nB. <!-- =b.two -->");
    assert_eq!(
        &input[assert_err!(SelectedLetter::try_from(valid(&input))).range],
        "<!-- =b.default -->"
    );
}

#[test]
fn unannotated_letters_convert_with_every_paragraph_and_span_unchanged() {
    let input = body("One {{title}}.\n\nTwo.");
    let source = valid(&input);
    let selected = assert_ok!(SelectedLetter::try_from(source.clone()));
    assert_eq!(selected.body.len(), 2);
    for (kept, item) in selected.body.iter().zip(&source.body) {
        assert_eq!(kept, assert_some!(item.paragraph()));
    }
}

/// One body paragraph the generator writes, so the test knows exactly what the parser must hold.
#[derive(Debug, Clone)]
enum Written {
    Plain,
    Marked(&'static str),
    /// `n` adjacent variants of one group, the first of them the default.
    Group(usize),
}

fn written() -> impl Strategy<Value = Written> {
    prop_oneof![
        Just(Written::Plain),
        prop::sample::select(vec!["@core", "@extra", "@draft", "@verify", "#ai", "@core #ai"])
            .prop_map(Written::Marked),
        (1usize..5).prop_map(Written::Group),
    ]
}

proptest! {
    #[test]
    fn what_is_written_is_exactly_what_is_held(
        plan in prop::collection::vec(written(), 1..12),
        ending in prop::sample::select(vec!["\n", "\r\n"]),
        bom in prop::sample::select(vec!["", "\u{feff}"]),
    ) {
        let mut paragraphs = Vec::new();
        let mut groups = 0;
        let mut expected_items = 0;
        for (index, item) in plan.iter().enumerate() {
            match item {
                Written::Plain => { paragraphs.push(format!("Plain {index}.")); expected_items += 1; }
                Written::Marked(note) => { paragraphs.push(format!("Marked {index}. <!-- {note} -->")); expected_items += 1; }
                Written::Group(n) => {
                    groups += 1;
                    expected_items += 1;
                    for v in 0..*n {
                        let name = if v == 0 { "default".to_string() } else { format!("v{v}") };
                        paragraphs.push(format!("Group {index} wording {v}. <!-- =g{index}.{name} @core -->"));
                    }
                }
            }
        }
        let input = format!("{bom}{}", body(&paragraphs.join("\n\n"))).replace('\n', ending);
        let source = valid(&input);
        // Every paragraph written is held, in authored order, with no one invented or lost.
        let texts: Vec<String> = source.body.iter().flat_map(variants).map(|i| text_of(i).trim().to_string()).collect();
        let wanted: Vec<String> = paragraphs.iter().map(|p| p.split(" <!--").next().unwrap_or("").to_string()).collect();
        prop_assert_eq!(texts, wanted);
        prop_assert_eq!(source.body.len(), expected_items);
        prop_assert_eq!(source.body.iter().filter(|i| matches!(i, SourceItem::Alternates(_))).count(), groups);
        for item in source.body.iter().flat_map(variants) {
            prop_assert!(input.get(item.piece.range.clone()).is_some());
        }
    }

    #[test]
    fn arbitrary_annotated_fragments_keep_gating_spans_and_never_panic(
        parts in prop::collection::vec(prop::sample::select(vec![
            "Text.", " <!-- @core -->", " <!-- =g.default -->", " <!-- =g.alt -->", " <!-- #ai -->",
            "\n\n", "\n", "<!-- @draft -->", " <!-- @", " -->", "`", "\\", "> ", "- ", "{{company}}",
        ]), 1..40)
    ) {
        let input = body(&parts.concat());
        let parsed = parse_letter("letter.md", &input);
        prop_assert_eq!(parsed.valid().is_none(), parsed.has_errors());
        if let Some(source) = parsed.valid() {
            for item in std::iter::once(&source.greeting).chain(&source.body).chain(std::iter::once(&source.closing)) {
                for v in variants(item) {
                    prop_assert!(input.get(v.piece.range.clone()).is_some());
                    if let Some(range) = &v.annotation_range {
                        prop_assert!(input[range.clone()].starts_with("<!--"));
                    }
                }
            }
        }
        for d in parsed.diagnostics() {
            prop_assert!(input.get(assert_some!(d.span.as_ref()).range.clone()).is_some());
        }
    }
}

#[test]
fn many_groups_in_one_letter_parse_in_linear_time() {
    // The shared group check once compared every group and variant name with all the earlier ones, which a letter
    // at the input cap could drive to about a second and a half in a release build.
    let groups: String = (0..30_000).map(|i| format!("A <!-- =g{i}.default -->\n\n")).collect();
    let one_group: String = std::iter::once("A <!-- =g.default -->\n\n".to_string())
        .chain((0..30_000).map(|i| format!("A <!-- =g.v{i} -->\n\n")))
        .collect();
    for (name, text) in [("many groups", groups), ("one big group", one_group)] {
        let input = body(&text);
        let started = std::time::Instant::now();
        let parsed = parse_letter("letter.md", &input);
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "{name}: {} bytes took {elapsed:?}",
            input.len()
        );
        assert_some!(parsed.valid(), "{name}: {:?}", parsed.diagnostics().first());
    }
}

#[test]
fn a_hidden_annotation_is_located_exactly_under_every_ending_and_bom() {
    // Each of these sits where the tree cannot show it, so the second pass reports it, from offsets it computes
    // itself: they must point at the comment whatever the line endings and whether or not a BOM leads the file.
    for ending in ["\n", "\r\n", "\r"] {
        for bom in ["", "\u{feff}"] {
            for text in [
                "> A. <!-- @core -->",
                "> <!--\n> @core\n> -->",
                "- A. <!-- #ai -->",
                "> A. <!-- @core -->\n\nB.\n\n> C. <!-- #ai -->",
            ] {
                let input = format!("{bom}{}", body(text)).replace('\n', ending);
                let found = spans_of(&input, "annotation-misplaced");
                let comments = input.matches("<!--").count();
                assert_eq!(found.len(), comments, "{text:?} {ending:?} {bom:?}: {found:?}");
                for span in &found {
                    assert!(
                        span.starts_with("<!--") && span.ends_with("-->"),
                        "{text:?} {ending:?} {bom:?}: {span:?}"
                    );
                }
            }
            let reference =
                format!("{bom}{}", body("A.\n\n[x]: https://example.invalid/{{company}}")).replace('\n', ending);
            assert_eq!(
                spans_of(&reference, "placeholder-position"),
                ["[x]: https://example.invalid/{{company}}"],
                "{ending:?} {bom:?}"
            );
        }
    }
}

#[test]
fn only_a_second_level_heading_opens_a_section_and_a_second_name_is_refused() {
    assert_eq!(
        codes(&body("A.").replace("## Body", "### Body")),
        [
            "letter-section-shape",
            "letter-unsupported-block",
            "letter-section-order",
            "letter-missing-section"
        ]
    );
    assert_eq!(
        codes(&body("<!-- @core -->\n### Sub\n\nB.")),
        ["annotation-misplaced", "letter-unsupported-block"]
    );
    // A second name before any section, and one after the sections began, are each a duplicate.
    assert_eq!(
        codes("# One\n\n# Two\n\n## Greeting\n\nHi.\n\n## Body\n\nB.\n\n## Closing\n\nBye.\n"),
        ["duplicate-name"]
    );
    assert_eq!(
        codes(&body("B.").replace("## Closing", "# Two\n\n## Closing")),
        ["duplicate-name"]
    );
}

#[test]
fn each_hidden_comment_block_is_judged_on_its_own_and_an_empty_comment_hides_nothing_after_it() {
    for ending in ["\n", "\r\n", "\r"] {
        for bom in ["", "\u{feff}"] {
            let at = |text: &str| format!("{bom}{}", body(text)).replace('\n', ending);
            // Two separate hidden comment blocks: the second must not carry the first one over and report it twice.
            let two = at("> <!--\n> @core\n> -->\n\nB.\n\n> <!--\n> #ai\n> -->");
            assert_eq!(spans_of(&two, "annotation-misplaced").len(), 2, "{ending:?} {bom:?}");
            // An empty comment ends where it ends; the annotation right after it is still seen.
            for empty in ["<!-->", "<!--->"] {
                let after = at(&format!("> {empty}<!-- @core -->"));
                assert_eq!(
                    spans_of(&after, "annotation-misplaced"),
                    ["<!-- @core -->"],
                    "{empty} {ending:?} {bom:?}"
                );
            }
            // A comment in the middle of an HTML block's line is located at the comment, not at the line's start.
            let mid = at("<div> <!-- @core -->");
            assert_eq!(
                spans_of(&mid, "annotation-misplaced"),
                ["<!-- @core -->"],
                "{ending:?} {bom:?}"
            );
            // A placeholder in a link target is located exactly, past a BOM.
            let link = at("A [x](https://example.invalid/{{company}}) b.");
            assert_eq!(
                spans_of(&link, "placeholder-position"),
                ["[x](https://example.invalid/{{company}})"],
                "{ending:?} {bom:?}"
            );
        }
    }
}

// ---- `&id` item names in a letter (rsm-yjj.4) ------------------------------------------------------------

fn id_of(item: &SourceItem) -> Option<String> {
    variants(item)
        .first()
        .and_then(|v| v.annotations.id.as_ref().map(ToString::to_string))
}

#[test]
fn a_body_paragraph_takes_an_id_trailing_or_above() {
    let source = valid(&body(
        "One. <!-- &opening @core -->\n\n<!-- &evidence #ai -->\nTwo.\n\nThree.",
    ));
    let ids: Vec<Option<String>> = source.body.iter().map(id_of).collect();
    assert_eq!(ids, [Some("opening".to_string()), Some("evidence".to_string()), None]);
}

#[test]
fn an_id_is_refused_on_the_greeting_and_the_closing() {
    for (greeting, closing) in [
        ("Hi. <!-- &hello -->", "Bye."),
        ("<!-- &hello -->\nHi.", "Bye."),
        ("Hi.", "Bye. <!-- &bye -->"),
        ("Hi.", "<!-- &bye -->\nBye."),
    ] {
        let input = letter(greeting, "Body.", closing);
        only_code(&input, "annotation-misplaced");
    }
}

#[test]
fn an_id_may_not_repeat_in_one_letter() {
    only_code(&body("One. <!-- &same -->\n\nTwo. <!-- &same -->"), "item-id-reused");
}

#[test]
fn an_id_may_not_also_be_a_group_name_even_when_the_group_is_a_frame_group() {
    only_code(
        &letter(
            "Hi. <!-- =gateway.default -->\n\nHello. <!-- =gateway.short -->",
            "Body. <!-- &gateway -->",
            "Bye.",
        ),
        "item-id-reused",
    );
}

#[test]
fn an_id_on_a_body_variant_is_refused() {
    only_code(
        &body("One. <!-- =x.default &x -->\n\nTwo. <!-- =x.short -->"),
        "item-id-on-alternate",
    );
}

#[test]
fn a_letter_and_a_resume_may_use_the_same_id() {
    // Separate documents, separate namespaces: `[select]` and `[letter.select]` choose the document.
    valid(&body("One. <!-- &opening -->"));
    let resume = resume::markdown::parse("r.md", "# Ada\n\nLondon · a@b.co\n\n## S\n\n- A. <!-- &opening -->\n");
    assert!(resume.valid().is_some());
}

#[test]
fn an_id_adds_nothing_to_the_text_beyond_what_any_trailing_annotation_does() {
    // A trailing comment leaves the space before it in the text, for every annotation kind; the id is no different.
    let with_tag = valid(&body("One. <!-- #a -->\n\nTwo. <!-- #b -->"));
    let with_id = valid(&body("One. <!-- &a -->\n\nTwo. <!-- &b -->"));
    let text = |source: &LetterSource| source.body.iter().flat_map(variants).map(text_of).collect::<Vec<_>>();
    assert_eq!(text(&with_tag), text(&with_id));
    let above = valid(&body("<!-- &a -->\nOne.\n\n<!-- &b -->\nTwo."));
    let plain = valid(&body("One.\n\nTwo."));
    assert_eq!(
        text(&plain),
        text(&above),
        "an id on the line above leaves the text exactly as it was"
    );
}
