//! ABOUTME: Proves every `resume doctor --explain` example reproduces its own code, and that its fix removes it.
//! ABOUTME: Markdown examples are parsed, command examples are run through the real doctor, theme lines get a stub style.

use std::collections::BTreeSet;

use pretty_assertions::assert_eq;
use resume::cli::Cli;
use resume::diagnostics::Severity;
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

/// Decides how an example is checked, from the code's category and the shape of its `before`.
///
/// Layout and Setup examples are a `resume ...` command line or, when they do not start with `resume `, one line of
/// a style's `theme.typ`. A Layout or Setup example that is neither (a resume fragment, say) would be run as a theme
/// line and fail with a message about fonts: give it a `resume ...` command, or extend this function.
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

/// True when `code`, as the report names it, is an error by default.
fn is_error(code: &str) -> bool {
    DiagnosticCode::iter().any(|c| c.to_string() == code && c.default_severity() == Severity::Error)
}

/// The error codes among `codes`.
fn errors_in(codes: &BTreeSet<String>) -> BTreeSet<String> {
    codes.iter().filter(|c| is_error(c)).cloned().collect()
}

/// For each of the surroundings of `before` that reports `code`, the errors `after` adds there. The fix is clean
/// when some such surrounding gets no new error: the piece may only make sense inside a role, say.
fn new_errors_by_surrounding(before: &str, after: &str, code: &str) -> Vec<Vec<String>> {
    let (befores, afters) = (surroundings(before), surroundings(after));
    befores
        .iter()
        .zip(&afters)
        .filter(|(text, _)| markdown_codes(text).contains(code))
        .map(|(before_text, after_text)| {
            let had = errors_in(&markdown_codes(before_text));
            errors_in(&markdown_codes(after_text))
                .difference(&had)
                .cloned()
                .collect()
        })
        .collect()
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
    if !args.iter().any(|arg| arg == "--format") {
        args.extend(["--format".to_string(), "json".to_string()]);
    }
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

/// The examples whose `after` is parsed but not run. `font-fallback-metric`'s fix is `--font-path <DIR>`, which the
/// harness cannot run: it forces `--fonts bundled` so results do not depend on the machine, and the command line
/// refuses `--font-path` together with `--fonts bundled`. The note also never appears on a machine that has the
/// preferred font installed, so there is no run whose output could show it gone.
const AFTER_IS_ONLY_PARSED: [DiagnosticCode; 1] = [DiagnosticCode::FontFallbackMetric];

/// Why a command example is not accepted by the command line, or `None` when it is.
fn command_line_problem(command: &str) -> Option<String> {
    Cli::try_parse_validated(command.split_whitespace())
        .err()
        .map(|e| e.to_string().lines().next().unwrap_or_default().to_string())
}

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
            Kind::Markdown => {
                // The fix must not trade the finding for a new error where the finding was reproduced.
                let by_surrounding = new_errors_by_surrounding(before, after, &id);
                if !by_surrounding.is_empty() && by_surrounding.iter().all(|new| !new.is_empty()) {
                    problems.push(format!(
                        "{id}: the `after` example adds the error(s) {by_surrounding:?}: {after:?}"
                    ));
                }
                (!by_surrounding.is_empty(), !markdown_reports(after, &id))
            }
            Kind::Command => {
                for (which, command) in [("before", before), ("after", after)] {
                    if let Some(why) = command_line_problem(command) {
                        problems.push(format!(
                            "{id}: the `{which}` command is not accepted ({why}): {command:?}"
                        ));
                    }
                }
                let reported = command_codes(&work, &resume_md, before);
                let fixed = if AFTER_IS_ONLY_PARSED.contains(&code) {
                    true
                } else {
                    let now = command_codes(&work, &resume_md, after);
                    let new_errors: Vec<_> = errors_in(&now).difference(&errors_in(&reported)).cloned().collect();
                    if !new_errors.is_empty() {
                        problems.push(format!(
                            "{id}: the `after` command adds the error(s) {new_errors:?}: {after:?}"
                        ));
                    }
                    !now.contains(&id)
                };
                (reported.contains(&id), fixed)
            }
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
    let with_examples = DiagnosticCode::iter().filter(|c| c.advice().example.is_some()).count();
    assert!(
        checked > 0 && checked == with_examples,
        "the check ran on {checked} of {with_examples} examples"
    );
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
fn an_after_command_that_the_command_line_rejects_or_a_fix_that_adds_an_error_is_caught() {
    assert!(command_line_problem("resume check resume.md --font-path ~/Fonts").is_none());
    assert!(command_line_problem("resume check resume.md --font-pth ~/Fonts").is_some());
    assert!(command_line_problem("complete nonsense (((").is_some());
    let found = |piece: &str| errors_in(&markdown_codes(&surroundings(piece)[2]));
    assert!(found("### Acme | Engineer | 2015 – 2017\n\n- Cut 40 to 9.").contains("role-separator"));
    assert!(found("### Acme · Engineer · 2015 – 2017\n\n- Cut 40 to 9.").is_empty());
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
