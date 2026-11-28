//! ABOUTME: Runs the `resume-advisor` binary on synthetic baselines: what it prints, and that a refusal prints nothing.
//! ABOUTME: No private resume enters this repository; every baseline is written into a temporary directory here.

use assert_cmd::Command;
use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;
use serde_json::Value;

const BASELINE: &str = "# Ada Lovelace\n\nLondon · ada@example.com\n\n## Experience\n\n### Acme · Engineer · 2015 – 2020\n\n\
- Built the engine. <!-- =engine.default -->\n- Hidden claim. <!-- @draft -->\n\n## Education\n\nB.S. in Mathematics.\n";

fn baseline(text: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = assert_ok!(tempfile::tempdir());
    let path = dir.path().join("Baseline.md");
    assert_ok!(std::fs::write(&path, text));
    (dir, path)
}

fn advisor() -> Command {
    assert_ok!(Command::cargo_bin("resume-advisor"))
}

fn stdout_json(args: &[&std::ffi::OsStr]) -> Value {
    let output = advisor()
        .arg("evidence")
        .args(args)
        .assert()
        .success()
        .get_output()
        .clone();
    assert!(output.stderr.is_empty(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_ok!(serde_json::from_slice(&output.stdout))
}

#[test]
fn evidence_lists_the_statements_a_model_may_cite_with_local_provenance() {
    let (_dir, path) = baseline(BASELINE);
    let json = stdout_json(&[path.as_os_str()]);
    assert_eq!(json["schema"], "resume-evidence/v1");
    let ids: Vec<&str> = json["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["record"]["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 3, "{ids:?}");
    assert!(ids.contains(&"engine.default"));
    let education = assert_some!(
        json["records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["record"]["text"] == "B.S. in Mathematics.")
    );
    assert!(education["record"]["id"].as_str().unwrap().starts_with("src:"));
    assert_eq!(json["excluded"].as_array().unwrap().len(), 1);
    assert!(json["records"][0]["provenance"]["byte_range"].is_object());
}

#[test]
fn the_model_payload_flag_leaves_out_everything_local() {
    let (_dir, path) = baseline(BASELINE);
    let output = advisor()
        .args(["evidence", "--model-payload"])
        .arg(&path)
        .assert()
        .success()
        .get_output()
        .clone();
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    for private in [
        "provenance",
        "byte_range",
        "excluded",
        "Hidden claim",
        "ada@example.com",
        "Ada Lovelace",
        "Baseline.md",
    ] {
        assert!(!text.contains(private), "leaked {private}: {text}");
    }
    assert!(text.contains("B.S. in Mathematics."));
}

#[test]
fn a_refusal_exits_with_an_error_and_leaves_stdout_empty() {
    for text in [
        "no name heading\n",
        "# A B\n\n## S\n\n- Only a draft. <!-- @draft -->\n",
    ] {
        let (_dir, path) = baseline(text);
        let output = advisor()
            .arg("evidence")
            .arg(&path)
            .assert()
            .failure()
            .get_output()
            .clone();
        assert!(
            output.stdout.is_empty(),
            "stdout: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn a_missing_file_and_a_directory_are_refused_with_the_path() {
    let (dir, _path) = baseline(BASELINE);
    for target in [dir.path().join("Missing.md"), dir.path().to_path_buf()] {
        let output = advisor()
            .arg("evidence")
            .arg(&target)
            .assert()
            .failure()
            .get_output()
            .clone();
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert!(stderr.contains(&target.display().to_string()), "{stderr}");
    }
}

#[test]
fn output_is_the_same_on_every_run() {
    let (_dir, path) = baseline(BASELINE);
    let first = advisor()
        .arg("evidence")
        .arg(&path)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let second = advisor()
        .arg("evidence")
        .arg(&path)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(first, second);
}
