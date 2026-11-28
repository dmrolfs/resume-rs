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

/// The HTTP and TLS stack. The `resume` binary must never link one. `ring` is left out: it is general cryptography
/// and its presence alone does not mean a network connection. A name here shows the stack is present; the list cannot
/// prove the absence of every way to open a socket.
const NETWORK_CRATES: [&str; 10] = [
    "ureq",
    "native-tls",
    "openssl",
    "openssl-sys",
    "reqwest",
    "hyper",
    "hyper-util",
    "rustls",
    "rustls-webpki",
    "webpki-roots",
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
    checked_stdout(args, output.status.code(), &output.stdout, &output.stderr)
}

/// The standard output of a finished cargo command, or a panic that names the command and carries its standard error.
///
/// Only exit code 0 is success; no code at all (a signal) is a failure too. The stderr is shown lossily so a
/// diagnostic is never lost to invalid UTF-8.
fn checked_stdout(args: &[&str], code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> String {
    assert!(
        code == Some(0),
        "cargo {args:?} failed with exit code {code:?}:\n{}",
        String::from_utf8_lossy(stderr)
    );
    String::from_utf8(stdout.to_vec()).expect("cargo prints UTF-8")
}

/// The workspace shares one `Cargo.lock`, and the desktop app brings network crates into it. What matters is what
/// the `resume` package itself links: its normal and build dependencies, for every target and with every one of its
/// own features on, so a network crate behind an optional feature is caught too.
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
        "--all-features",
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
    let tree = "resume v0.1.0 (/x)\nhyper v1.13.0\nhyper-utils v0.1.21\nrustls-pki-types v1.16.0\n";
    assert_eq!(
        network_crates_in(tree),
        ["hyper"],
        "`hyper-utils` and `rustls-pki-types` are other crates"
    );
    assert!(network_crates_in("resume v0.1.0\nurl v2.5.8\n").is_empty());
}

/// Every locked version of `crate_name`, sorted and without repeats, from the text of a `Cargo.lock`.
///
/// Absence is read from the lock file, never inferred from a cargo failure: cargo exits with the same code for a
/// name that is not there and for one that is there twice. A lock file that is not TOML, or a package without a name
/// or version, panics instead of reading as an empty inventory.
fn locked_versions_of(lock: &str, crate_name: &str) -> Vec<String> {
    let lock: toml::Value = lock.parse().unwrap_or_else(|e| panic!("Cargo.lock is not TOML: {e}"));
    let packages = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .expect("Cargo.lock has a `package` list");
    let mut versions: Vec<String> = packages
        .iter()
        .filter_map(|package| {
            let name = package
                .get("name")
                .and_then(toml::Value::as_str)
                .expect("a locked package has a name");
            let version = package
                .get("version")
                .and_then(toml::Value::as_str)
                .unwrap_or_else(|| panic!("locked package {name} has no version"));
            (name == crate_name).then(|| version.to_string())
        })
        .collect();
    versions.sort();
    versions.dedup();
    versions
}

/// The inverted tree of each locked version of a crate over the whole workspace, or `None` when the lock file has
/// no package of that name.
///
/// `--workspace` makes cargo combine the features that every member asks for, which is what `just test` and
/// `just clippy` build. So this shows a network crate that another member switched on inside a crate `resume`
/// shares. Each version is asked for as `name@version`: a bare name that is locked twice is an error in cargo, and
/// that error must never read as "not there". Any failure of a query for a locked version fails the test with
/// cargo's own message.
fn workspace_dependents_of(crate_name: &str) -> Option<Vec<String>> {
    let lock = read_project_file("Cargo.lock");
    dependents_of(&lock, crate_name, |spec| {
        cargo_output(&[
            "tree",
            "--workspace",
            "--edges",
            "normal,build",
            "--target",
            "all",
            "--invert",
            spec,
            "--locked",
        ])
    })
}

/// [`workspace_dependents_of`] over a lock file and a way to run one query, so a test can feed it any outcome.
fn dependents_of(lock: &str, crate_name: &str, mut query: impl FnMut(&str) -> String) -> Option<Vec<String>> {
    let versions = locked_versions_of(lock, crate_name);
    if versions.is_empty() {
        return None;
    }
    Some(versions.iter().map(|v| query(&format!("{crate_name}@{v}"))).collect())
}

/// True when the inverted tree lists `resume` itself as something that depends on the crate.
fn lists_resume_as_dependent(inverted_tree: &str) -> bool {
    inverted_tree.lines().skip(1).any(|line| {
        line.trim_start_matches(|c: char| !c.is_alphanumeric())
            .split_whitespace()
            .next()
            == Some("resume")
    })
}

/// Workspace members that may link the HTTP and TLS stack: the advisor talks to a local model server, and the app
/// is a GPUI program. Any other member starts offline until its networking has been reviewed.
const NETWORK_MEMBERS: [&str; 2] = ["advisor", "workbench"];

/// The workspace members that appear as dependents in an inverted tree (every line after the crate itself).
fn members_depending_in<'a>(inverted_tree: &str, members: &'a [String]) -> Vec<&'a str> {
    members
        .iter()
        .map(String::as_str)
        .filter(|member| {
            inverted_tree.lines().skip(1).any(|line| {
                line.trim_start_matches(|c: char| !c.is_alphanumeric())
                    .split_whitespace()
                    .next()
                    == Some(member)
            })
        })
        .collect()
}

fn workspace_member_names() -> Vec<String> {
    let mut names: Vec<String> = workspace_packages()
        .iter()
        .filter_map(|p| p["name"].as_str().map(str::to_string))
        .collect();
    names.sort();
    names
}

/// Only the packages named in [`NETWORK_MEMBERS`] may bring the HTTP and TLS stack into the build. `resume`, and any
/// member added later, must not.
#[test]
fn only_the_advisor_and_the_app_may_depend_on_the_network_stack() {
    let members = workspace_member_names();
    let mut offenders = Vec::new();
    for name in NETWORK_CRATES {
        for tree in workspace_dependents_of(name).into_iter().flatten() {
            for member in members_depending_in(&tree, &members) {
                if !NETWORK_MEMBERS.contains(&member) {
                    offenders.push(format!("{member} -> {name}"));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "members outside the allowance link the network stack: {offenders:?}"
    );
}

#[test]
fn the_allowance_reads_members_from_an_inverted_tree_and_refuses_the_rest() {
    let members: Vec<String> = ["advisor", "resume", "workbench", "newcomer"]
        .map(String::from)
        .to_vec();
    let tree = "hyper v1.13.0\n├── gpui v0.3.14\n│   └── workbench v0.1.0 (/x)\n└── ureq v3.0.0\n    └── advisor v0.1.0 (/x)\n";
    let found = members_depending_in(tree, &members);
    assert_eq!(found, ["advisor", "workbench"]);
    assert!(found.iter().all(|m| NETWORK_MEMBERS.contains(m)));
    let leak = "hyper v1.13.0\n└── newcomer v0.1.0 (/x)\n";
    assert_eq!(members_depending_in(leak, &members), ["newcomer"]);
    assert!(!NETWORK_MEMBERS.contains(&"newcomer"));
    assert!(!NETWORK_MEMBERS.contains(&"resume"));
    assert!(members_depending_in("hyper v1.13.0\n", &members).is_empty());
    assert!(
        members_depending_in("hyper v1.13.0\n└── advisor-extra v0.1.0 (/x)\n", &members).is_empty(),
        "a longer name is another package"
    );
}

/// `resume` is the library the others build on. It must not depend on a package that may use the network.
#[test]
fn the_resume_tree_holds_no_other_workspace_member() {
    let tree = cargo_output(&[
        "tree",
        "--package",
        "resume",
        "--edges",
        "normal,build,dev",
        "--target",
        "all",
        "--all-features",
        "--prefix",
        "none",
        "--locked",
    ]);
    let others: Vec<String> = workspace_member_names().into_iter().filter(|m| m != "resume").collect();
    let present: Vec<&String> = others
        .iter()
        .filter(|m| {
            tree.lines()
                .any(|line| line.split_whitespace().next() == Some(m.as_str()))
        })
        .collect();
    assert!(present.is_empty(), "`resume` depends on other members: {present:?}");
}

/// The app turns on `serde_json/preserve_order` through GPUI, which reorders the CLI's JSON keys. The advisor needs
/// it for byte-exact requests. Each is its own package, and the shipped `resume` must not get it: its own tree is
/// checked, never the workspace's.
#[test]
fn serde_json_in_the_resume_tree_does_not_keep_object_order() {
    let tree = cargo_output(&[
        "tree",
        "--package",
        "resume",
        "--edges",
        "normal,build",
        "--target",
        "all",
        "--all-features",
        "--prefix",
        "none",
        "--format",
        "{p}|{f}",
        "--locked",
    ]);
    let rows: Vec<&str> = tree.lines().filter(|l| l.starts_with("serde_json ")).collect();
    assert!(!rows.is_empty(), "the tree must list serde_json:\n{tree}");
    for row in rows {
        assert!(!features_of(row).contains(&"preserve_order"), "{row}");
    }
}

/// The comma-separated features at the end of a `{p}|{f}` row, without cargo's trailing ` (*)` repeat marker.
fn features_of(row: &str) -> Vec<&str> {
    let features = row.rsplit_once('|').map_or("", |(_, f)| f);
    features.trim_end_matches(" (*)").split(',').map(str::trim).collect()
}

#[test]
fn a_feature_row_is_read_whole_with_cargos_repeat_marker_removed() {
    assert_eq!(features_of("serde_json v1.0.154|default,std"), ["default", "std"]);
    assert_eq!(
        features_of("serde_json v1.0.154|default,preserve_order (*)"),
        ["default", "preserve_order"]
    );
    assert!(!features_of("serde_json v1.0.154|std,preserve_orderly").contains(&"preserve_order"));
}

/// The gate runs each package alone, so this no longer protects `just check`. It still protects `cargo test --workspace`
/// run by hand or from an editor: a network feature the app switches on for a crate `resume` shares (for example
/// `typst-kit/system-downloader`, which pulls in `ureq`) would reach `resume` there. Fail on it.
#[test]
fn the_app_does_not_switch_on_a_network_crate_inside_resume() {
    let leaks: Vec<&str> = NETWORK_CRATES
        .iter()
        .copied()
        .filter(|name| {
            workspace_dependents_of(name).is_some_and(|trees| trees.iter().any(|tree| lists_resume_as_dependent(tree)))
        })
        .collect();
    assert!(
        leaks.is_empty(),
        "the workspace links network crates into `resume`: {leaks:?}"
    );
}

#[test]
fn the_dependent_check_reads_an_inverted_tree() {
    let leaky = "ureq v2.0.0\n└── typst-kit v0.15.1\n    └── resume v0.1.0 (/x)\n";
    let clean = "hyper v1.13.0\n└── gpui v0.3.14\n    └── workbench v0.1.0 (/x/crates/workbench)\n";
    assert!(lists_resume_as_dependent(leaky));
    assert!(!lists_resume_as_dependent(clean));
    assert!(
        !lists_resume_as_dependent("resume-extra v0.1.0\n"),
        "a longer name is another package"
    );
}

// ---------------------------------------------------------------------------------------------------
// The workspace check must fail closed: a cargo failure, an ambiguous name or a bad lock file never reads as "absent".
// ---------------------------------------------------------------------------------------------------

const TWO_VERSIONS: &str = "version = 4\n\n[[package]]\nname = \"aes\"\nversion = \"0.8.4\"\n\n\
[[package]]\nname = \"aes\"\nversion = \"0.9.0\"\n\n[[package]]\nname = \"aes-extra\"\nversion = \"1.0.0\"\n";

/// The panic message of `run`, which must panic.
fn panic_message(run: impl FnOnce() + std::panic::UnwindSafe) -> String {
    let payload = std::panic::catch_unwind(run).expect_err("expected a panic");
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(ToString::to_string))
        .unwrap_or_default()
}

#[test]
fn every_locked_version_of_a_name_is_listed_once_and_a_longer_name_is_another_crate() {
    assert_eq!(locked_versions_of(TWO_VERSIONS, "aes"), ["0.8.4", "0.9.0"]);
    assert!(locked_versions_of(TWO_VERSIONS, "ae").is_empty());
    assert!(locked_versions_of(TWO_VERSIONS, "ureq").is_empty());
}

#[test]
fn a_lock_file_that_cannot_be_read_is_an_error_not_an_empty_inventory() {
    assert!(panic_message(|| drop(locked_versions_of("not [ toml", "aes"))).contains("not TOML"));
    assert!(panic_message(|| drop(locked_versions_of("version = 4\n", "aes"))).contains("`package` list"));
    let no_version = "[[package]]\nname = \"aes\"\n";
    assert!(panic_message(|| drop(locked_versions_of(no_version, "aes"))).contains("no version"));
}

#[test]
fn a_name_that_is_not_locked_asks_cargo_nothing_and_reads_as_absent() {
    let mut asked = Vec::new();
    let found = dependents_of(TWO_VERSIONS, "ureq", |spec| {
        asked.push(spec.to_string());
        String::new()
    });
    assert!(found.is_none());
    assert!(asked.is_empty(), "{asked:?}");
}

#[test]
fn each_locked_version_is_asked_for_as_name_at_version() {
    let mut asked = Vec::new();
    let found = dependents_of(TWO_VERSIONS, "aes", |spec| {
        asked.push(spec.to_string());
        format!("tree of {spec}")
    });
    assert_eq!(asked, ["aes@0.8.4", "aes@0.9.0"]);
    assert_eq!(
        found,
        Some(vec!["tree of aes@0.8.4".to_string(), "tree of aes@0.9.0".to_string()])
    );
}

#[test]
fn a_leak_in_the_second_version_is_still_found() {
    let found = dependents_of(TWO_VERSIONS, "aes", |spec| {
        if spec.ends_with("0.9.0") {
            "aes v0.9.0\n└── resume v0.1.0 (/x)\n".to_string()
        } else {
            "aes v0.8.4\n└── workbench v0.1.0 (/x)\n".to_string()
        }
    });
    let trees = found.expect("aes is locked");
    assert!(trees.iter().any(|tree| lists_resume_as_dependent(tree)));
}

#[test]
fn a_failed_query_for_a_locked_version_fails_with_cargos_message() {
    let ambiguous = panic_message(|| {
        drop(checked_stdout(
            &["tree", "--invert", "aes"],
            Some(101),
            b"",
            b"error: specification `aes` is ambiguous\n",
        ));
    });
    assert!(ambiguous.contains("is ambiguous"), "{ambiguous}");
    assert!(ambiguous.contains("101"), "{ambiguous}");
    // "did not match any packages" is how cargo says a name is absent. For a name the lock file has, it is a failure.
    let unmatched = panic_message(|| {
        drop(checked_stdout(
            &["tree", "--invert", "aes@0.8.4"],
            Some(101),
            b"",
            b"error: package ID specification `aes@0.8.4` did not match any packages\n",
        ));
    });
    assert!(unmatched.contains("did not match any packages"), "{unmatched}");
    let signalled = panic_message(|| drop(checked_stdout(&["tree"], None, b"", b"killed")));
    assert!(signalled.contains("None"), "{signalled}");
}

#[test]
fn a_failing_query_cannot_be_turned_into_absence_by_the_version_loop() {
    let mut calls = 0;
    let outcome = panic_message(std::panic::AssertUnwindSafe(|| {
        drop(dependents_of(TWO_VERSIONS, "aes", |spec| {
            calls += 1;
            checked_stdout(&["tree", spec], Some(101), b"", b"error: boom\n")
        }));
    }));
    assert!(outcome.contains("boom"), "{outcome}");
    assert_eq!(calls, 1, "the first failure stops the check");
}

#[test]
fn a_successful_query_with_no_output_is_an_empty_tree_not_absence() {
    let found = dependents_of(TWO_VERSIONS, "aes", |_| checked_stdout(&["tree"], Some(0), b"", b""));
    assert_eq!(found, Some(vec![String::new(), String::new()]));
}

#[test]
fn the_network_list_covers_the_http_and_tls_stack_by_whole_name() {
    let tree = "resume v0.1.0 (/x)\nhyper-util v0.1.21\nwebpki-roots v1.0.0\nrustls-webpki v0.103.0\nring v0.17.0\n\
rustls-pki-types v1.16.0\n";
    assert_eq!(network_crates_in(tree), ["hyper-util", "rustls-webpki", "webpki-roots"]);
}

fn manifest_value(relative: &str) -> toml::Value {
    read_project_file(relative)
        .parse()
        .unwrap_or_else(|e| panic!("{relative} is not TOML: {e}"))
}

/// Every package takes the shared lint policy, and the policy still denies an unused dependency.
#[test]
fn every_package_inherits_the_workspace_lints_and_the_root_denies_unused_dependencies() {
    for manifest in ["Cargo.toml", "crates/workbench/Cargo.toml", "crates/advisor/Cargo.toml"] {
        let value = manifest_value(manifest);
        assert_eq!(
            value["lints"]["workspace"].as_bool(),
            Some(true),
            "{manifest} must have `[lints] workspace = true`"
        );
    }
    let root = manifest_value("Cargo.toml");
    assert_eq!(
        root["workspace"]["lints"]["cargo"]["unused_dependencies"].as_str(),
        Some("deny"),
        "`[workspace.lints.cargo] unused_dependencies` must stay `deny`"
    );
}

/// The packages of the workspace as `cargo metadata` reads them: the root package and the members under `crates/`.
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
fn the_app_and_the_advisor_are_workspace_members_beside_the_resume_package() {
    let mut names: Vec<String> = workspace_packages()
        .iter()
        .filter_map(|p| p["name"].as_str().map(str::to_string))
        .collect();
    names.sort();
    assert_eq!(names, ["advisor", "resume", "workbench"]);
}

/// The advisor is a program of this repository, not a crate to publish; `resume` is the one that ships.
#[test]
fn the_advisor_and_the_app_are_not_published() {
    for package in ["advisor", "workbench"] {
        let found = workspace_packages().into_iter().find(|p| p["name"] == package);
        let publish = found.map(|p| p["publish"].clone());
        assert_eq!(
            publish,
            Some(serde_json::json!([])),
            "{package} must set `publish = false`"
        );
    }
}

#[test]
fn the_cli_keeps_its_lower_rust_floor_than_the_app() {
    // gpui-pre-macos 0.3.14 needs Rust 1.98 (measured in rsm-yjj.1); the CLI must not be dragged up with it.
    assert_eq!(rust_version_of("resume").as_deref(), Some("1.92"));
    assert_eq!(rust_version_of("workbench").as_deref(), Some("1.98"));
    // The advisor builds on `resume`, so it needs no more than `resume` does.
    assert_eq!(rust_version_of("advisor").as_deref(), Some("1.92"));
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
