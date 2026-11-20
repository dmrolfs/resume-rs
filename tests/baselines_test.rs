//! ABOUTME: Builds the owner's two real resumes in every built-in style, to show the tool can replace the old one.
//! ABOUTME: Ignored by default: the resumes live outside this repository and never enter it; set RESUME_BASELINES.

use std::fs;
use std::path::PathBuf;

use claims::assert_ok;
use pretty_assertions::assert_eq;
use resume::commands::Outcome;

#[path = "common/workspace.rs"]
mod workspace;
use workspace::Workspace;

const BASELINES: [&str; 2] = ["Resume-Principal-Architect.md", "Resume-Org-Leader.md"];
const STYLES: [&str; 3] = ["pragmatic", "mono", "experienced"];

/// The directory of the real resumes, from `RESUME_BASELINES`; the test fails loudly when it is not set.
fn baselines_dir() -> PathBuf {
    let Some(dir) = std::env::var_os("RESUME_BASELINES") else {
        panic!(
            "set RESUME_BASELINES to the directory holding {} and {}, for example: \
             RESUME_BASELINES=~/Documents/vrm/my/job/drafts cargo test --test baselines_test -- --ignored",
            BASELINES[0], BASELINES[1]
        );
    };
    PathBuf::from(dir)
}

/// Builds both baselines in all three styles with the bundled fonts and expects no error finding and a PDF.
///
/// Warnings are allowed: they are advice about the writing, which is the author's to take or leave, and this test
/// would otherwise fail every time a resume was edited. Run with
/// `RESUME_BASELINES=~/Documents/vrm/my/job/drafts cargo test --test baselines_test -- --ignored`.
#[test]
#[ignore = "reads the real resumes from RESUME_BASELINES"]
fn baselines_build_in_all_styles() {
    let dir = baselines_dir();
    let work = Workspace::new();
    for style in STYLES {
        for name in BASELINES {
            let path = dir.join(name);
            let text = assert_ok!(fs::read_to_string(&path), "{}", path.display());
            // A copy in the workspace, so a build never writes beside the owner's files.
            let input = work.file(name, &text);
            let out = work.path().join("out").join(style);
            let run = work.run(&[
                "build".to_string(),
                input.display().to_string(),
                "--style".into(),
                style.into(),
                "--fonts".into(),
                "bundled".into(),
                "--out".into(),
                out.display().to_string(),
            ]);
            assert_eq!(run.outcome, Outcome::Success, "{name} in {style}:\n{}", run.stderr);
            assert!(
                !run.stderr.lines().any(|line| line.starts_with("error")),
                "{name} in {style}:\n{}",
                run.stderr
            );
            let pdf = out.join(name.replace(".md", ".pdf"));
            let bytes = assert_ok!(fs::read(&pdf), "{name} in {style}: no PDF at {}", pdf.display());
            assert!(bytes.starts_with(b"%PDF-"), "{name} in {style}: not a PDF");
            assert!(
                bytes.windows(5).any(|window| window == b"%%EOF"),
                "{name} in {style}: a PDF that ends early"
            );
        }
    }
}
