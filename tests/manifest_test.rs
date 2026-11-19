//! ABOUTME: Guards the one dependency the unused-dependency checks are told to ignore: `regex`.
//! ABOUTME: Both the cargo lint and cargo-machete are blind to it, so a test must notice when it stops being needed.

use std::fs;
use std::path::{Path, PathBuf};

use claims::assert_ok;

fn crate_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

/// Every `.rs` file under `dir`, read into one string.
fn sources_under(dir: &Path) -> String {
    let mut all = String::new();
    for entry in assert_ok!(fs::read_dir(dir)) {
        let path = assert_ok!(entry).path();
        if path.is_dir() {
            all.push_str(&sources_under(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            all.push_str(&assert_ok!(fs::read_to_string(&path)));
        }
    }
    all
}

/// `regex` is not named anywhere in `src/`: `nutype(validate(regex = ...))` generates the code that uses it. That
/// makes it invisible to cargo-machete (so Cargo.toml tells machete to ignore it), and it is also a transitive
/// dependency (so the cargo lint cannot see it either). If the validation goes away, `regex` becomes a real unused
/// dependency that nothing in the gate would report: this test is what reports it.
#[test]
fn regex_is_a_dependency_exactly_while_nutype_validates_with_it() {
    let manifest = assert_ok!(fs::read_to_string(crate_path("Cargo.toml")));
    let needed = sources_under(&crate_path("src")).contains("validate(regex");
    let listed = manifest.lines().any(|line| line.starts_with("regex = "));
    let ignored = manifest.contains("ignored = [\"regex\"]");

    assert!(
        !needed || listed,
        "src/ uses `validate(regex = ...)`, so `regex` must be a dependency in Cargo.toml"
    );
    assert!(
        needed || (!listed && !ignored),
        "nothing in src/ uses `validate(regex = ...)` any more: remove `regex` from [dependencies] and its \
         [package.metadata.cargo-machete] ignore entry in Cargo.toml"
    );
    assert!(
        !needed || ignored,
        "machete cannot see nutype's generated use of `regex`, so Cargo.toml must keep it in \
         [package.metadata.cargo-machete] ignored"
    );
}
