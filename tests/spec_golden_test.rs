//! ABOUTME: Pins what every existing spec fixture resolves to (printed spec, origins, or error text), so a change to the loader cannot alter a resume-only spec unseen.
//! ABOUTME: The snapshot in tests/fixtures/specs/golden.txt was taken before the letter table existed; bless it only for an intended change.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use resume::spec::resolve;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/specs")
}

/// Every spec file of the resume-only cases, in a fixed order. `letter-scopes` is the letter case and is not here.
fn specs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut cases: Vec<_> = std::fs::read_dir(root())
        .expect("fixtures exist")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir() && !p.ends_with("letter-scopes"))
        .collect();
    cases.sort();
    for case in cases {
        let mut files: Vec<_> = std::fs::read_dir(case.join("positions"))
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect();
        files.sort();
        out.extend(files);
    }
    out
}

fn shown(text: &str) -> String {
    // Separators first, so a Windows root (backslashes, or a mix in an error text) is found the same way.
    let base = root().display().to_string().replace('\\', "/");
    text.replace('\\', "/").replace(&base, "<fixtures>")
}

fn snapshot() -> String {
    let mut out = String::new();
    for spec in specs() {
        let name = shown(&spec.display().to_string());
        writeln!(out, "=== {name}").expect("a string takes writes");
        match resolve(&spec, None) {
            Ok(resolved) => {
                out.push_str(&shown(&resolved.to_toml()));
                writeln!(out, "content: {}", shown(&resolved.content.path.display().to_string())).expect("write");
                for (key, at) in resolved.origins() {
                    let file = at
                        .path
                        .file_name()
                        .map_or_else(String::new, |f| f.to_string_lossy().into_owned());
                    writeln!(out, "origin {key:?} {file}:{}:{}", at.line, at.column).expect("write");
                }
            }
            Err(error) => writeln!(out, "error: {}", shown(&error.to_string())).expect("write"),
        }
    }
    out
}

#[test]
fn every_resume_only_fixture_resolves_as_it_did_before_the_letter_table() {
    let golden = root().join("golden.txt");
    let now = snapshot();
    if std::env::var_os("RESUME_BLESS_GOLDEN").is_some() {
        std::fs::write(&golden, &now).expect("the snapshot can be written");
        return;
    }
    // A Windows checkout may have turned the line ends into CRLF.
    let before = std::fs::read_to_string(&golden)
        .expect("golden.txt exists")
        .replace("\r\n", "\n");
    pretty_assertions::assert_eq!(before, now);
}
