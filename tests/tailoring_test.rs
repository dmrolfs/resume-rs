//! ABOUTME: In-process tests of tailoring from the command line: the selection flags, the build summary and its
//! ABOUTME: JSON record, warnings for names a file does not use, the core-only page count, and `resume tags`.

use std::fs;

use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;
use resume::commands::Outcome;
use serde_json::Value;

mod common;

#[path = "common/workspace.rs"]
mod workspace;
use workspace::{Workspace, fixture};

/// A baseline with two audiences: `#arch` and `#lead` items, one alternate group, core, verify, and a draft.
const BASELINE: &str = "# Ada Lovelace\n\nLondon · ada@example.com\n\n\
## Summary\n\nBuilds platforms and the teams that run them. <!-- @core -->\n\n\
## Skills <!-- #arch -->\n\n- Rust and Go for distributed systems.\n\n\
## Experience\n\n### Acme · Principal Engineer · 2015 – 2023\n\n\
- Designed the gateway for 20 teams. <!-- =gateway.default #arch @core -->\n\
- Led the gateway team of 6 engineers. <!-- =gateway.leader #lead @core -->\n\
- Cut costs by 40 percent across 3 regions.\n\
- Grew the team from 4 to 12 engineers. <!-- #lead -->\n\
- Wrote the storage engine in 9 months. <!-- #arch -->\n\
- Check the number: 35 percent. <!-- @verify -->\n\
- An unfinished claim about 10 things. <!-- @draft -->\n";

/// The text of every page of the PDF the build wrote for `Resume.md`.
fn pdf_text(work: &Workspace) -> String {
    let pdf = assert_ok!(fs::read(work.out().join("Resume.pdf")));
    common::page_texts(&pdf).join("\n")
}

fn build(work: &Workspace, extra: &[&str]) -> workspace::Run {
    let file = work.file("Resume.md", BASELINE);
    let run = work.build(&[&file], extra);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    run
}

/// The one `selection` record a JSON build printed, if any.
fn selection_record(stdout: &str) -> Option<Value> {
    let mut records = stdout
        .lines()
        .map(|line| assert_ok!(serde_json::from_str::<Value>(line)))
        .filter(|record| record["type"] == "selection");
    let first = records.next();
    assert!(records.next().is_none(), "one selection record per file");
    first
}

/// The `  selection:` line of a build's stderr; a build that should have one fails here when it does not.
fn selection_line(stderr: &str) -> &str {
    assert_some!(stderr.lines().find(|l| l.starts_with("  selection:")), "{stderr}")
}

/// The selection block: from the `  selection:` line to the last indented line after it.
fn selection_block(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .skip_while(|l| !l.starts_with("  selection:"))
        .take_while(|l| l.starts_with("  "))
        .collect()
}

// ============================================================================
// Each flag produces the selected resume
// ============================================================================

#[test]
fn tags_keep_the_tagged_core_and_untagged_items_and_drop_the_rest() {
    let work = Workspace::new();
    build(&work, &["--tags", "lead"]);
    let text = pdf_text(&work);
    for kept in ["Builds platforms", "Grew the team", "Cut costs", "Designed the gateway"] {
        assert!(text.contains(kept), "{kept} missing: {text}");
    }
    for dropped in ["Rust and Go", "storage engine", "unfinished claim"] {
        assert!(!text.contains(dropped), "{dropped} kept: {text}");
    }
}

#[test]
fn prefer_renders_the_preferred_wording_of_a_group() {
    let work = Workspace::new();
    build(&work, &["--prefer", "leader"]);
    let text = pdf_text(&work);
    assert!(text.contains("Led the gateway team"), "{text}");
    assert!(!text.contains("Designed the gateway"), "{text}");
}

#[test]
fn core_only_keeps_only_core_items() {
    let work = Workspace::new();
    build(&work, &["--core-only"]);
    let text = pdf_text(&work);
    assert!(
        text.contains("Builds platforms") && text.contains("Designed the gateway"),
        "{text}"
    );
    for dropped in ["Cut costs", "Grew the team", "storage engine", "Rust and Go"] {
        assert!(!text.contains(dropped), "{dropped} kept: {text}");
    }
}

#[test]
fn drafts_puts_the_drafts_back() {
    let work = Workspace::new();
    build(&work, &[]);
    assert!(!pdf_text(&work).contains("unfinished claim"));
    build(&work, &["--drafts"]);
    assert!(pdf_text(&work).contains("unfinished claim"));
}

#[test]
fn tags_given_twice_or_with_commas_are_the_same_selection() {
    let work = Workspace::new();
    let commas = build(&work, &["--tags", "lead,arch"]);
    let first = pdf_text(&work);
    let repeated = build(&work, &["--tags", "lead", "--tags", "arch"]);
    assert_eq!(pdf_text(&work), first);
    assert_eq!(selection_line(&commas.stderr), "  selection: tags arch, lead");
    assert_eq!(selection_line(&repeated.stderr), "  selection: tags arch, lead");
}

// ============================================================================
// The summary, human and JSON
// ============================================================================

#[test]
fn a_tailored_build_prints_the_counts_the_wordings_the_surprises_and_the_core_pages() {
    let work = Workspace::new();
    let run = build(&work, &["--tags", "lead", "--prefer", "leader"]);
    let lines = selection_block(&run.stderr);
    assert_eq!(
        lines,
        vec![
            "  selection: tags lead; prefer leader",
            "  kept: 2 sections, 1 role, 1 paragraph, 4 bullets; dropped: 1 section, 4 bullets (of which 1 other wording)",
            "  wordings: 1 chosen (1 preference)",
            "  dropped by an inherited tag: bullet at line 11 \"Rust and Go for distributed systems.\"",
            "  dropped as emptied: section at line 9 \"Skills\"",
            "  to verify: bullet at line 22 \"Check the number: 35 percent.\"",
            "  drafts left out: 1",
            "  core alone: 1 page",
        ],
        "{}",
        run.stderr
    );
}

#[test]
fn the_json_record_carries_what_the_human_lines_say() {
    let work = Workspace::new();
    let file = work.file("Resume.md", BASELINE);
    let human = work.build(&[&file], &["--tags", "lead", "--prefer", "leader"]);
    let mut args = vec!["--format".to_string(), "json".to_string()];
    args.extend(work.build_args(&[&file], &["--tags", "lead", "--prefer", "leader"]));
    let json = work.run(&args);
    assert_eq!(json.outcome, Outcome::Success, "{}", json.stderr);
    let record = assert_some!(selection_record(&json.stdout));
    assert_eq!(record["file"], "Resume.md");
    assert_eq!(record["scope"], "tags");
    assert_eq!(record["tags"], serde_json::json!(["lead"]));
    assert_eq!(record["prefer"], serde_json::json!(["leader"]));
    assert_eq!(record["drafts"], false);
    assert_eq!(
        record["counts"]["bullet"],
        serde_json::json!({"baseline": 8, "kept": 4})
    );
    assert_eq!(record["chosen"][0]["variant"], "leader");
    assert_eq!(record["chosen"][0]["by"], "preference");
    let inherited: Vec<&Value> = assert_some!(record["dropped"].as_array())
        .iter()
        .filter(|d| d["reason"] == "inherited-tags-only")
        .collect();
    assert_eq!(inherited.len(), 1);
    assert_eq!(inherited[0]["line"], 11);
    assert_eq!(record["verify"][0]["line"], 22);
    assert_eq!(record["drafts_left_out"], 1);
    assert_eq!(record["core"], serde_json::json!({"status": "measured", "pages": 1}));
    // The human lines say the same.
    assert!(human.stderr.contains("bullet at line 11") && human.stderr.contains("core alone: 1 page"));
}

#[test]
fn an_unannotated_resume_built_without_flags_prints_nothing_new() {
    let work = Workspace::new();
    let file = work.file("Resume.md", &fixture("full.md"));
    let run = work.build(&[&file], &[]);
    assert_eq!(run.outcome, Outcome::Success);
    assert!(!run.stderr.contains("selection:"), "{}", run.stderr);
    let mut args = vec!["--format".to_string(), "json".to_string()];
    args.extend(work.build_args(&[&file], &[]));
    assert!(selection_record(&work.run(&args).stdout).is_none());
}

#[test]
fn drafts_says_it_is_a_preview_in_both_outputs() {
    let work = Workspace::new();
    let run = build(&work, &["--drafts"]);
    assert!(
        run.stderr.contains("selection: everything; with drafts"),
        "{}",
        run.stderr
    );
    assert!(run.stderr.contains("drafts: 1 included (preview)"), "{}", run.stderr);
    let file = work.file("Resume.md", BASELINE);
    let mut args = vec!["--format".to_string(), "json".to_string()];
    args.extend(work.build_args(&[&file], &["--drafts"]));
    let record = assert_some!(selection_record(&work.run(&args).stdout));
    assert_eq!(
        (record["drafts"].clone(), record["drafts_included"].clone()),
        (Value::Bool(true), 1.into())
    );
}

// ============================================================================
// Names the file does not use
// ============================================================================

#[test]
fn an_unknown_tag_or_wording_warns_and_lists_the_files_own() {
    let work = Workspace::new();
    let run = build(&work, &["--tags", "lead,lede", "--prefer", "shrot,leader"]);
    assert!(
        run.stderr.contains("warning[unknown-tag]: --tags `lede`"),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr.contains("the resume's tags are `arch`, `lead`"),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr.contains("warning[unknown-wording]: --prefer `shrot`"),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr.contains("the resume's wordings are `default`, `leader`"),
        "{}",
        run.stderr
    );
    assert!(!run.stderr.contains("`lead`:"), "a known tag is not reported");
}

#[test]
fn a_draft_only_tag_is_not_unknown() {
    let work = Workspace::new();
    let file = work.file(
        "Resume.md",
        &BASELINE.replace("<!-- @draft -->", "<!-- @draft #hidden -->"),
    );
    let run = work.build(&[&file], &["--tags", "hidden"]);
    assert!(!run.stderr.contains("unknown-tag"), "{}", run.stderr);
}

#[test]
fn a_bad_name_or_tags_with_core_only_is_a_usage_error() {
    for args in [
        vec!["build", "r.md", "--tags", "AI"],
        vec!["build", "r.md", "--tags", "ai", "--core-only"],
        vec!["build", "r.md", "--prefer", ""],
    ] {
        let error = claims::assert_err!(resume::cli::Cli::try_parse_validated(
            std::iter::once("resume").chain(args.iter().copied())
        ));
        assert_eq!(error.exit_code(), 2, "{args:?}");
    }
}

// ============================================================================
// The core-only page count
// ============================================================================

/// A baseline where the core alone is shorter than the whole: many long non-core bullets.
fn long_baseline() -> String {
    let filler: String = (1..=60)
        .map(|n| format!("- Delivered project {n} with a long description of the work, the people, and the measurable result of 10 percent.\n"))
        .collect();
    format!("{BASELINE}{filler}")
}

#[test]
fn the_core_page_count_equals_a_separate_core_only_build() {
    let work = Workspace::new();
    let file = work.file("Resume.md", &long_baseline());
    let full = work.build(&[&file], &["--prefer", "leader"]);
    assert_eq!(full.outcome, Outcome::Success, "{}", full.stderr);
    let full_pages = common::page_texts(&assert_ok!(fs::read(work.out().join("Resume.pdf")))).len();
    let core = work.build(&[&file], &["--core-only", "--prefer", "leader"]);
    let core_pages = common::page_texts(&assert_ok!(fs::read(work.out().join("Resume.pdf")))).len();
    assert!(
        full_pages > core_pages,
        "the fixture must make the core shorter ({full_pages} vs {core_pages})"
    );
    let said = format!(
        "core alone: {core_pages} page{}",
        if core_pages == 1 { "" } else { "s" }
    );
    assert!(full.stderr.contains(&said), "{said} not in {}", full.stderr);
    assert!(
        core.stderr.contains("core alone: not measured (already-core-only)"),
        "{}",
        core.stderr
    );
}

#[test]
fn the_core_page_count_is_skipped_when_nothing_is_core() {
    let work = Workspace::new();
    let file = work.file("Resume.md", &BASELINE.replace("@core", ""));
    let run = work.build(&[&file], &["--tags", "lead"]);
    assert!(
        run.stderr.contains("core alone: not measured (no-core)"),
        "{}",
        run.stderr
    );
}

// ============================================================================
// The selection reaches doctor, check, watch's job, and export
// ============================================================================

#[test]
fn doctor_and_check_warn_about_unknown_names_and_print_no_selection_summary() {
    let work = Workspace::new();
    let file = work.file("Resume.md", BASELINE);
    let unknown = ["--tags", "lede", "--prefer", "shrot"];
    for run in [work.doctor(&[&file], &unknown), work.check(&[&file], &unknown)] {
        assert!(
            run.stderr.contains("warning[unknown-tag]") && run.stderr.contains("`arch`, `lead`"),
            "{}",
            run.stderr
        );
        assert!(
            run.stderr.contains("warning[unknown-wording]") && run.stderr.contains("`default`, `leader`"),
            "{}",
            run.stderr
        );
        assert!(
            !run.stderr.contains("selection:") && !run.stderr.contains("core alone"),
            "{}",
            run.stderr
        );
    }
    for command in ["doctor", "check"] {
        let mut args = vec!["--format".to_string(), "json".to_string()];
        args.extend(work.assess_args(command, &[&file], &unknown));
        let run = work.run(&args);
        assert!(selection_record(&run.stdout).is_none(), "{command}");
        assert!(
            run.stdout.contains("\"unknown-tag\"") && run.stdout.contains("\"unknown-wording\""),
            "{command}"
        );
    }
}

#[test]
fn export_writes_the_selected_resume() {
    let work = Workspace::new();
    let file = work.file("Resume.md", BASELINE);
    let dir = work.path().join("preview");
    let run = work.export(&file, &dir, &["--tags", "lead", "--prefer", "leader"]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let json = assert_ok!(fs::read_to_string(dir.join("resume.json")));
    assert!(
        json.contains("Led the gateway team") && !json.contains("storage engine"),
        "{json}"
    );
}

// ============================================================================
// resume tags
// ============================================================================

#[test]
fn tags_lists_the_tags_keywords_and_groups_with_their_lines() {
    let work = Workspace::new();
    let file = work.file("Resume.md", BASELINE);
    let run = work.run(&["tags".to_string(), file.display().to_string()]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert_eq!(
        run.stdout,
        "tags\n\
         \x20 #arch  3 own (lines 9, 17, 21), and 1 more under it\n\
         \x20 #lead  2 own (lines 18, 20)\n\
         keywords\n\
         \x20 @core    3 (lines 7, 17, 18)\n\
         \x20 @extra   0\n\
         \x20 @draft   1 (lines 23)\n\
         \x20 @verify  1 (lines 22)\n\
         groups\n\
         \x20 gateway (bullets)\n\
         \x20   =gateway.default  line 17 [default]  \"Designed the gateway for 20 teams.\"\n\
         \x20   =gateway.leader   line 18  \"Led the gateway team of 6 engineers.\"\n"
    );
}

#[test]
fn tags_with_format_json_prints_one_inventory_object() {
    let work = Workspace::new();
    let file = work.file("Resume.md", BASELINE);
    let run = work.run(&["--format", "json", "tags", &file.display().to_string()].map(String::from));
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert_eq!(run.stdout.lines().count(), 1);
    let record: Value = assert_ok!(serde_json::from_str(run.stdout.trim_end()));
    assert_eq!(record["type"], "inventory");
    assert_eq!(record["file"], "Resume.md");
    assert_eq!(
        record["tags"][0],
        serde_json::json!({"name": "arch", "own": [9, 17, 21], "inherited": 1})
    );
    assert_eq!(record["keywords"].as_array().map(Vec::len), Some(4));
    let variants = &record["groups"][0]["variants"];
    assert_eq!(variants[0]["name"], "default");
    assert_eq!(variants[1]["line"], 18);
}

#[test]
fn tags_marks_a_draft_wording_and_one_under_a_draft_role() {
    let work = Workspace::new();
    let text = BASELINE
        .replace("=gateway.leader #lead @core", "=gateway.leader #lead @core @draft")
        .replace(
            "### Acme · Principal Engineer · 2015 – 2023",
            "### Acme · Principal Engineer · 2015 – 2023 <!-- @draft -->",
        );
    let file = work.file("Resume.md", &text);
    let run = work.run(&["--format", "json", "tags", &file.display().to_string()].map(String::from));
    let record: Value = assert_ok!(serde_json::from_str(run.stdout.trim_end()));
    let variants = &record["groups"][0]["variants"];
    assert_eq!(
        (variants[0]["draft"].clone(), variants[1]["draft"].clone()),
        (Value::Bool(true), Value::Bool(true))
    );
}

#[test]
fn tags_on_a_file_with_format_errors_prints_no_inventory_and_exits_1() {
    let work = Workspace::new();
    let file = work.file("Resume.md", &BASELINE.replace("@verify", "@verfy"));
    let run = work.run(&["tags".to_string(), file.display().to_string()]);
    assert_eq!(run.outcome, Outcome::Failure);
    assert_eq!(run.stdout, "");
    assert!(
        run.stderr.contains("annotation-unknown-keyword") && run.stderr.contains("no inventory"),
        "{}",
        run.stderr
    );
}

#[test]
fn tags_of_an_unannotated_resume_lists_none_and_four_zero_keywords() {
    let work = Workspace::new();
    let file = work.file("Resume.md", &fixture("full.md"));
    let run = work.run(&["tags".to_string(), file.display().to_string()]);
    assert_eq!(run.outcome, Outcome::Success);
    assert!(
        run.stdout.starts_with("tags\n  (none)\nkeywords\n  @core    0\n"),
        "{}",
        run.stdout
    );
    assert!(run.stdout.ends_with("groups\n  (none)\n"), "{}", run.stdout);
}

// ============================================================================
// Review additions: full JSON parity, core status, silence, export, core pages, failures, inventory
// ============================================================================

/// Every kind of node, a preference, an inherited-tag drop, an emptied section, a verify item, and a skipped draft
/// wording, for the record and the human lines to agree on.
const EVERY_KIND: &str = "# Ada Lovelace\n\nLondon · ada@example.com\n\n\
## Summary\n\nBuilds platforms for 20 teams. <!-- @core -->\n\n\
## Skills <!-- #arch -->\n\n- Rust and Go in 3 systems.\n\n\
## Experience\n\n### Acme · Engineer · 2015 – 2023\n\nAcme makes 5 products.\n\n\
**Platform · 2016 – 2019**\n\n\
- Led 6 people. <!-- =team.default -->\n- Led a team of 6. <!-- =team.short @draft -->\n- Led 6 engineers. <!-- =team.leader #lead -->\n\
- Check this 40 percent. <!-- @verify -->\n";

fn json_build(work: &Workspace, file: &std::path::Path, options: &[&str]) -> Value {
    let mut args = vec!["--format".to_string(), "json".to_string()];
    args.extend(work.build_args(&[file], options));
    let run = work.run(&args);
    assert_some!(selection_record(&run.stdout), "{}", run.stderr)
}

#[test]
fn the_json_record_and_the_human_lines_report_the_same_facts() {
    let work = Workspace::new();
    let file = work.file("Resume.md", EVERY_KIND);
    let options = ["--tags", "lead", "--prefer", "short,leader"];
    let human = work.build(&[&file], &options);
    assert_eq!(human.outcome, Outcome::Success, "{}", human.stderr);
    let record = json_build(&work, &file, &options);
    assert_eq!(
        record["counts"],
        serde_json::json!({
            "section": {"baseline": 3, "kept": 2},
            "role": {"baseline": 1, "kept": 1},
            "era": {"baseline": 1, "kept": 1},
            "blurb": {"baseline": 1, "kept": 1},
            "paragraph": {"baseline": 1, "kept": 1},
            "bullet": {"baseline": 5, "kept": 2},
        })
    );
    assert_eq!(
        record["chosen"],
        serde_json::json!([{"group": "team", "variant": "leader", "by": "preference", "not_chosen": ["default", "short"]}])
    );
    assert_eq!(record["skipped_drafts"], serde_json::json!(["team.short"]));
    assert_eq!(
        record["dropped"],
        serde_json::json!([
            {"kind": "bullet", "line": 11, "label": "Rust and Go in 3 systems.", "reason": "inherited-tags-only"},
            {"kind": "section", "line": 9, "label": "Skills", "reason": "empty"},
        ])
    );
    assert_eq!(
        record["verify"],
        serde_json::json!([{"kind": "bullet", "line": 24, "label": "Check this 40 percent."}])
    );
    assert_eq!(record["drafts_left_out"], 1);
    assert_eq!(record["drafts_included"], 0);
    assert_eq!(record["core"], serde_json::json!({"status": "measured", "pages": 1}));
    // The human lines state the same facts.
    assert_eq!(
        selection_block(&human.stderr),
        vec![
            "  selection: tags lead; prefer short, leader",
            "  kept: 2 sections, 1 role, 1 era, 1 blurb, 1 paragraph, 2 bullets; dropped: 1 section, 3 bullets (of which 2 other wordings)",
            "  wordings: 1 chosen (1 preference)",
            "  skipped draft wording: team.short",
            "  dropped by an inherited tag: bullet at line 11 \"Rust and Go in 3 systems.\"",
            "  dropped as emptied: section at line 9 \"Skills\"",
            "  to verify: bullet at line 24 \"Check this 40 percent.\"",
            "  drafts left out: 1",
            "  core alone: 1 page",
        ]
    );
}

#[test]
fn the_core_status_says_why_it_was_not_measured_in_json() {
    let work = Workspace::new();
    for (text, options, reason) in [
        (BASELINE.to_string(), vec!["--core-only"], "already-core-only"),
        (BASELINE.replace("@core", ""), vec!["--tags", "lead"], "no-core"),
    ] {
        let file = work.file("Resume.md", &text);
        let record = json_build(&work, &file, &options);
        assert_eq!(
            record["core"],
            serde_json::json!({"status": "skipped", "reason": reason})
        );
    }
}

#[test]
fn default_only_wordings_built_without_flags_print_nothing_new() {
    let work = Workspace::new();
    let file = work.file(
        "Resume.md",
        "# Ada Lovelace\n\nLondon\n\n## S\n\n- Led 6 people. <!-- =team.default -->\n- Led a team of 6. <!-- =team.short -->\n",
    );
    let run = work.build(&[&file], &[]);
    assert!(!run.stderr.contains("selection:"), "{}", run.stderr);
    let mut args = vec!["--format".to_string(), "json".to_string()];
    args.extend(work.build_args(&[&file], &[]));
    assert!(selection_record(&work.run(&args).stdout).is_none());
    let text = pdf_text(&work);
    assert!(
        text.contains("Led 6 people.") && !text.contains("Led a team of 6."),
        "{text}"
    );
}

#[test]
fn export_warns_about_unknown_names_from_the_whole_baseline() {
    let work = Workspace::new();
    let file = work.file(
        "Resume.md",
        &BASELINE.replace("<!-- @draft -->", "<!-- @draft #hidden -->"),
    );
    let dir = work.path().join("preview");
    let run = work.export(&file, &dir, &["--tags", "lede,hidden"]);
    assert!(
        run.stderr.contains("warning[unknown-tag]: --tags `lede`"),
        "{}",
        run.stderr
    );
    assert!(
        !run.stderr.contains("`hidden`:"),
        "a tag only a draft carries is still known: {}",
        run.stderr
    );
}

/// Core content where a draft and a long preferred wording each add pages.
fn page_sensitive_core() -> String {
    let long =
        "Delivered a long description of the work, the people, and the measurable result of 10 percent. ".repeat(4);
    let drafts: String = (1..=25)
        .map(|n| format!("- Draft {n}: {long}<!-- @core @draft -->\n"))
        .collect();
    let others: String = (1..=25).map(|n| format!("- Other {n}: {long}\n")).collect();
    format!(
        "# Ada Lovelace\n\nLondon\n\n## S\n\n- Short wording. <!-- =w.default @core -->\n- {}<!-- =w.long @core -->\n{drafts}\n## Other\n\n{others}",
        long.repeat(12)
    )
}

#[test]
fn the_core_page_count_follows_drafts_and_preferences_like_a_core_only_build() {
    let work = Workspace::new();
    let file = work.file("Resume.md", &page_sensitive_core());
    let mut seen = std::collections::BTreeSet::new();
    for options in [
        vec![],
        vec!["--drafts"],
        vec!["--prefer", "long"],
        vec!["--prefer", "long", "--drafts"],
    ] {
        let mut full = options.clone();
        full.extend(["--tags", "x"]);
        let built = work.build(&[&file], &full);
        assert_eq!(built.outcome, Outcome::Success, "{}", built.stderr);
        let mut core = options.clone();
        core.push("--core-only");
        let core_run = work.build(&[&file], &core);
        assert_eq!(core_run.outcome, Outcome::Success, "{}", core_run.stderr);
        let pages = common::page_texts(&assert_ok!(fs::read(work.out().join("Resume.pdf")))).len();
        let said = format!("core alone: {pages} page{}", if pages == 1 { "" } else { "s" });
        assert!(
            built.stderr.contains(&said),
            "{options:?}: {said} not in {}",
            built.stderr
        );
        seen.insert(pages);
    }
    assert!(
        seen.len() >= 3,
        "drafts and preferences must change the core's pages: {seen:?}"
    );
}

#[test]
fn a_core_only_compile_that_fails_fails_the_build_and_keeps_the_pdf() {
    let work = Workspace::new();
    // A style that compiles the whole resume but fails when there is only one section.
    work.file(
        "styles/fragile/style.typ",
        "#let resume = json(\"/resume.json\")\n#assert(resume.sections.len() > 1, message: \"needs two sections\")\n#for s in resume.sections [#s.title]\n",
    );
    let file = work.file(
        "Resume.md",
        "# Ada Lovelace\n\nLondon\n\n## A\n\n- Core 1. <!-- @core -->\n\n## B\n\n- Plain 2.\n",
    );
    let styles = work.path().join("styles").display().to_string();
    let run = work.build(
        &[&file],
        &["--style", "fragile", "--styles-dir", &styles, "--tags", "x"],
    );
    assert_eq!(run.outcome, Outcome::Failure, "{}", run.stderr);
    assert!(run.stderr.contains("needs two sections"), "{}", run.stderr);
    assert!(
        run.stderr
            .contains("core alone: not measured (the core-only compile failed)"),
        "{}",
        run.stderr
    );
    assert!(work.out().join("Resume.pdf").exists(), "the main PDF is still written");
}

#[test]
fn unknown_names_are_shown_even_when_the_style_fails() {
    let work = Workspace::new();
    work.file("styles/broken/style.typ", "#panic(\"broken\")\n");
    let file = work.file("Resume.md", BASELINE);
    let styles = work.path().join("styles").display().to_string();
    let run = work.build(
        &[&file],
        &["--style", "broken", "--styles-dir", &styles, "--tags", "lede"],
    );
    assert_eq!(run.outcome, Outcome::Failure);
    assert!(run.stderr.contains("warning[unknown-tag]"), "{}", run.stderr);
}

#[test]
fn tags_counts_each_inheriting_node_once_when_two_containers_share_a_tag() {
    let work = Workspace::new();
    let text = "# Ada Lovelace\n\nLondon\n\n## Experience <!-- #ai -->\n\n### Acme · Engineer · 2015 – 2017 <!-- #ai -->\n\n\
        The blurb.\n\n- One.\n- Two.\n\n## Other\n\n- Three.\n";
    let file = work.file("Resume.md", text);
    let run = work.run(&["--format", "json", "tags", &file.display().to_string()].map(String::from));
    let record: Value = assert_ok!(serde_json::from_str(run.stdout.trim_end()));
    assert_eq!(
        record["tags"],
        serde_json::json!([{"name": "ai", "own": [5, 7], "inherited": 3}])
    );
}
