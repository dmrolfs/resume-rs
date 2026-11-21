//! ABOUTME: End-to-end tests of `resume build --spec` through the real binary, from a real working directory.
//! ABOUTME: In-process tests cannot see the process's working directory, so relative paths and exit codes live here.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use claims::{assert_ok, assert_some};
use predicates::prelude::PredicateBooleanExt as _;
use predicates::str::contains;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/specs")
}

fn copy_tree(from: &Path, to: &Path) {
    assert_ok!(fs::create_dir_all(to));
    for entry in assert_ok!(fs::read_dir(from)) {
        let entry = assert_ok!(entry);
        let target = to.join(entry.file_name());
        if assert_ok!(entry.file_type()).is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            assert_ok!(fs::copy(entry.path(), target));
        }
    }
}

/// The `two-profiles` case laid out as a user has it, and the directory to run in.
fn staged() -> tempfile::TempDir {
    let dir = assert_ok!(tempfile::tempdir());
    assert_ok!(fs::copy(fixtures().join("baseline.md"), dir.path().join("baseline.md")));
    copy_tree(&fixtures().join("two-profiles"), &dir.path().join("two-profiles"));
    dir
}

fn resume(dir: &Path) -> Command {
    let mut command = Command::cargo_bin("resume").expect("binary `resume` is built by cargo test");
    command.current_dir(dir).args(["--color", "never"]);
    command
}

const SPEC: &str = "two-profiles/positions/acme.toml";

#[test]
fn a_relative_spec_builds_from_the_working_directory() {
    let dir = staged();
    resume(dir.path())
        .args(["build", "--spec", SPEC, "--fonts", "bundled", "-o", "out"])
        .assert()
        .success()
        .stderr(contains("ok out/acme.pdf").or(contains("ok out\\acme.pdf")))
        .stderr(contains("content: baseline.md"));
    assert!(dir.path().join("out/acme.pdf").is_file());
}

#[test]
fn a_relative_profiles_directory_is_relative_to_the_working_directory() {
    let dir = staged();
    assert_ok!(fs::rename(
        dir.path().join("two-profiles/profiles"),
        dir.path().join("my-profiles")
    ));
    resume(dir.path())
        .args(["build", "--spec", SPEC, "--fonts", "bundled", "-o", "out"])
        .assert()
        .code(1)
        .stdout("")
        .stderr(contains("has no directory of profiles"));
    resume(dir.path())
        .args([
            "build",
            "--spec",
            SPEC,
            "--profiles",
            "my-profiles",
            "--fonts",
            "bundled",
            "-o",
            "out",
        ])
        .assert()
        .success()
        .stderr(contains("profiles: principal -> ai-platform -> the spec"));
}

#[test]
fn a_spec_in_the_working_directory_names_the_directory_it_looked_in() {
    let dir = staged();
    let spec = dir.path().join("two-profiles/positions/acme.toml");
    assert_ok!(fs::copy(&spec, dir.path().join("acme.toml")));
    let error = resume(dir.path())
        .args(["build", "--spec", "acme.toml", "--fonts", "bundled", "-o", "out"])
        .assert()
        .code(1)
        .stderr(contains("has no directory of profiles"))
        .get_output()
        .stderr
        .clone();
    let text = String::from_utf8_lossy(&error);
    assert!(
        !text.contains("./../profiles"),
        "an absolute directory, not a relative one: {text}"
    );
    let searched = assert_some!(text.split("looked for ").nth(1), "{text}");
    assert!(
        Path::new(searched.trim_end_matches(')').lines().next().unwrap_or("")).is_absolute(),
        "{text}"
    );
}

#[test]
fn stray_flags_beside_a_spec_exit_two_naming_every_one_of_them() {
    let dir = staged();
    resume(dir.path())
        .args(["build", "--spec", SPEC, "--tags", "ai", "--paper", "a4"])
        .assert()
        .code(2)
        .stderr(contains("--tags and --paper cannot be used beside --spec"))
        .stderr(contains("--allow-overrides"));
    assert!(!dir.path().join("out").exists());
}

#[test]
fn the_same_spec_built_by_two_processes_gives_identical_bytes() {
    let dir = staged();
    for out in ["one", "two"] {
        resume(dir.path())
            .args(["build", "--spec", SPEC, "--fonts", "bundled", "-o", out])
            .assert()
            .success();
    }
    let read = |out: &str| assert_ok!(fs::read(dir.path().join(out).join("acme.pdf")));
    assert!(read("one") == read("two"));
}

#[test]
fn print_spec_exits_zero_and_prints_to_stdout() {
    let dir = staged();
    resume(dir.path())
        .args(["build", "--spec", SPEC, "--print-spec"])
        .assert()
        .success()
        .stdout(contains("# Resolved from: principal, ai-platform, the spec"))
        .stderr("");
}

#[test]
fn a_build_that_changes_nothing_about_the_machine_echoes_no_environment() {
    let dir = staged();
    // No `--fonts` and no `-o`: the defaults, which the summary does not repeat.
    resume(dir.path())
        .args(["build", "--spec", SPEC, "--dry-run"])
        .assert()
        .success()
        .stderr(contains("environment:").not())
        .stderr(contains("profiles: principal -> ai-platform -> the spec"));
}

#[test]
fn a_spec_that_cannot_be_read_fails_with_exit_one_and_nothing_on_stdout() {
    let dir = staged();
    resume(dir.path())
        .args([
            "--format",
            "json",
            "build",
            "--spec",
            "no-such-spec.toml",
            "--fonts",
            "bundled",
        ])
        .assert()
        .code(1)
        .stdout("")
        .stderr(contains("no-such-spec.toml"));
}

#[test]
fn the_same_spec_in_another_directory_and_named_by_another_path_gives_the_same_bytes() {
    // Where the files are, and whether the spec is named by a relative or an absolute path, is not in the PDF.
    let (here, there) = (staged(), staged());
    resume(here.path())
        .args(["build", "--spec", SPEC, "--fonts", "bundled", "-o", "out"])
        .assert()
        .success();
    let absolute = there.path().join(SPEC);
    resume(there.path())
        .args(["build", "--spec"])
        .arg(&absolute)
        .args(["--fonts", "bundled", "-o", "out"])
        .assert()
        .success();
    let read = |dir: &tempfile::TempDir| assert_ok!(fs::read(dir.path().join("out/acme.pdf")));
    assert!(read(&here) == read(&there));
}
