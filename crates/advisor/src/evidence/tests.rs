//! Synthetic evidence-policy tests. No private baseline enters this repository.
use super::*;
use claims::{assert_err, assert_ok};
use pretty_assertions::assert_eq;

const HEAD: &str = "# Ada Lovelace\n\nLondon · ada@example.com\n\n";

fn fixture(body: &str) -> Evidence {
    assert_ok!(project(&format!("{HEAD}{body}")))
}
fn texts(e: &Evidence) -> Vec<&str> {
    e.records.iter().map(|r| r.record.text.as_str()).collect()
}

#[test]
fn full_plain_text_keeps_group_ids_labels_and_visible_links_not_destinations() {
    let e = fixture(
        "## Skills\n\n- **Languages:** *Rust*, `Java`, [Scala](https://private.example). <!-- =languages.default -->\n\n## Education\n\nB.S. in Electrical Engineering. <!-- &degree -->\n",
    );
    assert_eq!(
        texts(&e),
        ["Languages: Rust, Java, Scala.", "B.S. in Electrical Engineering."]
    );
    assert_eq!(e.records[0].record.id, "languages.default");
    assert_eq!(e.records[1].record.id, format!("src:{}:node:3", e.source_id));
    assert_eq!(e.records[1].provenance.item.as_deref(), Some("degree"));
    let bytes = assert_ok!(e.model_artifact());
    let json = String::from_utf8_lossy(bytes.bytes());
    for omitted in [
        "Ada Lovelace",
        "ada@example.com",
        "private.example",
        "byte_range",
        "node_index",
        "excluded",
        "provenance",
        "degree",
    ] {
        assert!(!json.contains(omitted), "leaked {omitted}: {json}");
    }
}

#[test]
fn blurb_group_variants_are_context_even_if_the_default_is_a_draft() {
    let e = fixture(
        "## Experience\n\n### Acme · Engineer · 2015 – 2020\n\nCompany description. <!-- =company.default @draft -->\n\nShort description. <!-- =company.short -->\n\nAn achievement. <!-- =achievement.default -->\n\n- Delivered it.\n",
    );
    assert_eq!(
        texts(&e),
        ["Acme · Engineer · 2015 – 2020", "An achievement.", "Delivered it."]
    );
    let blurbs: Vec<_> = e
        .context
        .iter()
        .filter(|r| r.record.kind == ContextKind::RoleDescription)
        .collect();
    assert_eq!(blurbs.len(), 1);
    assert_eq!(blurbs[0].record.text, "Short description.");
    assert_eq!(blurbs[0].record.group.as_deref(), Some("company"));
    assert_eq!(e.excluded.len(), 1);
}

#[test]
fn section_role_era_and_nested_status_exclude_descendants_not_siblings() {
    for status in ["draft", "verify"] {
        let e = fixture(&format!(
            "## Experience\n\n### Acme · Engineer · 2015 – 2020\n\n- Parent. <!-- @{status} -->\n  - Child.\n    - Grandchild.\n- Sibling.\n\n**Era · 2016 – 2017** <!-- @{status} -->\n\n- Era child.\n\nReady paragraph.\n\n### Beta · 2010 – 2014 <!-- @{status} -->\n\n- Role child.\n\n## Hidden <!-- @{status} -->\n\n- Section child.\n\n## Visible\n\nReady.\n"
        ));
        assert_eq!(
            texts(&e),
            [
                "Acme · Engineer · 2015 – 2020",
                "Sibling.",
                "Ready paragraph.",
                "Ready."
            ]
        );
        assert_eq!(e.excluded.len(), 9);
        let child = &e.excluded[1];
        assert_eq!(child.inherited.as_ref().map(|b| b.node_index), Some(2));
        assert_eq!(child.provenance.source_parent_node, Some(2));
    }
}

#[test]
fn dropped_image_only_bullet_cannot_break_an_exclusion_chain() {
    let e = fixture("## S\n\n- Parent. <!-- @draft -->\n  - ![](x.png)\n    - Hidden child.\n- Ready sibling.\n");
    assert_eq!(texts(&e), ["Ready sibling."]);
    assert_eq!(e.excluded.len(), 2);
}

#[test]
fn draft_paragraph_and_blurb_do_not_exclude_following_siblings() {
    let e = fixture(
        "## S\n\nExcluded. <!-- @draft -->\n\nIncluded.\n\n## Experience\n\n### Acme · 2015 – 2020\n\nDescription. <!-- @verify -->\n\n- Ready.\n",
    );
    assert_eq!(texts(&e), ["Included.", "Acme · 2015 – 2020", "Ready."]);
}

#[test]
fn priorities_and_tags_do_not_change_eligibility() {
    let e = fixture("## S\n\n- Core. <!-- @core #ai -->\n- Extra. <!-- @extra #other -->\n- Plain.\n");
    assert_eq!(texts(&e), ["Core.", "Extra.", "Plain."]);
}

#[test]
fn eras_keep_periods_and_a_following_paragraph_clears_the_era() {
    let e = fixture(
        "## Experience\n\n### Acme · Engineer · 2015 – 2020\n\n**AI · 2016 – 2017**\n\n- First.\n\nClosing.\n\n- Outside.\n\n**AI · 2018 – 2019**\n\n- Second.\n",
    );
    let eras: Vec<_> = e
        .context
        .iter()
        .filter(|r| r.record.kind == ContextKind::Era)
        .map(|r| r.record.text.as_str())
        .collect();
    assert_eq!(eras, ["AI · 2016 – 2017", "AI · 2018 – 2019"]);
    assert!(e.records[2].record.parents.era.is_none());
    assert!(e.records[3].record.parents.era.is_none());
    assert_ne!(e.records[1].record.parents.era, e.records[4].record.parents.era);
}

#[test]
fn source_identity_is_exact_bytes_and_generated_ids_are_baseline_indexes() {
    let text = format!("{HEAD}## S\n\n- Excluded. <!-- @draft -->\n- Same.\n- Same.\n");
    let a = assert_ok!(project(&text));
    let b = assert_ok!(project(&text));
    assert_eq!(a, b);
    assert_eq!(a.records[0].record.id, format!("src:{}:node:2", a.source_id));
    assert_ne!(a.records[0].record.id, a.records[1].record.id);
    let crlf = assert_ok!(project(&text.replace('\n', "\r\n")));
    assert_ne!(a.source_id, crlf.source_id);
    assert_eq!(texts(&a), texts(&crlf));
    let artifact = assert_ok!(a.model_artifact());
    assert_eq!(artifact, assert_ok!(b.model_artifact()));
    assert_eq!(artifact.sha256().len(), 64);
    assert!(!artifact.bytes().ends_with(b"\n"));
}

#[test]
fn metadata_coordinates_agree_with_original_unicode_bom_and_line_endings() {
    let lf = format!("\u{feff}{HEAD}## S\n\n- Café. <!-- =cafe.default -->\n");
    for text in [lf.clone(), lf.replace('\n', "\r\n"), lf.replace('\n', "\r")] {
        let e = assert_ok!(project(&text));
        let p = &e.records[0].provenance;
        assert_eq!(p.line, 7);
        assert_eq!(&text[p.byte_range.clone()], "Café. <!-- =cafe.default -->");
    }
}

#[test]
fn empty_unreferenced_sections_are_pruned() {
    let e = fixture("## Empty\n\n## Filled\n\nReady.\n");
    assert_eq!(e.context.len(), 1);
    assert_eq!(e.context[0].record.text, "Filled");
}

#[test]
fn invalid_or_empty_baselines_fail_closed() {
    assert!(matches!(
        assert_err!(project("no name")),
        EvidenceError::InvalidResume { .. }
    ));
    assert!(matches!(
        assert_err!(project(&"a".repeat(resume::markdown::MAX_INPUT_BYTES + 1))),
        EvidenceError::InputOverLimit { .. }
    ));
    assert!(matches!(
        assert_err!(project(&format!("{HEAD}## S\n\n- Hidden. <!-- @verify -->\n"))),
        EvidenceError::EmptyEvidenceSet { .. }
    ));
    assert_err!(project(&format!(
        "{HEAD}## S\n\n- A. <!-- =same.default -->\n- B. <!-- =same.default -->\n"
    )));
}
