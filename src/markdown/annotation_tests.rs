//! ABOUTME: Tests of how annotations attach to the resume: each position, both placements, and the error cases.
//! ABOUTME: Each test compares the annotated parse with the plain one, so an annotation never changes content.

use std::collections::BTreeSet;

use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;

use crate::diagnostics::{Diagnostic, DiagnosticCode};
use crate::markdown::parse;
use crate::model::{Alternate, Annotations, Block, GroupName, Priority, Resume, RoleBlock, Status, Tag, VariantName};

const HEAD: &str = "# Ada Lovelace\n\nLondon · a@b.co\n\n";

fn parsed(markdown: &str) -> (Resume, Vec<Diagnostic>) {
    let (resume, diagnostics) = parse("r.md", markdown).into_parts();
    (assert_some!(resume, "{markdown}"), diagnostics)
}

/// The resume of a document that must parse with no findings at all.
fn clean(markdown: &str) -> Resume {
    let (resume, diagnostics) = parsed(markdown);
    assert_eq!(diagnostics, vec![], "{markdown}");
    resume
}

/// The code of each finding and the source text under its span.
fn findings(markdown: &str) -> Vec<(DiagnosticCode, String)> {
    parse("r.md", markdown)
        .diagnostics()
        .iter()
        .map(|d| {
            (
                d.code,
                markdown[assert_some!(d.span.as_ref()).range.clone()].to_string(),
            )
        })
        .collect()
}

/// `resume` with every annotation cleared, to compare with the parse of the document without them.
fn stripped(mut resume: Resume) -> Resume {
    for slot in resume.annotations_mut() {
        *slot = Annotations::default();
    }
    resume
}

/// How many nodes of `resume` carry an annotation.
fn annotated_count(mut resume: Resume) -> usize {
    resume.annotations_mut().into_iter().filter(|a| !a.is_empty()).count()
}

/// Asserts `annotated` parses cleanly to the same resume as `plain`, apart from the annotations.
fn same_content(annotated: &str, plain: &str) -> Resume {
    let resume = clean(annotated);
    assert_eq!(stripped(resume.clone()), clean(plain), "{annotated}");
    resume
}

fn tags(names: &[&str]) -> BTreeSet<Tag> {
    names.iter().map(|n| assert_ok!(Tag::try_new(*n))).collect()
}

fn alternate(group: &str, variant: &str) -> Option<Alternate> {
    Some(Alternate {
        group: assert_ok!(GroupName::try_new(group)),
        variant: assert_ok!(VariantName::try_new(variant)),
    })
}

fn role(resume: &Resume) -> &crate::model::Role {
    match &resume.sections[0].blocks[0] {
        Block::Role(role) => role,
        other => panic!("expected a role, got {other:?}"),
    }
}

// ---- inline placement: the annotation marks its own line ----------------------------------------------

#[test]
fn an_inline_annotation_marks_its_own_bullet() {
    let resume = same_content(
        &format!("{HEAD}## S\n\n- Led the gateway. <!-- =gateway.default @core #ai -->\n- Built it.\n"),
        &format!("{HEAD}## S\n\n- Led the gateway.\n- Built it.\n"),
    );
    let Block::Bullets { items } = &resume.sections[0].blocks[0] else {
        panic!()
    };
    assert_eq!(
        items[0].annotations,
        Annotations {
            priority: Some(Priority::Core),
            status: None,
            tags: tags(&["ai"]),
            alternate: alternate("gateway", "default"),
            id: None,
        }
    );
    assert!(items[1].annotations.is_empty(), "the next bullet is not annotated");
}

#[test]
fn an_inline_annotation_marks_a_labeled_bullet_and_keeps_the_label() {
    let resume = same_content(
        &format!("{HEAD}## S\n\n- **Languages:** Rust, Go <!-- #ai -->\n"),
        &format!("{HEAD}## S\n\n- **Languages:** Rust, Go\n"),
    );
    let Block::Bullets { items } = &resume.sections[0].blocks[0] else {
        panic!()
    };
    assert_eq!(items[0].annotations.tags, tags(&["ai"]));
}

#[test]
fn an_inline_annotation_marks_a_section_heading() {
    let resume = same_content(
        &format!("{HEAD}## Patents <!-- @extra #ip -->\n\nText.\n"),
        &format!("{HEAD}## Patents\n\nText.\n"),
    );
    assert_eq!(resume.sections[0].annotations.priority, Some(Priority::Extra));
    assert_eq!(resume.sections[0].annotations.tags, tags(&["ip"]));
}

#[test]
fn a_role_line_with_an_inline_annotation_keeps_its_company_title_and_period() {
    let resume = same_content(
        &format!("{HEAD}## S\n\n### Acme · Staff Engineer · Aug 2017 – Present <!-- #ai @core -->\n\n- Did it.\n"),
        &format!("{HEAD}## S\n\n### Acme · Staff Engineer · Aug 2017 – Present\n\n- Did it.\n"),
    );
    let role = role(&resume);
    assert_eq!(role.company.as_ref(), "Acme");
    assert_eq!(role.annotations.priority, Some(Priority::Core));
    assert_eq!(role.annotations.tags, tags(&["ai"]));
}

#[test]
fn an_era_line_with_an_inline_annotation_keeps_its_name_and_period() {
    let body =
        "### Acme · Engineer · 2015 – 2020\n\n**Platform · 2016 – 2018** <!-- #platform @verify -->\n\n- Did it.\n";
    let resume = same_content(
        &format!("{HEAD}## S\n\n{body}"),
        &format!("{HEAD}## S\n\n{}", body.replace(" <!-- #platform @verify -->", "")),
    );
    let RoleBlock::Era {
        name,
        annotations,
        items,
        ..
    } = &role(&resume).body[0]
    else {
        panic!()
    };
    assert_eq!(name.as_ref(), "Platform");
    assert_eq!(annotations.status, Some(Status::Verify));
    assert_eq!(annotations.tags, tags(&["platform"]));
    assert!(items[0].annotations.is_empty());
}

#[test]
fn an_inline_annotation_marks_a_blurb_and_a_later_paragraph_separately() {
    let body =
        "### Acme · Engineer · 2015 – 2020\n\nThe blurb. <!-- =acme.default -->\n\nLater prose. <!-- @draft -->\n";
    let resume = same_content(
        &format!("{HEAD}## S\n\n{body}"),
        &format!("{HEAD}## S\n\n### Acme · Engineer · 2015 – 2020\n\nThe blurb.\n\nLater prose.\n"),
    );
    let role = role(&resume);
    assert_eq!(role.blurb_annotations.alternate, alternate("acme", "default"));
    assert!(role.annotations.is_empty(), "the blurb's annotation is not the role's");
    let RoleBlock::Paragraph { annotations, .. } = &role.body[0] else {
        panic!()
    };
    assert_eq!(annotations.status, Some(Status::Draft));
}

#[test]
fn an_inline_annotation_marks_a_section_paragraph() {
    let resume = same_content(
        &format!("{HEAD}## Summary\n\nI build platforms. <!-- =summary.default #platform -->\n"),
        &format!("{HEAD}## Summary\n\nI build platforms.\n"),
    );
    let Block::Paragraph { annotations, .. } = &resume.sections[0].blocks[0] else {
        panic!()
    };
    assert_eq!(annotations.alternate, alternate("summary", "default"));
}

#[test]
fn an_inline_annotation_at_the_end_of_a_wrapped_paragraph_marks_the_whole_paragraph() {
    let resume = same_content(
        &format!("{HEAD}## Summary\n\nI build\nplatforms. <!-- #platform -->\n"),
        &format!("{HEAD}## Summary\n\nI build\nplatforms.\n"),
    );
    let Block::Paragraph { annotations, .. } = &resume.sections[0].blocks[0] else {
        panic!()
    };
    assert_eq!(annotations.tags, tags(&["platform"]));
}

#[test]
fn an_ordinary_comment_is_still_dropped_and_annotates_nothing() {
    let resume = same_content(
        &format!("{HEAD}## S <!-- a note -->\n\n- Led it. <!-- core, says who -->\n"),
        &format!("{HEAD}## S\n\n- Led it.\n"),
    );
    assert!(resume.sections[0].annotations.is_empty());
}

#[test]
fn an_ordinary_comment_after_an_annotation_still_lets_it_end_the_line() {
    let resume = same_content(
        &format!("{HEAD}## S\n\n- Led it. <!-- #ai --> <!-- note -->\n"),
        &format!("{HEAD}## S\n\n- Led it.\n"),
    );
    let Block::Bullets { items } = &resume.sections[0].blocks[0] else {
        panic!()
    };
    assert_eq!(items[0].annotations.tags, tags(&["ai"]));
}

// ---- inline placement: errors ---------------------------------------------------------------------------

#[test]
fn an_annotation_in_the_middle_of_a_line_is_misplaced() {
    use DiagnosticCode::AnnotationMisplaced;
    for (line, comment) in [
        ("- Led <!-- #ai --> the gateway.", "<!-- #ai -->"),
        ("- [Led <!-- #ai -->](https://a.example) it.", "<!-- #ai -->"),
        ("Some <!-- @core --> prose.", "<!-- @core -->"),
    ] {
        assert_eq!(
            findings(&format!("{HEAD}## S\n\n{line}\n")),
            vec![(AnnotationMisplaced, comment.to_string())],
            "{line}"
        );
    }
}

#[test]
fn an_annotation_inside_bold_is_misplaced_whatever_else_the_bold_is_reported_for() {
    let found = findings(&format!("{HEAD}## S\n\n- **Led <!-- #ai -->** the gateway.\n"));
    assert!(
        found.contains(&(DiagnosticCode::AnnotationMisplaced, "<!-- #ai -->".to_string())),
        "{found:?}"
    );
}

#[test]
fn an_annotation_at_the_end_of_an_earlier_line_of_a_paragraph_is_misplaced() {
    assert_eq!(
        findings(&format!("{HEAD}## S\n\nI build <!-- #ai -->\nplatforms.\n")),
        vec![(DiagnosticCode::AnnotationMisplaced, "<!-- #ai -->".to_string())]
    );
}

#[test]
fn two_inline_annotations_on_one_line_are_a_duplicate() {
    assert_eq!(
        findings(&format!("{HEAD}## S\n\n- Led it. <!-- #ai --> <!-- @core -->\n")),
        vec![(DiagnosticCode::AnnotationDuplicate, "<!-- @core -->".to_string())]
    );
}

#[test]
fn an_annotation_on_the_name_or_the_contact_line_is_misplaced() {
    use DiagnosticCode::AnnotationMisplaced;
    assert_eq!(
        findings("# Ada Lovelace <!-- @core -->\n\nLondon\n\n## S\n\nText.\n"),
        vec![(AnnotationMisplaced, "<!-- @core -->".to_string())]
    );
    assert_eq!(
        findings("# Ada Lovelace\n\nLondon · a@b.co <!-- #ai -->\n\n## S\n\nText.\n"),
        vec![(AnnotationMisplaced, "<!-- #ai -->".to_string())]
    );
}

#[test]
fn a_bullet_that_is_only_an_annotation_is_misplaced() {
    assert_eq!(
        findings(&format!("{HEAD}## S\n\n- <!-- #ai -->\n- Real.\n")),
        vec![(DiagnosticCode::AnnotationMisplaced, "<!-- #ai -->".to_string())]
    );
}

#[test]
fn an_alternate_on_a_heading_role_line_or_era_line_is_not_allowed_and_the_rest_is_kept() {
    use DiagnosticCode::AlternateNotAllowed;
    let section = format!("{HEAD}## Patents <!-- =p.default #ip -->\n\nText.\n");
    assert_eq!(
        findings(&section),
        vec![(AlternateNotAllowed, "<!-- =p.default #ip -->".to_string())]
    );
    let (resume, _) = parsed(&section);
    assert_eq!(resume.sections[0].annotations.tags, tags(&["ip"]));
    assert_eq!(resume.sections[0].annotations.alternate, None);

    let role_line = format!("{HEAD}## S\n\n### Acme · Engineer · 2015 – 2017 <!-- =acme.short -->\n\n- Did.\n");
    assert_eq!(
        findings(&role_line),
        vec![(AlternateNotAllowed, "<!-- =acme.short -->".to_string())]
    );

    let era = format!(
        "{HEAD}## S\n\n### Acme · Engineer · 2015 – 2020\n\n**AI · 2016 – 2017** <!-- =ai.default -->\n\n- Did.\n"
    );
    assert_eq!(
        findings(&era),
        vec![(AlternateNotAllowed, "<!-- =ai.default -->".to_string())]
    );
}

#[test]
fn a_bad_token_is_located_inside_the_comment_of_its_line() {
    assert_eq!(
        findings(&format!("{HEAD}## S\n\n- Led it. <!-- @cor #ai -->\n")),
        vec![(DiagnosticCode::AnnotationUnknownKeyword, "@cor".to_string())]
    );
}

#[test]
fn an_annotation_in_a_second_paragraph_of_a_bullet_is_misplaced() {
    let found: Vec<DiagnosticCode> = findings(&format!("{HEAD}## S\n\n- Led it.\n\n  More. <!-- #ai -->\n"))
        .into_iter()
        .map(|(c, _)| c)
        .collect();
    assert_eq!(
        found,
        vec![DiagnosticCode::UnsupportedMarkdown, DiagnosticCode::AnnotationMisplaced]
    );
}

// ---- full-line placement: the annotation marks the line directly below it --------------------------------

#[test]
fn a_full_line_annotation_marks_the_section_heading_below_it() {
    let resume = same_content(
        &format!("{HEAD}## S\n\nText.\n\n<!-- @extra #ip -->\n## Patents\n\nOne.\n"),
        &format!("{HEAD}## S\n\nText.\n\n## Patents\n\nOne.\n"),
    );
    assert!(resume.sections[0].annotations.is_empty());
    assert_eq!(resume.sections[1].annotations.priority, Some(Priority::Extra));
}

#[test]
fn a_full_line_annotation_above_a_list_marks_only_its_first_bullet() {
    let resume = same_content(
        &format!("{HEAD}## S\n\n<!-- #ai -->\n- A\n- B\n"),
        &format!("{HEAD}## S\n\n- A\n- B\n"),
    );
    let Block::Bullets { items } = &resume.sections[0].blocks[0] else {
        panic!()
    };
    assert_eq!(items[0].annotations.tags, tags(&["ai"]));
    assert!(items[1].annotations.is_empty());
}

#[test]
fn a_full_line_annotation_between_bullets_marks_the_next_one_and_keeps_one_list() {
    for (annotated, plain) in [
        (
            format!("{HEAD}## S\n\n- A\n<!-- #ai -->\n- B\n- C\n"),
            format!("{HEAD}## S\n\n- A\n- B\n- C\n"),
        ),
        (
            format!("{HEAD}## S\n\n### Acme · Eng · 2015 – 2017\n\n- A\n<!-- #ai -->\n- B\n"),
            format!("{HEAD}## S\n\n### Acme · Eng · 2015 – 2017\n\n- A\n- B\n"),
        ),
        (
            format!("{HEAD}## S\n\n### Acme · Eng · 2015 – 2017\n\n**AI · 2016 – 2017**\n\n- A\n<!-- #ai -->\n- B\n"),
            format!("{HEAD}## S\n\n### Acme · Eng · 2015 – 2017\n\n**AI · 2016 – 2017**\n\n- A\n- B\n"),
        ),
    ] {
        let resume = same_content(&annotated, &plain);
        let items = match &resume.sections[0].blocks[0] {
            Block::Bullets { items } => items.clone(),
            Block::Role(role) => match &role.body[0] {
                RoleBlock::Bullets { items } | RoleBlock::Era { items, .. } => items.clone(),
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        };
        let marked: Vec<bool> = items.iter().map(|b| !b.annotations.is_empty()).collect();
        assert_eq!(&marked[..2], &[false, true], "{annotated}");
        assert!(
            marked[2..].iter().all(|m| !m),
            "only the bullet below the comment: {annotated}"
        );
    }
}

#[test]
fn a_full_line_annotation_marks_a_role_its_era_its_blurb_and_a_nested_bullet() {
    let annotated = format!(
        "{HEAD}## S\n\n<!-- #ai -->\n### Acme · Eng · 2015 – 2020\n<!-- =acme.default -->\nThe blurb.\n\n\
         <!-- #data -->\n**Data · 2016 – 2018**\n\n- A\n  <!-- @extra -->\n  - A1\n"
    );
    let plain =
        format!("{HEAD}## S\n\n### Acme · Eng · 2015 – 2020\nThe blurb.\n\n**Data · 2016 – 2018**\n\n- A\n  - A1\n");
    let (resume, diagnostics) = parsed(&annotated);
    let unexpected: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code != DiagnosticCode::NestedList)
        .collect();
    assert!(unexpected.is_empty(), "{unexpected:?}");
    assert_eq!(stripped(resume.clone()), parsed(&plain).0);
    let role = role(&resume);
    assert_eq!(role.annotations.tags, tags(&["ai"]));
    assert_eq!(role.blurb_annotations.alternate, alternate("acme", "default"));
    let RoleBlock::Era { annotations, items, .. } = &role.body[0] else {
        panic!()
    };
    assert_eq!(annotations.tags, tags(&["data"]));
    assert!(items[0].annotations.is_empty());
    assert_eq!(items[1].annotations.priority, Some(Priority::Extra));
}

#[test]
fn a_full_line_annotation_works_with_crlf_line_endings() {
    let resume = same_content(
        &format!("{HEAD}## S\n\n<!-- #ai -->\n- A\n").replace('\n', "\r\n"),
        &format!("{HEAD}## S\n\n- A\n").replace('\n', "\r\n"),
    );
    let Block::Bullets { items } = &resume.sections[0].blocks[0] else {
        panic!()
    };
    assert_eq!(items[0].annotations.tags, tags(&["ai"]));
}

#[test]
fn a_full_line_annotation_may_span_lines() {
    let resume = same_content(
        &format!("{HEAD}## S\n\n<!-- @core\n     #ai #platform -->\n- A\n"),
        &format!("{HEAD}## S\n\n- A\n"),
    );
    let Block::Bullets { items } = &resume.sections[0].blocks[0] else {
        panic!()
    };
    assert_eq!(items[0].annotations.tags, tags(&["ai", "platform"]));
    assert_eq!(items[0].annotations.priority, Some(Priority::Core));
}

// ---- full-line placement: errors --------------------------------------------------------------------------

fn only(markdown: &str, code: DiagnosticCode, under: &str) {
    assert_eq!(findings(markdown), vec![(code, under.to_string())], "{markdown}");
}

#[test]
fn a_blank_line_or_an_ordinary_comment_detaches_an_annotation() {
    only(
        &format!("{HEAD}## S\n\n<!-- #ai -->\n\n- A\n"),
        DiagnosticCode::AnnotationDetached,
        "<!-- #ai -->",
    );
    only(
        &format!("{HEAD}<!-- @extra -->\n\n## P\n\nOne.\n"),
        DiagnosticCode::AnnotationDetached,
        "<!-- @extra -->",
    );
    only(
        &format!("{HEAD}## S\n\n<!-- #ai -->\n<!-- a note -->\n- A\n"),
        DiagnosticCode::AnnotationDetached,
        "<!-- #ai -->",
    );
}

#[test]
fn two_annotations_for_one_item_are_a_duplicate_and_never_merged() {
    use DiagnosticCode::AnnotationDuplicate;
    only(
        &format!("{HEAD}## S\n\n<!-- #ai -->\n<!-- @core -->\n- A\n"),
        AnnotationDuplicate,
        "<!-- #ai -->",
    );
    only(
        &format!("{HEAD}## S\n\n<!-- #ai -->\n- A <!-- @core -->\n"),
        AnnotationDuplicate,
        "<!-- @core -->",
    );
    only(
        &format!("{HEAD}## S\n\n<!-- #ai --> <!-- @core -->\n- A\n"),
        AnnotationDuplicate,
        "<!-- @core -->",
    );
    let (resume, _) = parsed(&format!("{HEAD}## S\n\n<!-- #ai -->\n- A <!-- @core -->\n"));
    let Block::Bullets { items } = &resume.sections[0].blocks[0] else {
        panic!()
    };
    assert_eq!(
        items[0].annotations.priority, None,
        "the second annotation is not merged in"
    );
}

#[test]
fn an_annotation_with_nothing_after_it_has_no_target() {
    use DiagnosticCode::AnnotationWithoutTarget;
    only(
        &format!("{HEAD}## S\n\n- A\n\n<!-- #ai -->\n"),
        AnnotationWithoutTarget,
        "<!-- #ai -->",
    );
    // Indented under a bullet, the comment belongs to that bullet, not to the one after it.
    only(
        &format!("{HEAD}## S\n\n- A\n  <!-- #ai -->\n- B\n"),
        AnnotationWithoutTarget,
        "<!-- #ai -->",
    );
    let hints = parse("r.md", &format!("{HEAD}## S\n\n- A\n  <!-- #ai -->\n- B\n")).diagnostics()[0]
        .hints
        .clone();
    assert!(
        hints
            .iter()
            .any(|h| h.contains("start the comment where that bullet's `-` starts")),
        "{hints:?}"
    );
}

#[test]
fn an_annotation_above_the_name_the_contact_line_or_a_page_break_is_misplaced() {
    use DiagnosticCode::AnnotationMisplaced;
    only(
        "<!-- @core -->\n# Ada\n\nLondon\n\n## S\n\nText.\n",
        AnnotationMisplaced,
        "<!-- @core -->",
    );
    only(
        "# Ada\n<!-- #ai -->\nLondon\n\n## S\n\nText.\n",
        AnnotationMisplaced,
        "<!-- #ai -->",
    );
    only(
        &format!("{HEAD}## S\n\nText.\n\n<!-- #ai -->\n<div class=\"page-break\"></div>\n\n## T\n\nMore.\n"),
        AnnotationMisplaced,
        "<!-- #ai -->",
    );
}

#[test]
fn an_annotation_line_that_would_split_text_is_misplaced() {
    use DiagnosticCode::AnnotationMisplaced;
    only(
        &format!("{HEAD}## S\n\nFirst line\n<!-- #ai -->\nsecond line.\n"),
        AnnotationMisplaced,
        "<!-- #ai -->",
    );
    only(
        &format!("{HEAD}## S\n\n- A\n<!-- #ai -->\nlazy line.\n"),
        AnnotationMisplaced,
        "<!-- #ai -->",
    );
}

#[test]
fn an_annotation_indented_four_spaces_is_code_not_an_annotation() {
    let found = findings(&format!("{HEAD}## S\n\n    <!-- #ai -->\n- B\n"));
    assert_eq!(
        found.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
        vec![DiagnosticCode::UnsupportedMarkdown]
    );
}

// ---- property: annotations never change content ----------------------------------------------------------

/// The full fixture, the plain parse to compare with.
fn full_fixture() -> String {
    assert_ok!(std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/full.md"
    )))
}

/// True for a line of `full.md` that starts an annotatable node: a heading, role line, bullet, era line, or
/// paragraph line that does not continue the line above it.
fn starts_a_node(line: &str, previous: &str) -> bool {
    let continues = !previous.trim().is_empty() && !previous.starts_with('#') && !previous.starts_with("<!--");
    match line {
        l if l.starts_with("## ") || l.starts_with("### ") || l.starts_with("- ") => true,
        l if l.trim().is_empty() || l.starts_with('#') || l.starts_with('<') => false,
        _ => !continues,
    }
}

/// Asserts `annotated` parses to the plain fixture plus exactly `inserted` annotations, with the same findings.
fn annotates_everything(annotated: &str, inserted: usize) {
    assert!(inserted >= 20, "the generator annotated only {inserted} nodes");
    let plain = parse("r.md", &full_fixture());
    let (resume, diagnostics) = parse("r.md", annotated).into_parts();
    let codes = |d: &[Diagnostic]| d.iter().map(|d| d.code).collect::<Vec<_>>();
    assert_eq!(codes(&diagnostics), codes(plain.diagnostics()), "{annotated}");
    let resume = assert_some!(resume);
    assert_eq!(annotated_count(resume.clone()), inserted, "every annotation attached");
    assert_eq!(stripped(resume), assert_some!(plain.best_effort()).clone());
}

#[test]
fn annotating_every_node_of_the_full_fixture_inline_changes_nothing_else() {
    let fixture = full_fixture();
    let mut annotated = String::new();
    let mut inserted = 0;
    let lines: Vec<&str> = fixture.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        annotated.push_str(line);
        // Inline, an annotation ends a node's last line: the next line is blank, a comment, or starts a node.
        // Lines up to the contact line (line 6) take none.
        let next = lines.get(index + 1).copied().unwrap_or("");
        let ends_node = next.trim().is_empty() || next.starts_with('<') || starts_a_node(next, line);
        if index > 5 && !line.trim().is_empty() && !line.starts_with('<') && ends_node {
            annotated.push_str(" <!-- @core #tagged -->");
            inserted += 1;
        }
        annotated.push('\n');
    }
    annotates_everything(&annotated, inserted);
}

#[test]
fn annotating_every_node_of_the_full_fixture_from_the_line_above_changes_nothing_else() {
    let fixture = full_fixture();
    let mut annotated = String::new();
    let mut inserted = 0;
    let mut previous = "";
    for (index, line) in fixture.lines().enumerate() {
        // After the contact line (line 6); a node right under text would join it, so those are left alone.
        let under_text = !previous.trim().is_empty() && !previous.starts_with('#') && !previous.starts_with("- ");
        if index > 5 && starts_a_node(line, previous) && !under_text {
            annotated.push_str("<!-- @extra #above -->\n");
            inserted += 1;
        }
        annotated.push_str(line);
        annotated.push('\n');
        previous = line;
    }
    annotates_everything(&annotated, inserted);
}

// ---- alternate groups ---------------------------------------------------------------------------------------

#[test]
fn adjacent_variants_with_a_default_are_one_valid_group() {
    let markdown = format!(
        "{HEAD}## S\n\n- Led the gateway. <!-- =gateway.default @core -->\n- Built it. <!-- =gateway.short @core -->\n\
         - Gave leaders control. <!-- =gateway.business @core -->\n- Other.\n"
    );
    let parsed = parse("r.md", &markdown);
    assert_eq!(parsed.diagnostics().iter().map(|d| d.code).collect::<Vec<_>>(), vec![]);
    let outline = assert_some!(parsed.outline());
    assert_eq!(outline.groups.len(), 1);
    assert_eq!(outline.groups[0].len(), 3);
    assert_eq!(outline.groups[0][1].trimmed(), "Built it.");
}

#[test]
fn variants_may_mix_placements_and_be_paragraphs_or_a_blurb() {
    let markdown = format!(
        "{HEAD}## Summary\n\nI build platforms. <!-- =summary.default -->\n\n<!-- =summary.short -->\nPlatforms.\n\n\
         ## X\n\n### Acme · Eng · 2015 – 2017\n\nThe blurb. <!-- =acme.default -->\n\nShort blurb. <!-- =acme.short -->\n"
    );
    let parsed = parse("r.md", &markdown);
    assert_eq!(parsed.diagnostics().iter().map(|d| d.code).collect::<Vec<_>>(), vec![]);
    assert_eq!(assert_some!(parsed.outline()).groups.len(), 2);
}

#[test]
fn each_group_rule_is_a_located_error_on_the_offending_variant() {
    use DiagnosticCode::*;
    let cases: &[(&str, (DiagnosticCode, &str))] = &[
        (
            "- A. <!-- =g.long -->\n- B. <!-- =g.short -->\n",
            (AlternateMissingDefault, "<!-- =g.long -->"),
        ),
        (
            "- A. <!-- =g.default -->\n- B. <!-- =g.default -->\n",
            (AlternateVariantRepeated, "<!-- =g.default -->"),
        ),
        (
            "- A. <!-- =g.default @core -->\n- B. <!-- =g.short @extra -->\n",
            (AlternatePriorityConflict, "<!-- =g.short @extra -->"),
        ),
        (
            "- A. <!-- =g.default -->\n- Other.\n- B. <!-- =g.short -->\n",
            (AlternateNotAdjacent, "<!-- =g.short -->"),
        ),
        (
            "- A. <!-- =g.default -->\n- Other.\n- B. <!-- =g.default -->\n",
            (AlternateGroupReused, "<!-- =g.default -->"),
        ),
    ];
    for (body, (code, under)) in cases {
        let markdown = format!("{HEAD}## S\n\n{body}");
        let found = findings(&markdown);
        let expected = (*code, under.to_string());
        assert_eq!(found.len(), 1, "{body}: {found:?}");
        assert_eq!(found[0].0, expected.0, "{body}");
        let span_text = &found[0].1;
        assert_eq!(span_text, &expected.1, "{body}");
        // The finding points at the later comment when the comment text repeats.
        if *code == AlternateVariantRepeated || *code == AlternateGroupReused {
            let at = assert_some!(parse("r.md", &markdown).diagnostics()[0].span.as_ref())
                .range
                .start;
            assert!(
                at > assert_some!(markdown.find(under)),
                "{body}: points at the second one"
            );
        }
    }
}

#[test]
fn a_group_never_spans_containers_or_a_page_break() {
    use DiagnosticCode::AlternateNotAdjacent;
    let across_roles = format!(
        "{HEAD}## S\n\n### Acme · Eng · 2015 – 2017\n\n- A. <!-- =g.default -->\n\n### Beta · Eng · 2013 – 2015\n\n- B. <!-- =g.short -->\n"
    );
    assert_eq!(
        findings(&across_roles),
        vec![(AlternateNotAdjacent, "<!-- =g.short -->".to_string())]
    );
    let across_eras = format!(
        "{HEAD}## S\n\n### Acme · Eng · 2015 – 2020\n\n**AI · 2016 – 2017**\n\n- A. <!-- =g.default -->\n\n**Data · 2018 – 2019**\n\n- B. <!-- =g.short -->\n"
    );
    assert_eq!(
        findings(&across_eras),
        vec![(AlternateNotAdjacent, "<!-- =g.short -->".to_string())]
    );
    let across_sections = format!("{HEAD}## S\n\n- A. <!-- =g.default -->\n\n## T\n\n- B. <!-- =g.short -->\n");
    assert_eq!(
        findings(&across_sections),
        vec![(AlternateNotAdjacent, "<!-- =g.short -->".to_string())]
    );
    let across_a_break =
        format!("{HEAD}## S\n\nA. <!-- =g.default -->\n\n<div class=\"page-break\"></div>\n\nB. <!-- =g.short -->\n");
    assert_eq!(
        findings(&across_a_break),
        vec![(AlternateNotAdjacent, "<!-- =g.short -->".to_string())]
    );
}

#[test]
fn a_group_name_is_unique_in_the_whole_document() {
    let markdown = format!(
        "{HEAD}## S\n\n- A. <!-- =g.default -->\n- A2. <!-- =g.short -->\n\n## T\n\n- B. <!-- =g.default -->\n- B2. <!-- =g.short -->\n"
    );
    let found = findings(&markdown);
    assert_eq!(
        found.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
        vec![DiagnosticCode::AlternateGroupReused]
    );
}

// ---- review fixes: ordinary comments stay ordinary, nothing is dropped, boundaries hold ---------------------

#[test]
fn a_commented_out_section_a_banner_or_a_numbered_note_stays_an_ordinary_comment() {
    for comment in [
        "<!--\n## Hobbies\n- Chess\n-->",
        "<!-- ===== Old roles ===== -->",
        "<!-- #1 priority for Q3 -->",
        "<!-- # heading -->",
    ] {
        let markdown = format!("{HEAD}## S\n\n{comment}\n\n- A\n");
        assert_eq!(findings(&markdown), vec![], "{comment}");
        if comment.contains('\n') {
            continue; // a comment over several lines cannot end a bullet's line
        }
        let inline = format!("{HEAD}## S\n\n- A {comment}\n");
        let (resume, diagnostics) = parsed(&inline);
        assert_eq!(
            diagnostics.iter().map(|d| d.code).collect::<Vec<_>>(),
            vec![],
            "{comment}"
        );
        assert_eq!(stripped(resume.clone()), resume, "{comment}: no annotation attached");
    }
}

#[test]
fn an_annotation_on_a_line_that_shows_nothing_is_reported_not_dropped() {
    use DiagnosticCode::{AnnotationMisplaced, Image};
    for (markdown, expected) in [
        (
            format!("{HEAD}## S\n\n- ![pic](a.png) <!-- #x -->\n"),
            [Image, AnnotationMisplaced],
        ),
        (
            format!("{HEAD}## S\n\n<!-- #x -->\n- ![pic](a.png)\n"),
            [AnnotationMisplaced, Image],
        ),
        (
            format!("{HEAD}## S\n\n![pic](a.png) <!-- #x -->\n"),
            [Image, AnnotationMisplaced],
        ),
    ] {
        let found: Vec<DiagnosticCode> = findings(&markdown).into_iter().map(|(c, _)| c).collect();
        assert_eq!(found, expected, "{markdown}");
    }
}

#[test]
fn a_variant_on_a_line_that_shows_nothing_cannot_hide_a_repeated_variant() {
    let found = findings(&format!(
        "{HEAD}## S\n\n- A <!-- =g.default -->\n- ![p](a.png) <!-- =g.default -->\n"
    ));
    assert!(
        found.iter().any(|(c, _)| *c == DiagnosticCode::AnnotationMisplaced),
        "{found:?}"
    );
}

#[test]
fn a_group_never_spans_an_empty_era_or_a_role_line() {
    use DiagnosticCode::AlternateNotAdjacent;
    let empty_era = format!(
        "{HEAD}## S\n\n### Acme · Engineer · 2015 – 2017\n\nBlurb.\n\n- A <!-- =g.default -->\n\n**Data · 2016 – 2017**\n\nLater. <!-- =g.short -->\n"
    );
    assert_eq!(
        findings(&empty_era),
        vec![(AlternateNotAdjacent, "<!-- =g.short -->".to_string())]
    );
    let empty_role = format!(
        "{HEAD}## S\n\nIntro. <!-- =g.default -->\n\n### Acme · Engineer · 2015 – 2017\n\n## T\n\nLater. <!-- =g.short -->\n"
    );
    assert_eq!(
        findings(&empty_role),
        vec![(AlternateNotAdjacent, "<!-- =g.short -->".to_string())]
    );
}

#[test]
fn every_variant_states_the_same_priority_or_none_does() {
    use DiagnosticCode::AlternatePriorityConflict;
    only(
        &format!("{HEAD}## S\n\n- A. <!-- =g.default -->\n- B. <!-- =g.short @extra -->\n"),
        AlternatePriorityConflict,
        "<!-- =g.short @extra -->",
    );
    only(
        &format!("{HEAD}## S\n\n- A. <!-- =g.default @core -->\n- B. <!-- =g.short -->\n"),
        AlternatePriorityConflict,
        "<!-- =g.short -->",
    );
    assert_eq!(
        findings(&format!(
            "{HEAD}## S\n\n- A. <!-- =g.default @core -->\n- B. <!-- =g.short @core -->\n"
        )),
        vec![]
    );
}

#[test]
fn the_variants_of_a_group_are_all_bullets_or_all_paragraphs() {
    only(
        &format!("{HEAD}## S\n\nPara. <!-- =g.default -->\n\n- Bullet. <!-- =g.short -->\n"),
        DiagnosticCode::AlternateNotAdjacent,
        "<!-- =g.short -->",
    );
}

#[test]
fn a_parent_bullet_and_its_child_are_never_variants_of_one_group() {
    let markdown = format!("{HEAD}## S\n\n- A. <!-- =g.default -->\n  - A1. <!-- =g.short -->\n");
    let found: Vec<DiagnosticCode> = findings(&markdown).into_iter().map(|(c, _)| c).collect();
    assert_eq!(
        found,
        vec![DiagnosticCode::NestedList, DiagnosticCode::AlternateNotAdjacent]
    );
    // Two children of one parent are a group.
    let siblings = format!("{HEAD}## S\n\n- A.\n  - A1. <!-- =g.default -->\n  - A2. <!-- =g.short -->\n");
    let found: Vec<DiagnosticCode> = findings(&siblings).into_iter().map(|(c, _)| c).collect();
    assert_eq!(found, vec![DiagnosticCode::NestedList]);
}

#[test]
fn an_annotation_line_between_nested_bullets_keeps_one_nested_list_warning() {
    let annotated = format!("{HEAD}## S\n\n- A\n  - A1\n  <!-- #x -->\n  - A2\n");
    let plain = format!("{HEAD}## S\n\n- A\n  - A1\n  - A2\n");
    let codes = |m: &str| findings(m).into_iter().map(|(c, _)| c).collect::<Vec<_>>();
    assert_eq!(codes(&annotated), codes(&plain));
    assert_eq!(stripped(parsed(&annotated).0), parsed(&plain).0);
}

#[test]
fn a_column_zero_annotation_line_above_a_nested_bullet_is_misplaced_not_a_rejoin() {
    let markdown = format!("{HEAD}## S\n\n- A 1\n<!-- #ai -->\n  - A1 1\n");
    let found: Vec<DiagnosticCode> = findings(&markdown).into_iter().map(|(c, _)| c).collect();
    assert!(found.contains(&DiagnosticCode::AnnotationMisplaced), "{found:?}");
}

#[test]
fn bullets_with_different_markers_stay_two_lists_with_or_without_an_annotation_line() {
    same_content(
        &format!("{HEAD}## S\n\n- A\n<!-- #x -->\n* B\n"),
        &format!("{HEAD}## S\n\n- A\n* B\n"),
    );
}

#[test]
fn crlf_line_endings_keep_every_placement_error() {
    let crlf = |m: String| m.replace('\n', "\r\n");
    only(
        &crlf(format!("{HEAD}## S\n\n<!-- #ai -->\n\n- A\n")),
        DiagnosticCode::AnnotationDetached,
        "<!-- #ai -->",
    );
    only(
        &crlf(format!("{HEAD}## S\n\nFirst\n<!-- #ai -->\nsecond.\n")),
        DiagnosticCode::AnnotationMisplaced,
        "<!-- #ai -->",
    );
    let resume = clean(&crlf(format!(
        "{HEAD}## S\n\n- A <!-- =g.default -->\n- B <!-- =g.short -->\n"
    )));
    let Block::Bullets { items } = &resume.sections[0].blocks[0] else {
        panic!()
    };
    assert_eq!(items[1].annotations.alternate, alternate("g", "short"));
}

#[test]
fn an_annotation_above_an_unsupported_block_is_misplaced() {
    let found: Vec<DiagnosticCode> = findings(&format!("{HEAD}## S\n\n<!-- #ai -->\n> quoted\n"))
        .into_iter()
        .map(|(c, _)| c)
        .collect();
    assert_eq!(
        found,
        vec![DiagnosticCode::AnnotationMisplaced, DiagnosticCode::UnsupportedMarkdown]
    );
}

#[test]
fn an_annotation_in_the_middle_of_a_role_or_era_line_is_misplaced() {
    let role_line = format!("{HEAD}## S\n\n### Acme <!-- #ai --> · Eng · 2015 – 2017\n\n- Did.\n");
    let found: Vec<DiagnosticCode> = findings(&role_line).into_iter().map(|(c, _)| c).collect();
    assert!(found.contains(&DiagnosticCode::AnnotationMisplaced), "{found:?}");
    let era = format!("{HEAD}## S\n\n### Acme · Eng · 2015 – 2020\n\n**AI <!-- #ai --> · 2016 – 2017**\n\n- Did.\n");
    let found: Vec<DiagnosticCode> = findings(&era).into_iter().map(|(c, _)| c).collect();
    assert!(found.contains(&DiagnosticCode::AnnotationMisplaced), "{found:?}");
}

#[test]
fn an_invalid_group_gets_no_facts_lint_even_when_its_numbers_differ() {
    let markdown = format!("{HEAD}## S\n\n- Served 5 users. <!-- =g.long -->\n- Served 6 users. <!-- =g.short -->\n");
    let parsed = parse("r.md", &markdown);
    let lints = crate::doctor::content::lint_parsed(&parsed, std::path::Path::new("r.md"));
    assert!(
        !lints.iter().any(|d| d.code == DiagnosticCode::AlternateFactsDiffer),
        "{lints:?}"
    );
}

// ---- node lines (rsm-141.22.3): the parser's lines line up with `Resume::nodes` ----------------------

#[test]
fn every_node_has_its_own_start_line_in_model_order() {
    let markdown = format!(
        "{HEAD}## Summary\n\nIntro.\n\n- S1\n- S2\n\n## Experience\n\n### Acme · Engineer · 2015 – 2020\n\nThe blurb.\n\n\
         - R1\n  - N1\n- R2\n\n**AI · 2016 – 2017**\n\n- E1\n\nClosing.\n\n<div class=\"page-break\"></div>\n\n\
         ### Beta · 2010 – 2014\n\n- B1\n<!-- #x -->\n- B2\n"
    );
    let parsed = parse("r.md", &markdown);
    let resume = assert_some!(parsed.valid());
    let lines = parsed.node_lines(&markdown);
    let nodes = resume.nodes();
    assert_eq!(lines.len(), nodes.len());
    let source: Vec<&str> = markdown.lines().collect();
    let first_words: Vec<String> = nodes
        .iter()
        .zip(&lines)
        .map(|(node, line)| {
            format!(
                "{} {}",
                node.kind,
                source[line - 1].trim_start_matches(['#', '-', ' ', '*'])
            )
        })
        .collect();
    assert_eq!(
        first_words,
        vec![
            "section Summary",
            "paragraph Intro.",
            "bullet S1",
            "bullet S2",
            "section Experience",
            "role Acme · Engineer · 2015 – 2020",
            "blurb The blurb.",
            "bullet R1",
            "bullet N1",
            "bullet R2",
            "era AI · 2016 – 2017**",
            "bullet E1",
            "paragraph Closing.",
            "role Beta · 2010 – 2014",
            "bullet B1",
            "bullet B2",
        ]
    );
}

#[test]
fn node_lines_name_the_survivors_after_a_dropped_role_in_lf_and_crlf() {
    let lf = format!("{HEAD}## S\n\n- A\n\n### Acme · bad · role\n\n- Lost\n\n## T\n\n- B\n");
    for markdown in [lf.clone(), lf.replace('\n', "\r\n")] {
        let parsed = parse("r.md", &markdown);
        let resume = assert_some!(parsed.best_effort());
        let lines = parsed.node_lines(&markdown);
        let source: Vec<&str> = markdown.lines().collect();
        let named: Vec<String> = resume
            .nodes()
            .iter()
            .zip(&lines)
            .map(|(node, line)| {
                format!(
                    "{} {line} {}",
                    node.kind,
                    source[line - 1].trim_start_matches(['#', '-', ' '])
                )
            })
            .collect();
        assert_eq!(
            named,
            ["section 5 S", "bullet 7 A", "section 13 T", "bullet 15 B"],
            "{markdown:?}"
        );
    }
}
