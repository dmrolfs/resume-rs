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

/// `(name, version)` of every package in `Cargo.lock`.
fn locked_packages() -> Vec<(String, String)> {
    let lock = read_project_file("Cargo.lock");
    let mut packages = Vec::new();
    let mut name: Option<String> = None;
    for line in lock.lines() {
        if let Some(n) = line.strip_prefix("name = \"") {
            name = n.strip_suffix('"').map(str::to_string);
        } else if let (Some(v), Some(n)) = (line.strip_prefix("version = \""), name.take()) {
            packages.push((n, v.trim_end_matches('"').to_string()));
        }
    }
    packages
}

#[test]
fn every_typst_crate_in_the_lock_file_is_the_pinned_version() {
    let typst: Vec<_> = locked_packages()
        .into_iter()
        .filter(|(n, _)| n == "typst" || n.starts_with("typst-"))
        .collect();
    assert!(typst.len() >= 8, "expected the whole Typst family, found {typst:?}");
    let off: Vec<_> = typst.iter().filter(|(_, v)| v != "0.15.1").collect();
    assert!(off.is_empty(), "Typst crates not at 0.15.1: {off:?}");
}

#[test]
fn the_pdf_writer_is_the_version_the_output_was_verified_with() {
    // krilla writes the PDF bytes; a bump can change them, so it moves only with a deliberate decision.
    let krilla: Vec<_> = locked_packages().into_iter().filter(|(n, _)| n == "krilla").collect();
    assert_eq!(krilla, [("krilla".to_string(), "0.8.2".to_string())]);
}
