//! Runs the guide's letter commands, not merely their clap syntax.
use super::*;
use pretty_assertions::assert_eq;

#[test]
fn documented_letter_commands_produce_the_same_checked_letter() {
    let guide = guide_text();
    let commands: Vec<_> = guide
        .lines()
        .filter(|line| line.starts_with("resume letter "))
        .collect();
    assert!(commands.len() >= 3, "text, limit and PDF examples must be exercised");
    for command in commands {
        assert!(
            !command.contains(['\"', '\'']),
            "guide examples use unquoted single-word values"
        );
        let dir = assert_ok!(tempfile::tempdir());
        // Run the documented path unchanged from a checkout-shaped temporary directory.
        let fixture = PathBuf::from(assert_some!(command.split_whitespace().nth(2)));
        assert_ok!(std::fs::create_dir_all(dir.path().join(assert_some!(fixture.parent()))));
        assert_ok!(std::fs::copy(root().join(&fixture), dir.path().join(&fixture)));
        let output = assert_ok!(
            std::process::Command::new(env!("CARGO_BIN_EXE_resume"))
                .current_dir(dir.path())
                .args(command.split_whitespace().skip(1))
                .output()
        );
        assert!(
            output.status.success(),
            "{command}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            if command.contains("tailored.md") {
                "Hello Team,\n\nI build dependable services. \n\nI deliver shared infrastructure. \n\nRegards,\nAlex Example\n"
            } else {
                "Dear Team,\n\nI am applying for Architect at Acme.\n\nMy work includes shared platforms (https://example.invalid/) and Rust.\n\nSincerely,\nAlex Example\n"
            }
        );
        let pdf = dir.path().join("out/valid-letter.pdf");
        if command.contains("--output-format both") {
            let pages = common::page_texts(&assert_ok!(std::fs::read(pdf)));
            assert_eq!(pages.len(), 1);
            assert!(pages[0].contains("February 29, 2028"));
            assert!(pages[0].contains("I am applying for Architect at Acme."));
        } else {
            assert!(!dir.path().join("out").exists());
        }
    }
}
