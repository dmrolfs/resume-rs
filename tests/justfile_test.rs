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
#[cfg(not(windows))]
fn the_checker_detects_a_gated_dependency_and_an_ungated_script() {
    // Proves the rule is not vacuous: a justfile built to break it must be reported. This fixture only loads
    // where its `[unix]` recipe exists; on Windows `just` itself refuses it (see the next test).
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
fn a_gated_dependency_really_fails_to_load_on_another_os() {
    // Emulates how another OS sees the fixture, which is exactly the situation that broke CI twice. On Windows
    // the `[unix]` helper is already absent; anywhere else, swap it for `[windows]`, which is absent there.
    // (Swapping unconditionally would make the helper exist on Windows and the file would load.) Runs on every OS.
    let fixture = std::fs::read_to_string(crate_path("tests/fixtures/justfiles/platform-violation.just"))
        .expect("fixture is readable");
    let absent_here = if cfg!(windows) { "[unix]" } else { "[windows]" };
    let windows_view = fixture.replace("[unix]", absent_here);
    let dir = std::env::temp_dir().join(format!("resume-justfile-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("windows-view.just");
    std::fs::write(&path, windows_view).expect("write temp justfile");
    let output = Command::new("just")
        .arg("--justfile")
        .arg(&path)
        .args(["--dump", "--dump-format", "json"])
        .output();
    let _ = std::fs::remove_dir_all(&dir);
    let output = output.expect("`just` must be installed to test the justfile");
    assert!(
        !output.status.success(),
        "a gated dependency must stop `just` from loading the file"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown dependency"),
        "unexpected error from just:\n{stderr}"
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

#[test]
fn the_gate_is_formatting_lints_unused_dependencies_and_tests() {
    let dumped = dump(&crate_path("justfile"));
    let deps: Vec<&str> = dumped["recipes"]["check"]["dependencies"]
        .as_array()
        .expect("`check` recipe exists")
        .iter()
        .filter_map(|d| d["recipe"].as_str())
        .collect();
    assert_eq!(
        deps,
        ["fmt", "clippy", "unused-deps", "test"],
        "`just check` is the gate beads tasks run and CI runs"
    );
}

#[test]
fn the_unused_dependency_check_is_a_plain_cargo_machete_run() {
    let dumped = dump(&crate_path("justfile"));
    let recipe = &dumped["recipes"]["unused-deps"];
    assert!(recipe.is_object(), "`unused-deps` recipe exists");
    assert!(
        !is_platform_gated(recipe),
        "the gate runs on Windows too, so the recipe must carry no platform attribute"
    );
    let body = recipe["body"].to_string();
    assert!(body.contains("cargo machete"), "{body}");
    // Without `--with-metadata`, machete only reads [dependencies]; with it, [dev-dependencies] are checked
    // too, and the cargo lint in Cargo.toml never checks those.
    assert!(body.contains("--with-metadata"), "{body}");
    // Clippy's all-targets run builds the test units, so a normal dependency used only by `#[cfg(test)]` code
    // looks used there. Checking the library and binary alone is what shows it is not.
    assert!(body.contains("cargo check --workspace --lib --bins"), "{body}");
}

/// The command lines of a recipe, one `Vec` of words each. A recipe body in the JSON dump is a list of lines, each a
/// list of text and `{ "variable": .. }` parts; text parts are joined and comment lines are dropped, so a commented-out
/// command is not counted.
fn command_lines(recipe: &Value) -> Vec<Vec<String>> {
    recipe["body"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|line| {
            line.as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<String>()
        })
        .filter(|line| !line.trim_start().starts_with('#'))
        .map(|line| line.split_whitespace().map(str::to_string).collect())
        .collect()
}

fn has_command(recipe: &Value, words: &[&str]) -> bool {
    command_lines(recipe)
        .iter()
        .any(|line| words.iter().all(|w| line.iter().any(|x| x == w)))
}

#[test]
fn the_gate_covers_every_workspace_member() {
    // The desktop app is a workspace member, not the root package, so a plain `cargo test` or `cargo clippy` would
    // skip it. Word order does not matter here, and a flag such as `--exclude` would fail the check.
    let dumped = dump(&crate_path("justfile"));
    for (recipe, words) in [
        ("clippy", &["cargo", "clippy", "--workspace", "--all-targets"][..]),
        ("test", &["cargo", "test", "--workspace"][..]),
        ("unused-deps", &["cargo", "check", "--workspace", "--lib", "--bins"][..]),
    ] {
        let found = &dumped["recipes"][recipe];
        assert!(
            has_command(found, words),
            "`{recipe}` must run {words:?}: {:?}",
            command_lines(found)
        );
        assert!(
            !command_lines(found)
                .iter()
                .flatten()
                .any(|w| w == "--exclude" || w == "--package" || w == "-p"),
            "`{recipe}` must cover every member: {:?}",
            command_lines(found)
        );
    }
}

#[test]
fn the_release_build_is_the_resume_package_alone() {
    // `build --release` at the root builds the default member today; naming the package keeps it that way if the app
    // joins `default-members` or the recipe is run from the workspace.
    let dumped = dump(&crate_path("justfile"));
    let release = &dumped["recipes"]["release"];
    assert!(
        has_command(
            release,
            &["cargo", "build", "--release", "--locked", "--package", "resume"]
        ),
        "{:?}",
        command_lines(release)
    );
    assert!(
        !command_lines(release)
            .iter()
            .flatten()
            .any(|w| w == "--workspace" || w == "--all")
    );
    // The cross-builds are script recipes; their cargo lines must name the package too.
    let text = std::fs::read_to_string(crate_path("justfile")).expect("the justfile is readable");
    let builds: Vec<&str> = text
        .lines()
        .filter(|l| l.trim_start().starts_with("cargo build --release"))
        .collect();
    assert_eq!(builds.len(), 3, "release, build-macos and build-windows: {builds:?}");
    assert!(
        builds.iter().all(|l| l.contains("--package resume")),
        "every release build names `resume`: {builds:?}"
    );
}

#[test]
fn the_minimum_rust_check_runs_cargo_on_the_package_it_reads_the_floor_of() {
    // `msrv` is a `[unix]` recipe, so `just --dump` leaves it out on Windows and the dump cannot be read there.
    // The justfile is the same text everywhere, so read that, and ignore comment lines.
    let text = std::fs::read_to_string(crate_path("justfile")).expect("the justfile is readable");
    let live: Vec<&str> = text.lines().filter(|l| !l.trim_start().starts_with('#')).collect();
    let has = |needle: &str| live.iter().any(|l| l.trim() == needle);
    assert!(
        has("check_floor Cargo.toml resume"),
        "msrv must check resume at its floor"
    );
    assert!(
        has("check_floor crates/workbench/Cargo.toml workbench"),
        "msrv must check the app at its floor"
    );
    let cargo_line = live
        .iter()
        .find(|l| l.contains("cargo +"))
        .expect("msrv runs a toolchain-pinned cargo");
    assert!(
        cargo_line.contains("--package \"$package\""),
        "the floor of one package must build that package: {cargo_line}"
    );
}
