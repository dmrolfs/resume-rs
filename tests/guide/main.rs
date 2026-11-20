//! ABOUTME: Tests that bind docs/User-Guide.md to the code, so the guide cannot drift from the tool.
//! ABOUTME: Each check compares structure (sets of names, keys, codes, options), never wording; each has a negative control.

mod checks;
#[path = "../common/mod.rs"]
mod common;
mod extract;
mod facts;
mod theme_keys;

use std::path::PathBuf;

use extract::Guide;

/// The repository root.
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn guide_path() -> PathBuf {
    root().join("docs/User-Guide.md")
}

fn load() -> Guide {
    let text = std::fs::read_to_string(guide_path()).unwrap_or_else(|e| panic!("docs/User-Guide.md is missing: {e}"));
    Guide::parse(&text)
}

#[test]
fn the_guide_exists_and_starts_with_the_two_aboutme_lines() {
    let guide = load();
    let first_two: Vec<&str> = guide.text.lines().take(2).collect();
    assert!(
        first_two
            .iter()
            .all(|l| l.starts_with("<!-- ABOUTME:") && l.ends_with("-->")),
        "the first two lines are `<!-- ABOUTME: ... -->`, as in every file of the project: {first_two:?}"
    );
}

#[test]
fn the_facts_the_checks_read_are_what_the_binary_has() {
    let commands = facts::commands();
    let names: Vec<&str> = commands.keys().map(String::as_str).collect();
    assert_eq!(
        names,
        ["build", "check", "doctor", "export-style", "licenses", "styles"]
    );
    let build: Vec<String> = commands["build"].iter().map(facts::CliOption::key).collect();
    assert!(
        build.contains(&"--paper".to_string()) && build.contains(&"<FILE>".to_string()),
        "{build:?}"
    );
    let paper = commands["build"].iter().find(|o| o.name == "paper").expect("--paper");
    assert_eq!(paper.values, ["letter", "a4"]);
    assert_eq!(paper.default.as_deref(), Some("letter"));
    let font_path = commands["build"]
        .iter()
        .find(|o| o.name == "font-path")
        .expect("--font-path");
    assert!(font_path.repeatable && font_path.takes_value);
    let globals: Vec<String> = facts::global_options().iter().map(facts::CliOption::key).collect();
    assert_eq!(globals, ["--dry-run", "--format", "--color"]);
    let force = commands["export-style"]
        .iter()
        .find(|o| o.name == "force")
        .expect("--force");
    assert!(
        !force.takes_value && force.values.is_empty(),
        "a switch has no values: {force:?}"
    );
}

#[test]
fn the_facts_know_the_codes_the_styles_the_components_and_the_font_families() {
    assert!(facts::codes().len() >= 40, "the codes");
    assert!(facts::built_in_styles().contains(&"pragmatic".to_string()));
    assert!(facts::components().contains("render-sections") && !facts::components().contains("_has-link"));
    assert!(facts::bundled_families().contains("carlito"));
    assert!(facts::font_roles().contains("body"));
    assert!(facts::theme_keys_by_style()["pragmatic"].contains("spacing.section"));
}

// ---- helpers for the checks and their negative controls ----------------------------------------------

/// The guide with the first `from` replaced by `to`; fails when `from` is not there, so a control cannot pass
/// by changing nothing.
fn mutated(from: &str, to: &str) -> Guide {
    let guide = load();
    assert!(guide.text.contains(from), "the control needs `{from}` in the guide");
    Guide::parse(&guide.text.replacen(from, to, 1))
}

/// The guide with the first line that starts with `prefix` changed by `edit`.
fn with_line(prefix: &str, edit: impl Fn(&str) -> String) -> Guide {
    let guide = load();
    let mut done = false;
    let text: Vec<String> = guide
        .text
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
    assert!(done, "the control needs a line that starts with `{prefix}`");
    Guide::parse(&(text.join("\n") + "\n"))
}

/// The guide without the first line that starts with `prefix`.
fn without_line(prefix: &str) -> Guide {
    with_line(prefix, |_| "<!-- removed -->".to_string())
}

fn appended(extra: &str) -> Guide {
    Guide::parse(&format!("{}\n{extra}\n", load().text))
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

// ---- headings and planned sections -------------------------------------------------------------------

#[test]
fn the_guide_has_every_required_heading_in_order_and_none_is_empty() {
    assert_clean(checks::required_headings(&load()));
}

#[test]
fn a_missing_a_repeated_or_an_empty_heading_is_caught() {
    assert_problem(
        &checks::required_headings(&mutated("\n## Fonts\n", "\n## Typefaces\n")),
        "no `## Fonts` heading",
    );
    assert_problem(
        &checks::required_headings(&appended("## Fonts\n\nagain")),
        "more than once",
    );
    let emptied = {
        let g = load();
        let h = g.heading(2, "Fonts").expect("Fonts");
        Guide::parse(&format!("{}## Fonts\n\n{}", &g.text[..h.start], &g.text[h.end..]))
    };
    assert_problem(&checks::required_headings(&emptied), "`Fonts`");
}

#[test]
fn the_planned_sections_are_stubs_that_claim_nothing_and_name_a_witness() {
    assert_clean(checks::planned_sections(&load()));
}

#[test]
fn a_planned_section_fails_when_its_feature_has_arrived_or_it_claims_something() {
    let arrived = mutated("witness=build:--tags", "witness=build:--paper");
    assert_problem(&checks::planned_sections(&arrived), "now exists");
    let command = mutated("witness=command:watch", "witness=command:build");
    assert_problem(&checks::planned_sections(&command), "now exists");
    assert_problem(
        &checks::planned_sections(&mutated("witness=build:--tags", "")),
        "witness=KIND:VALUE",
    );
    assert_problem(
        &checks::planned_sections(&mutated("witness=build:--tags", "witness=nonsense")),
        "is not `command:NAME`",
    );
    let with_example = mutated(
        "## Annotations\n",
        "## Annotations\n\n```markdown unchecked\n# x\n```\n",
    );
    assert_problem(&checks::planned_sections(&with_example), "fenced block");
    let with_option = mutated("## Annotations\n", "## Annotations\n\nUse `--tags` to pick.\n");
    assert_problem(&checks::planned_sections(&with_option), "names an option");
}

// ---- Markdown and shell examples ---------------------------------------------------------------------

#[test]
fn every_markdown_example_produces_exactly_the_findings_the_guide_says() {
    assert_clean(checks::markdown_examples(&load()));
}

#[test]
fn a_wrong_expectation_a_missing_mode_or_a_missing_link_is_caught() {
    let wrong = mutated("markdown resume expect=none", "markdown resume expect=weak-opener");
    assert_problem(&checks::markdown_examples(&wrong), "documented as producing");
    let no_mode = mutated("markdown resume expect=none", "markdown expect=none");
    assert_problem(&checks::markdown_examples(&no_mode), "needs exactly one of");
    let no_expect = mutated("markdown resume expect=none", "markdown resume");
    assert_problem(&checks::markdown_examples(&no_expect), "needs `expect=");
    let two = appended("```markdown unchecked\nx\n```\n\n```markdown unchecked\ny\n```");
    assert_problem(&checks::markdown_examples(&two), "at most one");
}

#[test]
fn an_example_whose_findings_are_not_linked_in_the_paragraph_before_it_is_caught() {
    let guide = load();
    let linked = guide
        .fences
        .iter()
        .find(|f| !f.links_before.is_empty() && f.info.contains("expect=") && !f.info.contains("expect=none"))
        .expect("an example that expects a finding, with its links");
    let url = linked
        .links_before
        .iter()
        .find(|l| l.starts_with("#code-"))
        .expect("a code link")
        .clone();
    let broken = Guide::parse(&guide.text.replacen(&format!("]({url})"), "](#fonts)", 1));
    assert_problem(&checks::markdown_examples(&broken), "must link");
}

#[test]
fn every_shell_example_is_accepted_by_the_program() {
    assert_clean(checks::shell_examples(&load()));
}

#[test]
fn a_shell_example_with_a_renamed_option_is_caught() {
    assert_problem(
        &checks::shell_examples(&mutated("--paper a4", "--papre a4")),
        "is not accepted",
    );
    assert_problem(
        &checks::shell_examples(&appended("```sh\nls -l\n```")),
        "must start with `resume `",
    );
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
fn a_removed_a_misspelled_or_a_mis_attributed_theme_key_is_caught() {
    assert_problem(
        &checks::theme_keys(&without_line("| `bullet.indent` |")),
        "`bullet.indent`",
    );
    let typo = with_line("| `spacing.section` |", |l| {
        l.replacen("spacing.section", "spacing.sectoin", 1)
    });
    let problems = checks::theme_keys(&typo);
    assert_problem(&problems, "documents theme key `spacing.sectoin`");
    assert_problem(&problems, "`spacing.section`");
    let only_mono = with_line("| `header.gap` |", |l| l.replace("mono", "pragmatic"));
    assert_problem(&checks::theme_keys(&only_mono), "Styles column");
}

#[test]
fn every_shared_component_has_a_row() {
    assert_clean(checks::components(&load()));
}

#[test]
fn a_missing_or_an_invented_component_is_caught() {
    assert_problem(
        &checks::components(&without_line("| `render-sections` |")),
        "`render-sections`",
    );
    let invented = with_line("| `render-sections` |", |l| {
        l.replacen("render-sections", "render-everything", 1)
    });
    assert_problem(&checks::components(&invented), "does not define");
}

#[test]
fn every_built_in_style_has_a_section_and_no_other_does() {
    assert_clean(checks::styles(&load()));
}

#[test]
fn a_renamed_style_section_is_caught() {
    let problems = checks::styles(&mutated("#### mono", "#### monoo"));
    assert_problem(&problems, "`#### mono`");
    assert_problem(&problems, "`monoo`");
}

// ---- commands and options ----------------------------------------------------------------------------

#[test]
fn every_command_and_option_is_documented_with_its_short_form_values_and_default() {
    assert_clean(checks::commands(&load()));
}

#[test]
fn a_missing_command_option_default_or_value_is_caught() {
    assert_problem(
        &checks::commands(&mutated("### resume licenses", "### resume license")),
        "resume licenses",
    );
    assert_problem(&checks::commands(&without_line("| `--font-path")), "--font-path");
    let default = with_line("| `--paper", |l| l.replace("`letter`", "`legal`"));
    assert_problem(&checks::commands(&default), "default `letter`");
    let value = with_line("| `--paper", |l| l.replace("`a4`", "`a5`"));
    assert_problem(&checks::commands(&value), "value `a4`");
    let short = with_line("| `-o, --out", |l| l.replace("-o, ", ""));
    assert_problem(&checks::commands(&short), "short form `-o`");
    let repeat = with_line("| `--font-path", |l| l.replace("repeat", "again"));
    assert_problem(&checks::commands(&repeat), "repeatable");
}

#[test]
fn documenting_an_option_that_does_not_exist_is_caught() {
    let guide = with_line("| `--paper", |l| {
        format!("{l}\n| `--debounce <DURATION>` | `150ms` | wait before rebuilding |")
    });
    assert_problem(&checks::commands(&guide), "--debounce");
    assert_problem(
        &checks::commands(&mutated(
            "### Planned commands\n",
            "### Planned commands\n\nUse `--open` to see it.\n",
        )),
        "--open",
    );
}

// ---- diagnostics: the generated appendix and the links ---------------------------------------------------

#[test]
fn the_code_tables_are_what_the_code_generates() {
    assert_clean(checks::code_tables(&load()));
}

#[test]
fn a_changed_severity_a_removed_row_or_a_missing_marker_is_caught() {
    assert_problem(&checks::code_tables(&mutated("| warning |", "| note |")), "out of date");
    assert_problem(
        &checks::code_tables(&without_line("| <a id=\"code-role-separator\">")),
        "out of date",
    );
    assert_problem(
        &checks::code_tables(&mutated("<!-- guide:generated codes -->", "")),
        "marker",
    );
}

#[test]
fn every_link_in_the_guide_goes_somewhere() {
    assert_clean(checks::links(&load()));
}

#[test]
fn a_link_to_no_code_no_heading_or_no_file_is_caught() {
    assert_problem(
        &checks::links(&appended("[x](#code-no-such-code)")),
        "names no diagnostic code",
    );
    assert_problem(
        &checks::links(&appended("[x](#no-such-heading)")),
        "points at no heading",
    );
    assert_problem(&checks::links(&appended("[x](../nowhere/at-all.md)")), "does not exist");
}

#[test]
fn the_fonts_section_covers_the_roles_fields_families_codes_and_variables() {
    assert_clean(checks::fonts(&load()));
}

#[test]
fn a_fonts_section_that_loses_a_role_a_family_or_a_code_link_is_caught() {
    let g = load();
    let fonts = g.heading(2, "Fonts").expect("Fonts");
    let without = |from: &str, to: &str| {
        let body = g.body(fonts).replace(from, to);
        Guide::parse(&format!("{}{}{}", &g.text[..fonts.start], body, &g.text[fonts.end..]))
    };
    assert_problem(&checks::fonts(&without("`mono`", "mono")), "role `mono`");
    assert_problem(&checks::fonts(&without("Carlito", "the free font")), "carlito");
    assert_problem(
        &checks::fonts(&without("(#code-font-missing)", "(#fonts)")),
        "font-missing",
    );
    assert_problem(
        &checks::fonts(&without("RESUME_FONT_PATH", "the variable")),
        "RESUME_FONT_PATH",
    );
}

// ---- no personal content -----------------------------------------------------------------------------

#[test]
fn the_guide_holds_only_synthetic_names_companies_addresses_and_numbers() {
    assert_clean(checks::no_personal_content(&load()));
}

#[test]
fn an_email_a_link_a_phone_number_a_name_or_a_company_that_is_not_synthetic_is_caught() {
    assert_problem(
        &checks::no_personal_content(&appended("Write to jane.doe@gmail.com.")),
        "jane.doe@gmail.com",
    );
    assert_problem(
        &checks::no_personal_content(&appended("[me](https://www.linkedin.com/in/jane)")),
        "linkedin",
    );
    assert_problem(
        &checks::no_personal_content(&appended("Call +1 415 555 0123.")),
        "phone-like",
    );
    let name = mutated("# Ada Lovelace", "# Jane Doe");
    assert_problem(&checks::no_personal_content(&name), "Jane Doe");
    let company = appended(
        "```markdown resume expect=none\n# Ada Lovelace\n\nLondon\n\n## Experience\n\n### Secretco · 2020 – 2022\n```",
    );
    assert_problem(&checks::no_personal_content(&company), "Secretco");
}

/// Reads the owner's real baselines at run time and fails if anything private from them is in the guide.
/// No private string is committed: the details are read from the files named by `RESUME_BASELINES`.
///
/// Run with `RESUME_BASELINES=~/Documents/vrm/my/job/drafts cargo test --test guide -- --ignored`.
#[test]
#[ignore = "reads the real resumes from RESUME_BASELINES"]
fn the_guide_contains_nothing_from_the_real_baselines() {
    let Some(dir) = std::env::var_os("RESUME_BASELINES") else {
        panic!(
            "set RESUME_BASELINES to the directory holding the real resumes, for example: \
             RESUME_BASELINES=~/Documents/vrm/my/job/drafts cargo test --test guide -- --ignored"
        );
    };
    let details = checks::private_details(std::path::Path::new(&dir));
    assert!(
        details.len() >= 4,
        "the baselines gave only {} details: is the directory right?",
        details.len()
    );
    let guide = load().text.to_lowercase();
    let leaked: Vec<&String> = details.iter().filter(|d| guide.contains(&d.to_lowercase())).collect();
    // The detail itself is not printed: it is private, and a log can be shared.
    assert!(
        leaked.is_empty(),
        "{} private detail(s) from the baselines are in docs/User-Guide.md",
        leaked.len()
    );
}

#[test]
fn the_private_details_reader_finds_names_companies_addresses_and_hosts() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(
        dir.path().join("r.md"),
        "# Jordan Quill\n\nHere · jq@private.test · [site](https://jq.private.test/me)\n\n## Experience\n\n### Hidden Works · Lead · 2020 – 2022\n",
    )
    .expect("a file is written");
    let details = checks::private_details(dir.path());
    for want in ["Jordan Quill", "Hidden Works", "jq@private.test", "jq.private.test"] {
        assert!(details.contains(want), "{want} in {details:?}");
    }
}

#[test]
fn a_public_host_that_a_resume_links_to_is_not_a_private_detail() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(
        dir.path().join("r.md"),
        "# Jordan Quill\n\nHere · [code](https://github.com/jq) · [site](https://jq.private.test)\n\n## Experience\n\n### Hidden Works · 2020 – 2022\n",
    )
    .expect("a file is written");
    let details = checks::private_details(dir.path());
    assert!(!details.contains("github.com"), "{details:?}");
    assert!(
        details.contains("jq.private.test"),
        "a private host still counts: {details:?}"
    );
}

// ---- the example style compiles ----------------------------------------------------------------------

#[test]
fn the_style_the_guide_shows_compiles_and_renders_a_page() {
    use resume::model::StyleName;
    use resume::model::options::Paper;
    use resume::style::StyleSource;
    use resume::typeset::{RenderOptions, Typesetter};

    let guide = load();
    let fence = guide
        .fences
        .iter()
        .find(|f| f.info.split_whitespace().collect::<Vec<_>>() == ["typst", "style"])
        .expect("a ```typst style fence under `### Writing a style`");
    let dir = tempfile::tempdir().expect("a temporary directory");
    for (from, to) in [
        ("styles/lib/components.typ", "lib/components.typ"),
        ("styles/pragmatic/theme.typ", "mine/theme.typ"),
    ] {
        let target = dir.path().join(to);
        std::fs::create_dir_all(target.parent().expect("a parent")).expect("a directory");
        std::fs::write(target, facts::read(from)).expect("a file is written");
    }
    std::fs::write(dir.path().join("mine/style.typ"), &fence.body).expect("the style is written");
    let style = StyleSource::directory(dir.path(), StyleName::try_new("mine").expect("a style name")).expect("a style");
    let compiled = Typesetter::new(common::bundled_fonts())
        .compile(&common::full_resume(), &style, RenderOptions::new(Paper::Letter))
        .unwrap_or_else(|e| panic!("the style in the guide (line {}) does not compile: {e}", fence.line));
    assert!(compiled.page_count() >= 1);
}

// ---- keeping the generated block current ---------------------------------------------------------------

/// Rewrites the diagnostic code tables in the guide from the code.
///
/// Run with `cargo test --test guide -- --ignored bless_guide_tables` after a code is added, removed, or changed.
#[test]
#[ignore = "rewrites docs/User-Guide.md; run it by name after a code changes"]
fn bless_guide_tables() {
    let text = std::fs::read_to_string(guide_path()).expect("the guide");
    let (start, end) = checks::generated_block(&text).unwrap_or_else(|e| panic!("{e}: add both markers first"));
    let updated = format!("{}\n\n{}{}", &text[..start], checks::render_code_tables(), &text[end..]);
    if updated != text {
        std::fs::write(guide_path(), updated).expect("the guide is written");
    }
}
