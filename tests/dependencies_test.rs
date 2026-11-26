//! ABOUTME: Guards the dependency manifest: exact Typst pins, and no network stack in what `resume` links.
//! ABOUTME: Typst is pre-1.0, so a drifting version would change output bytes silently.

use std::path::PathBuf;
use std::process::Command;

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

/// Crates that open network connections. The `resume` binary must never link one.
const NETWORK_CRATES: [&str; 7] = [
    "ureq",
    "native-tls",
    "openssl",
    "openssl-sys",
    "reqwest",
    "hyper",
    "rustls",
];

/// The forbidden crates named in `cargo tree --prefix none` output (one `name vX.Y.Z ...` per line).
fn network_crates_in(tree: &str) -> Vec<&str> {
    NETWORK_CRATES
        .iter()
        .copied()
        .filter(|name| tree.lines().any(|line| line.split_whitespace().next() == Some(name)))
        .collect()
}

/// Runs `cargo` in the project directory and returns its standard output, failing the test on a non-zero exit.
fn cargo_output(args: &[&str]) -> String {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("cannot run cargo {args:?}: {e}"));
    assert!(
        output.status.success(),
        "cargo {args:?} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("cargo prints UTF-8")
}

/// The workspace shares one `Cargo.lock`, and the desktop app brings network crates into it. What matters is what
/// the `resume` package itself links: its normal and build dependencies, for every target.
#[test]
fn no_network_stack_in_the_resume_dependency_tree() {
    let tree = cargo_output(&[
        "tree",
        "--package",
        "resume",
        "--edges",
        "normal,build",
        "--target",
        "all",
        "--prefix",
        "none",
        "--locked",
    ]);
    assert!(
        tree.lines().any(|l| l.starts_with("typst ")),
        "the tree must list Typst, or it listed nothing:\n{tree}"
    );
    let present = network_crates_in(&tree);
    assert!(present.is_empty(), "network crates linked by `resume`: {present:?}");
}

#[test]
fn the_network_check_names_a_crate_only_on_a_whole_name_match() {
    let tree = "resume v0.1.0 (/x)\nhyper v1.13.0\nhyper-util v0.1.21\nrustls-pki-types v1.16.0\n";
    assert_eq!(
        network_crates_in(tree),
        ["hyper"],
        "`hyper-util` and `rustls-pki-types` are other crates"
    );
    assert!(network_crates_in("resume v0.1.0\nurl v2.5.8\n").is_empty());
}

/// `Cargo.toml` and `crates/workbench/Cargo.toml` as `cargo metadata` reads them: one root package, one app member.
fn workspace_packages() -> Vec<serde_json::Value> {
    let metadata = cargo_output(&["metadata", "--no-deps", "--format-version", "1", "--locked"]);
    let metadata: serde_json::Value = serde_json::from_str(&metadata).expect("cargo metadata prints JSON");
    metadata["packages"].as_array().expect("packages is a list").clone()
}

fn rust_version_of(package: &str) -> Option<String> {
    workspace_packages()
        .into_iter()
        .find(|p| p["name"] == package)
        .and_then(|p| p["rust_version"].as_str().map(str::to_string))
}

#[test]
fn the_app_is_a_workspace_member_beside_the_resume_package() {
    let mut names: Vec<String> = workspace_packages()
        .iter()
        .filter_map(|p| p["name"].as_str().map(str::to_string))
        .collect();
    names.sort();
    assert_eq!(names, ["resume", "workbench"]);
}

#[test]
fn the_cli_keeps_its_lower_rust_floor_than_the_app() {
    // gpui-pre 0.3.14 needs Rust 1.95 (measured in rsm-yjj); the CLI must not be dragged up with it.
    assert_eq!(rust_version_of("resume").as_deref(), Some("1.92"));
    assert_eq!(rust_version_of("workbench").as_deref(), Some("1.95"));
}

#[test]
fn the_published_resume_crate_holds_no_app_files() {
    // Cargo may print `\` as the separator on Windows, so compare with `/` everywhere.
    let listing =
        cargo_output(&["package", "--package", "resume", "--list", "--allow-dirty", "--locked"]).replace('\\', "/");
    let app_files: Vec<&str> = listing.lines().filter(|l| l.starts_with("crates/")).collect();
    assert!(
        app_files.is_empty(),
        "the resume package would ship app files: {app_files:?}"
    );
    assert!(
        listing.lines().any(|l| l == "src/main.rs"),
        "the listing must be the resume package's:\n{listing}"
    );
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
