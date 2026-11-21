//! ABOUTME: Resolves the spec fixtures end to end: the merge worked example, a diamond, relative paths, and every refusal.
//! ABOUTME: Files that need no fixture (a BOM, CRLF, a link out of the directory) are written to a temporary directory.

use std::path::{Path, PathBuf};

use claims::{assert_err, assert_ok, assert_some};
use pretty_assertions::assert_eq;
use resume::model::options::Paper;
use resume::model::{GroupName, Tag, VariantName};
use resume::spec::names::Level;
use resume::spec::{ChoiceKey, ResolvedSpec, SpecError, resolve};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/specs")
}

fn spec_of(case: &str) -> PathBuf {
    fixtures().join(case).join("positions/acme.toml")
}

fn strings<T: AsRef<str>>(items: impl IntoIterator<Item = T>) -> Vec<String> {
    items.into_iter().map(|i| i.as_ref().to_string()).collect()
}

fn chain_names(spec: &ResolvedSpec) -> Vec<String> {
    spec.chain
        .iter()
        .map(|link| link.name.as_ref().map_or("spec".to_string(), ToString::to_string))
        .collect()
}

fn group(name: &str) -> GroupName {
    assert_ok!(GroupName::try_new(name))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
}

// ---- the cases that load ----------------------------------------------------------------------------------

#[test]
fn two_profiles_and_a_spec_resolve_to_the_worked_example() {
    let spec = assert_ok!(resolve(&spec_of("two-profiles"), None));
    assert_eq!(chain_names(&spec), ["principal", "ai-platform", "spec"]);
    let c = &spec.choices;
    assert_eq!(strings(&c.select.tags), ["ai", "iceberg", "leadership", "platform"]);
    assert_eq!(strings(&c.select.prefer), ["short", "business", "technical"]);
    assert_eq!(c.select.pick[&group("gateway")].as_ref(), "short");
    assert_eq!(c.select.pick[&group("summary")].as_ref(), "acme");
    assert_eq!(c.style.as_ref().map(AsRef::as_ref), Some("mono"));
    assert_eq!(c.paper, Some(Paper::A4));
}

#[test]
fn each_choice_names_the_file_and_line_that_decided_it() {
    let spec = assert_ok!(resolve(&spec_of("two-profiles"), None));
    let origin = |key: ChoiceKey| {
        let at = assert_some!(spec.origin_of(&key)).clone();
        (file_name(&at.path), at.line)
    };
    assert_eq!(
        origin(ChoiceKey::Pick(group("gateway"))),
        ("ai-platform.toml".to_string(), 10)
    );
    assert_eq!(origin(ChoiceKey::Pick(group("summary"))), ("acme.toml".to_string(), 23));
    assert_eq!(origin(ChoiceKey::Style).0, "acme.toml");
    assert_eq!(origin(ChoiceKey::Paper).0, "ai-platform.toml");
    assert_eq!(
        origin(ChoiceKey::Tag(assert_ok!(Tag::try_new("leadership")))).0,
        "principal.toml"
    );
    assert_eq!(
        origin(ChoiceKey::Prefer(assert_ok!(VariantName::try_new("short")))).0,
        "acme.toml"
    );
}

#[test]
fn the_position_is_read_with_typed_values() {
    let spec = assert_ok!(resolve(&spec_of("two-profiles"), None));
    let position = assert_some!(spec.position.as_ref());
    assert_eq!(assert_some!(position.company.as_ref()).as_ref(), "Acme");
    assert_eq!(assert_some!(position.level.as_ref()).as_ref(), "principal");
    assert_eq!(position.emphasis.technical, Some(Level::High));
    assert_eq!(assert_some!(position.date).to_string(), "2026-11-19");
    assert_eq!(
        assert_some!(position.posting.as_ref()).as_str(),
        "https://example.com/jobs/123"
    );
}

#[test]
fn a_diamond_applies_its_shared_base_once_and_the_last_arm_wins() {
    let spec = assert_ok!(resolve(&spec_of("diamond"), None));
    assert_eq!(chain_names(&spec), ["base", "left", "right", "spec"]);
    assert_eq!(strings(&spec.choices.select.prefer), ["business", "short", "technical"]);
    assert_eq!(spec.choices.select.pick[&group("gateway")].as_ref(), "long");
}

#[test]
fn the_content_path_is_relative_to_the_spec_and_not_to_the_working_directory() {
    let relative = Path::new("tests/fixtures/specs/path-relative/positions/2026-11-acme.toml");
    let absolute = fixtures().join("path-relative/positions/2026-11-acme.toml");
    for spec_path in [
        relative.to_path_buf(),
        absolute.clone(),
        assert_ok!(std::fs::canonicalize(&absolute)),
    ] {
        let spec = assert_ok!(resolve(&spec_path, None));
        assert_eq!(spec.content.declared.as_str(), "../baselines/baseline.md");
        let tail: Vec<String> = spec
            .content
            .path
            .components()
            .rev()
            .take(4)
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            tail,
            ["baseline.md", "baselines", "..", "positions"],
            "{}",
            spec_path.display()
        );
        assert!(spec.content.path.is_file(), "{}", spec.content.path.display());
    }
}

#[test]
fn the_resolved_choices_drive_the_real_selection_of_the_fixture_baseline() {
    let spec = assert_ok!(resolve(&spec_of("two-profiles"), None));
    let text = assert_ok!(std::fs::read_to_string(fixtures().join("baseline.md")));
    let parsed = resume::markdown::parse("baseline.md", &text);
    let resume = assert_some!(parsed.valid());
    let selection = assert_ok!(spec.to_selection());
    let (_, report) = assert_ok!(resume::select::select(resume, &selection));
    assert_ok!(spec.check_picks(&report.unknown_picks));
    let chosen: std::collections::BTreeMap<String, String> = report
        .chosen
        .iter()
        .map(|c| (c.group.to_string(), c.variant.to_string()))
        .collect();
    assert_eq!(chosen["gateway"], "short");
    assert_eq!(chosen["summary"], "acme");
}

#[test]
fn a_spec_that_extends_nothing_needs_no_profiles_directory() {
    let dir = assert_ok!(tempfile::tempdir());
    let path = dir.path().join("acme.toml");
    assert_ok!(std::fs::write(&path, "[build]\ncontent = \"a.md\"\n"));
    let spec = assert_ok!(resolve(&path, None));
    assert_eq!(chain_names(&spec), ["spec"]);
    assert_eq!(spec.choices, resume::spec::Choices::default());
    assert_ok!(resolve(&path, Some(&dir.path().join("no-such-directory"))));
}

#[test]
fn printing_a_resolved_spec_and_resolving_it_again_gives_the_same_choices() {
    let spec = assert_ok!(resolve(&spec_of("two-profiles"), None));
    let text = spec.to_toml();
    assert!(
        text.starts_with("# Resolved from: principal, ai-platform, the spec\n"),
        "{text}"
    );
    let dir = assert_ok!(tempfile::tempdir());
    let flat = dir.path().join("flat.toml");
    assert_ok!(std::fs::write(&flat, &text));
    let again = assert_ok!(resolve(&flat, None));
    assert_eq!(again.choices, spec.choices);
    assert_eq!(again.position, spec.position);
    assert_eq!(again.content.declared, spec.content.declared);
    assert_eq!(chain_names(&again), ["spec"]);
    assert_eq!(
        again.to_toml().lines().skip(2).collect::<Vec<_>>(),
        text.lines().skip(2).collect::<Vec<_>>()
    );
}

#[test]
fn the_printed_spec_is_the_same_text_every_time() {
    let first = assert_ok!(resolve(&spec_of("two-profiles"), None)).to_toml();
    for _ in 0..5 {
        assert_eq!(assert_ok!(resolve(&spec_of("two-profiles"), None)).to_toml(), first);
    }
}

// ---- the cases that are refused ---------------------------------------------------------------------------

#[test]
fn a_misspelled_key_is_named_with_its_line_and_the_close_key() {
    match assert_err!(resolve(&spec_of("unknown-key"), None)) {
        SpecError::UnknownKey {
            at, key, suggestion, ..
        } => {
            assert_eq!(
                (file_name(&at.path), at.line, at.column),
                ("acme.toml".to_string(), 8, 1)
            );
            assert_eq!(
                (key.as_str(), suggestion.as_deref()),
                ("select.tag", Some("select.tags"))
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_position_in_a_profile_is_refused_in_the_profile_file() {
    match assert_err!(resolve(&spec_of("position-in-profile"), None)) {
        SpecError::ForbiddenInProfile { at, key, .. } => {
            assert_eq!((file_name(&at.path), key), ("principal.toml".to_string(), "position"));
            assert_eq!(at.line, 4);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_cycle_prints_the_way_round() {
    match assert_err!(resolve(&spec_of("cycle"), None)) {
        SpecError::Cycle { at, path } => {
            assert_eq!(path, ["a", "b", "c", "a"]);
            assert_eq!((file_name(&at.path), at.line), ("c.toml".to_string(), 4));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_missing_profile_lists_the_ones_that_exist_and_where_it_looked() {
    match assert_err!(resolve(&spec_of("missing-profile"), None)) {
        SpecError::MissingProfile {
            at,
            name,
            searched,
            available,
        } => {
            assert_eq!(
                (file_name(&at.path), at.line, name.as_str()),
                ("acme.toml".to_string(), 4, "ai-platfrom")
            );
            assert_eq!(available, ["principal"]);
            assert!(searched.ends_with("positions/../profiles"), "{}", searched.display());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_spec_that_lists_a_profile_before_the_one_it_builds_on_is_refused() {
    match assert_err!(resolve(&spec_of("extends-order-conflict"), None)) {
        SpecError::ExtendsOrderConflict {
            listed_first,
            listed_after,
            at,
            must_come_first,
        } => {
            assert_eq!((listed_first.as_str(), listed_after.as_str()), ("b", "base"));
            assert_eq!(must_come_first, ["base", "b"]);
            assert_eq!(file_name(&at.path), "acme.toml");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_profiles_directory_that_is_given_is_the_one_searched() {
    let elsewhere = fixtures().join("diamond/profiles");
    match assert_err!(resolve(&spec_of("two-profiles"), Some(&elsewhere))) {
        SpecError::MissingProfile {
            searched, available, ..
        } => {
            assert_eq!(searched, elsewhere);
            assert_eq!(available, ["base", "left", "right"]);
        }
        other => panic!("{other:?}"),
    }
    let missing = fixtures().join("no-such-directory");
    assert!(matches!(
        assert_err!(resolve(&spec_of("two-profiles"), Some(&missing))),
        SpecError::ProfilesPathNotADirectory { .. }
    ));
    let a_file = fixtures().join("baseline.md");
    assert!(matches!(
        assert_err!(resolve(&spec_of("two-profiles"), Some(&a_file))),
        SpecError::ProfilesPathNotADirectory { .. }
    ));
}

#[test]
fn a_spec_beside_no_profiles_directory_says_how_to_name_one() {
    let dir = assert_ok!(tempfile::tempdir());
    let path = dir.path().join("acme.toml");
    assert_ok!(std::fs::write(
        &path,
        "extends = [\"x\"]\n[build]\ncontent = \"a.md\"\n"
    ));
    let error = assert_err!(resolve(&path, None));
    assert!(matches!(error, SpecError::ProfilesDirectoryMissing { .. }), "{error:?}");
    assert!(error.to_string().contains("--profiles DIR"), "{error}");
}

#[test]
fn every_refusal_reads_as_one_line_that_names_the_file() {
    for (case, needle) in [
        ("unknown-key", "acme.toml:8:1"),
        ("position-in-profile", "principal.toml:4:"),
        ("cycle", "a -> b -> c -> a"),
        ("missing-profile", "ai-platfrom"),
        ("extends-order-conflict", "list `base` first"),
    ] {
        let text = assert_err!(resolve(&spec_of(case), None)).to_string();
        assert!(text.contains(needle) && !text.contains('\n'), "{case}: {text}");
    }
}

// ---- files made on the spot --------------------------------------------------------------------------------

fn tree(profile: &str, bytes: &[u8]) -> (tempfile::TempDir, PathBuf) {
    let dir = assert_ok!(tempfile::tempdir());
    assert_ok!(std::fs::create_dir_all(dir.path().join("positions")));
    assert_ok!(std::fs::create_dir_all(dir.path().join("profiles")));
    assert_ok!(std::fs::write(
        dir.path().join(format!("profiles/{profile}.toml")),
        bytes
    ));
    let spec = dir.path().join("positions/acme.toml");
    assert_ok!(std::fs::write(
        &spec,
        format!("extends = [\"{profile}\"]\n[build]\ncontent = \"../b.md\"\n")
    ));
    (dir, spec)
}

#[test]
fn a_profile_with_a_byte_order_mark_and_crlf_endings_loads_and_reports_true_lines() {
    let (_dir, spec) = tree("p", "\u{feff}[select]\r\ntags = [\"ai\"]\r\n".as_bytes());
    assert_eq!(strings(&assert_ok!(resolve(&spec, None)).choices.select.tags), ["ai"]);
    let (_dir, spec) = tree("p", "\u{feff}[select]\r\ntag = [\"ai\"]\r\n".as_bytes());
    match assert_err!(resolve(&spec, None)) {
        SpecError::UnknownKey { at, .. } => assert_eq!((at.line, at.column), (2, 1)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_profile_that_is_not_utf8_or_not_a_file_is_refused() {
    let (_dir, spec) = tree("p", b"[select]\ntags = [\"\xff\"]\n");
    assert!(matches!(assert_err!(resolve(&spec, None)), SpecError::NotUtf8 { .. }));
    let (dir, spec) = tree("p", b"");
    assert_ok!(std::fs::remove_file(dir.path().join("profiles/p.toml")));
    assert_ok!(std::fs::create_dir(dir.path().join("profiles/p.toml")));
    assert!(matches!(assert_err!(resolve(&spec, None)), SpecError::NotAFile { .. }));
}

#[cfg(unix)]
#[test]
fn a_profile_that_is_a_link_out_of_the_directory_is_refused() {
    let (dir, spec) = tree("p", b"");
    let outside = dir.path().join("outside.toml");
    assert_ok!(std::fs::write(&outside, "[select]\ntags = [\"stolen\"]\n"));
    let link = dir.path().join("profiles/p.toml");
    assert_ok!(std::fs::remove_file(&link));
    assert_ok!(std::os::unix::fs::symlink(&outside, &link));
    match assert_err!(resolve(&spec, None)) {
        SpecError::ProfileOutsideRoot { name, .. } => assert_eq!(name, "p"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_profile_name_that_could_leave_the_directory_never_reaches_the_file_system() {
    for name in ["../x", "x/y", "X", "con", "a.b", "C:x"] {
        let dir = assert_ok!(tempfile::tempdir());
        assert_ok!(std::fs::create_dir_all(dir.path().join("profiles")));
        let spec = dir.path().join("acme.toml");
        assert_ok!(std::fs::write(
            &spec,
            format!("extends = [\"{name}\"]\n[build]\ncontent = \"a.md\"\n")
        ));
        let error = assert_err!(resolve(&spec, Some(&dir.path().join("profiles"))));
        assert!(matches!(error, SpecError::InvalidName { .. }), "{name}: {error:?}");
    }
}

// ---- what a reprinted spec and a chain record must keep ---------------------------------------------------------

fn spec_in(dir: &tempfile::TempDir, name: &str, text: &str) -> PathBuf {
    let path = dir.path().join(name);
    assert_ok!(std::fs::write(&path, text));
    path
}

#[test]
fn promote_and_core_only_survive_printing_and_reloading() {
    let dir = assert_ok!(tempfile::tempdir());
    let path = spec_in(
        &dir,
        "acme.toml",
        "[build]\ncontent = \"a.md\"\n[select]\npromote = [\"people\", \"scale\"]\ncore-only = true\n",
    );
    let spec = assert_ok!(resolve(&path, None));
    let text = spec.to_toml();
    assert!(text.contains("promote = [\"people\", \"scale\"]"), "{text}");
    assert!(text.contains("core-only = true"), "{text}");
    let flat = spec_in(&dir, "flat.toml", &text);
    assert_eq!(assert_ok!(resolve(&flat, None)).choices, spec.choices);
}

#[test]
fn a_select_table_is_printed_when_only_promote_or_only_core_only_is_set() {
    let dir = assert_ok!(tempfile::tempdir());
    for body in ["promote = [\"people\"]", "core-only = false"] {
        let path = spec_in(
            &dir,
            "acme.toml",
            &format!("[build]\ncontent = \"a.md\"\n[select]\n{body}\n"),
        );
        assert!(
            assert_ok!(resolve(&path, None)).to_toml().contains("[select]"),
            "{body}"
        );
    }
}

#[test]
fn every_label_of_the_position_survives_printing_and_reloading() {
    let dir = assert_ok!(tempfile::tempdir());
    let path = spec_in(
        &dir,
        "acme.toml",
        "[position]\nlevel = \"l\"\ndomain = \"d\"\naudience = \"a\"\ncompany = \"c\"\ntitle = \"t\"\n[build]\ncontent = \"a.md\"\n",
    );
    let spec = assert_ok!(resolve(&path, None));
    let flat = spec_in(&dir, "flat.toml", &spec.to_toml());
    let again = assert_ok!(resolve(&flat, None));
    assert_eq!(again.position, spec.position);
    let position = assert_some!(again.position);
    assert_eq!(assert_some!(position.domain).as_ref(), "d");
    assert_eq!(assert_some!(position.audience).as_ref(), "a");
}

#[test]
fn each_chain_link_records_what_its_file_extends_in_the_order_it_was_written() {
    let spec = assert_ok!(resolve(&spec_of("two-profiles"), None));
    let last = assert_some!(spec.chain.last());
    let declared: Vec<&str> = last.declared_extends.iter().map(AsRef::as_ref).collect();
    assert_eq!(declared, ["principal", "ai-platform"]);
    assert!(spec.chain[0].declared_extends.is_empty());
    assert!(spec.chain[0].path.ends_with("principal.toml"));
}

#[test]
fn a_position_in_a_profile_is_refused_however_toml_spells_it() {
    for text in [
        "[position.emphasis]\nbusiness = \"low\"\n",
        "position.company = \"A\"\n",
    ] {
        let (_dir, spec) = tree("p", text.as_bytes());
        match assert_err!(resolve(&spec, None)) {
            SpecError::ForbiddenInProfile { key, at, .. } => {
                assert_eq!((key, at.line), ("position", 1), "{text:?}");
                assert_eq!(file_name(&at.path), "p.toml");
            }
            other => panic!("{text:?}: {other:?}"),
        }
    }
}

#[test]
fn a_content_path_that_names_a_directory_or_climbs_out_by_name_only_is_refused_at_load() {
    let dir = assert_ok!(tempfile::tempdir());
    for content in [".", "..", "a/.."] {
        let path = spec_in(&dir, "acme.toml", &format!("[build]\ncontent = \"{content}\"\n"));
        assert!(
            matches!(assert_err!(resolve(&path, None)), SpecError::InvalidPath { .. }),
            "{content}"
        );
    }
}

#[cfg(windows)]
#[test]
fn a_spec_given_with_backslashes_and_a_verbatim_prefix_resolves_like_one_given_with_slashes() {
    let slashes = spec_of("two-profiles");
    let backslashes = PathBuf::from(slashes.to_string_lossy().replace('/', "\\"));
    let verbatim = assert_ok!(std::fs::canonicalize(&slashes));
    assert!(
        verbatim.to_string_lossy().starts_with(r"\\?\"),
        "{}",
        verbatim.display()
    );
    let want = assert_ok!(resolve(&slashes, None)).choices;
    for spec in [backslashes, verbatim] {
        assert_eq!(assert_ok!(resolve(&spec, None)).choices, want, "{}", spec.display());
    }
}
