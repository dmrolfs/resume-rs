//! ABOUTME: End-to-end tests of the `resume` binary's command-line interface.
//! ABOUTME: Uses assert_cmd to run the built binary and check exit codes and output.

use assert_cmd::Command;
use predicates::str::{contains, is_match};

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

#[test]
fn help_names_the_binary_resume_with_no_extension() {
    // On Windows the real executable is `resume.exe`, so this catches the extension leaking into help.
    resume()
        .arg("--help")
        .assert()
        .success()
        .stdout(is_match(r"(?m)Usage: resume(\s|$)").expect("valid regex"));
}

#[cfg(unix)]
#[test]
fn help_ignores_an_exe_suffix_in_argv0() -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;

    // Simulates how Windows starts the binary: argv[0] ends in `.exe`.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_resume"))
        .arg0("resume.exe")
        .arg("--help")
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("Usage: resume\n") || stdout.contains("Usage: resume "),
        "help leaked the extension:\n{stdout}"
    );
    Ok(())
}
