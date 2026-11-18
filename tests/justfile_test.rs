//! ABOUTME: Guards the justfile's cross-platform rules, because CI loads it on both macOS and Windows.
//! ABOUTME: Reads recipes with `just --dump --dump-format json`, so a Windows load failure is caught here too.

use std::{path::Path, process::Command};

use serde_json::Value;

/// Platform attributes `just` accepts on a recipe.
const PLATFORM_ATTRIBUTES: [&str; 5] = ["unix", "macos", "linux", "windows", "openbsd"];

/// Dumps a justfile as JSON through the installed `just`, failing loudly if `just` is missing or cannot load it.
fn dump(justfile: &Path) -> Value {
    let output = Command::new("just")
        .arg("--justfile")
        .arg(justfile)
        .args(["--dump", "--dump-format", "json"])
        .output()
        .expect("`just` must be installed to test the justfile (the quality gate needs it anyway)");
    assert!(
        output.status.success(),
        "`just` could not load {}:\n{}",
        justfile.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("`just --dump-format json` prints JSON")
}

/// True when a recipe carries a platform attribute such as `[unix]` or `[macos]`.
fn is_platform_gated(recipe: &Value) -> bool {
    recipe["attributes"]
        .as_array()
        .map(|attrs| {
            attrs
                .iter()
                .filter_map(Value::as_str)
                .any(|a| PLATFORM_ATTRIBUTES.contains(&a))
        })
        .unwrap_or(false)
}

/// Lists every way the justfile could fail to load or run on Windows.
///
/// A recipe with no platform attribute runs on every OS, so it must not be a bash script (Windows uses
/// PowerShell) and must not depend on a platform-gated recipe (which does not exist on other OSes, so
/// `just` would refuse to load the file there).
fn platform_violations(justfile: &Path) -> Vec<String> {
    let dumped = dump(justfile);
    let recipes = dumped["recipes"]
        .as_object()
        .expect("justfile JSON has a `recipes` object");
    let mut violations = Vec::new();
    for (name, recipe) in recipes {
        if is_platform_gated(recipe) {
            continue;
        }
        if recipe["shebang"].as_bool().unwrap_or(false) {
            violations.push(format!("`{name}` is a script recipe with no platform attribute"));
        }
        for dependency in recipe["dependencies"].as_array().into_iter().flatten() {
            let target = dependency["recipe"].as_str().unwrap_or_default();
            if recipes.get(target).is_some_and(is_platform_gated) {
                violations.push(format!("`{name}` depends on platform-gated `{target}`"));
            }
        }
    }
    violations.sort();
    violations
}

fn crate_path(relative: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)
}

#[test]
fn the_real_justfile_follows_the_platform_rule() {
    let violations = platform_violations(&crate_path("justfile"));
    assert!(
        violations.is_empty(),
        "justfile would break on Windows:\n{}",
        violations.join("\n")
    );
}

#[test]
fn the_checker_detects_a_gated_dependency_and_an_ungated_script() {
    // Proves the rule is not vacuous: a justfile built to break it must be reported.
    let violations = platform_violations(&crate_path("tests/fixtures/justfiles/platform-violation.just"));
    assert_eq!(
        violations,
        vec![
            "`broken-dependency` depends on platform-gated `gated-helper`".to_string(),
            "`broken-shebang` is a script recipe with no platform attribute".to_string(),
        ]
    );
}

#[test]
fn every_listed_recipe_has_a_doc_and_a_group() {
    // `just --list` shows only the comment on the line above a recipe, which was misleading; explicit
    // `[doc]` and `[group]` attributes keep the listing accurate and organized.
    let dumped = dump(&crate_path("justfile"));
    let mut problems = Vec::new();
    for (name, recipe) in dumped["recipes"].as_object().expect("recipes object") {
        let attrs: Vec<&Value> = recipe["attributes"].as_array().into_iter().flatten().collect();
        let is_private = name.starts_with('_') || attrs.iter().any(|a| a.as_str() == Some("private"));
        let has = |key: &str| attrs.iter().any(|a| a.get(key).is_some());
        if !is_private && !has("doc") {
            problems.push(format!("`{name}` has no [doc(...)]"));
        }
        if !is_private && !has("group") {
            problems.push(format!("`{name}` has no [group(...)]"));
        }
    }
    problems.sort();
    assert!(
        problems.is_empty(),
        "recipes missing metadata:\n{}",
        problems.join("\n")
    );
}

#[test]
fn the_ci_leg_is_the_gate_plus_a_locked_release_build() {
    let dumped = dump(&crate_path("justfile"));
    let deps: Vec<&str> = dumped["recipes"]["ci"]["dependencies"]
        .as_array()
        .expect("`ci` recipe exists")
        .iter()
        .filter_map(|d| d["recipe"].as_str())
        .collect();
    assert_eq!(deps, ["check", "release"], "`just ci` must reproduce one CI leg");
}
