//! ABOUTME: `SpecLayer::from_text` must accept and refuse exactly what `SpecLayer::load` does, with the same locations.
//! ABOUTME: Every fixture spec is read both ways, then bad texts are written to a temp file and compared the same way.

use std::path::{Path, PathBuf};

use claims::{assert_err, assert_ok};
use pretty_assertions::assert_eq;
use resume::spec::{SpecError, SpecLayer};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/specs")
}

/// Every `.toml` under the spec fixtures, specs and profiles alike, in a fixed order.
fn fixture_files() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![fixtures()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("the fixture directory is readable") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|e| e == "toml") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// Reads `path` as text and loads it both ways, returning what each said as comparable text.
fn both_ways(path: &Path) -> (Result<SpecLayer, SpecError>, Result<SpecLayer, SpecError>) {
    let text = std::fs::read_to_string(path).expect("the fixture is UTF-8");
    (SpecLayer::load(path), SpecLayer::from_text(path, &text))
}

/// The two results are the same layer, or the same error: the message carries the file, line and column.
fn assert_same(path: &Path, loaded: &Result<SpecLayer, SpecError>, parsed: &Result<SpecLayer, SpecError>) {
    match (loaded, parsed) {
        (Ok(a), Ok(b)) => assert_eq!(a, b, "{}", path.display()),
        (Err(a), Err(b)) => {
            assert_eq!(a.to_string(), b.to_string(), "{}", path.display());
            assert_eq!(a.hint(), b.hint(), "{}", path.display());
        }
        (a, b) => panic!("{}: load said {a:?}, from_text said {b:?}", path.display()),
    }
}

#[test]
fn every_fixture_spec_reads_the_same_from_text_as_from_its_file() {
    let files = fixture_files();
    assert!(files.len() >= 20, "the fixture set is found: {}", files.len());
    let mut specs = 0;
    for path in &files {
        let (loaded, parsed) = both_ways(path);
        assert_same(path, &loaded, &parsed);
        specs += usize::from(loaded.is_ok());
    }
    assert!(
        specs >= 8,
        "the comparison covers real specs, not only refusals: {specs}"
    );
}

/// Writes `text` to `name` in a fresh directory and loads it both ways.
fn compare_text(name: &str, text: &str) -> (Result<SpecLayer, SpecError>, Result<SpecLayer, SpecError>) {
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join(name);
    std::fs::write(&path, text).expect("the file is written");
    let loaded = SpecLayer::load(&path);
    let parsed = SpecLayer::from_text(&path, text);
    assert_same(&path, &loaded, &parsed);
    (loaded, parsed)
}

#[test]
fn a_bad_text_is_refused_with_the_same_message_and_location_both_ways() {
    let cases = [
        ("syntax.toml", "[build\ncontent = \"a.md\"\n"),
        ("unknown-key.toml", "[build]\ncontent = \"a.md\"\ncontnt = 1\n"),
        ("wrong-type.toml", "[build]\ncontent = 3\n"),
        ("no-content.toml", "[select]\ntags = [\"ai\"]\n"),
        (
            "bad-tag.toml",
            "[select]\ntags = [\"Not A Tag\"]\n[build]\ncontent = \"a.md\"\n",
        ),
        ("empty.toml", ""),
        ("unicode.toml", "# résumé 履歴書\n[build]\ncontent = \"a.md\"\nx = 1\n"),
        ("crlf.toml", "[build]\r\ncontent = \"a.md\"\r\nbad = 1\r\n"),
    ];
    for (name, text) in cases {
        let (loaded, parsed) = compare_text(name, text);
        // Both refuse: agreeing on success would prove nothing about the validation.
        assert_err!(loaded);
        assert_err!(parsed);
    }
}

#[test]
fn a_text_that_is_a_valid_spec_loads_both_ways() {
    let (loaded, parsed) = compare_text("ok.toml", "[build]\ncontent = \"a.md\"\n");
    assert_ok!(loaded);
    assert_ok!(parsed);
}

#[test]
fn an_error_names_the_path_given_even_when_no_such_file_exists() {
    let path = Path::new("positions/not-saved-yet.toml");
    let error = assert_err!(SpecLayer::from_text(path, "[build]\ncontent = 3\n"));
    let shown = error.to_string();
    assert!(shown.contains("positions/not-saved-yet.toml:2:"), "{shown}");
}

/// A spec of exactly `len` bytes: a valid one padded with comment text.
fn spec_of_len(len: usize) -> String {
    let head = "[build]\ncontent = \"a.md\"\n# ";
    format!("{head}{}", "x".repeat(len - head.len()))
}

#[test]
fn a_text_over_the_size_cap_is_refused_as_a_file_would_be() {
    let (loaded, parsed) = compare_text("big.toml", &spec_of_len(64 * 1024 + 1));
    assert!(matches!(assert_err!(loaded), SpecError::TooLarge { .. }));
    assert!(matches!(assert_err!(parsed), SpecError::TooLarge { .. }));
}

#[test]
fn a_text_of_exactly_the_cap_is_accepted_both_ways() {
    let (loaded, parsed) = compare_text("cap.toml", &spec_of_len(64 * 1024));
    assert_ok!(loaded);
    assert_ok!(parsed);
}

#[test]
fn a_saved_profile_cannot_replace_unsaved_spec_text_at_the_same_path() {
    let dir = assert_ok!(tempfile::tempdir());
    let path = dir.path().join("base.toml");
    assert_ok!(std::fs::write(&path, "[select]\ntags = ['rust']\n"));
    let layer = assert_ok!(SpecLayer::from_text(
        &path,
        "extends = ['base']\n[build]\ncontent = 'missing.md'\n"
    ));
    let error = assert_err!(resume::spec::resolve_with(
        layer,
        &resume::spec::DirSource::new(dir.path().to_owned())
    ));
    assert!(matches!(error, SpecError::ConflictingSources { path: ref conflicted } if conflicted == &path));
    assert!(error.to_string().contains("different loaded texts"), "{error}");
}

#[test]
fn diamond_snapshots_follow_chain_identity_and_print_like_disk_loaded_specs() {
    let dir = assert_ok!(tempfile::tempdir());
    for (name, text) in [
        ("base", "[select]\ntags = ['rust']\n"),
        ("left", "extends = ['base']\n"),
        ("right", "extends = ['base']\n"),
    ] {
        assert_ok!(std::fs::write(dir.path().join(format!("{name}.toml")), text));
    }
    let path = dir.path().join("spec.toml");
    let text = "extends = ['left', 'right']\n[build]\ncontent = 'missing.md'\n";
    assert_ok!(std::fs::write(&path, text));
    let profiles = resume::spec::DirSource::new(dir.path().to_owned());
    let disk = assert_ok!(resume::spec::resolve_with(
        assert_ok!(SpecLayer::load(&path)),
        &profiles
    ));
    let memory = assert_ok!(resume::spec::resolve_with(
        assert_ok!(SpecLayer::from_text(&path, text)),
        &profiles
    ));
    assert_eq!(disk.to_toml(), memory.to_toml());
    assert_eq!(disk.sources().len(), 4);
    for (link, snapshot) in disk.chain.iter().zip(disk.sources()) {
        assert_eq!(link.path, snapshot.path());
    }
    assert_ok!(std::fs::write(&path, "changed"));
    assert_eq!(disk.sources()[3].text(), text);
    assert_eq!(disk.to_toml(), memory.to_toml());
}

#[test]
fn snapshots_survive_resolution_disk_changes_and_missing_baselines() {
    use claims::assert_some;
    use resume::diagnostics::{SourceKind, Sources};
    let dir = assert_ok!(tempfile::tempdir());
    let path = dir.path().join("position.toml");
    let profile = dir.path().join("base.toml");
    let profile_text = "[select]\ntags = ['rust']\n";
    assert_ok!(std::fs::write(&profile, profile_text));
    let edited = "extends = ['base']\n# unsaved é\n[build]\ncontent = 'missing.md'\n";
    let layer = assert_ok!(SpecLayer::from_text(&path, edited));
    assert_eq!(layer.layer().source().text(), edited);
    let resolved = assert_ok!(resume::spec::resolve_with(
        layer,
        &resume::spec::DirSource::new(dir.path().to_owned())
    ));
    assert_ok!(std::fs::write(&path, "changed"));
    assert_ok!(std::fs::remove_file(&profile));
    let snapshots = resolved.sources();
    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0].path(), profile);
    assert_eq!(snapshots[0].text(), profile_text);
    assert_eq!(snapshots[1].path(), path);
    assert_eq!(snapshots[1].text(), edited);
    assert_eq!(snapshots[1].kind(), SourceKind::Spec);
    let mut sources = Sources::new(Some(dir.path().to_owned()));
    for snapshot in snapshots {
        sources.insert_text(snapshot);
    }
    let profile_span = assert_some!(snapshots[0].span(17..21));
    let profile_position = assert_some!(sources.position(&profile_span));
    assert_eq!(
        (
            profile_position.file.as_str(),
            profile_position.line,
            profile_position.column
        ),
        ("base.toml", 2, 9)
    );
    let offset = assert_some!(edited.find("missing.md"));
    let span = assert_some!(snapshots[1].span(offset..offset + 10));
    let position = assert_some!(sources.position(&span));
    assert_eq!(
        (position.file.as_str(), position.line, position.column),
        ("position.toml", 4, 12)
    );
    assert!(!resolved.to_toml().contains("unsaved"));
    let diagnostic = resume::diagnostics::Diagnostic {
        severity: resume::diagnostics::Severity::Warning,
        code: resume::diagnostics::DiagnosticCode::UnknownTag,
        message: "snapshot rendering probe".into(),
        span: Some(span),
        hints: vec![],
        fix: None,
    };
    let mut human = resume::diagnostics::termcolor::Buffer::no_color();
    assert_ok!(resume::diagnostics::emit_human(&mut human, &sources, [&diagnostic]));
    let human = assert_ok!(std::str::from_utf8(human.as_slice()));
    assert!(human.contains("missing.md"), "{human}");
    assert!(!human.contains("source not available"), "{human}");
    let mut json = Vec::new();
    assert_ok!(resume::diagnostics::emit_json(&mut json, &sources, [&diagnostic]));
    let record: serde_json::Value = assert_ok!(serde_json::from_slice(&json));
    assert_eq!(record["file"], "position.toml");
    assert_eq!(record["line"], 4);
    assert_eq!(record["column"], 12);
}

#[test]
fn the_text_given_is_what_is_read_not_the_file_saved_at_that_path() {
    // An editor validates what is on screen, not the last save.
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = dir.path().join("acme.toml");
    std::fs::write(&path, "[build]\ncontent = \"saved.md\"\n").expect("the file is written");
    let edited = "[build]\ncontent = \"edited.md\"\n";
    let layer = assert_ok!(SpecLayer::from_text(&path, edited));
    let content = layer.layer().content.as_ref().expect("the spec names its content");
    assert_eq!(content.value.as_str(), "edited.md");
    // And a saved spec that is fine does not hide edited text that is not.
    let error = assert_err!(SpecLayer::from_text(&path, "[build]\ncontent = 3\n"));
    assert!(error.to_string().contains(":2:"), "{error}");
    // A path with no file behind it reads the text too.
    let nowhere = Path::new("unsaved/new.toml");
    let layer = assert_ok!(SpecLayer::from_text(nowhere, edited));
    assert_eq!(layer.layer().path(), nowhere);
}
