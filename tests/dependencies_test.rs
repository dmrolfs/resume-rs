//! ABOUTME: Guards the dependency manifest: exact Typst pins and no network stack in the tree.
//! ABOUTME: Typst is pre-1.0, so a drifting version would change output bytes silently.

use std::path::PathBuf;

fn read_project_file(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The version requirement written for a dependency in `[dependencies]`, if any.
fn requirement_of(manifest: &str, dependency: &str) -> Option<String> {
    let prefix = format!("{dependency} ");
    let line = manifest
        .lines()
        .find(|l| l.starts_with(&prefix) || l.starts_with(&format!("{dependency}=")))?;
    let after_version = line.split("version").nth(1).unwrap_or(line);
    let start = after_version.find('"')? + 1;
    let end = start + after_version[start..].find('"')?;
    Some(after_version[start..end].to_string())
}

#[test]
fn typst_crates_are_pinned_exactly() {
    let manifest = read_project_file("Cargo.toml");
    for dependency in ["typst", "typst-pdf", "typst-kit", "typst-layout"] {
        assert_eq!(
            requirement_of(&manifest, dependency).as_deref(),
            Some("=0.15.1"),
            "{dependency} must be pinned with `=0.15.1`"
        );
    }
}

#[test]
fn no_network_stack_in_dependency_tree() {
    let lock = read_project_file("Cargo.lock");
    let forbidden = [
        "ureq",
        "native-tls",
        "openssl",
        "openssl-sys",
        "reqwest",
        "hyper",
        "rustls",
    ];
    let present: Vec<&str> = forbidden
        .iter()
        .copied()
        .filter(|name| lock.lines().any(|l| l == format!("name = \"{name}\"")))
        .collect();
    assert!(present.is_empty(), "network crates in Cargo.lock: {present:?}");
}
