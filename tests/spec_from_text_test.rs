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
    assert_eq!(layer.layer().path.as_path(), nowhere);
}
