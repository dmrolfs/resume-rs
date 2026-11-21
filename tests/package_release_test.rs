//! ABOUTME: Tests scripts/package-release.sh, which makes the release archives and their SHA-256 lines.
//! ABOUTME: Runs on unix only: a bare `bash` on Windows can be the WSL launcher. The "binary" is a small stand-in.

#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::process::Command as Process;

use assert_cmd::Command;
use claims::assert_ok;
use predicates::str::contains;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

fn package(binary: &Path, label: &str, version: &str, out: &Path) -> assert_cmd::assert::Assert {
    Command::new("bash")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .arg("scripts/package-release.sh")
        .arg(binary)
        .args([label, version])
        .arg(out)
        .assert()
}

/// A stand-in program with the given file name, in a directory of its own.
fn program(name: &str) -> (TempDir, std::path::PathBuf) {
    let dir = assert_ok!(tempfile::tempdir());
    let path = dir.path().join(name);
    assert_ok!(fs::write(&path, b"not really a program"));
    (dir, path)
}

fn listing(archive: &Path) -> Vec<String> {
    let output = assert_ok!(Process::new("tar").arg("-tzf").arg(archive).output());
    assert!(output.status.success(), "{output:?}");
    let mut names: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect();
    names.sort();
    names
}

#[test]
fn a_tarball_holds_one_directory_with_the_program_and_the_license_files() {
    let (_keep, binary) = program("resume");
    let out = assert_ok!(tempfile::tempdir());
    package(&binary, "macos-universal2", "v0.1.0", out.path()).success();
    let archive = out.path().join("resume-v0.1.0-macos-universal2.tar.gz");
    assert_eq!(
        listing(&archive),
        [
            "resume-v0.1.0-macos-universal2/",
            "resume-v0.1.0-macos-universal2/FONT-LICENSES.md",
            "resume-v0.1.0-macos-universal2/LICENSE",
            "resume-v0.1.0-macos-universal2/README.md",
            "resume-v0.1.0-macos-universal2/resume",
        ]
    );
}

#[test]
fn the_program_in_a_tarball_is_executable_even_when_it_came_from_a_download_without_permissions() {
    let (_keep, binary) = program("resume");
    assert_ok!(Process::new("chmod").arg("644").arg(&binary).status());
    let out = assert_ok!(tempfile::tempdir());
    package(&binary, "macos-universal2", "v0.1.0", out.path()).success();
    let unpacked = assert_ok!(tempfile::tempdir());
    let status = assert_ok!(
        Process::new("tar")
            .arg("-xzf")
            .arg(out.path().join("resume-v0.1.0-macos-universal2.tar.gz"))
            .arg("-C")
            .arg(unpacked.path())
            .status()
    );
    assert!(status.success());
    use std::os::unix::fs::PermissionsExt;
    let mode = assert_ok!(fs::metadata(
        unpacked.path().join("resume-v0.1.0-macos-universal2/resume")
    ))
    .permissions()
    .mode();
    assert_eq!(mode & 0o111, 0o111, "mode {mode:o}");
}

#[test]
fn the_checksum_file_is_the_form_shasum_check_reads_and_it_verifies() {
    let (_keep, binary) = program("resume");
    let out = assert_ok!(tempfile::tempdir());
    package(&binary, "macos-universal2", "v0.1.0", out.path()).success();
    let line = assert_ok!(fs::read_to_string(
        out.path().join("resume-v0.1.0-macos-universal2.tar.gz.sha256")
    ));
    let (hash, name) = line
        .trim_end()
        .split_once("  ")
        .expect("two spaces between hash and name");
    assert_eq!(hash.len(), 64);
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()), "{hash}");
    assert_eq!(name, "resume-v0.1.0-macos-universal2.tar.gz");
    let check = assert_ok!(
        Process::new("shasum")
            .args(["-a", "256", "-c"])
            .arg("resume-v0.1.0-macos-universal2.tar.gz.sha256")
            .current_dir(out.path())
            .output()
    );
    assert!(check.status.success(), "{check:?}");
    // And it fails when the archive is not the one that was hashed.
    assert_ok!(fs::write(
        out.path().join("resume-v0.1.0-macos-universal2.tar.gz"),
        b"changed"
    ));
    let check = assert_ok!(
        Process::new("shasum")
            .args(["-a", "256", "-c"])
            .arg("resume-v0.1.0-macos-universal2.tar.gz.sha256")
            .current_dir(out.path())
            .output()
    );
    assert!(!check.status.success());
}

#[test]
fn the_hash_is_printed_so_the_release_log_records_it() {
    let (_keep, binary) = program("resume");
    let out = assert_ok!(tempfile::tempdir());
    package(&binary, "macos-universal2", "v0.1.0", out.path())
        .success()
        .stdout(contains("  resume-v0.1.0-macos-universal2.tar.gz"));
}

#[test]
fn an_exe_is_packed_as_a_zip_with_the_same_files() {
    let (_keep, binary) = program("resume.exe");
    let out = assert_ok!(tempfile::tempdir());
    package(&binary, "windows-x64", "v0.1.0", out.path()).success();
    assert!(out.path().join("resume-v0.1.0-windows-x64.zip").is_file());
    assert!(out.path().join("resume-v0.1.0-windows-x64.zip.sha256").is_file());
    assert!(!out.path().join("resume-v0.1.0-windows-x64.tar.gz").exists());
    let python = assert_ok!(
        Process::new("python3")
            .args([
                "-c",
                "import sys, zipfile; print('\\n'.join(sorted(zipfile.ZipFile(sys.argv[1]).namelist())))"
            ])
            .arg(out.path().join("resume-v0.1.0-windows-x64.zip"))
            .output()
    );
    assert!(
        python.status.success(),
        "python could not read the zip: {}",
        String::from_utf8_lossy(&python.stderr)
    );
    let names: Vec<String> = String::from_utf8_lossy(&python.stdout)
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(
        names,
        [
            "resume-v0.1.0-windows-x64/",
            "resume-v0.1.0-windows-x64/FONT-LICENSES.md",
            "resume-v0.1.0-windows-x64/LICENSE",
            "resume-v0.1.0-windows-x64/README.md",
            "resume-v0.1.0-windows-x64/resume.exe",
        ]
    );
}

#[test]
fn packing_again_replaces_the_archive_and_leaves_no_staging_directory() {
    let (_keep, binary) = program("resume.exe");
    let out = assert_ok!(tempfile::tempdir());
    // The staging directory is made under $TMPDIR, so give the script an empty one and look in it afterwards.
    let scratch = assert_ok!(tempfile::tempdir());
    for _ in 0..2 {
        Command::new("bash")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .env("TMPDIR", scratch.path())
            .arg("scripts/package-release.sh")
            .arg(&binary)
            .args(["windows-x64", "v0.1.0"])
            .arg(out.path())
            .assert()
            .success();
    }
    assert_eq!(
        assert_ok!(fs::read_dir(scratch.path())).count(),
        0,
        "the staging directory was left behind"
    );
    let mut names: Vec<String> = assert_ok!(fs::read_dir(out.path()))
        .map(|entry| assert_ok!(entry).file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["resume-v0.1.0-windows-x64.zip", "resume-v0.1.0-windows-x64.zip.sha256"]
    );
}

#[test]
fn a_missing_program_a_bad_label_and_the_wrong_argument_count_are_errors_and_write_nothing() {
    let out = assert_ok!(tempfile::tempdir());
    package(&out.path().join("nope"), "x", "v1", &out.path().join("o"))
        .failure()
        .code(1)
        .stderr(contains("no such file"));
    let (_keep, binary) = program("resume");
    package(&binary, "../escape", "v1", &out.path().join("o"))
        .failure()
        .code(2)
        .stderr(contains("may hold only"));
    package(&binary, "mac os", "v1", &out.path().join("o"))
        .failure()
        .code(2);
    Command::new("bash")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .arg("scripts/package-release.sh")
        .arg("only-one")
        .assert()
        .failure()
        .code(2)
        .stderr(contains("usage:"));
    assert!(!out.path().join("o").exists());
}
