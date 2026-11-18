//! ABOUTME: End-to-end tests of the `resume` binary's command-line interface.
//! ABOUTME: Uses assert_cmd to run the built binary and check exit codes and output.

use assert_cmd::Command;
use predicates::str::contains;

fn resume() -> Command {
    Command::cargo_bin("resume").expect("binary `resume` is built by cargo test")
}

#[test]
fn cli_help_exits_zero() {
    resume()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("Usage: resume"))
        .stdout(contains("resumes"));
}

#[test]
fn cli_version_prints_crate_version() {
    resume()
        .arg("--version")
        .assert()
        .success()
        .stdout(contains(format!("resume {}", env!("CARGO_PKG_VERSION"))));
}

#[test]
fn cli_unknown_flag_exits_two() {
    resume().arg("--no-such-flag").assert().code(2);
}
