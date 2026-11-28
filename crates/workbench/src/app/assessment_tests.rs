use super::content::Outcome;
use super::index::{FileIndex, FileKey};
use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;
use std::{path::Path, sync::Arc};

fn content(index: &FileIndex, path: &Path) -> Arc<super::content::Content> {
    assert_some!(index.content.get(&FileKey::Disk(path.to_owned()))).clone()
}

#[test]
fn additions_keep_source_ownership_overrides_and_promoted_output_order() {
    let dir = assert_ok!(tempfile::tempdir());
    let root = dir.path();
    for (name, text) in [
        (
            "Resume.md",
            include_str!("../../../../tests/fixtures/additions/Resume.md"),
        ),
        (
            "Letter.md",
            include_str!("../../../../tests/fixtures/additions/Letter.md"),
        ),
        (
            "Additions.toml",
            include_str!("../../../../tests/fixtures/additions/Additions.toml"),
        ),
    ] {
        assert_ok!(std::fs::write(root.join(name), text));
    }
    let spec = root.join("Additions.toml");
    let text = assert_ok!(std::fs::read_to_string(&spec)).replace("[letter]\n", "[letter]\ndate = 2028-02-29\n");
    assert_ok!(std::fs::write(&spec, text));
    let index = FileIndex::scan_active(root, Some(&spec));
    assert!(index.problems.is_empty(), "{:?}", index.problems);
    let r = content(&index, &root.join("Resume.md"));
    let added = assert_some!(r.items.iter().find(|i| {
        i.annotations
            .id
            .as_ref()
            .is_some_and(|id| id.as_ref().as_ref() == "application-result")
    }));
    assert_eq!(added.origin.span().file, spec);
    assert_eq!(added.promoted, Some((3, 2)));
    assert!(added.chips().contains(&"Spec addition".into()));
    let rendered = assert_some!(r.rendered.as_ref());
    let migrated = assert_some!(rendered.find("Migrated"));
    let reduced = assert_some!(rendered.find("Reduced"));
    assert!(migrated < reduced);
    assert!(!rendered.contains("never render"));
    let overridden = assert_some!(r.items.iter().find(|i| {
        i.annotations
            .id
            .as_ref()
            .is_some_and(|id| id.as_ref().as_ref() == "delivery")
    }));
    assert_eq!(overridden.annotations.priority, Some(resume::model::Priority::Core));
    let letter = content(&index, &root.join("Letter.md"));
    let rendered = assert_some!(letter.rendered.as_ref());
    assert!(rendered.contains("2028"), "{rendered}");
    assert!(rendered.contains("Team"));
    assert!(rendered.contains("Acme"));
}

#[test]
fn letter_group_choice_and_dropped_group_follow_engine_report() {
    let dir = assert_ok!(tempfile::tempdir());
    let root = dir.path();
    assert_ok!(std::fs::write(root.join("r.md"), "# Ada\n\n## Work\n\nExperience.\n"));
    assert_ok!(std::fs::write(
        root.join("l.md"),
        "# Ada\n\n## Greeting\n\nHello,\n\n## Body\n\nDefault. <!-- =fit.default @core -->\n\nShort. <!-- =fit.short @core -->\n\nOther default. <!-- =other.default -->\n\nOther short. <!-- =other.short -->\n\n## Closing\n\nRegards,\n"
    ));
    let spec = root.join("job.toml");
    assert_ok!(std::fs::write(
        &spec,
        "[build]\ncontent = 'r.md'\n[letter]\ncontent = 'l.md'\n[letter.select]\ncore-only = true\nprefer = ['short']\n"
    ));
    let index = FileIndex::scan_active(root, Some(&spec));
    assert!(index.problems.is_empty(), "{:?}", index.problems);
    let letter = content(&index, &root.join("l.md"));
    assert_eq!(letter.items[1].outcome, Outcome::NotChosen);
    assert_eq!(
        letter.items[2].outcome,
        Outcome::Chosen(resume::select::ChosenBy::Preference)
    );
    assert_eq!(
        letter.items[3].outcome,
        Outcome::Dropped(resume::select::DropReason::CoreOnly)
    );
    assert_eq!(letter.items[4].outcome, letter.items[3].outcome);
    assert!(letter.items.iter().all(|i| i.outcome != Outcome::Unavailable));
}

#[test]
fn activation_selects_only_named_targets_and_clear_restores_unselected() {
    let dir = assert_ok!(tempfile::tempdir());
    let root = dir.path();
    let text = "# Ada\n\n## Work\n\n- Core <!-- @core -->\n- Extra <!-- @extra -->\n";
    for name in ["r.md", "other.md"] {
        assert_ok!(std::fs::write(root.join(name), text));
    }
    let spec = root.join("job.toml");
    assert_ok!(std::fs::write(
        &spec,
        "[build]\ncontent = 'r.md'\n[select]\ncore-only = true\n"
    ));
    let index = FileIndex::scan_active(root, Some(&spec));
    assert!(index.problems.is_empty(), "{:?}", index.problems);
    let r = content(&index, &root.join("r.md"));
    assert!(r.assessed);
    assert_eq!(
        r.items[2].outcome,
        Outcome::Dropped(resume::select::DropReason::CoreOnly)
    );
    assert!(
        content(&index, &root.join("other.md"))
            .items
            .iter()
            .all(|i| i.outcome == Outcome::Unselected)
    );
    let clear = FileIndex::scan(root);
    assert!(!content(&clear, &root.join("r.md")).assessed);
}

#[test]
fn failed_resolution_retains_added_items_and_truthful_field_diagnostics() {
    let dir = assert_ok!(tempfile::tempdir());
    let root = dir.path();
    assert_ok!(std::fs::write(root.join("r.md"), "# Ada\n\n## Work\n\nExperience.\n"));
    assert_ok!(std::fs::write(
        root.join("l.md"),
        "# Ada\n\n## Greeting\n\nDear {{recipient}},\n\n## Body\n\nExperience.\n\n## Closing\n\nRegards,\n"
    ));
    let spec = root.join("job.toml");
    assert_ok!(std::fs::write(
        &spec,
        "[build]\ncontent = 'r.md'\n[letter]\ncontent = 'l.md'\n[[letter.add]]\nid = 'added'\ncontent = 'More experience.'\nplace = { at = 'end' }\n"
    ));
    let index = FileIndex::scan_active(root, Some(&spec));
    let entry = assert_some!(index.entries.get(&FileKey::Disk(spec.clone())));
    assert!(entry.problems.is_empty(), "{:?}", entry.problems);
    let letter = content(&index, &root.join("l.md"));
    assert_eq!(letter.items.len(), 4);
    assert!(letter.items.iter().all(|i| i.outcome == Outcome::Unavailable));
    assert!(letter.rendered.is_none());
    assert!(
        index
            .problems
            .iter()
            .any(|p| p.file == root.join("l.md") && p.code.is_some())
    );
}

#[test]
fn literal_fields_are_resolved_only_for_kept_content_and_escaped_for_markdown() {
    let dir = assert_ok!(tempfile::tempdir());
    let root = dir.path();
    assert_ok!(std::fs::write(root.join("r.md"), "# Ada\n\n## Work\n\nExperience.\n"));
    assert_ok!(std::fs::write(
        root.join("l.md"),
        "# Ada\n\n## Greeting\n\nHello,\n\n## Body\n\nFor {{company}}. <!-- @core -->\n\nDear {{recipient}}. <!-- @draft -->\n\n## Closing\n\nRegards,\n"
    ));
    let spec = root.join("job.toml");
    assert_ok!(std::fs::write(
        &spec,
        "[position]\ncompany = '*Acme*'\ntitle = 'Engineer'\n[build]\ncontent = 'r.md'\n[letter]\ncontent = 'l.md'\n"
    ));
    let index = FileIndex::scan_active(root, Some(&spec));
    assert!(index.problems.is_empty(), "{:?}", index.problems);
    let letter = content(&index, &root.join("l.md"));
    assert!(letter.assessed);
    assert!(letter.items[1].text.contains("\\*Acme\\*"));
    assert!(letter.items[2].text.contains("{{recipient}}"));
    assert_eq!(
        letter.items[2].outcome,
        Outcome::Dropped(resume::select::DropReason::Draft)
    );
}
