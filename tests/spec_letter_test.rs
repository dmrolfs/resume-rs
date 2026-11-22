//! ABOUTME: The letter half of position specs (rsm-vqr.4): `[letter]`, `[letter.select]`, and their independence from the resume.
//! ABOUTME: Files that need no fixture are written to a temporary directory; the `letter-scopes` fixture is the worked example.

use std::path::{Path, PathBuf};

use claims::{assert_err, assert_none, assert_ok, assert_some};
use pretty_assertions::assert_eq;
use proptest::prelude::*;
use resume::letter::{self, LengthUnit, LetterOutput, LinkStyle};
use resume::markdown::{parse, parse_letter};
use resume::model::options::Paper;
use resume::model::{GroupName, Tag, VariantName};
use resume::select::select;
use resume::spec::{ChoiceKey, PickProblemKind, ResolvedSpec, SpecError, resolve};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/specs")
}

fn strings<T: AsRef<str>>(items: impl IntoIterator<Item = T>) -> Vec<String> {
    items.into_iter().map(|i| i.as_ref().to_string()).collect()
}

fn group(name: &str) -> GroupName {
    assert_ok!(GroupName::try_new(name))
}

fn scopes() -> ResolvedSpec {
    assert_ok!(resolve(&fixtures().join("letter-scopes/positions/acme.toml"), None))
}

/// Writes `files` (name, text) into a fresh directory and resolves `spec.toml` there.
fn resolve_files(files: &[(&str, &str)]) -> (tempfile::TempDir, Result<ResolvedSpec, SpecError>) {
    let dir = assert_ok!(tempfile::tempdir());
    for (name, text) in files {
        let path = dir.path().join(name);
        assert_ok!(std::fs::create_dir_all(path.parent().expect("a file has a parent")));
        assert_ok!(std::fs::write(path, text));
    }
    let result = resolve(&dir.path().join("spec.toml"), None);
    (dir, result)
}

/// A spec whose resume and letter files do not exist, with `extra` after its `[build]` table.
fn spec_text(extra: &str) -> String {
    format!("[build]\ncontent = \"missing-resume.md\"\n{extra}")
}

fn spec_error(extra: &str) -> SpecError {
    let (_dir, result) = resolve_files(&[("spec.toml", &spec_text(extra))]);
    assert_err!(result)
}

// ---- the worked example -------------------------------------------------------------------------------------

#[test]
fn the_same_group_gets_a_different_pick_in_each_document() {
    let spec = scopes();
    let letter = assert_some!(spec.letter.as_ref());
    assert_eq!(spec.choices.select.pick[&group("summary")].as_ref(), "acme");
    assert_eq!(letter.choices.pick[&group("summary")].as_ref(), "brief");
    assert_eq!(spec.choices.select.pick[&group("gateway")].as_ref(), "long");
    assert_none!(letter.choices.pick.get(&group("gateway")));
    assert_none!(spec.choices.select.pick.get(&group("evidence")));
    assert_eq!(letter.choices.pick[&group("evidence")].as_ref(), "short");
    assert_eq!(letter.choices.pick[&group("opening")].as_ref(), "warm");
}

#[test]
fn letter_select_merges_over_the_same_chain_with_the_same_rules() {
    let letter = assert_some!(scopes().letter);
    assert_eq!(strings(&letter.choices.tags), ["ai", "platform"]);
    assert_eq!(strings(&letter.choices.prefer), ["formal", "short", "warm"]);
    assert_eq!(letter.choices.core_only, None);
    assert!(letter.choices.promote.is_empty(), "a letter keeps its written order");
}

#[test]
fn the_letter_selection_is_built_from_the_letter_choices_only() {
    let spec = scopes();
    let selection = assert_ok!(spec.to_letter_selection());
    assert!(!selection.drafts, "no file can turn drafts on");
    assert_eq!(strings(&selection.prefer), ["formal", "short", "warm"]);
    assert_eq!(selection.picks.len(), 3);
    assert_eq!(selection.picks[&group("summary")].as_ref(), "brief");
    assert!(
        !selection.picks.contains_key(&group("gateway")),
        "a resume pick is not a letter pick"
    );
    let resume = assert_ok!(spec.to_selection());
    assert_eq!(resume.picks[&group("summary")].as_ref(), "acme");
    assert!(resume.prefer.is_empty(), "the resume chose no wording order");
    assert!(matches!(selection.scope, resume::select::Scope::Tags(ref t) if t.as_set().len() == 2));
    assert!(matches!(resume.scope, resume::select::Scope::All));
}

#[test]
fn a_resume_promote_list_orders_the_resume_and_never_the_letter() {
    let toml = spec_text("[select]\npromote = [\"ai\"]\n[letter]\ncontent = \"l.md\"\n");
    let (_dir, result) = resolve_files(&[("spec.toml", &toml)]);
    let spec = assert_ok!(result);
    assert_eq!(strings(&assert_ok!(spec.to_selection()).promote), ["ai"]);
    assert!(assert_ok!(spec.to_letter_selection()).promote.is_empty());
}

#[test]
fn each_scope_keeps_its_own_scope_kind() {
    let toml =
        spec_text("[select]\ntags = [\"ai\"]\n[letter]\ncontent = \"l.md\"\n[letter.select]\ncore-only = true\n");
    let (_dir, result) = resolve_files(&[("spec.toml", &toml)]);
    let spec = assert_ok!(result);
    assert!(matches!(
        assert_ok!(spec.to_selection()).scope,
        resume::select::Scope::Tags(_)
    ));
    assert!(matches!(
        assert_ok!(spec.to_letter_selection()).scope,
        resume::select::Scope::CoreOnly
    ));
    let toml =
        spec_text("[select]\ncore-only = true\n[letter]\ncontent = \"l.md\"\n[letter.select]\ntags = [\"ai\"]\n");
    let (_dir, result) = resolve_files(&[("spec.toml", &toml)]);
    let spec = assert_ok!(result);
    assert!(matches!(
        assert_ok!(spec.to_selection()).scope,
        resume::select::Scope::CoreOnly
    ));
    assert!(matches!(
        assert_ok!(spec.to_letter_selection()).scope,
        resume::select::Scope::Tags(_)
    ));
}

#[test]
fn letter_choices_remember_their_own_files_apart_from_the_resumes() {
    let spec = scopes();
    let letter = assert_some!(spec.letter.as_ref());
    let file = |at: &resume::spec::Located| at.path.file_name().expect("a file").to_string_lossy().into_owned();
    let pick = ChoiceKey::Pick(group("summary"));
    assert_eq!(file(assert_some!(spec.origin_of(&pick))), "acme.toml");
    assert_eq!(file(assert_some!(letter.origin_of(&pick))), "principal.toml");
    let evidence = ChoiceKey::Pick(group("evidence"));
    assert_eq!(file(assert_some!(letter.origin_of(&evidence))), "ai-platform.toml");
    assert_none!(spec.origin_of(&evidence));
    let tag = ChoiceKey::Tag(assert_ok!(Tag::try_new("ai")));
    assert_none!(spec.origin_of(&tag), "the resume chose no tags");
    assert_eq!(file(assert_some!(letter.origin_of(&tag))), "ai-platform.toml");
}

#[test]
fn the_letter_table_gives_typed_settings_and_a_path_beside_the_spec() {
    let spec = scopes();
    let letter = assert_some!(spec.letter.as_ref());
    let s = &letter.settings;
    assert_eq!(assert_some!(s.recipient.as_ref()).value, "Hiring Team");
    assert_eq!(assert_some!(s.date.as_ref()).value.to_string(), "2026-11-19");
    assert_eq!(assert_some!(s.style.as_ref()).value.as_ref(), "pragmatic");
    assert_eq!(assert_some!(s.paper.as_ref()).value, Paper::A4);
    assert_eq!(assert_some!(s.output_format.as_ref()).value, LetterOutput::Both);
    assert_eq!(assert_some!(s.max_length.as_ref()).value, 2000);
    assert_eq!(assert_some!(s.length_unit.as_ref()).value, LengthUnit::Scalars);
    assert_eq!(assert_some!(s.links.as_ref()).value, LinkStyle::TextOnly);
    let spec_dir = fixtures().join("letter-scopes/positions");
    assert_eq!(letter.content.path, spec_dir.join("..").join("letters").join("base.md"));
    assert_eq!(letter.content.declared.as_str(), "../letters/base.md");
    assert_eq!(letter.content.at.line, 17);
}

#[test]
fn letter_fields_come_from_the_position_and_the_letter_table() {
    let fields = assert_ok!(assert_some!(scopes().letter).fields());
    assert_eq!(assert_some!(fields.company).to_string(), "Acme");
    assert_eq!(assert_some!(fields.title).to_string(), "Principal Architect");
    assert_eq!(assert_some!(fields.recipient).to_string(), "Hiring Team");
    assert_eq!(assert_some!(fields.date).to_string(), "2026-11-19");
}

#[test]
fn the_printed_spec_keeps_the_letter_and_resolves_to_the_same_choices() {
    let spec = scopes();
    let text = spec.to_toml();
    let dir = assert_ok!(tempfile::tempdir());
    let path = dir.path().join("positions/spec.toml");
    assert_ok!(std::fs::create_dir_all(path.parent().expect("parent")));
    assert_ok!(std::fs::write(path, text));
    let again = assert_ok!(resolve(&dir.path().join("positions/spec.toml"), None));
    assert_eq!(again.choices, spec.choices);
    let (a, b) = (assert_some!(again.letter), assert_some!(spec.letter));
    assert_eq!(a.choices, b.choices);
    assert_eq!(a.content.declared, b.content.declared);
    let values = |l: &resume::spec::ResolvedLetter| {
        let s = &l.settings;
        (
            s.recipient.as_ref().map(|v| v.value.clone()),
            s.date.as_ref().map(|v| v.value),
            s.style.as_ref().map(|v| v.value.clone()),
            s.paper.as_ref().map(|v| v.value),
            s.output_format.as_ref().map(|v| v.value),
            s.max_length.as_ref().map(|v| v.value),
            s.length_unit.as_ref().map(|v| v.value),
            s.links.as_ref().map(|v| v.value),
        )
    };
    assert_eq!(values(&a), values(&b));
    assert_eq!(assert_some!(values(&a).5), 2000, "the printed spec kept every setting");
}

// ---- resume-only specs are unchanged ------------------------------------------------------------------------

#[test]
fn a_resume_only_spec_has_no_letter_and_prints_no_letter_table() {
    for case in ["two-profiles", "diamond", "path-relative"] {
        let path = match case {
            "path-relative" => fixtures().join("path-relative/positions/2026-11-acme.toml"),
            _ => fixtures().join(case).join("positions/acme.toml"),
        };
        let spec = assert_ok!(resolve(&path, None));
        assert_none!(spec.letter.as_ref(), "{case}");
        assert!(!spec.to_toml().contains("letter"), "{case}");
    }
}

// ---- the other document is never opened ---------------------------------------------------------------------

#[test]
fn neither_content_file_is_opened_by_resolving() {
    let (_dir, result) = resolve_files(&[("spec.toml", &spec_text("[letter]\ncontent = \"missing-letter.md\"\n"))]);
    let resolved = assert_ok!(result);
    assert!(!resolved.content.path.exists() && !assert_some!(resolved.letter).content.path.exists());
}

#[test]
fn a_bad_recipient_stops_a_letter_build_but_not_the_resume_choices() {
    for recipient in ["\\u0007bell", "\\u202Eflip", "line\\u2028break", "para\\u2029graph"] {
        let (_dir, result) = resolve_files(&[(
            "spec.toml",
            &spec_text(&format!("[letter]\ncontent = \"l.md\"\nrecipient = \"{recipient}\"\n")),
        )]);
        let resolved = assert_ok!(result);
        assert_ok!(resolved.to_selection());
        match assert_err!(assert_some!(resolved.letter).fields()) {
            SpecError::UnsafeText { key, .. } => assert_eq!(key, "letter.recipient"),
            other => panic!("{other:?}"),
        }
    }
    let long = "r".repeat(121);
    let (_dir, result) = resolve_files(&[(
        "spec.toml",
        &spec_text(&format!("[letter]\ncontent = \"l.md\"\nrecipient = \"{long}\"\n")),
    )]);
    assert!(matches!(
        assert_err!(assert_some!(assert_ok!(result).letter).fields()),
        SpecError::TextLength { max: 120, .. }
    ));
}

proptest! {
    #[test]
    fn any_position_value_the_loader_accepts_is_a_letter_field(
        company in "[!-~\\u{a1}-\\u{2ff}][ -~\\u{a0}-\\u{2ff}]{0,119}",
        title in "[!-~\\u{a1}-\\u{2ff}][ -~\\u{a0}-\\u{2ff}]{0,119}",
    ) {
        // Starts with a printable, non-space character and stays within 120 characters, so the loader must accept it.
        let text = spec_text(&format!(
            "[position]\ncompany = {}\ntitle = {}\n[letter]\ncontent = \"l.md\"\n",
            toml::Value::String(company),
            toml::Value::String(title),
        ));
        let (_dir, result) = resolve_files(&[("spec.toml", &text)]);
        let resolved = result.map_err(|e| TestCaseError::fail(e.to_string()))?;
        prop_assert!(assert_some!(resolved.letter).fields().is_ok());
    }
}

// ---- each scope reports its own unknown picks ---------------------------------------------------------------

fn resume_text() -> String {
    std::fs::read_to_string(fixtures().join("baseline.md")).expect("the fixture exists")
}

fn letter_text() -> String {
    std::fs::read_to_string(fixtures().join("letter-scopes/letters/base.md")).expect("the fixture exists")
}

#[test]
fn a_misspelled_pick_is_reported_by_its_own_document_with_its_own_line() {
    let toml = "[build]\ncontent = \"r.md\"\n[select]\npick = { summry = \"acme\" }\n\n[letter]\ncontent = \"l.md\"\n[letter.select]\npick = { opening = \"warn\", evidence = \"short\" }\n";
    let (_dir, result) = resolve_files(&[("spec.toml", toml)]);
    let spec = assert_ok!(result);
    let resume_source = resume_text();
    let parsed = parse("r.md", &resume_source);
    let (_, resume_report) = assert_ok!(select(assert_some!(parsed.valid()), &assert_ok!(spec.to_selection())));
    let letter_source = letter_text();
    let letter_parsed = parse_letter("l.md", &letter_source);
    let model = assert_some!(letter_parsed.into_parts().0);
    let (_, letter_report) = assert_ok!(letter::select(&model, &assert_ok!(spec.to_letter_selection())));

    match assert_err!(spec.check_picks(&resume_report.unknown_picks)) {
        SpecError::UnknownPicks { problems } => {
            assert_eq!(problems.len(), 1);
            assert_eq!(problems[0].at.line, 4);
            assert!(matches!(&problems[0].kind, PickProblemKind::UnknownGroup { group } if group == "summry"));
        }
        other => panic!("{other:?}"),
    }
    match assert_err!(spec.check_letter_picks(&letter_report.unknown_picks)) {
        SpecError::UnknownPicks { problems } => {
            assert_eq!(problems.len(), 1, "{problems:?}");
            assert_eq!(problems[0].at.line, 9);
            assert!(matches!(
                &problems[0].kind,
                PickProblemKind::UnknownVariant { group, variant } if group == "opening" && variant == "warn"
            ));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_letter_pick_the_resume_has_no_group_for_is_not_an_error_for_the_resume() {
    let spec = scopes();
    let resume_source = resume_text();
    let parsed = parse("r.md", &resume_source);
    let (_, report) = assert_ok!(select(assert_some!(parsed.valid()), &assert_ok!(spec.to_selection())));
    // `gateway` and `summary` exist in the resume; the letter's `evidence` and `opening` do not and are not its concern.
    assert!(report.unknown_picks.is_empty(), "{:?}", report.unknown_picks);
    let letter_source = letter_text();
    let model = assert_some!(parse_letter("l.md", &letter_source).into_parts().0);
    let (_, report) = assert_ok!(letter::select(&model, &assert_ok!(spec.to_letter_selection())));
    assert_ok!(spec.check_letter_picks(&report.unknown_picks));
}

#[test]
fn a_letter_that_is_not_a_table_is_a_wrong_type() {
    let (_dir, result) = resolve_files(&[("spec.toml", "letter = 4\n[build]\ncontent = \"r.md\"\n")]);
    assert!(matches!(assert_err!(result), SpecError::WrongType { ref key, .. } if key == "letter"));
}

#[test]
fn a_letter_check_without_a_letter_table_says_so() {
    let (_dir, result) = resolve_files(&[("spec.toml", &spec_text(""))]);
    let resolved = assert_ok!(result);
    assert!(matches!(
        assert_err!(resolved.to_letter_selection()),
        SpecError::NoLetterTable
    ));
    assert!(matches!(
        assert_err!(resolved.check_letter_picks(&[])),
        SpecError::NoLetterTable
    ));
}

#[test]
fn core_only_and_tags_conflict_inside_one_scope_only() {
    let toml =
        spec_text("[select]\ncore-only = true\n[letter]\ncontent = \"l.md\"\n[letter.select]\ntags = [\"ai\"]\n");
    let (_dir, result) = resolve_files(&[("spec.toml", &toml)]);
    let spec = assert_ok!(result);
    assert_ok!(spec.to_selection());
    assert_ok!(spec.to_letter_selection());

    let toml = spec_text(
        "[select]\ntags = [\"ai\"]\n[letter]\ncontent = \"l.md\"\n[letter.select]\ncore-only = true\ntags = [\"ai\"]\n",
    );
    let (_dir, result) = resolve_files(&[("spec.toml", &toml)]);
    let spec = assert_ok!(result);
    assert_ok!(spec.to_selection());
    match assert_err!(spec.to_letter_selection()) {
        SpecError::ScopeConflict { tags_at, core_only_at } => assert_eq!((core_only_at.line, tags_at.line), (8, 9)),
        other => panic!("{other:?}"),
    }
}

// ---- what a profile and a spec may say ----------------------------------------------------------------------

fn profile_error(letter: &str) -> SpecError {
    let dir = assert_ok!(tempfile::tempdir());
    assert_ok!(std::fs::create_dir(dir.path().join("profiles")));
    assert_ok!(std::fs::create_dir(dir.path().join("positions")));
    assert_ok!(std::fs::write(dir.path().join("profiles/base.toml"), letter));
    assert_ok!(std::fs::write(
        dir.path().join("positions/spec.toml"),
        "extends = [\"base\"]\n[build]\ncontent = \"a.md\"\n",
    ));
    assert_err!(resolve(&dir.path().join("positions/spec.toml"), None))
}

#[test]
fn a_profile_may_hold_letter_select_and_nothing_else_of_the_letter() {
    for (key, line) in [
        ("content = \"l.md\"", "letter.content"),
        ("recipient = \"Team\"", "letter.recipient"),
        ("date = 2026-11-19", "letter.date"),
        ("style = \"mono\"", "letter.style"),
        ("paper = \"a4\"", "letter.paper"),
        ("output-format = \"pdf\"", "letter.output-format"),
        ("max-length = 10", "letter.max-length"),
        ("length-unit = \"bytes\"", "letter.length-unit"),
        ("links = \"text-only\"", "letter.links"),
    ] {
        let err = profile_error(&format!("[letter]\n{key}\n[letter.select]\ntags = [\"ai\"]\n"));
        match err {
            SpecError::ForbiddenInProfile { key: got, at, .. } => {
                assert_eq!((got, at.line), (line, 2), "{key}");
                assert!(at.path.ends_with("base.toml"));
            }
            other => panic!("{key}: {other:?}"),
        }
    }
}

#[test]
fn a_profile_error_points_at_the_first_forbidden_key_written() {
    match profile_error("[letter]\nlinks = \"text-only\"\nrecipient = \"Team\"\n") {
        SpecError::ForbiddenInProfile { key, at, .. } => assert_eq!((key, at.line), ("letter.links", 2)),
        other => panic!("{other:?}"),
    }
    match profile_error("[letter]\nrecipient = \"Team\"\nlinks = \"text-only\"\n") {
        SpecError::ForbiddenInProfile { key, at, .. } => assert_eq!((key, at.line), ("letter.recipient", 2)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_inline_letter_table_is_checked_like_a_written_out_one() {
    match profile_error("letter = { recipient = \"Team\" }\n") {
        SpecError::ForbiddenInProfile { key, at, .. } => assert_eq!((key, at.line), ("letter.recipient", 1)),
        other => panic!("{other:?}"),
    }
    let (_dir, result) = resolve_files(&[(
        "spec.toml",
        "letter = { recipient = \"Team\" }\n[build]\ncontent = \"r.md\"\n",
    )]);
    match assert_err!(result) {
        SpecError::MissingKey { key, at } => assert_eq!((key, at.line), ("letter.content", 1)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_profile_with_only_letter_select_loads() {
    let dir = assert_ok!(tempfile::tempdir());
    assert_ok!(std::fs::create_dir(dir.path().join("profiles")));
    assert_ok!(std::fs::create_dir(dir.path().join("positions")));
    assert_ok!(std::fs::write(
        dir.path().join("profiles/base.toml"),
        "[letter.select]\ntags = [\"ai\"]\n",
    ));
    assert_ok!(std::fs::write(
        dir.path().join("positions/spec.toml"),
        "extends = [\"base\"]\n[build]\ncontent = \"a.md\"\n[letter]\ncontent = \"l.md\"\n",
    ));
    let spec = assert_ok!(resolve(&dir.path().join("positions/spec.toml"), None));
    assert_eq!(strings(&assert_some!(spec.letter).choices.tags), ["ai"]);
}

#[test]
fn the_spec_beats_a_profile_in_the_letter_scope_and_the_resume_scope_is_untouched() {
    let dir = assert_ok!(tempfile::tempdir());
    assert_ok!(std::fs::create_dir(dir.path().join("profiles")));
    assert_ok!(std::fs::create_dir(dir.path().join("positions")));
    assert_ok!(std::fs::write(
        dir.path().join("profiles/base.toml"),
        "[select]\npick = { summary = \"ai\" }\n[letter.select]\npick = { summary = \"brief\" }\ncore-only = true\n",
    ));
    assert_ok!(std::fs::write(
        dir.path().join("positions/spec.toml"),
        "extends = [\"base\"]\n[build]\ncontent = \"a.md\"\n[letter]\ncontent = \"l.md\"\n[letter.select]\npick = { summary = \"long\" }\ncore-only = false\n",
    ));
    let spec = assert_ok!(resolve(&dir.path().join("positions/spec.toml"), None));
    let letter = assert_some!(spec.letter.as_ref());
    assert_eq!(letter.choices.pick[&group("summary")].as_ref(), "long");
    assert_eq!(letter.choices.core_only, Some(false));
    assert_eq!(spec.choices.select.pick[&group("summary")].as_ref(), "ai");
    assert_eq!(spec.choices.select.core_only, None);
    let origin = assert_some!(letter.origin_of(&ChoiceKey::Pick(group("summary"))));
    assert!(origin.path.ends_with("spec.toml"));
    assert_eq!(origin.line, 7);
}

#[test]
fn a_profile_letter_select_without_a_spec_letter_table_resolves_no_letter() {
    let dir = assert_ok!(tempfile::tempdir());
    assert_ok!(std::fs::create_dir(dir.path().join("profiles")));
    assert_ok!(std::fs::create_dir(dir.path().join("positions")));
    assert_ok!(std::fs::write(
        dir.path().join("profiles/base.toml"),
        "[letter.select]\ntags = [\"ai\"]\n"
    ));
    assert_ok!(std::fs::write(
        dir.path().join("positions/spec.toml"),
        "extends = [\"base\"]\n[build]\ncontent = \"a.md\"\n",
    ));
    assert_none!(assert_ok!(resolve(&dir.path().join("positions/spec.toml"), None)).letter);
}

#[test]
fn a_spec_letter_table_needs_content_even_when_it_holds_only_select() {
    for extra in [
        "[letter]\nrecipient = \"Team\"\n",
        "[letter.select]\ntags = [\"ai\"]\n",
        "[letter]\n",
    ] {
        match spec_error(extra) {
            SpecError::MissingKey { key, at } => {
                assert_eq!(key, "letter.content", "{extra}");
                assert_eq!(at.line, 3, "{extra}");
            }
            other => panic!("{extra}: {other:?}"),
        }
    }
}

#[test]
fn letter_keys_are_checked_strictly_whatever_command_will_run() {
    let cases: [(&str, &str); 13] = [
        ("[letter]\ncontent = \"l.md\"\nrecipent = \"x\"\n", "letter.recipent"),
        ("[letter.select]\ndrafts = true\n", "letter.select.drafts"),
        ("[letter.select]\npromote = [\"a\"]\n", "letter.select.promote"),
        (
            "[letter]\ncontent = \"l.md\"\nmax-length = \"10\"\n",
            "letter.max-length",
        ),
        ("[letter]\ncontent = \"l.md\"\nmax-length = -1\n", "letter.max-length"),
        ("[letter]\ncontent = \"l.md\"\ndate = \"2026-11-19\"\n", "letter.date"),
        ("[letter]\ncontent = 4\n", "letter.content"),
        ("[letter]\ncontent = \"l.md\"\nrecipient = 4\n", "letter.recipient"),
        (
            "[letter]\ncontent = \"l.md\"\n[letter.select]\ntags = \"ai\"\n",
            "letter.select.tags",
        ),
        (
            "[letter]\ncontent = \"l.md\"\n[letter.select]\ncore-only = \"yes\"\n",
            "letter.select.core-only",
        ),
        (
            "[letter]\ncontent = \"l.md\"\n[letter.select]\npick = { a = 4 }\n",
            "letter.select.pick.a",
        ),
        ("[letter.select]\nTags = []\n", "letter.select.Tags"),
        ("[letter]\ncontent = \"l.md\"\n[letter.pick]\n", "letter.pick"),
    ];
    for (extra, key) in cases {
        match spec_error(extra) {
            SpecError::UnknownKey { key: got, .. } | SpecError::WrongType { key: got, .. } => {
                assert_eq!(got, key, "{extra}");
            }
            other => panic!("{extra}: {other:?}"),
        }
    }
}

#[test]
fn letter_values_are_checked_for_shape_by_every_command() {
    match spec_error("[letter]\ncontent = \"l.md\"\noutput-format = \"html\"\n") {
        SpecError::InvalidKeyword { key, allowed, at, .. } => {
            assert_eq!((key, at.line), ("letter.output-format", 5));
            assert_eq!(allowed, ["text", "pdf", "both"]);
        }
        other => panic!("{other:?}"),
    }
    match spec_error("[letter]\ncontent = \"l.md\"\nlinks = \"url\"\n") {
        SpecError::InvalidKeyword { key, allowed, .. } => {
            assert_eq!(
                (key, allowed.as_slice()),
                (
                    "letter.links",
                    &["text-and-url".to_string(), "text-only".to_string()][..]
                )
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        spec_error("[letter]\ncontent = \"l.md\"\nlength-unit = \"utf9\"\nmax-length = 5\n"),
        SpecError::InvalidKeyword {
            key: "letter.length-unit",
            ..
        }
    ));
    match spec_error("[letter]\ncontent = \"l.md\"\nlength-unit = \"bytes\"\n") {
        SpecError::RequiresKey { key, needs, at } => {
            assert_eq!((key, needs, at.line), ("letter.length-unit", "letter.max-length", 5))
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        spec_error("[letter]\ncontent = \"l.md\"\npaper = \"folio\"\n"),
        SpecError::InvalidPaper { .. }
    ));
    assert!(matches!(
        spec_error("[letter]\ncontent = \"l.md\"\nstyle = \"bad_name\"\n"),
        SpecError::InvalidStyle { .. }
    ));
    for date in ["2026-11-19T10:00:00", "10:00:00"] {
        match spec_error(&format!("[letter]\ncontent = \"l.md\"\ndate = {date}\n")) {
            SpecError::InvalidDate { key, .. } => assert_eq!(key, "letter.date", "{date}"),
            other => panic!("{date}: {other:?}"),
        }
    }
    assert!(matches!(
        spec_error("[letter.select]\ntags = [\"Bad Tag\"]\n[letter]\ncontent = \"l.md\"\n"),
        SpecError::InvalidName { ref key, .. } if key == "letter.select.tags"
    ));
    assert!(matches!(
        spec_error("[letter]\ncontent = \"l.md\"\n[letter.select]\npick = { a = \"Bad\" }\n"),
        SpecError::InvalidName { ref key, .. } if key == "letter.select.pick"
    ));
    assert!(matches!(
        spec_error("[letter]\ncontent = \"l.md\"\n[letter.select]\nprefer = [\"a\", \"a\"]\n"),
        SpecError::DuplicateEntry { ref key, .. } if key == "letter.select.prefer"
    ));
}

#[test]
fn a_letter_content_path_follows_the_same_rules_as_a_resume_content_path() {
    for bad in ["C:/x.md", "/x.md", "a\\\\b.md", "dir/", ""] {
        match spec_error(&format!("[letter]\ncontent = \"{bad}\"\n")) {
            SpecError::InvalidPath { key, .. } => assert_eq!(key, "letter.content", "{bad}"),
            other => panic!("{bad}: {other:?}"),
        }
    }
}

// ---- paths are relative to the file that declares them ------------------------------------------------------

#[test]
fn a_letter_path_is_joined_onto_the_spec_directory_not_the_working_directory() {
    let (dir, result) = resolve_files(&[(
        "positions/2026/spec.toml",
        "[build]\ncontent = \"../../r.md\"\n[letter]\ncontent = \"../../letters/l.md\"\n",
    )]);
    drop(result);
    let spec_path = dir.path().join("positions/2026/spec.toml");
    let resolved = assert_ok!(resolve(&spec_path, None));
    let letter = assert_some!(resolved.letter);
    let base = dir.path().join("positions/2026");
    assert_eq!(
        letter.content.path,
        base.join("..").join("..").join("letters").join("l.md")
    );
    assert_eq!(resolved.content.path, base.join("..").join("..").join("r.md"));
}

// ---- merging the letter scope is the resume's merge, over the same chain --------------------------------------

/// One chain of profiles `p0 .. pn` and a spec, written to a directory; each profile body is `select_body` as
/// `[select]` and, in the copy, as `[letter.select]`.
fn resolve_chain(bodies: &[String], both: bool, only_letter: bool) -> ResolvedSpec {
    let dir = assert_ok!(tempfile::tempdir());
    assert_ok!(std::fs::create_dir(dir.path().join("profiles")));
    assert_ok!(std::fs::create_dir(dir.path().join("positions")));
    let names: Vec<String> = (0..bodies.len()).map(|i| format!("\"p{i}\"")).collect();
    for (i, body) in bodies.iter().enumerate() {
        let mut text = String::new();
        if both || !only_letter {
            text.push_str(&format!("[select]\n{body}\n"));
        }
        if both || only_letter {
            text.push_str(&format!("[letter.select]\n{body}\n"));
        }
        assert_ok!(std::fs::write(dir.path().join(format!("profiles/p{i}.toml")), text));
    }
    assert_ok!(std::fs::write(
        dir.path().join("positions/spec.toml"),
        format!(
            "extends = [{}]\n[build]\ncontent = \"a.md\"\n[letter]\ncontent = \"l.md\"\n",
            names.join(", ")
        ),
    ));
    assert_ok!(resolve(&dir.path().join("positions/spec.toml"), None))
}

fn body() -> impl Strategy<Value = String> {
    let names = ["ai", "llm", "platform", "short", "long"];
    (
        proptest::sample::subsequence(names.to_vec(), 0..=3),
        proptest::sample::subsequence(names.to_vec(), 0..=3),
        proptest::collection::btree_map(0..3usize, 0..names.len(), 0..3),
        proptest::option::of(any::<bool>()),
    )
        .prop_map(move |(tags, prefer, picks, core)| {
            let groups = ["gateway", "summary", "scale"];
            let list = |v: &[&str]| v.iter().map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(", ");
            let mut text = format!("tags = [{}]\nprefer = [{}]\n", list(&tags), list(&prefer));
            if !picks.is_empty() {
                let items: Vec<String> = picks
                    .iter()
                    .map(|(g, v)| format!("{} = \"{}\"", groups[*g], names[*v]))
                    .collect();
                text.push_str(&format!("pick = {{ {} }}\n", items.join(", ")));
            }
            if let Some(core) = core {
                text.push_str(&format!("core-only = {core}\n"));
            }
            text
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn letter_select_merges_exactly_as_select_does_on_the_same_chain(bodies in proptest::collection::vec(body(), 0..4)) {
        let spec = resolve_chain(&bodies, true, false);
        let letter = assert_some!(spec.letter.as_ref());
        prop_assert_eq!(&letter.choices, &spec.choices.select);
        let origins: Vec<_> = spec.origins().map(|(k, at)| (k.clone(), at.line, at.path.clone())).collect();
        let letter_origins: Vec<_> = letter.origins().map(|(k, at)| (k.clone(), at.line, at.path.clone())).collect();
        // The files are written with the resume table first, so only the key sets and files are the same.
        prop_assert_eq!(
            origins.iter().map(|(k, _, p)| (k.clone(), p.clone())).collect::<Vec<_>>(),
            letter_origins.iter().map(|(k, _, p)| (k.clone(), p.clone())).collect::<Vec<_>>()
        );
    }

    #[test]
    fn each_scope_depends_only_on_its_own_table(a in proptest::collection::vec(body(), 1..4), b in proptest::collection::vec(body(), 1..4)) {
        let n = a.len().min(b.len());
        let (a, b) = (&a[..n], &b[..n]);
        // Resume table from `a`, letter table from `b`, written into the same chain of files.
        let dir = assert_ok!(tempfile::tempdir());
        assert_ok!(std::fs::create_dir(dir.path().join("profiles")));
        assert_ok!(std::fs::create_dir(dir.path().join("positions")));
        for i in 0..n {
            assert_ok!(std::fs::write(
                dir.path().join(format!("profiles/p{i}.toml")),
                format!("[select]\n{}\n[letter.select]\n{}\n", a[i], b[i]),
            ));
        }
        let names: Vec<String> = (0..n).map(|i| format!("\"p{i}\"")).collect();
        assert_ok!(std::fs::write(
            dir.path().join("positions/spec.toml"),
            format!("extends = [{}]\n[build]\ncontent = \"a.md\"\n[letter]\ncontent = \"l.md\"\n", names.join(", ")),
        ));
        let mixed = assert_ok!(resolve(&dir.path().join("positions/spec.toml"), None));
        let resume_alone = resolve_chain(a, false, false);
        let letter_alone = resolve_chain(b, false, true);
        prop_assert_eq!(&mixed.choices.select, &resume_alone.choices.select);
        prop_assert_eq!(&assert_some!(mixed.letter.as_ref()).choices, &assert_some!(letter_alone.letter.as_ref()).choices);
    }
}

#[test]
fn a_circle_of_profiles_is_refused_whatever_table_the_profiles_fill() {
    let dir = assert_ok!(tempfile::tempdir());
    assert_ok!(std::fs::create_dir(dir.path().join("profiles")));
    assert_ok!(std::fs::create_dir(dir.path().join("positions")));
    assert_ok!(std::fs::write(
        dir.path().join("profiles/a.toml"),
        "extends = [\"b\"]\n[letter.select]\ntags = [\"ai\"]\n"
    ));
    assert_ok!(std::fs::write(
        dir.path().join("profiles/b.toml"),
        "extends = [\"a\"]\n[letter.select]\ntags = [\"ai\"]\n"
    ));
    assert_ok!(std::fs::write(
        dir.path().join("positions/spec.toml"),
        "extends = [\"a\"]\n[build]\ncontent = \"a.md\"\n[letter]\ncontent = \"l.md\"\n",
    ));
    assert!(matches!(
        assert_err!(resolve(&dir.path().join("positions/spec.toml"), None)),
        SpecError::Cycle { .. }
    ));
}

#[test]
fn the_variant_and_tag_names_of_a_letter_pick_are_the_same_types_the_resume_uses() {
    let letter = assert_some!(scopes().letter);
    assert!(letter.choices.tags.contains(&assert_ok!(Tag::try_new("platform"))));
    assert_eq!(
        letter.choices.pick[&group("opening")],
        assert_ok!(VariantName::try_new("warm"))
    );
}

#[test]
fn a_blank_recipient_is_a_length_problem_and_a_letter_date_error_names_its_key() {
    let (_dir, result) = resolve_files(&[(
        "spec.toml",
        &spec_text("[letter]\ncontent = \"l.md\"\nrecipient = \"   \"\n"),
    )]);
    match assert_err!(assert_some!(assert_ok!(result).letter).fields()) {
        SpecError::TextLength { key, max, .. } => assert_eq!((key.as_str(), max), ("letter.recipient", 120)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn every_date_problem_has_its_own_plain_explanation() {
    let shown = |text: &str| {
        let toml = spec_text(&format!("[letter]\ncontent = \"l.md\"\ndate = {text}\n"));
        let (_dir, result) = resolve_files(&[("spec.toml", &toml)]);
        assert_err!(result).to_string()
    };
    assert!(shown("2026-11-19T10:00:00").ends_with("must be a date such as 2026-11-19, with no time of day"));
}

#[test]
fn a_blank_recipient_is_a_length_problem() {
    let (_dir, result) = resolve_files(&[(
        "spec.toml",
        &spec_text("[letter]\ncontent = \"l.md\"\nrecipient = \"   \"\n"),
    )]);
    match assert_err!(assert_some!(assert_ok!(result).letter).fields()) {
        SpecError::TextLength { key, max, .. } => assert_eq!((key.as_str(), max), ("letter.recipient", 120)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_select_table_with_one_field_is_printed_whichever_field_it_is() {
    for field in [
        "tags = [\"ai\"]",
        "prefer = [\"short\"]",
        "pick = { gateway = \"short\" }",
        "core-only = true",
    ] {
        for table in ["select", "letter.select"] {
            let toml = spec_text(&format!("[letter]\ncontent = \"l.md\"\n[{table}]\n{field}\n"));
            let (_dir, result) = resolve_files(&[("spec.toml", &toml)]);
            let printed = assert_ok!(result).to_toml();
            // `pick` prints as its own sub-table, so look for the table's name as a prefix of a header.
            assert!(printed.contains(&format!("[{table}")), "{table} {field}:\n{printed}");
        }
    }
    let (_dir, result) = resolve_files(&[("spec.toml", &spec_text("[letter]\ncontent = \"l.md\"\n"))]);
    let printed = assert_ok!(result).to_toml();
    assert!(
        !printed.contains("select"),
        "an empty select is not printed:\n{printed}"
    );
}
