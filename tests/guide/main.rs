//! ABOUTME: Tests that bind docs/User-Guide.md to the code, so the guide cannot drift from the tool.
//! ABOUTME: Each check compares structure (sets of names, keys, codes, options), never wording; each has a negative control.

mod checks;
#[path = "../common/mod.rs"]
mod common;
mod extract;
mod facts;
mod letters;
mod synthetic;
mod theme_keys;

use std::path::PathBuf;

use claims::{assert_ok, assert_some};
use extract::Guide;
use facts::CliOption;

/// The repository root.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn guide_path() -> PathBuf {
    root().join("docs/User-Guide.md")
}

fn guide_text() -> String {
    let text = assert_ok!(std::fs::read_to_string(guide_path()));
    text.replace("\r\n", "\n")
}

fn load() -> Guide {
    Guide::parse(&guide_text())
}

// ---- helpers for the checks and their negative controls ----------------------------------------------

/// `text` with the first `from` replaced by `to`. Fails when `from` is not there, so a control cannot pass by
/// changing nothing.
fn replaced(text: &str, from: &str, to: &str) -> String {
    assert!(text.contains(from), "the control needs `{from}` in:\n{text}");
    text.replacen(from, to, 1)
}

/// `text` with the first line that starts with `prefix` changed by `edit`.
fn edited_line(text: &str, prefix: &str, edit: impl Fn(&str) -> String) -> String {
    let mut done = false;
    let lines: Vec<String> = text
        .lines()
        .map(|line| {
            if !done && line.starts_with(prefix) {
                done = true;
                edit(line)
            } else {
                line.to_string()
            }
        })
        .collect();
    assert!(done, "the control needs a line that starts with `{prefix}` in:\n{text}");
    lines.join("\n") + "\n"
}

/// `text` without the first line that starts with `prefix`.
fn without_line(text: &str, prefix: &str) -> String {
    edited_line(text, prefix, |_| "<!-- removed -->".to_string())
}

fn assert_clean(problems: Vec<String>) {
    assert!(problems.is_empty(), "\n  {}\n", problems.join("\n  "));
}

fn assert_problem(problems: &[String], containing: &str) {
    assert!(
        problems.iter().any(|p| p.contains(containing)),
        "expected a problem containing `{containing}`, got {problems:#?}"
    );
}

/// The first option, with its command, that `wanted` accepts.
fn find_option(wanted: impl Fn(&CliOption) -> bool) -> (String, CliOption) {
    assert_some!(
        facts::commands()
            .into_iter()
            .flat_map(|(command, options)| options.into_iter().map(move |o| (command.clone(), o)))
            .find(|(_, option)| wanted(option))
    )
}

/// True when the table row `line` documents `option`: its first cell shows the option.
fn row_documents(line: &str, option: &CliOption) -> bool {
    let Some(cell) = line.split('|').nth(1) else {
        return false;
    };
    let cell = cell.replace('`', "");
    if option.positional {
        cell.contains(&option.key())
    } else {
        cell.split(|c: char| !(c.is_alphanumeric() || c == '-'))
            .any(|w| w == option.key())
    }
}

/// `text` with the row of `option` in the section of `command` changed by `edit`.
fn edit_row(text: &str, command: &str, option: &CliOption, edit: impl Fn(&str) -> String) -> String {
    let heading = format!("### resume {command}\n");
    let start = assert_some!(text.find(&heading));
    let end = text[start + 1..].find("\n### ").map_or(text.len(), |i| start + 1 + i);
    let section = &text[start..end];
    let row = assert_some!(
        section
            .lines()
            .find(|l| l.starts_with("| ") && row_documents(l, option))
    )
    .to_string();
    let edited = section.replacen(&row, &edit(&row), 1);
    format!("{}{edited}{}", &text[..start], &text[end..])
}

// ---- the guide itself ---------------------------------------------------------------------------------

#[test]
fn the_guide_exists_and_starts_with_the_two_aboutme_lines() {
    let text = guide_text();
    let first_two: Vec<&str> = text.lines().take(2).map(str::trim_end).collect();
    assert!(
        first_two
            .iter()
            .all(|l| l.starts_with("<!-- ABOUTME:") && l.ends_with("-->")),
        "the first two lines are `<!-- ABOUTME: ... -->`, as in every file of the project: {first_two:?}"
    );
}

#[test]
fn a_guide_with_windows_line_endings_gets_the_same_verdicts() {
    let crlf = Guide::parse(&guide_text().replace('\n', "\r\n"));
    assert_clean(checks::code_tables(&crlf));
    assert_clean(checks::required_headings(&crlf));
    assert_clean(checks::markdown_examples(&crlf));
}

#[test]
fn the_extractors_and_facts_see_what_the_repository_has() {
    // Sanity only: a change to the CLI or the styles is caught by the checks, which compare with these facts.
    assert!(!facts::commands().is_empty() && !facts::global_options().is_empty());
    assert!(facts::commands().values().flatten().any(|o| o.positional));
    assert!(facts::commands().values().flatten().any(|o| o.short.is_some()));
    assert!(facts::commands().values().flatten().any(|o| o.repeatable));
    assert!(
        facts::commands()
            .values()
            .flatten()
            .any(|o| !o.values.is_empty() && o.default.is_some())
    );
    assert!(facts::every_short_option().contains(&'h'));
    assert!(facts::built_in_styles().contains(&"pragmatic".to_string()));
    assert!(facts::components().contains("render-sections") && !facts::components().contains("_has-link"));
    assert!(facts::bundled_families().contains("carlito"));
    assert!(facts::font_roles().contains("body"));
    assert!(facts::theme_keys_by_style()["pragmatic"].contains("spacing.section"));
    assert!(checks::env_vars_read_by_the_program().contains("RESUME_FONT_PATH"));
}

// ---- headings and planned sections -------------------------------------------------------------------

#[test]
fn the_guide_has_every_required_heading_in_order_and_none_is_empty() {
    assert_clean(checks::required_headings(&load()));
}

#[test]
fn a_missing_a_repeated_an_empty_or_a_misplaced_heading_is_caught() {
    let clean = synthetic::headings();
    assert_clean(checks::required_headings(&Guide::parse(&clean)));
    let body = "A first line.\n\nA second line.\n\nA third line.\n\n";
    assert_problem(
        &checks::required_headings(&Guide::parse(&replaced(&clean, "## Fonts\n", "## Typefaces\n"))),
        "no `## Fonts` heading",
    );
    assert_problem(
        &checks::required_headings(&Guide::parse(&format!("{clean}## Fonts\n\n{body}"))),
        "more than once",
    );
    assert_problem(
        &checks::required_headings(&Guide::parse(&replaced(
            &clean,
            &format!("## Fonts\n\n{body}"),
            "## Fonts\n\n",
        ))),
        "`Fonts`",
    );
    let moved = replaced(&clean, &format!("## Quick start\n\n{body}"), "");
    let moved = format!("{moved}## Quick start\n\n{body}");
    assert_problem(&checks::required_headings(&Guide::parse(&moved)), "out of order");
}

#[test]
fn the_planned_sections_are_stubs_that_claim_nothing_and_name_a_witness() {
    assert_clean(checks::planned_sections(&load()));
}

#[test]
fn a_planned_section_fails_when_its_feature_has_arrived_or_it_claims_something() {
    let planned = |witness: &str, extra: &str| Guide::parse(&synthetic::planned("Annotations", witness, extra));
    let (command, _) = find_option(|_| true);
    let global = format!("option:--{}", facts::global_options()[0].name);

    assert_clean(checks::planned_sections(&planned("command:no-such-command", "")));
    assert_clean(checks::planned_sections(&planned("option:--no-such-option", "")));
    assert_problem(
        &checks::planned_sections(&planned(&format!("command:{command}"), "")),
        "now exists",
    );
    assert_problem(&checks::planned_sections(&planned(&global, "")), "now exists");
    assert_problem(
        &checks::planned_sections(&Guide::parse(&replaced(
            &synthetic::planned("Annotations", "command:x", ""),
            "witness=command:x",
            "",
        ))),
        "witness=KIND:VALUE",
    );
    assert_problem(
        &checks::planned_sections(&planned("nonsense", "")),
        "is not `command:NAME`",
    );
    assert_problem(
        &checks::planned_sections(&planned("command:x", "```markdown unchecked\n# x\n```\n")),
        "fenced block",
    );
    assert_problem(
        &checks::planned_sections(&planned("command:x", "Use `--tags` to pick.\n")),
        "names an option",
    );
    assert_problem(
        &checks::planned_sections(&planned("command:x", &"A line.\n\n".repeat(10))),
        "longer than 14 lines",
    );
    assert_problem(
        &checks::planned_sections(&Guide::parse("<!-- guide:planned x witness=command:x -->\n")),
        "outside any section",
    );
}

#[test]
fn a_guide_whose_last_planned_section_has_been_written_is_clean() {
    // The planned markers are the only thing that makes a section a stub: with none, nothing is planned.
    assert_clean(checks::planned_sections(&Guide::parse(&synthetic::headings())));
    assert_clean(checks::required_headings(&Guide::parse(&synthetic::headings())));
}

// ---- Markdown and shell examples ---------------------------------------------------------------------

/// A small guide with one checked example of each mode, to break one thing at a time.
fn examples() -> String {
    "# Guide\n\n## Examples\n\n\
A role with a pipe is [`role-separator`](#code-role-separator):\n\n\
```markdown in-section expect=role-separator\n### Acme | Engineer | 2015 – 2017\n\n- Cut build time from 40 minutes to 9.\n```\n\n\
A whole resume:\n\n\
```markdown resume expect=none\n# Ada Lovelace\n\nLondon, UK · ada@example.com\n\n## Experience\n\n### Acme · Engineer · 2015 – 2017\n\n- Cut build time from 40 minutes to 9.\n```\n"
        .to_string()
}

#[test]
fn every_markdown_example_produces_exactly_the_findings_the_guide_says() {
    assert_clean(checks::markdown_examples(&load()));
}

#[test]
fn a_wrong_expectation_a_missing_mode_a_missing_link_or_a_disguised_fence_is_caught() {
    let clean = examples();
    assert_clean(checks::markdown_examples(&Guide::parse(&clean)));
    let wrong = replaced(&clean, "expect=none", "expect=weak-opener");
    assert_problem(
        &checks::markdown_examples(&Guide::parse(&wrong)),
        "documented as producing",
    );
    let no_mode = replaced(&clean, "markdown resume expect=none", "markdown expect=none");
    assert_problem(
        &checks::markdown_examples(&Guide::parse(&no_mode)),
        "needs exactly one of",
    );
    let no_expect = replaced(&clean, "markdown resume expect=none", "markdown resume");
    assert_problem(&checks::markdown_examples(&Guide::parse(&no_expect)), "needs `expect=");
    let two = format!("{clean}\n```markdown unchecked\nx\n```\n\n```markdown unchecked\ny\n```\n");
    assert_problem(&checks::markdown_examples(&Guide::parse(&two)), "at most one");
    let unlinked = replaced(&clean, "(#code-role-separator)", "(#fonts)");
    assert_problem(&checks::markdown_examples(&Guide::parse(&unlinked)), "must link");
    for spelling in ["md", "Markdown", "MD"] {
        let disguised = replaced(
            &clean,
            "```markdown in-section expect=role-separator",
            &format!("```{spelling} in-section expect=none"),
        );
        assert_problem(
            &checks::markdown_examples(&Guide::parse(&disguised)),
            "documented as producing",
        );
    }
    let comma = replaced(
        &clean,
        "```markdown in-section expect=role-separator",
        "```markdown,in-section expect=none",
    );
    assert_problem(
        &checks::markdown_examples(&Guide::parse(&comma)),
        "needs exactly one of",
    );
}

#[test]
fn every_shell_example_is_accepted_by_the_program() {
    assert_clean(checks::shell_examples(&load()));
}

#[test]
fn a_shell_example_with_a_renamed_option_or_a_foreign_command_is_caught() {
    // A command that needs no arguments, so the only difference between the two lines is the option.
    let command = assert_some!(
        facts::commands()
            .into_iter()
            .find(|(_, options)| options.iter().all(|o| !o.positional))
            .map(|(name, _)| name)
    );
    let fence = |line: &str| Guide::parse(&format!("# Guide\n\n```sh\n{line}\n```\n"));
    assert_clean(checks::shell_examples(&fence(&format!("resume {command} # a comment"))));
    assert_problem(
        &checks::shell_examples(&fence(&format!("resume {command} --no-such-option"))),
        "is not accepted",
    );
    assert_problem(&checks::shell_examples(&fence("ls -l")), "must start with `resume `");
    assert_problem(
        &checks::shell_examples(&fence(&format!("resume {command} | cat"))),
        "free of quotes, pipes",
    );
}

// ---- the worked example of specs and profiles ------------------------------------------------------

#[test]
fn the_worked_spec_example_resolves_and_prints_what_the_guide_says() {
    assert_clean(checks::spec_examples(&load()));
}

#[test]
fn a_worked_example_that_does_not_resolve_or_misprints_or_shows_an_unused_profile_is_caught() {
    let text = guide_text();
    let with = |from: &str, to: &str| Guide::parse(&replaced(&text, from, to));
    assert_problem(
        &checks::spec_examples(&with("tags = [\"iceberg\"]\nprefer", "tag = [\"iceberg\"]\nprefer")),
        "does not resolve",
    );
    assert_problem(
        &checks::spec_examples(&with(
            "paper = \"a4\"\n\n[select]\ntags = [\"ai\", \"iceberg\"",
            "paper = \"letter\"\n\n[select]\ntags = [\"ai\", \"iceberg\"",
        )),
        "differs from what --print-spec prints",
    );
    assert_problem(
        &checks::spec_examples(&with(
            "extends = [\"principal\", \"ai-platform\"]",
            "extends = [\"principal\"]",
        )),
        "which the spec does not extend",
    );
    assert_problem(
        &checks::spec_examples(&with("```toml profiles/principal.toml", "```toml")),
        "must name its file",
    );
    // The printed example and the sample of a build are each compared with the program.
    assert_problem(
        &checks::spec_examples(&with(
            "  profiles: principal -> ai-platform -> the spec",
            "  profiles: ai-platform -> principal -> the spec",
        )),
        "sample of what a spec build prints differs",
    );
    assert_problem(
        &checks::spec_examples(&with("  wordings: 2 chosen (2 pick)", "  wordings: 3 chosen (3 pick)")),
        "sample of what a spec build prints differs",
    );
    assert_problem(
        &checks::spec_examples(&with("```text\nok out/acme.pdf", "```text\nnot a sample out/acme.pdf")),
        "no `text` sample",
    );
    assert_problem(
        &checks::spec_examples(&with("```toml resolved", "```toml positions/other.toml")),
        "exactly one spec under positions/",
    );
}

#[test]
fn a_letter_sample_that_drifts_or_goes_missing_is_caught() {
    let text = guide_text();
    let with = |from: &str, to: &str| Guide::parse(&replaced(&text, from, to));
    assert_problem(
        &checks::spec_examples(&with(
            "  settings: output text (default); links text-and-url (default); max-length 2000",
            "  settings: output both (default); links text-and-url (default); max-length 2000",
        )),
        "sample of what a spec letter prints differs",
    );
    assert_problem(
        &checks::spec_examples(&with("wording opening.warm: pick", "wording opening.formal: pick")),
        "sample of what a spec letter prints differs",
    );
    assert_problem(
        &checks::spec_examples(&with(
            "```text\nselection: tags platform",
            "```text\nnot a sample: tags platform",
        )),
        "no `text` sample of what a spec letter prints",
    );
    // The letter tables are part of what the guide's `toml resolved` pins.
    assert_problem(
        &checks::spec_examples(&with(
            "max-length = 2000\n\n[letter.select]\ntags",
            "max-length = 3000\n\n[letter.select]\ntags",
        )),
        "differs from what --print-spec prints",
    );
}

#[test]
fn a_worked_example_without_its_printed_resolution_is_caught() {
    let text = guide_text();
    let without = Guide::parse(&replaced(&text, "```toml resolved", "```text"));
    assert_problem(&checks::spec_examples(&without), "no `toml resolved` example");
}

// ---- theme keys, components, styles ------------------------------------------------------------------

#[test]
fn every_theme_key_of_every_built_in_style_is_documented_and_nothing_else() {
    assert_clean(checks::theme_keys(&load()));
}

#[test]
fn the_typst_sources_read_only_keys_that_a_theme_defines() {
    assert_clean(checks::theme_reads_are_defined());
}

#[test]
fn a_typst_read_of_an_undefined_key_is_caught_in_every_style_and_in_the_components() {
    let defined: std::collections::BTreeSet<String> = facts::theme_keys_by_style().into_values().flatten().collect();
    for (name, _) in checks::typst_sources() {
        let mut sources = checks::typst_sources();
        for (n, text) in &mut sources {
            if *n == name {
                text.push_str("\n#let extra = theme.skills.colgap\n");
            }
        }
        assert_problem(
            &checks::theme_reads_problems(&sources, &defined),
            &format!("{name} reads `theme.skills.colgap`"),
        );
    }
    let nothing = vec![("styles/x/style.typ".to_string(), "#let x = 1".to_string())];
    assert_problem(&checks::theme_reads_problems(&nothing, &defined), "reads no theme key");
}

#[test]
fn what_a_typst_source_reads_from_the_theme_is_read_exactly() {
    let reads = checks::theme_read_paths(
        "// theme.commented.out\n#let a = theme.size.body\n#let b = theme.fonts.display.preferred + theme.fonts.display\n#let c = theme.at(\"gutter\", default: 0pt)\n#let d = mytheme.size.name\n",
    );
    let reads: Vec<&str> = reads.iter().map(String::as_str).collect();
    assert_eq!(reads, ["fonts.<role>", "fonts.<role>.preferred", "size.body"]);
}

#[test]
fn a_removed_a_misspelled_or_a_mis_attributed_theme_key_is_caught() {
    let clean = synthetic::theme_keys();
    assert_clean(checks::theme_keys(&Guide::parse(&clean)));
    let row = assert_some!(clean.lines().find(|l| l.starts_with("| `size.body`"))).to_string();
    assert_problem(
        &checks::theme_keys(&Guide::parse(&without_line(&clean, "| `size.body`"))),
        "`size.body`",
    );
    let typo = clean.replacen(&row, &row.replace("size.body", "size.bdoy"), 1);
    let problems = checks::theme_keys(&Guide::parse(&typo));
    assert_problem(&problems, "documents theme key `size.bdoy`");
    assert_problem(&problems, "`size.body`");
    let not_all = assert_some!(clean.lines().find(|l| l.starts_with("| `") && !l.ends_with("| all |"))).to_string();
    let (head, _) = assert_some!(not_all.rsplit_once(" | "));
    let claims_all = clean.replacen(&not_all, &format!("{head} | all |"), 1);
    assert_problem(&checks::theme_keys(&Guide::parse(&claims_all)), "Styles column");
    let no_column = replaced(&clean, "| Key | Unit | Styles |", "| Key | Unit | Where |");
    assert_problem(&checks::theme_keys(&Guide::parse(&no_column)), "`Styles` column");
}

#[test]
fn every_shared_component_has_a_row() {
    assert_clean(checks::components(&load()));
}

#[test]
fn a_missing_or_an_invented_component_is_caught() {
    let clean = synthetic::components();
    assert_clean(checks::components(&Guide::parse(&clean)));
    assert_problem(
        &checks::components(&Guide::parse(&without_line(&clean, "| `render-sections` |"))),
        "`render-sections`",
    );
    let invented = replaced(&clean, "| `render-sections` |", "| `render-everything` |");
    assert_problem(&checks::components(&Guide::parse(&invented)), "does not define");
}

#[test]
fn every_built_in_style_has_a_section_and_no_other_does() {
    assert_clean(checks::styles(&load()));
}

#[test]
fn a_renamed_style_section_is_caught() {
    let clean = synthetic::styles();
    assert_clean(checks::styles(&Guide::parse(&clean)));
    let name = &facts::built_in_styles()[0];
    let problems = checks::styles(&Guide::parse(&replaced(
        &clean,
        &format!("#### {name}\n"),
        &format!("#### {name}x\n"),
    )));
    assert_problem(&problems, &format!("`#### {name}`"));
    assert_problem(&problems, &format!("`{name}x`"));
}

#[test]
fn every_built_in_style_readme_and_the_guide_are_linked_from_where_a_reader_looks() {
    let readme = std::fs::read_to_string(root().join("README.md")).unwrap_or_default();
    assert_clean(checks::reading_list(&load(), &readme));
}

#[test]
fn a_style_readme_or_the_guide_link_that_is_missing_is_caught() {
    let styles = facts::built_in_styles();
    let links: String = styles
        .iter()
        .map(|s| format!("- [{s}](../styles/{s}/README.md)\n"))
        .collect();
    let clean = format!("## Where to read more\n\n{links}");
    let readme = "See [guide](docs/User-Guide.md).";
    assert_clean(checks::reading_list(&Guide::parse(&clean), readme));
    let missing = replaced(
        &clean,
        &format!("(../styles/{}/README.md)", styles[0]),
        "(../README.md)",
    );
    assert_problem(
        &checks::reading_list(&Guide::parse(&missing), readme),
        &format!("../styles/{}/README.md", styles[0]),
    );
    assert_problem(
        &checks::reading_list(&Guide::parse(&clean), "No link."),
        "must link docs/User-Guide.md",
    );
}

#[test]
fn every_environment_variable_the_program_reads_is_documented() {
    assert_clean(checks::environment_variables(
        &load(),
        &checks::env_vars_read_by_the_program(),
    ));
}

#[test]
fn an_undocumented_environment_variable_is_caught() {
    let read: std::collections::BTreeSet<String> = ["NO_COLOR", "TERM"].map(String::from).into();
    let documented = Guide::parse(&synthetic::environment(&["NO_COLOR", "TERM"]));
    assert_clean(checks::environment_variables(&documented, &read));
    let partial = Guide::parse(&synthetic::environment(&["NO_COLOR"]));
    assert_problem(
        &checks::environment_variables(&partial, &read),
        "does not document `TERM`",
    );
    assert_problem(
        &checks::environment_variables(&documented, &std::collections::BTreeSet::new()),
        "the scan may be broken",
    );
}

// ---- commands and options ----------------------------------------------------------------------------

#[test]
fn every_command_and_option_is_documented_with_its_short_form_values_and_default() {
    assert_clean(checks::commands(&load()));
}

#[test]
fn the_synthetic_commands_guide_is_clean() {
    assert_clean(checks::commands(&Guide::parse(&synthetic::commands())));
}

#[test]
fn a_missing_command_a_missing_option_or_a_missing_value_is_caught() {
    let clean = synthetic::commands();
    let (command, _) = find_option(|_| true);
    let renamed = replaced(
        &clean,
        &format!("### resume {command}\n"),
        &format!("### resume {command}x\n"),
    );
    let problems = checks::commands(&Guide::parse(&renamed));
    assert_problem(&problems, &format!("resume {command}"));
    assert_problem(&problems, &format!("resume {command}x"));

    let (command, option) = find_option(|o| !o.positional);
    let dropped = edit_row(&clean, &command, &option, |_| {
        "| `--zzz-unrelated-row` | x | x |".to_string()
    });
    assert_problem(
        &checks::commands(&Guide::parse(&dropped)),
        &format!("`{}` is not documented", option.key()),
    );

    let (command, option) = find_option(|o| !o.values.is_empty());
    let value = &option.values[0];
    let lost = edit_row(&clean, &command, &option, |row| {
        row.replace(&format!("`{value}`"), value)
    });
    assert_problem(&checks::commands(&Guide::parse(&lost)), &format!("value `{value}`"));
}

#[test]
fn a_default_that_changed_is_caught_even_when_the_values_still_list_it() {
    let clean = synthetic::commands();
    // The case that matters: an enumeration whose default moved to another of its own values.
    let (command, option) = find_option(|o| o.default.is_some() && o.values.len() > 1);
    let default = assert_some!(option.default.clone());
    let other = assert_some!(option.values.iter().find(|v| **v != default)).clone();
    let drifted = edit_row(&clean, &command, &option, |row| {
        row.replace(&format!("default `{default}`"), &format!("default `{other}`"))
    });
    assert_problem(&checks::commands(&Guide::parse(&drifted)), "must state its default");

    let (command, option) = find_option(|o| o.default.is_some());
    let default = assert_some!(option.default.clone());
    let unstated = edit_row(&clean, &command, &option, |row| {
        row.replace(&format!("default `{default}`"), "usually the first")
    });
    assert_problem(&checks::commands(&Guide::parse(&unstated)), "must state its default");

    let (command, option) = find_option(|o| o.default.is_none() && !o.positional);
    let invented = edit_row(&clean, &command, &option, |row| {
        row.replacen(" | what it does |", " | default `x` | what it does |", 1)
    });
    assert_problem(&checks::commands(&Guide::parse(&invented)), "but the option has none");
}

#[test]
fn a_short_form_that_is_missing_or_does_not_exist_is_caught() {
    let clean = synthetic::commands();
    let (command, option) = find_option(|o| o.short.is_some());
    let short = assert_some!(option.short);
    let missing = edit_row(&clean, &command, &option, |row| row.replace(&format!("-{short}, "), ""));
    assert_problem(
        &checks::commands(&Guide::parse(&missing)),
        &format!("short form `-{short}`"),
    );

    let (command, option) = find_option(|o| o.short.is_none() && !o.positional);
    let invented = edit_row(&clean, &command, &option, |row| row.replacen("| `", "| `-z, ", 1));
    assert_problem(&checks::commands(&Guide::parse(&invented)), "shows `-z`");

    let prose = format!("{clean}\nPress `-q` to be quiet.\n");
    assert_problem(&checks::commands(&Guide::parse(&prose)), "mentions `-q`");
    let real = format!("{clean}\nPress `-h` for help.\n");
    assert_clean(checks::commands(&Guide::parse(&real)));
}

#[test]
fn a_repeatable_option_and_a_documented_option_that_does_not_exist_are_caught() {
    let clean = synthetic::commands();
    let (command, option) = find_option(|o| o.repeatable && !o.positional);
    let again = edit_row(&clean, &command, &option, |row| row.replace("repeatable", "again"));
    assert_problem(&checks::commands(&Guide::parse(&again)), "repeatable");

    let (command, option) = find_option(|o| !o.positional);
    let invented = edit_row(&clean, &command, &option, |row| {
        format!("{row}\n| `--debounce <DURATION>` | `150ms` | wait |")
    });
    assert_problem(&checks::commands(&Guide::parse(&invented)), "--debounce");
    let prose = format!("{clean}\nUse `--no-such-flag` to see it.\n");
    assert_problem(&checks::commands(&Guide::parse(&prose)), "--no-such-flag");
}

// ---- diagnostics: the generated appendix and the links ---------------------------------------------------

/// A guide holding exactly the generated block, as `bless_guide_tables` would write it.
fn generated() -> String {
    format!(
        "# Guide\n\n{}\n\n{}{}\n",
        checks::START,
        checks::render_code_tables(),
        checks::END
    )
}

#[test]
fn the_code_tables_are_what_the_code_generates() {
    assert_clean(checks::code_tables(&load()));
}

#[test]
fn a_changed_severity_a_removed_row_or_a_missing_marker_is_caught() {
    let clean = generated();
    assert_clean(checks::code_tables(&Guide::parse(&clean)));
    assert_problem(
        &checks::code_tables(&Guide::parse(&replaced(&clean, "| warning |", "| note |"))),
        "out of date",
    );
    assert_problem(
        &checks::code_tables(&Guide::parse(&without_line(&clean, "| <a id=\"code-role-separator\">"))),
        "out of date",
    );
    assert_problem(
        &checks::code_tables(&Guide::parse(&replaced(&clean, checks::START, ""))),
        "marker",
    );
}

#[test]
fn every_link_in_the_guide_goes_somewhere() {
    assert_clean(checks::links(&load()));
}

#[test]
fn a_link_to_no_code_no_heading_no_file_or_nothing_is_caught() {
    let base = "# Guide\n\n## Real heading\n\nText.\n\n";
    assert_clean(checks::links(&Guide::parse(&format!(
        "{base}[a](#real-heading) [b](#code-period-hyphen) [c](../README.md)\n"
    ))));
    assert_problem(
        &checks::links(&Guide::parse(&format!("{base}[x](#code-no-such-code)\n"))),
        "names no diagnostic code",
    );
    assert_problem(
        &checks::links(&Guide::parse(&format!("{base}[x](#no-such-heading)\n"))),
        "points at no heading",
    );
    assert_problem(
        &checks::links(&Guide::parse(&format!("{base}[x](../nowhere/at-all.md)\n"))),
        "does not exist",
    );
    assert_problem(
        &checks::links(&Guide::parse(&format!("{base}[x](../styles)\n"))),
        "does not exist",
    );
    assert_problem(&checks::links(&Guide::parse(&format!("{base}[x]()\n"))), "no target");
}

#[test]
fn links_are_counted_without_the_parser_outside_fences_and_code_spans() {
    let text = "[a](#x) and `[not](#a-link)` and ``[nor](#this)`` and [b](#y)\n\n```md\n[c](#in-a-fence)\n```\n\n| t |\n|---|\n| [d](#z) |\n";
    assert_eq!(checks::links_in_text(text), 3);
    assert_eq!(Guide::parse(text).links.len(), 3);
}

#[test]
fn the_fonts_section_covers_the_roles_fields_families_codes_and_variables() {
    assert_clean(checks::fonts(&load()));
}

#[test]
fn a_fonts_section_that_loses_a_role_a_family_or_a_code_link_is_caught() {
    let clean = synthetic::fonts();
    assert_clean(checks::fonts(&Guide::parse(&clean)));
    let role = assert_some!(facts::font_roles().into_iter().next());
    assert_problem(
        &checks::fonts(&Guide::parse(&replaced(&clean, &format!("`{role}`"), &role))),
        &format!("role `{role}`"),
    );
    let family = assert_some!(facts::bundled_families().into_iter().next());
    assert_problem(
        &checks::fonts(&Guide::parse(&replaced(
            &clean,
            &format!("- {family}:"),
            "- the free font:",
        ))),
        &family,
    );
    assert_problem(
        &checks::fonts(&Guide::parse(&replaced(&clean, "(#code-font-missing)", "(#fonts)"))),
        "font-missing",
    );
    assert_problem(
        &checks::fonts(&Guide::parse(&replaced(&clean, "RESUME_FONT_PATH", "the variable"))),
        "RESUME_FONT_PATH",
    );
}

// ---- no personal content -----------------------------------------------------------------------------

/// A small guide with one example resume, as clean as the real guide's.
fn personal_base(extra: &str) -> String {
    format!(
        "# Guide\n\nAsk ada@example.com or see [Ada](https://github.com/ada) and [Typst](https://typst.app/docs).\nCall +44 20 7946 0000.\n\n```markdown resume expect=none\n# Ada Lovelace\n\nLondon, UK · ada@example.com\n\n## Experience\n\n### Acme · Engineer · 2015 – 2017\n\n- Cut build time from 40 minutes to 9.\n```\n\n{extra}\n"
    )
}

#[test]
fn the_guide_holds_only_synthetic_names_companies_addresses_and_numbers() {
    assert_clean(checks::no_personal_content(&load()));
}

#[test]
fn an_email_an_address_a_phone_number_a_name_or_a_company_that_is_not_synthetic_is_caught() {
    assert_clean(checks::no_personal_content(&Guide::parse(&personal_base(""))));
    let problems = |extra: &str| checks::no_personal_content(&Guide::parse(&personal_base(extra)));
    assert_problem(&problems("Write to jane.doe@gmail.com."), "jane.doe@gmail.com");
    assert_problem(&problems("[me](https://www.linkedin.com/in/jane)"), "linkedin");
    assert_problem(&problems("See linkedin.com/in/jane for more."), "linkedin.com");
    assert_problem(
        &problems("[me](https://github.com/jordanquill)"),
        "github.com/jordanquill",
    );
    assert_problem(&problems("See github.com/jordanquill."), "github.com/jordanquill");
    assert_problem(&problems("Call +1 415 555 0123."), "phone-like");
    assert_problem(&problems("Call 415-555-2671."), "phone-like");
    assert_clean(problems("The code is at https://github.com/ada/analytical-engine."));
    assert_clean(problems("Call 415-555-0142, a number reserved for fiction."));
    let name = replaced(&personal_base(""), "# Ada Lovelace", "# Jane Doe");
    assert_problem(&checks::no_personal_content(&Guide::parse(&name)), "Jane Doe");
    for separator in ["·", "|"] {
        let company = replaced(
            &personal_base(""),
            "### Acme · Engineer",
            &format!("### Secretco {separator} Engineer"),
        );
        assert_problem(&checks::no_personal_content(&Guide::parse(&company)), "Secretco");
    }
    let disguised = replaced(&personal_base(""), "```markdown resume", "```Markdown resume");
    let disguised = replaced(&disguised, "# Ada Lovelace", "# Jane Doe");
    assert_problem(&checks::no_personal_content(&Guide::parse(&disguised)), "Jane Doe");
}

/// Reads the owner's real baselines at run time and fails if anything private from them is in the guide.
/// No private string is committed: the details are read from the files named by `RESUME_BASELINES`, and only
/// counts are printed.
///
/// Run with `RESUME_BASELINES=<directory of baseline resumes> cargo test --test guide -- --ignored`.
#[test]
#[ignore = "reads the real resumes from RESUME_BASELINES"]
fn the_guide_contains_nothing_from_the_real_baselines() {
    let Some(dir) = std::env::var_os("RESUME_BASELINES") else {
        panic!(
            "set RESUME_BASELINES to the directory holding the real resumes, for example: \
             RESUME_BASELINES=<directory> cargo test --test guide -- --ignored"
        );
    };
    let details = checks::private_details(std::path::Path::new(&dir));
    assert!(
        details.len() >= 4,
        "the baselines gave only {} details: is the directory right?",
        details.len()
    );
    let text = guide_text().to_lowercase();
    let digits: String = text.chars().filter(char::is_ascii_digit).collect();
    let leaked = details
        .iter()
        .filter(|d| {
            let d = d.to_lowercase();
            text.contains(&d) || (d.chars().all(|c| c.is_ascii_digit()) && digits.contains(&d))
        })
        .count();
    // The detail itself is not printed: it is private, and a log can be shared.
    assert_eq!(
        leaked, 0,
        "{leaked} private detail(s) from the baselines are in docs/User-Guide.md"
    );
}

/// A directory with one real-looking baseline in a subdirectory.
fn baseline_dir(text: &str) -> tempfile::TempDir {
    let dir = assert_ok!(tempfile::tempdir());
    let nested = dir.path().join("drafts");
    assert_ok!(std::fs::create_dir_all(&nested));
    assert_ok!(std::fs::write(nested.join("r.md"), text));
    dir
}

const BASELINE: &str = "# Jordan Quill\n\nHarbor City, Qland · jq@private.test · +1 (415) 555-0123 · [site](https://jq.private.test/me) · [code](https://github.com/jquill)\n\n## Experience\n\n### Hidden Works · Lead · 2020 – 2022\n\n**Secret Project · 2021 – 2022**\n\n- Did a thing.\n\n### Quiet Labs | Engineer | 2018 – 2020\n";

#[test]
fn the_private_details_reader_finds_what_a_real_resume_holds() {
    let dir = baseline_dir(BASELINE);
    let details = checks::private_details(dir.path());
    for want in [
        "Jordan Quill",
        "Hidden Works",
        "Quiet Labs",
        "Secret Project",
        "jq@private.test",
        "jq.private.test",
        "jq.private.test/me",
        "github.com/jquill",
        "Harbor City, Qland",
        "14155550123",
    ] {
        assert!(details.contains(want), "{want} in {details:?}");
    }
}

#[test]
fn a_public_host_or_a_synthetic_detail_is_not_a_private_detail() {
    let dir = baseline_dir(BASELINE);
    let details = checks::private_details(dir.path());
    assert!(!details.contains("github.com"), "{details:?}");
    let synthetic = baseline_dir(
        "# Ada Lovelace\n\nLondon, UK · ada@example.com\n\n## Experience\n\n### Acme · Engineer · 2015 – 2017\n",
    );
    let details = checks::private_details(synthetic.path());
    assert!(
        !details.contains("Acme") && !details.contains("London, UK"),
        "{details:?}"
    );
}

// ---- the example style compiles ----------------------------------------------------------------------

#[test]
fn the_style_the_guide_shows_compiles_with_every_theme_that_fits_the_components() {
    use resume::model::StyleName;
    use resume::model::options::Paper;
    use resume::style::StyleSource;
    use resume::typeset::{RenderOptions, Typesetter};

    let guide = load();
    let fence = assert_some!(
        guide
            .fences
            .iter()
            .find(|f| f.info.split_whitespace().collect::<Vec<_>>() == ["typst", "style"])
    );
    let fitting = checks::styles_whose_theme_fits_the_components();
    assert!(
        fitting.len() >= 2 && fitting.contains("pragmatic"),
        "the themes that fit the components: {fitting:?}"
    );
    let heading = assert_some!(guide.heading(3, "Writing a style"));
    for style in &fitting {
        assert!(
            guide.body(heading).contains(&format!("`{style}`")),
            "`### Writing a style` must name `{style}`, whose theme the example can use"
        );
        let dir = assert_ok!(tempfile::tempdir());
        for (from, to) in [
            (
                "styles/lib/components.typ".to_string(),
                "lib/components.typ".to_string(),
            ),
            (format!("styles/{style}/theme.typ"), "mine/theme.typ".to_string()),
        ] {
            let target = dir.path().join(to);
            assert_ok!(std::fs::create_dir_all(assert_some!(target.parent())));
            assert_ok!(std::fs::write(target, facts::read(from)));
        }
        assert_ok!(std::fs::write(dir.path().join("mine/style.typ"), &fence.body));
        let style_source = assert_ok!(StyleSource::directory(
            dir.path(),
            assert_ok!(StyleName::try_new("mine"))
        ));
        let compiled = Typesetter::new(common::bundled_fonts())
            .compile(&common::full_resume(), &style_source, RenderOptions::new(Paper::Letter))
            .unwrap_or_else(|e| {
                panic!(
                    "the style in the guide (line {}) does not compile with the `{style}` theme: {e}",
                    fence.line
                )
            });
        assert!(compiled.page_count() >= 1);
    }
}

// ---- the Annotations section says what selection does ----------------------------------------------------

/// The examples under `## Annotations` that are whole sections of a resume, placed after the shared preamble.
fn annotation_example(guide: &Guide, containing: &str) -> resume::model::Resume {
    let heading = assert_some!(guide.heading(2, "Annotations"), "the guide has no Annotations section");
    let fence = assert_some!(
        guide
            .fences_in(Guide::range(heading))
            .find(|f| f.body.contains(containing)),
        "no Annotations example contains {containing:?}"
    );
    let text = format!("{}{}", checks::SECTION_PREAMBLE, fence.body);
    let parsed = resume::markdown::parse("guide.md", &text);
    assert_some!(parsed.valid(), "{text}").clone()
}

fn leaves(resume: &resume::model::Resume) -> Vec<String> {
    resume
        .nodes()
        .iter()
        .filter_map(|n| n.content.map(resume::model::plain_text))
        .collect()
}

#[test]
fn the_guides_wording_example_renders_default_or_the_preferred_wording() {
    use resume::select::{Selection, select};
    let guide = load();
    let resume = annotation_example(&guide, "=gateway.default");
    let plain = assert_ok!(select(&resume, &Selection::default())).0;
    assert_eq!(leaves(&plain), ["Led delivery of the LLM gateway for 20 teams."]);
    let short = Selection {
        prefer: vec![assert_ok!(resume::model::VariantName::try_new("short"))],
        ..Selection::default()
    };
    assert_eq!(
        leaves(&assert_ok!(select(&resume, &short)).0),
        ["Built the LLM gateway 20 teams use."]
    );
    // The first preferred wording the group has wins, whatever the order of the wordings in the file.
    let ordered = |names: &[&str]| Selection {
        prefer: names
            .iter()
            .map(|n| assert_ok!(resume::model::VariantName::try_new(*n)))
            .collect(),
        ..Selection::default()
    };
    let business = leaves(&assert_ok!(select(&resume, &ordered(&["business", "short"]))).0);
    assert_eq!(business, ["Gave 20 teams control of their AI spend."]);
    let missing_first = leaves(&assert_ok!(select(&resume, &ordered(&["tiny", "short"]))).0);
    assert_eq!(missing_first, ["Built the LLM gateway 20 teams use."]);
}

#[test]
fn the_guides_placement_example_keeps_what_its_keep_table_says() {
    use resume::select::{Scope, SelectedTags, Selection, select};
    let guide = load();
    let resume = annotation_example(&guide, "<!-- @extra #data -->");
    let tags = |names: &[&str]| Selection {
        scope: Scope::Tags(assert_some!(SelectedTags::new(
            names
                .iter()
                .map(|n| assert_ok!(resume::model::Tag::try_new(*n)))
                .collect()
        ))),
        ..Selection::default()
    };
    // No option keeps everything; `@core` survives any tags; the `@extra #data` bullet needs `--tags data`.
    let all = leaves(&assert_ok!(select(&resume, &Selection::default())).0);
    assert_eq!(
        all,
        [
            "Cut build time from 40 minutes to 9.",
            "Moved 3 data pipelines to a new scheduler."
        ]
    );
    let ai = leaves(&assert_ok!(select(&resume, &tags(&["ai"]))).0);
    assert_eq!(ai, ["Cut build time from 40 minutes to 9."]);
    let data = leaves(&assert_ok!(select(&resume, &tags(&["data"]))).0);
    assert_eq!(
        data,
        [
            "Cut build time from 40 minutes to 9.",
            "Moved 3 data pipelines to a new scheduler."
        ]
    );
    let core = Selection {
        scope: Scope::CoreOnly,
        ..Selection::default()
    };
    assert_eq!(
        leaves(&assert_ok!(select(&resume, &core)).0),
        ["Cut build time from 40 minutes to 9."]
    );
}

// ---- keeping the generated block current ---------------------------------------------------------------

/// Rewrites the diagnostic code tables in the guide from the code.
///
/// Run with `cargo test --test guide -- --ignored bless_guide_tables` after a code is added, removed, or changed.
#[test]
#[ignore = "rewrites docs/User-Guide.md; run it by name after a code changes"]
fn bless_guide_tables() {
    let text = guide_text();
    let (start, end) = checks::generated_block(&text).unwrap_or_else(|e| panic!("{e}: add both markers first"));
    let updated = format!("{}\n\n{}{}", &text[..start], checks::render_code_tables(), &text[end..]);
    if updated != text {
        assert_ok!(std::fs::write(guide_path(), updated));
    }
}
