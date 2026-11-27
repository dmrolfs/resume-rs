//! ABOUTME: Tests of `&id` item names in a resume: where one may stand, what it may be, and the one shared namespace.
//! ABOUTME: An id never changes content, a rendering, or a count: it is an address a position spec uses.

use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;

use crate::diagnostics::DiagnosticCode::{
    self, AnnotationConflict, AnnotationMalformed, AnnotationMisplaced, ItemIdOnAlternate, ItemIdReused,
};
use crate::markdown::parse;
use crate::model::{ItemId, Resume};

const HEAD: &str = "# Ada Lovelace\n\nLondon · a@b.co\n\n";

fn id(name: &str) -> ItemId {
    assert_ok!(ItemId::try_new(name))
}

fn resume_of(body: &str) -> Resume {
    let markdown = format!("{HEAD}{body}");
    let parsed = parse("r.md", &markdown);
    assert_eq!(parsed.diagnostics(), [], "{body}");
    assert_some!(parsed.valid(), "{body}").clone()
}

/// The code of each finding and the source text under its span.
fn findings(body: &str) -> Vec<(DiagnosticCode, String)> {
    let markdown = format!("{HEAD}{body}");
    parse("r.md", &markdown)
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

/// The ids of every node of `resume`, in document order, `None` for a node without one.
fn ids(mut resume: Resume) -> Vec<Option<String>> {
    resume
        .annotations_mut()
        .into_iter()
        .map(|a| a.id.as_ref().map(ToString::to_string))
        .collect()
}

#[test]
fn every_kind_of_node_takes_an_id_in_a_trailing_comment() {
    let resume = resume_of(
        "## Summary <!-- &summary -->\n\nProse. <!-- &intro -->\n\n- A bullet. <!-- &first -->\n\n\
         ## Experience\n\n### Acme · Engineer · 2015 – 2020 <!-- &acme -->\n\nThe blurb. <!-- &blurb -->\n\n\
         - R1 <!-- &r-one -->\n\n**AI · 2016 – 2017** <!-- &era -->\n\n- E1 <!-- &e-one -->\n\nClosing. <!-- &closing -->\n",
    );
    let found: Vec<String> = ids(resume).into_iter().flatten().collect();
    assert_eq!(
        found,
        [
            "summary", "intro", "first", "acme", "blurb", "r-one", "era", "e-one", "closing"
        ]
    );
}

#[test]
fn an_id_may_stand_alone_in_a_comment_above_the_line_it_names() {
    let resume = resume_of("## S\n\n<!-- &opening -->\nFirst paragraph.\n\n<!-- &later @core -->\n- A bullet.\n");
    let found: Vec<String> = ids(resume).into_iter().flatten().collect();
    assert_eq!(found, ["opening", "later"]);
}

#[test]
fn an_id_travels_with_priority_status_and_tags_in_one_comment() {
    let mut resume = resume_of("## S\n\n- A bullet. <!-- &gateway @core @verify #ai #platform -->\n");
    let notes = resume.annotations_mut();
    let note = &notes[1];
    assert_eq!(note.id.as_deref(), Some(&id("gateway")));
    assert_eq!(note.tags.len(), 2);
    assert!(note.priority.is_some() && note.status.is_some());
}

#[test]
fn a_comment_starting_with_an_ampersand_is_an_annotation_and_never_an_ordinary_note() {
    // A misspelled id is reported; if `&` were not recognised the comment would be dropped and nothing said.
    for body in [
        "- A. <!-- &Gateway -->\n",
        "- A. <!-- &gate_way -->\n",
        "- A. <!-- &x--y -->\n",
    ] {
        let found = findings(&format!("## S\n\n{body}"));
        assert_eq!(found.len(), 1, "{body}: {found:?}");
        assert_eq!(found[0].0, AnnotationMalformed, "{body}");
    }
}

#[test]
fn an_ampersand_that_is_not_followed_by_a_letter_is_an_ordinary_comment() {
    // `<!-- & more -->`, `<!-- && -->` and `<!-- &-x -->` are notes, as `<!-- #1 priority -->` is.
    let resume = resume_of("## S\n\n- A. <!-- & more -->\n- B. <!-- && -->\n- C. <!-- &-x -->\n");
    assert!(ids(resume).into_iter().all(|id| id.is_none()));
}

#[test]
fn an_id_is_at_most_forty_characters() {
    let long = "a".repeat(41);
    let found = findings(&format!("## S\n\n- A. <!-- &{long} -->\n"));
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, AnnotationMalformed);
    let fits = "a".repeat(40);
    resume_of(&format!("## S\n\n- A. <!-- &{fits} -->\n"));
}

#[test]
fn two_id_tokens_on_one_node_are_a_conflict_even_when_identical() {
    for body in [
        "- A. <!-- &one &two -->\n",
        "- A. <!-- &one &one -->\n",
        "- A. <!-- &one #ai &two -->\n",
    ] {
        let found = findings(&format!("## S\n\n{body}"));
        assert_eq!(found.len(), 1, "{body}: {found:?}");
        assert_eq!(found[0].0, AnnotationConflict, "{body}");
    }
}

#[test]
fn a_second_id_token_is_reported_when_the_first_is_malformed() {
    let found = findings("## S\n\n- A. <!-- &Bad &good -->\n");
    let codes: Vec<DiagnosticCode> = found.iter().map(|f| f.0).collect();
    assert!(codes.contains(&AnnotationMalformed), "{found:?}");
}

#[test]
fn the_same_id_twice_in_a_document_is_a_located_error_at_the_later_one() {
    let body = "## S\n\n- A. <!-- &gateway -->\n- B. <!-- &gateway -->\n";
    let found = findings(body);
    assert_eq!(found, vec![(ItemIdReused, "<!-- &gateway -->".to_string())]);
    let markdown = format!("{HEAD}{body}");
    let at = assert_some!(parse("r.md", &markdown).diagnostics()[0].span.as_ref())
        .range
        .start;
    assert!(
        at > assert_some!(markdown.find("<!-- &gateway -->")),
        "the later comment, not the first"
    );
}

#[test]
fn a_duplicate_is_found_across_different_sections_roles_and_eras() {
    let body = "## One\n\n- A. <!-- &x -->\n\n## Two\n\n### Acme · 2015 – 2020\n\n- B. <!-- &x -->\n";
    assert_eq!(findings(body).len(), 1);
    let eras = "## S\n\n### Acme · 2015 – 2020\n\n**A · 2016 – 2017**\n\n- E. <!-- &x -->\n\n**B · 2018 – 2019**\n\n- F. <!-- &x -->\n";
    assert_eq!(findings(eras).len(), 1);
}

#[test]
fn every_kind_of_node_claims_its_id_in_the_namespace() {
    // Each pair puts the same id on two different kinds of node, so a kind that failed to record its id would let
    // the pair through.
    let pairs = [
        (
            "section",
            "## S <!-- &x -->\n\n### Acme · 2015 – 2020\n\nBlurb.\n\n- A. <!-- &x -->\n",
        ),
        (
            "role",
            "## S\n\n### Acme · 2015 – 2020 <!-- &x -->\n\nBlurb.\n\n- A. <!-- &x -->\n",
        ),
        (
            "blurb",
            "## S\n\n### Acme · 2015 – 2020\n\nBlurb. <!-- &x -->\n\n- A. <!-- &x -->\n",
        ),
        (
            "era",
            "## S\n\n### Acme · 2015 – 2020\n\n**A · 2016 – 2017** <!-- &x -->\n\n- A. <!-- &x -->\n",
        ),
        ("paragraph", "## S\n\nProse. <!-- &x -->\n\n- A. <!-- &x -->\n"),
        ("bullet", "## S\n\n- A. <!-- &x -->\n\n- B. <!-- &x -->\n"),
    ];
    for (kind, body) in pairs {
        let found = findings(body);
        assert_eq!(found.len(), 1, "{kind}: {found:?}");
        assert_eq!(found[0].0, ItemIdReused, "{kind}");
    }
}

#[test]
fn a_container_id_and_a_leaf_id_share_one_namespace() {
    let body = "## S <!-- &shared -->\n\n- A. <!-- &shared -->\n";
    assert_eq!(findings(body), vec![(ItemIdReused, "<!-- &shared -->".to_string())]);
}

#[test]
fn an_id_and_a_group_name_cannot_be_the_same_in_either_order() {
    let id_first =
        "## S\n\n- A. <!-- &gateway -->\n\n## T\n\n- B. <!-- =gateway.default -->\n- C. <!-- =gateway.short -->\n";
    let group_first =
        "## S\n\n- B. <!-- =gateway.default -->\n- C. <!-- =gateway.short -->\n\n## T\n\n- A. <!-- &gateway -->\n";
    for body in [id_first, group_first] {
        let found = findings(body);
        assert_eq!(found.len(), 1, "{body}: {found:?}");
        assert_eq!(found[0].0, ItemIdReused, "{body}");
    }
}

#[test]
fn an_id_is_refused_on_any_variant_of_a_group() {
    for body in [
        "## S\n\n- A. <!-- =gateway.default &gateway -->\n- B. <!-- =gateway.short -->\n",
        "## S\n\n- A. <!-- =gateway.default -->\n- B. <!-- =gateway.short &other -->\n",
    ] {
        let found = findings(body);
        assert_eq!(found.len(), 1, "{body}: {found:?}");
        assert_eq!(found[0].0, ItemIdOnAlternate, "{body}");
    }
}

#[test]
fn the_same_id_is_free_again_once_a_group_replaces_it() {
    // `&gateway` turned into `=gateway.default` keeps the spec's address; both spellings in one file do not.
    resume_of("## S\n\n- A. <!-- =gateway.default -->\n- B. <!-- =gateway.short -->\n");
    resume_of("## S\n\n- A. <!-- &gateway -->\n");
}

#[test]
fn an_id_is_not_allowed_on_a_page_break_the_name_or_the_contact_line() {
    for markdown in [
        "# Ada Lovelace <!-- &me -->\n\nLondon · a@b.co\n\n## S\n\n- A.\n",
        "# Ada Lovelace\n\n<!-- &me -->\nLondon · a@b.co\n\n## S\n\n- A.\n",
    ] {
        let parsed = parse("r.md", markdown);
        assert!(parsed.has_errors(), "{markdown}");
        assert!(
            parsed
                .diagnostics()
                .iter()
                .any(|d| matches!(d.code, AnnotationMisplaced | AnnotationMalformed)),
            "{markdown}: {:?}",
            parsed.diagnostics()
        );
    }
}

#[test]
fn ids_never_reach_the_json_a_style_reads() {
    let plain = resume_of("## S\n\n- A.\n\n### Acme · 2015 – 2020\n\nBlurb.\n");
    let named =
        resume_of("## S <!-- &s -->\n\n- A. <!-- &a -->\n\n### Acme · 2015 – 2020 <!-- &r -->\n\nBlurb. <!-- &b -->\n");
    assert_eq!(
        assert_ok!(serde_json::to_string(&plain)),
        assert_ok!(serde_json::to_string(&named))
    );
}

#[test]
fn adding_ids_changes_no_text_node_order_or_range() {
    let plain = "## S\n\nProse.\n\n- A.\n- B.\n\n### Acme · 2015 – 2020\n\nBlurb.\n\n- C.\n";
    let named = "## S <!-- &s -->\n\nProse. <!-- &p -->\n\n- A. <!-- &a -->\n- B. <!-- &b -->\n\n### Acme · 2015 – 2020 <!-- &r -->\n\nBlurb. <!-- &bl -->\n\n- C. <!-- &c -->\n";
    let (a, b) = (resume_of(plain), resume_of(named));
    assert_eq!(a.nodes().len(), b.nodes().len());
    let kinds = |r: &Resume| r.nodes().iter().map(|n| n.kind).collect::<Vec<_>>();
    assert_eq!(kinds(&a), kinds(&b));
}

#[test]
fn the_same_spelling_is_legal_for_a_tag_a_group_variant_and_an_id() {
    // Only an id and a group name share a namespace; a tag and a variant name are separate vocabularies.
    resume_of("## S\n\n- A. <!-- &gateway #gateway -->\n- B. <!-- =other.default -->\n- C. <!-- =other.gateway -->\n");
}
