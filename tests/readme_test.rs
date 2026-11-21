//! ABOUTME: Keeps the README true: its quick-start resume is clean, and every `resume ...` command it shows parses.
//! ABOUTME: The commands are read out of the README itself, so a renamed option fails here, not in a user's terminal.

use std::fs;

use claims::{assert_err, assert_ok};
use pretty_assertions::assert_eq;
use resume::cli::Cli;
use resume::commands::Outcome;

#[path = "common/workspace.rs"]
mod workspace;
use workspace::Workspace;

fn readme() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md");
    assert_ok!(fs::read_to_string(path))
}

/// The text of the Quick start section.
fn quick_start(readme: &str) -> &str {
    let start = readme.find("## Quick start").expect("the README has a Quick start");
    let end = readme[start..].find("\n## Fonts").map_or(readme.len(), |n| start + n);
    &readme[start..end]
}

/// The fenced blocks of `section` in the language `lang`, without their fences.
fn blocks<'a>(section: &'a str, lang: &str) -> Vec<&'a str> {
    let fence = format!("```{lang}\n");
    let mut found = Vec::new();
    let mut rest = section;
    while let Some(at) = rest.find(&fence) {
        let body = &rest[at + fence.len()..];
        let end = body.find("```").expect("a fenced block is closed");
        found.push(&body[..end]);
        rest = &body[end + 3..];
    }
    found
}

/// Every `resume ...` command in the Quick start: the lines of its `sh` blocks and the inline code of its table.
fn commands(section: &str) -> Vec<String> {
    let mut commands: Vec<String> = blocks(section, "sh")
        .into_iter()
        .flat_map(|block| block.lines().map(str::trim).filter(|line| !line.is_empty()))
        .map(str::to_string)
        .collect();
    for part in section.split('`').skip(1).step_by(2) {
        // Inline code and not a fence: a fence is three backticks, which split into empty pieces.
        if part.starts_with("resume ") && !commands.iter().any(|known| known == part) {
            commands.push(part.to_string());
        }
    }
    commands
}

#[test]
fn the_quick_start_resume_builds_with_no_findings() {
    let readme = readme();
    let section = quick_start(&readme);
    let markdown = blocks(section, "markdown");
    assert_eq!(markdown.len(), 1, "the quick start has one example resume");
    let work = Workspace::new();
    let file = work.file("Resume.md", markdown[0]);
    let check = work.check(&[&file], &[]);
    assert_eq!(check.outcome, Outcome::Success, "{}", check.stderr);
    assert!(check.stderr.contains("ok 0 errors, 0 warnings"), "{}", check.stderr);
    let build = work.build(&[&file], &[]);
    assert_eq!(build.outcome, Outcome::Success, "{}", build.stderr);
    assert!(build.stderr.contains("0 errors, 0 warnings"), "{}", build.stderr);
}

#[test]
fn every_command_in_the_quick_start_parses_as_written() {
    let readme = readme();
    let section = quick_start(&readme);
    let found = commands(section);
    // What the section holds is counted from its own text, so a row that is deleted, or no longer written as inline
    // `resume ...` code, makes the commands found fall short of it.
    let in_blocks: usize = blocks(section, "sh")
        .iter()
        .map(|block| block.lines().filter(|line| !line.trim().is_empty()).count())
        .sum();
    let in_table = section
        .lines()
        .filter(|line| line.starts_with("| ") && line.contains("`resume "))
        .count();
    assert!(
        in_table >= 8 && in_blocks == 2,
        "the quick start changed shape: {in_blocks} lines, {in_table} rows"
    );
    assert!(
        found.len() >= in_blocks + in_table,
        "commands were not found: {found:?}"
    );
    for command in found {
        let words: Vec<&str> = command.split_whitespace().collect();
        assert_eq!(words.first(), Some(&"resume"), "`{command}` is not a resume command");
        let parsed = Cli::try_parse_validated(words.iter().copied());
        assert!(
            parsed.is_ok(),
            "`{command}` does not parse: {}",
            parsed.err().map(|e| e.to_string()).unwrap_or_default()
        );
    }
}

#[test]
fn the_commands_the_readme_runs_on_the_example_do_what_it_says() {
    let readme = readme();
    let markdown = blocks(quick_start(&readme), "markdown")[0].to_string();
    let work = Workspace::new();
    let file = work.file("Resume.md", &markdown);
    // A dry run on a clean file changes nothing whether or not `--dry-run` works, so it is tried on a fixable one.
    let fixable = markdown.replace("2017 – Present", "2017 - Present");
    assert_ne!(fixable, markdown, "the example has the dates the test breaks");
    let broken = work.file("Broken.md", &fixable);
    let dry = work.doctor(&[&broken], &["--fix", "--dry-run"]);
    assert_eq!(dry.outcome, Outcome::Success, "{}", dry.stderr);
    assert!(
        dry.stderr
            .contains("-### Analytical Engines Ltd · Principal Engineer · 2017 - Present"),
        "{}",
        dry.stderr
    );
    assert_eq!(
        assert_ok!(fs::read_to_string(&broken)),
        fixable,
        "a dry run changes nothing"
    );
    let fixed = work.doctor(&[&broken], &["--fix"]);
    assert_eq!(fixed.outcome, Outcome::Success, "{}", fixed.stderr);
    assert_eq!(
        assert_ok!(fs::read_to_string(&broken)),
        markdown,
        "the fix puts the en dash back"
    );
    let explain = work.run(&["doctor".into(), "--explain".into(), "period-hyphen".into()]);
    assert_eq!(explain.outcome, Outcome::Success);
    assert!(
        explain.stdout.starts_with("period-hyphen (format)"),
        "{}",
        explain.stdout
    );
    let export = work.run(&[
        "export".into(),
        file.display().to_string(),
        work.path().join("preview").display().to_string(),
    ]);
    assert_eq!(export.outcome, Outcome::Success, "{}", export.stderr);
    assert!(work.path().join("preview/resume.json").is_file());
}

#[test]
fn the_exit_codes_the_readme_promises_are_the_ones_the_program_gives() {
    let work = Workspace::new();
    let bad = work.file("Bad.md", "## Experience\n\n- x\n");
    assert_eq!(work.check(&[&bad], &[]).outcome, Outcome::Failure);
    assert_eq!(Outcome::Success as u8, 0);
    assert_eq!(Outcome::Failure as u8, 1);
    let usage = assert_err!(Cli::try_parse_validated(["resume", "build"]));
    assert_eq!(usage.exit_code(), 2, "a usage error is clap's, exit 2");
    let readme = readme();
    assert!(
        readme.contains("`0` success (warnings allowed), `1` an error finding or a failed build, `2` a usage error"),
        "the README's exit code line changed"
    );
}

#[test]
fn the_readme_names_the_archives_the_release_script_makes() {
    let readme = readme();
    for name in ["resume-v0.1.0-macos-universal2.tar.gz", "resume-v0.1.0-windows-x64.zip"] {
        assert!(readme.contains(name), "{name} is not in the README");
    }
}
