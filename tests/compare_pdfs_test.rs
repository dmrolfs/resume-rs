//! ABOUTME: Tests scripts/compare-pdfs.sh, which CI uses to compare PDFs built on different operating systems.
//! ABOUTME: Runs on unix only: a bare `bash` on Windows can be the WSL launcher. Files are small stand-ins for PDFs.

#![cfg(unix)]

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::TempDir;

/// A directory of files, written by name.
fn files(contents: &[(&str, &[u8])]) -> TempDir {
    let dir = tempfile::tempdir().expect("a temporary directory");
    for (name, bytes) in contents {
        std::fs::write(dir.path().join(name), bytes).expect("a file is written");
    }
    dir
}

/// Runs `compare-pdfs.sh` from the crate root with `args`, where `LABEL=name` arguments name files in `dir`.
fn compare(dir: &TempDir, args: &[&str]) -> assert_cmd::assert::Assert {
    let resolved: Vec<String> = args
        .iter()
        .map(|arg| match arg.split_once('=') {
            Some((label, name)) => format!("{label}={}", dir.path().join(name).display()),
            None => (*arg).to_string(),
        })
        .collect();
    Command::new("bash")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .arg("scripts/compare-pdfs.sh")
        .args(resolved)
        .assert()
}

const PDF: &[u8] = b"%PDF-1.7 Carlito-Regular /Producer (resume) %%EOF";

#[test]
fn identical_files_pass_and_print_the_hash_of_each() {
    let dir = files(&[("a.pdf", PDF), ("b.pdf", PDF), ("c.pdf", PDF), ("d.pdf", PDF)]);
    let out = compare(
        &dir,
        &[
            "macOS-LF=a.pdf",
            "macOS-CRLF=b.pdf",
            "Windows-LF=c.pdf",
            "Windows-CRLF=d.pdf",
        ],
    );
    let out = out.success().stdout(contains("PDFs are byte-identical"));
    let stdout = String::from_utf8_lossy(&out.get_output().stdout).to_string();
    for label in ["macOS-LF", "macOS-CRLF", "Windows-LF", "Windows-CRLF"] {
        assert!(stdout.contains(label), "{label} is listed with its hash: {stdout}");
    }
    // One 64-digit SHA-256 per file, plus the one in the notice.
    let hashes = stdout
        .split(|c: char| !c.is_ascii_hexdigit())
        .filter(|word| word.len() == 64)
        .count();
    assert!(hashes >= 4, "{stdout}");
}

#[test]
fn files_that_differ_fail_and_say_which_two_and_how() {
    let different = [PDF, b" and one more byte"].concat();
    let dir = files(&[("a.pdf", PDF), ("b.pdf", PDF), ("c.pdf", &different)]);
    compare(&dir, &["macOS-LF=a.pdf", "macOS-CRLF=b.pdf", "Windows-LF=c.pdf"])
        .failure()
        .stdout(contains("PDF bytes differ"))
        .stdout(contains("macOS-LF and Windows-LF"))
        .stdout(contains(format!("{} bytes", PDF.len())))
        .stdout(contains(format!("{} bytes", different.len())));
}

#[test]
fn a_difference_of_one_byte_in_the_same_length_is_caught() {
    let mut altered = PDF.to_vec();
    altered[10] ^= 1;
    let dir = files(&[("a.pdf", PDF), ("b.pdf", &altered)]);
    compare(&dir, &["one=a.pdf", "two=b.pdf"])
        .failure()
        .stdout(contains("PDF bytes differ"))
        .stdout(contains("byte 11"));
}

#[test]
fn a_missing_file_fails_closed_by_name() {
    let dir = files(&[("a.pdf", PDF)]);
    compare(&dir, &["macOS-LF=a.pdf", "Windows-LF=absent.pdf"])
        .failure()
        .stdout(contains("missing or empty"))
        .stdout(contains("Windows-LF"));
}

#[test]
fn an_empty_file_fails_closed() {
    let dir = files(&[("a.pdf", PDF), ("b.pdf", b"")]);
    compare(&dir, &["one=a.pdf", "two=b.pdf"])
        .failure()
        .stdout(contains("missing or empty"))
        .stdout(contains("two"));
}

#[test]
fn identical_files_that_embed_a_forbidden_name_fail() {
    let leaking = b"%PDF-1.7 Calibri-Bold /Producer (resume) %%EOF";
    let dir = files(&[("a.pdf", leaking), ("b.pdf", leaking)]);
    compare(&dir, &["one=a.pdf", "two=b.pdf", "--forbid", "Calibri"])
        .failure()
        .stdout(contains("forbidden"))
        .stdout(contains("Calibri"));
}

#[test]
fn files_free_of_the_forbidden_name_pass() {
    let dir = files(&[("a.pdf", PDF), ("b.pdf", PDF)]);
    compare(&dir, &["one=a.pdf", "two=b.pdf", "--forbid", "Calibri"])
        .success()
        .stdout(contains("PDFs are byte-identical"));
}

#[test]
fn fewer_than_two_files_is_a_usage_error() {
    let dir = files(&[("a.pdf", PDF)]);
    compare(&dir, &["only=a.pdf"]).code(2).stderr(contains("usage"));
    compare(&dir, &[]).code(2).stderr(contains("usage"));
}

#[test]
fn a_file_that_is_a_prefix_of_the_other_says_so_instead_of_naming_a_byte() {
    let longer = [PDF, b" and more"].concat();
    let dir = files(&[("a.pdf", PDF), ("b.pdf", &longer)]);
    compare(&dir, &["short=a.pdf", "long=b.pdf"])
        .failure()
        .stdout(contains("one file ends where the other continues"));
}
