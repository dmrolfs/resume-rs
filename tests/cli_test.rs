//! ABOUTME: End-to-end tests of the `resume` binary's command-line interface.
//! ABOUTME: Uses assert_cmd to run the built binary and check exit codes and output.

mod common;

use std::fs;
use std::path::Path;

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

// ============================================================================
// `resume build` through the real binary: exit codes, streams, and color
// ============================================================================

/// Copies the fixture `name` into `dir` as `file`, and returns a command that runs in `dir`.
fn in_workspace(dir: &Path, fixture: &str, file: &str) -> Command {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    fs::copy(&source, dir.join(file)).expect("the fixture exists");
    let mut command = resume();
    command.current_dir(dir);
    command
}

#[test]
fn a_clean_build_exits_0_and_writes_the_pdf() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    in_workspace(dir.path(), "full.md", "Resume.md")
        .args(["build", "Resume.md", "--fonts", "bundled"])
        .assert()
        .code(0)
        .stderr(contains("ok "))
        .stdout("");
    assert!(dir.path().join("out").join("Resume.pdf").is_file());
}

#[test]
fn an_error_in_the_resume_exits_1_and_writes_no_pdf() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    in_workspace(dir.path(), "errors-role.md", "Bad.md")
        .args(["build", "Bad.md", "--fonts", "bundled"])
        .assert()
        .code(1)
        .stderr(contains("no PDF written"))
        .stderr(contains("Bad.md:"));
    assert!(!dir.path().join("out").exists());
}

#[test]
fn a_missing_input_exits_1() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    resume()
        .current_dir(dir.path())
        .args(["build", "nothing.md", "--fonts", "bundled"])
        .assert()
        .code(1)
        .stderr(contains("cannot read"));
}

#[test]
fn the_dry_run_flag_writes_nothing_through_the_real_binary() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    in_workspace(dir.path(), "full.md", "Resume.md")
        .args(["--dry-run", "build", "Resume.md", "--fonts", "bundled"])
        .assert()
        .code(0)
        .stderr(contains("would write"));
    assert!(!dir.path().join("out").exists());
}

#[test]
fn no_command_exits_2() {
    resume().assert().code(2).stderr(contains("Usage"));
}

#[test]
fn build_without_a_file_exits_2() {
    resume().arg("build").assert().code(2);
}

#[test]
fn fonts_bundled_with_font_path_exits_2() {
    resume()
        .args(["build", "a.md", "--fonts", "bundled", "--font-path", "somewhere"])
        .assert()
        .code(2)
        .stderr(contains("--font-path"));
}

#[test]
fn two_inputs_that_would_write_the_same_pdf_exit_2_before_anything_is_written() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    resume()
        .current_dir(dir.path())
        .args(["build", "a/Resume.md", "b/resume.md"])
        .assert()
        .code(2)
        .stderr(contains("would both write"));
    assert!(!dir.path().join("out").exists());
}

#[test]
fn json_goes_to_stdout_and_the_summary_to_stderr() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let output = in_workspace(dir.path(), "full.md", "Resume.md")
        .args(["build", "Resume.md", "--fonts", "bundled", "--format", "json"])
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.lines().all(|line| line.starts_with("{\"type\":\"diagnostic\"")),
        "stdout holds only diagnostics: {stdout}"
    );
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("ok "));
}

#[test]
fn color_never_and_no_color_leave_no_escape_bytes_and_always_adds_them() {
    // A failing input gives colored output to compare: the summary marker and the excerpt.
    let run = |args: &[&str], no_color: bool| {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let mut command = in_workspace(dir.path(), "errors-role.md", "Bad.md");
        command.args(["build", "Bad.md", "--fonts", "bundled"]).args(args);
        if no_color {
            command.env("NO_COLOR", "1");
        } else {
            command.env_remove("NO_COLOR");
        }
        String::from_utf8_lossy(&command.output().expect("the binary runs").stderr).into_owned()
    };
    assert!(run(&["--color", "always"], false).contains('\u{1b}'), "always colors");
    assert!(!run(&["--color", "never"], false).contains('\u{1b}'), "never does not");
    assert!(!run(&["--color", "auto"], true).contains('\u{1b}'), "NO_COLOR does not");
    assert!(
        !run(&["--color", "auto"], false).contains('\u{1b}'),
        "auto does not color a pipe"
    );
}

// ============================================================================
// `resume styles` and `resume export-style` through the real binary
// ============================================================================

#[test]
fn styles_lists_the_built_in_styles_on_stdout() {
    resume()
        .arg("styles")
        .assert()
        .code(0)
        .stdout(contains("pragmatic"))
        .stdout(contains("built-in"))
        .stderr("");
}

#[test]
fn export_style_writes_the_files_and_then_refuses_to_overwrite_them() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    resume()
        .current_dir(dir.path())
        .args(["export-style", "pragmatic", "mine"])
        .assert()
        .code(0)
        .stderr(contains("exported pragmatic to mine"));
    assert!(dir.path().join("mine").join("pragmatic").join("style.typ").is_file());
    assert!(dir.path().join("mine").join("lib").join("components.typ").is_file());

    resume()
        .current_dir(dir.path())
        .args(["export-style", "pragmatic", "mine"])
        .assert()
        .code(1)
        .stderr(contains("already exists"))
        .stderr(contains("--force"));
    resume()
        .current_dir(dir.path())
        .args(["export-style", "pragmatic", "mine", "--force"])
        .assert()
        .code(0);
}

#[test]
fn an_unknown_style_exits_1_and_names_the_styles_there_are() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    in_workspace(dir.path(), "full.md", "Resume.md")
        .args(["build", "Resume.md", "--style", "nope", "--fonts", "bundled"])
        .assert()
        .code(1)
        .stderr(contains("unknown style `nope`"))
        .stderr(contains(common::available_styles_message()));
}

// An unconnected datagram socket is a valid stdout descriptor but cannot deliver bytes.
#[cfg(unix)]
#[test]
fn an_undeliverable_stdout_exits_1_for_small_and_large_products() {
    use claims::assert_ok;
    use pretty_assertions::assert_eq;
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixDatagram;

    for command in ["styles", "licenses"] {
        let socket = assert_ok!(UnixDatagram::unbound());
        let descriptor: OwnedFd = socket.into();
        let output = assert_ok!(
            std::process::Command::new(env!("CARGO_BIN_EXE_resume"))
                .arg(command)
                .stdout(descriptor)
                .output()
        );
        assert_eq!(output.status.code(), Some(1), "{command}");
        let message = String::from_utf8_lossy(&output.stderr);
        assert_eq!(message.matches("cannot write to stdout").count(), 1, "{message}");
    }
}

// /dev/full accepts the open but fails writes with ENOSPC, including writes delayed until flush.
#[cfg(target_os = "linux")]
#[test]
fn a_full_stdout_destination_exits_1_for_small_and_large_products() {
    use claims::assert_ok;
    use pretty_assertions::assert_eq;

    for command in ["styles", "licenses"] {
        let output = assert_ok!(
            std::process::Command::new(env!("CARGO_BIN_EXE_resume"))
                .arg(command)
                .stdout(assert_ok!(fs::File::options().write(true).open("/dev/full")))
                .output()
        );
        assert_eq!(output.status.code(), Some(1), "{command}");
        let message = String::from_utf8_lossy(&output.stderr);
        assert!(message.contains("cannot write to stdout"), "{message}");
    }
}

#[test]
fn licenses_prints_the_font_notices_on_stdout() {
    resume()
        .arg("licenses")
        .assert()
        .code(0)
        .stdout(contains("SIL OPEN FONT LICENSE"))
        .stdout(contains("Carlito"))
        .stdout(contains("Libertinus"))
        .stderr("");
}

// ============================================================================
// `resume doctor` and `resume check` through the real binary: exit codes
// ============================================================================

#[test]
fn doctor_exits_0_for_a_clean_resume_1_for_an_error_and_2_for_a_usage_error() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(
        dir.path().join("Clean.md"),
        "# Ada\n\nLondon\n\n## Experience\n\n### Acme \u{b7} 2015 \u{2013} 2017\n\n- Cut costs 40%.\n",
    )
    .expect("write");
    std::fs::write(dir.path().join("Bad.md"), "## Experience\n\n- x\n").expect("write");
    let run = |args: &[&str]| {
        let mut command = resume();
        command.current_dir(dir.path()).args(args);
        command.assert()
    };
    run(&["doctor", "Clean.md", "--fonts", "bundled"]).code(0);
    run(&["check", "Clean.md", "--fonts", "bundled"]).code(0);
    run(&["doctor", "Bad.md", "--fonts", "bundled"]).code(1);
    run(&["check", "Bad.md", "--fonts", "bundled"]).code(1);
    run(&["doctor", "--explain", "no-such-code"])
        .code(2)
        .stderr(contains("is not a diagnostic code"));
    run(&["doctor", "--explain", "period-hyphen"])
        .code(0)
        .stdout(contains("period-hyphen (format)"));
}

#[test]
fn check_with_fix_or_explain_exits_2_and_leaves_the_file_alone() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let text = "# Ada\n\nLondon\n\n## Experience\n\n### Acme | Engineer | 2015 - Current\n\n- Led 3 teams.\n";
    std::fs::write(dir.path().join("Resume.md"), text).expect("write");
    for args in [
        &["check", "Resume.md", "--fix"][..],
        &["check", "--explain", "period-hyphen"][..],
    ] {
        resume().current_dir(dir.path()).args(args).assert().code(2);
    }
    assert_eq!(
        std::fs::read_to_string(dir.path().join("Resume.md")).expect("read"),
        text
    );
}

#[test]
fn doctor_fix_through_the_binary_fixes_the_file_and_exits_by_what_is_left() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join("Resume.md");
    std::fs::write(
        &path,
        "# Ada\n\nLondon\n\n## Experience\n\n### Acme | Engineer | 2015 - Current\n\n- Led 3 teams.\n",
    )
    .expect("write");
    resume()
        .current_dir(dir.path())
        .args(["doctor", "Resume.md", "--fix", "--fonts", "bundled"])
        .assert()
        .code(0)
        .stderr(contains("fixed 3 findings in Resume.md"));
    let fixed = std::fs::read_to_string(&path).expect("read");
    assert!(
        fixed.contains("### Acme \u{b7} Engineer \u{b7} 2015 \u{2013} Present"),
        "{fixed}"
    );
}
