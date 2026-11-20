//! ABOUTME: Proves every `resume doctor --explain` example reproduces its own code, and that its fix removes it.
//! ABOUTME: Markdown examples are parsed, command examples are run through the real doctor, theme lines get a stub style.

use std::collections::BTreeSet;

use pretty_assertions::assert_eq;
use resume::diagnostics::code::{Category, DiagnosticCode};
use strum::IntoEnumIterator;

#[path = "common/workspace.rs"]
mod workspace;
use workspace::Workspace;

/// What an example's `before` and `after` are, which decides how they are checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Resume text, parsed and linted. It may be a whole file or a piece that needs surroundings.
    Markdown,
    /// A `resume ...` command line, run through the real `doctor`.
    Command,
    /// One line of a style's `theme.typ`, put in a stub style and run through `doctor`.
    ThemeLine,
}

fn kind_of(code: DiagnosticCode, before: &str) -> Kind {
    match code.category() {
        Category::Format | Category::Content => Kind::Markdown,
        Category::Layout | Category::Setup if before.starts_with("resume ") => Kind::Command,
        Category::Layout | Category::Setup => Kind::ThemeLine,
    }
}

/// The codes parsing and linting resume text reports, as the content checks of `resume check` do.
fn markdown_codes(text: &str) -> BTreeSet<String> {
    let parsed = resume::markdown::parse("example.md", text);
    let mut codes: BTreeSet<String> = parsed.diagnostics().iter().map(|d| d.code.to_string()).collect();
    codes.extend(
        resume::doctor::content::lint_parsed(&parsed, std::path::Path::new("example.md"))
            .iter()
            .map(|d| d.code.to_string()),
    );
    codes
}

/// The ways a piece of resume text can be given surroundings, from none to the most: a whole file, the inside of
/// a section, the inside of a role.
fn surroundings(piece: &str) -> [String; 3] {
    let head = "# Ada Lovelace\n\nLondon, UK · ada@example.com\n\n## Experience\n\n";
    [
        piece.to_string(),
        format!("{head}{piece}\n"),
        format!("{head}### Acme · Engineer · 2020 – 2022\n\n{piece}\n"),
    ]
}

/// True when `piece`, given some surroundings, reports `code`.
fn markdown_reports(piece: &str, code: &str) -> bool {
    surroundings(piece)
        .iter()
        .any(|text| markdown_codes(text).contains(code))
}

fn finding_codes(json_lines: &str) -> BTreeSet<String> {
    json_lines
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|value| value["type"] == "diagnostic")
        .filter_map(|value| value["code"].as_str().map(str::to_string))
        .collect()
}

/// Runs `command` (an example such as `resume doctor resume.md --style monoo`) in a workspace where `resume.md`
/// and `out` name a copy of the full fixture and its output directory, and returns the codes it reports.
fn command_codes(work: &Workspace, resume_md: &std::path::Path, command: &str) -> BTreeSet<String> {
    let mut args: Vec<String> = command
        .split_whitespace()
        .skip(1) // `resume`
        .map(|token| match token {
            "resume.md" => resume_md.display().to_string(),
            "out" => work.out().display().to_string(),
            other => other.to_string(),
        })
        .collect();
    args.extend(["--format".to_string(), "json".to_string()]);
    // The examples do not depend on installed fonts, and scanning them is slow and differs between machines.
    if !args.iter().any(|arg| arg == "--fonts") {
        args.extend(["--fonts".to_string(), "bundled".to_string()]);
    }
    let run = work.run(&args);
    finding_codes(&run.stdout)
}

/// The codes `doctor` reports for a style whose theme declares only `line` as its font roles.
fn theme_line_codes(line: &str) -> BTreeSet<String> {
    let work = Workspace::new();
    let resume_md = work.file("resume.md", &workspace::fixture("full.md"));
    work.file(
        "styles/stub/style.typ",
        &format!("#set page(paper: sys.inputs.paper)\n#metadata(({line})) <resume-fonts>\nstub\n"),
    );
    let styles = work.path().join("styles").display().to_string();
    let run = work.run(
        &["doctor", &resume_md.display().to_string()]
            .into_iter()
            .map(str::to_string)
            .chain(
                [
                    "--fonts",
                    "bundled",
                    "--styles-dir",
                    &styles,
                    "--style",
                    "stub",
                    "--format",
                    "json",
                ]
                .map(str::to_string),
            )
            .collect::<Vec<_>>(),
    );
    finding_codes(&run.stdout)
}

/// The examples whose `after` cannot be run: its fix is to install fonts, which a test cannot do.
const AFTER_NEEDS_INSTALLED_FONTS: [DiagnosticCode; 1] = [DiagnosticCode::FontFallbackMetric];

#[test]
fn every_example_reproduces_its_code_and_its_fix_removes_it() {
    let work = Workspace::new();
    let resume_md = work.file("resume.md", &workspace::fixture("full.md"));
    let mut problems = Vec::new();
    let mut checked = 0;
    for code in DiagnosticCode::iter() {
        let Some((before, after)) = code.advice().example else {
            continue;
        };
        let id = code.to_string();
        let (reproduces, fixed) = match kind_of(code, before) {
            Kind::Markdown => (markdown_reports(before, &id), !markdown_reports(after, &id)),
            Kind::Command => (
                command_codes(&work, &resume_md, before).contains(&id),
                AFTER_NEEDS_INSTALLED_FONTS.contains(&code) || !command_codes(&work, &resume_md, after).contains(&id),
            ),
            Kind::ThemeLine => (
                theme_line_codes(before).contains(&id),
                !theme_line_codes(after).contains(&id),
            ),
        };
        checked += 1;
        if !reproduces {
            problems.push(format!(
                "{id}: the `before` example does not produce `{id}`: {before:?}"
            ));
        }
        if !fixed {
            problems.push(format!("{id}: the `after` example still produces `{id}`: {after:?}"));
        }
    }
    assert!(checked >= 30, "the check ran on only {checked} examples");
    assert_eq!(
        problems,
        Vec::<String>::new(),
        "fix the example in src/diagnostics/advice.rs"
    );
}

#[test]
#[should_panic(expected = "--no-such-option")]
fn a_command_example_that_the_command_line_does_not_accept_fails_loudly() {
    // The examples are run, not just read: an option that was renamed or removed must break the test.
    let work = Workspace::new();
    let resume_md = work.file("resume.md", &workspace::fixture("full.md"));
    let _ = command_codes(&work, &resume_md, "resume doctor resume.md --no-such-option");
}

#[test]
fn a_real_style_is_not_reported_as_unknown() {
    let work = Workspace::new();
    let resume_md = work.file("resume.md", &workspace::fixture("full.md"));
    assert!(!command_codes(&work, &resume_md, "resume doctor resume.md --style mono").contains("unknown-style"));
    assert!(command_codes(&work, &resume_md, "resume doctor resume.md --style monoo").contains("unknown-style"));
}

#[test]
fn the_checks_can_tell_a_reproducing_example_from_one_that_does_not() {
    // Negative controls for each kind: a text that does not have the code, and a line that does not either.
    assert!(
        !markdown_reports("2015 - 2017", "period-hyphen"),
        "a bare fragment is not a role line"
    );
    assert!(markdown_reports("### Acme · Engineer · 2015 - 2017", "period-hyphen"));
    assert!(
        !markdown_reports("### Acme · Engineer · 2015 – 2017", "period-hyphen"),
        "the fixed line no longer has the code"
    );
    assert!(
        !theme_line_codes(r#"body: (preferred: "Calibri", fallback: "Carlito", metric-compatible: true)"#)
            .contains("font-missing")
    );
    assert!(
        theme_line_codes(r#"body: (preferred: "Calibri", fallback: "Calbri", metric-compatible: true)"#)
            .contains("font-missing")
    );
}
