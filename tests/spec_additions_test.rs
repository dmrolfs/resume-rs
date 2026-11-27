//! Real spec loading, printing and both output paths for spec-only leaf additions.
mod common;
#[path = "common/workspace.rs"]
mod workspace;
use claims::{assert_err, assert_ok};
use pretty_assertions::assert_eq;
use resume::commands::Outcome;
use std::fs;
use workspace::Workspace;

const BASE: &str =
    "# Ada\n\nLondon · ada@example.com\n\n## Summary <!-- &summary -->\n\nBaseline paragraph. <!-- &baseline -->\n";
const LETTER: &str = "# Ada\n\nLondon · ada@example.com\n\n## Greeting\n\nHello,\n\n## Body\n\nBaseline letter. <!-- &opening -->\n\n## Closing\n\nThanks,\n";
const SPEC: &str = "[build]\ncontent = 'baseline.md'\n[letter]\ncontent = 'letter.md'\n[[add]]\nid = 'new-bullet'\nkind = 'bullet'\nplace = {parent = 'summary', at = 'end'}\ncontent = 'Delivered 42 new systems.'\npriority = 'core'\n[[letter.add]]\nid = 'new-letter'\nplace = {after = 'opening'}\ncontent = 'Added letter paragraph.'\n";
fn stage(work: &Workspace, spec: &str) {
    assert_ok!(fs::write(work.path().join("baseline.md"), BASE));
    assert_ok!(fs::write(work.path().join("letter.md"), LETTER));
    assert_ok!(fs::write(work.path().join("position.toml"), spec));
}
fn run(work: &Workspace, command: &str, extra: &[&str]) -> workspace::Run {
    let mut args = vec![
        command.to_owned(),
        "--spec".into(),
        work.path().join("position.toml").display().to_string(),
    ];
    args.extend(extra.iter().map(|s| s.to_string()));
    work.run(&args)
}
#[test]
fn additions_reach_pdf_and_letter_without_changing_baselines_and_rebuild_identically() {
    let work = Workspace::new();
    stage(&work, SPEC);
    let out = work.out().display().to_string();
    let built = run(&work, "build", &["--fonts", "bundled", "--out", &out]);
    assert_eq!(built.outcome, Outcome::Success, "{}", built.stderr);
    let pdf = assert_ok!(fs::read(work.out().join("position.pdf")));
    assert!(common::page_texts(&pdf).join(" ").contains("Delivered 42 new systems."));
    let letter = run(&work, "letter", &[]);
    assert_eq!(letter.outcome, Outcome::Success, "{}", letter.stderr);
    assert!(letter.stdout.contains("Added letter paragraph."));
    assert_eq!(
        run(&work, "build", &["--fonts", "bundled", "--out", &out]).outcome,
        Outcome::Success
    );
    assert_eq!(assert_ok!(fs::read(work.out().join("position.pdf"))), pdf);
    assert_eq!(assert_ok!(fs::read_to_string(work.path().join("baseline.md"))), BASE);
    assert_eq!(assert_ok!(fs::read_to_string(work.path().join("letter.md"))), LETTER);
}
#[test]
fn print_preserves_both_addition_arrays_without_baseline_io_and_reaches_a_fixed_point() {
    let work = Workspace::new();
    stage(&work, SPEC);
    assert_ok!(fs::remove_file(work.path().join("baseline.md")));
    assert_ok!(fs::remove_file(work.path().join("letter.md")));
    let first = run(&work, "build", &["--print-spec"]);
    assert_eq!(first.outcome, Outcome::Success, "{}", first.stderr);
    assert!(first.stdout.contains("[[add]]"));
    assert!(first.stdout.contains("[[letter.add]]"));
    assert_ok!(fs::write(work.path().join("position.toml"), &first.stdout));
    let second = run(&work, "letter", &["--print-spec"]);
    assert_eq!(second.outcome, Outcome::Success, "{}", second.stderr);
    assert_eq!(second.stdout, first.stdout);
}
#[test]
fn empty_addition_arrays_are_forbidden_in_profiles() {
    for text in ["add = []", "[letter]\nadd = []"] {
        let work = Workspace::new();
        let path = work.path().join("profile.toml");
        assert_ok!(fs::write(&path, text));
        assert!(matches!(
            assert_err!(resume::spec::Layer::load(resume::spec::Kind::Profile, &path)),
            resume::spec::SpecError::ForbiddenInProfile { .. }
        ));
    }
}
#[test]
fn invalid_fragments_in_either_document_fail_loading_even_draft_and_print_only() {
    let work = Workspace::new();
    let spec = SPEC
        .replace("Delivered 42 new systems.", "# not a leaf")
        .replace("Added letter paragraph.", "{{unknown}}");
    stage(&work, &spec);
    for command in ["build", "letter"] {
        let result = run(&work, command, &["--print-spec"]);
        assert_eq!(result.outcome, Outcome::Failure);
        assert!(result.stderr.contains("spec-fragment-shape"), "{}", result.stderr);
        assert!(result.stderr.contains("placeholder-unknown"), "{}", result.stderr);
        assert!(!result.stderr.contains("source not available"));
        assert!(result.stdout.is_empty());
    }
}
#[test]
fn unused_document_and_draft_errors_fail_individually_before_any_product() {
    for (command, spec, code) in [
        (
            "build",
            SPEC.replace("Added letter paragraph.", "{{unknown}}")
                .replace("id = 'new-letter'", "id = 'new-letter'\nstatus = 'draft'"),
            "placeholder-unknown",
        ),
        (
            "letter",
            SPEC.replace("Delivered 42 new systems.", "# bad")
                .replace("id = 'new-bullet'", "id = 'new-bullet'\nstatus = 'draft'"),
            "spec-fragment-shape",
        ),
    ] {
        let work = Workspace::new();
        stage(&work, &spec);
        for extra in [vec![], vec!["--print-spec"]] {
            let result = run(&work, command, &extra);
            assert_eq!(result.outcome, Outcome::Failure);
            assert!(result.stderr.contains(code), "{}", result.stderr);
            assert!(result.stdout.is_empty());
        }
    }
}

#[test]
fn successful_warnings_are_structured_in_json_and_print_only_stdout_stays_toml() {
    let work = Workspace::new();
    stage(
        &work,
        &SPEC.replace("Delivered 42 new systems.", "Delivered **42** systems."),
    );
    let out = work.out().display().to_string();
    let result = run(
        &work,
        "build",
        &["--format", "json", "--fonts", "bundled", "--out", &out],
    );
    assert_eq!(result.outcome, Outcome::Success, "{}", result.stderr);
    let records: Vec<serde_json::Value> = result
        .stdout
        .lines()
        .map(|l| assert_ok!(serde_json::from_str(l)))
        .collect();
    let warnings: Vec<_> = records.iter().filter(|r| r["code"] == "mid-sentence-bold").collect();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0]["file"].as_str().unwrap().ends_with("position.toml"));
    assert_eq!(warnings[0]["line"], 9);
    let printed = run(&work, "build", &["--print-spec", "--format", "json"]);
    assert_eq!(printed.outcome, Outcome::Success);
    assert!(printed.stderr.contains("mid-sentence-bold"));
    assert_ok!(toml::from_str::<toml::Value>(&printed.stdout));
}

#[test]
fn same_document_duplicate_ids_fail_print_but_cross_document_ids_are_independent() {
    let valid = SPEC.replace("id = 'new-letter'", "id = 'new-bullet'");
    assert_ok!(resume::spec::SpecLayer::from_text(
        std::path::Path::new("position.toml"),
        &valid
    ));
    let duplicate = format!(
        "{SPEC}\n[[add]]\nid = 'new-bullet'\nkind = 'paragraph'\nplace = {{after = 'baseline'}}\ncontent = 'duplicate'\n"
    );
    assert!(matches!(
        assert_err!(resume::spec::SpecLayer::from_text(
            std::path::Path::new("position.toml"),
            &duplicate
        )),
        resume::spec::SpecError::DuplicateEntry { .. }
    ));
}

#[test]
fn letter_limit_names_aggregate_and_spec_context_without_writing_text() {
    let work = Workspace::new();
    stage(&work, &SPEC.replace("[letter]\n", "[letter]\nmax-length = 1\n"));
    let result = run(&work, "letter", &[]);
    assert_eq!(result.outcome, Outcome::Failure);
    assert!(result.stdout.is_empty());
    assert!(result.stderr.contains("text-limit-exceeded"));
    assert!(result.stderr.contains("spec length limit written at"));
    assert!(result.stderr.contains("selected body content:"));
}

#[test]
fn flat_print_keeps_metadata_placements_clears_and_decoded_markdown() {
    let text = format!(
        "{SPEC}\n[[add]]\nid = 'summary-extra'\nkind = 'paragraph'\nplace = {{before = 'baseline'}}\ncontent = \"line one\\nline two with \\\"quotes\\\"\"\ntags = []\nstatus = 'verify'\n\n[[add]]\nid = 'role-blurb'\nkind = 'blurb'\nplace = {{parent = 'role', at = 'blurb'}}\ncontent = 'Role description'\nstatus = 'draft'\n"
    );
    let work = Workspace::new();
    stage(&work, &text);
    let first = assert_ok!(resume::spec::resolve(&work.path().join("position.toml"), None));
    let printed = first.to_toml();
    assert_ok!(fs::write(work.path().join("position.toml"), &printed));
    let second = assert_ok!(resume::spec::resolve(&work.path().join("position.toml"), None));
    for (a, b) in first.additions().iter().zip(second.additions()) {
        assert_eq!(a.id().value, b.id().value);
        assert_eq!(a.place().kind(), b.place().kind());
        assert_eq!(a.content().markdown(), b.content().markdown());
        assert_eq!(
            a.metadata().annotations(&a.id().value),
            b.metadata().annotations(&b.id().value)
        );
    }
    assert_eq!(first.additions().len(), second.additions().len());
    assert!(printed.contains("tags = []"));
    assert_eq!(printed, second.to_toml());
}

#[test]
fn success_warning_budget_is_per_spec_and_late_errors_cannot_be_suppressed() {
    let mut text = "[build]\ncontent = 'missing.md'\n".to_owned();
    for i in 0..510 {
        text.push_str(&format!("[[add]]\nid = 'a-{i}'\nkind = 'paragraph'\nplace = {{after = 'baseline'}}\ncontent = 'Some **bold** text'\n"));
    }
    let path = std::path::Path::new("position.toml");
    let layer = assert_ok!(resume::spec::SpecLayer::from_text(path, &text));
    let resolved = assert_ok!(resume::spec::resolve_with(
        layer,
        &resume::spec::DirSource::new(std::path::PathBuf::new())
    ));
    let warnings: Vec<_> = resolved.content_warnings().collect();
    assert_eq!(warnings.len(), 501);
    assert_eq!(warnings[500].code, resume::diagnostics::DiagnosticCode::InputLimit);
    text.push_str("[[add]]\nid = 'bad'\nkind = 'paragraph'\nplace = {after = 'baseline'}\ncontent = '# heading'\n");
    let error = assert_err!(resume::spec::SpecLayer::from_text(path, &text));
    let (_, findings) = claims::assert_some!(error.content_findings());
    assert!(
        findings
            .iter()
            .any(|d| d.severity == resume::diagnostics::Severity::Error)
    );
}

#[test]
fn mixed_declaration_failures_retain_markdown_findings_and_later_warnings() {
    let work = Workspace::new();
    for spec in [
        SPEC.replace("Delivered 42 new systems.", "# invalid")
            .replace("{after = 'opening'}", "{after = 'opening', before = 'opening'}"),
        SPEC.replace("kind = 'bullet'", "kind = 'section'")
            .replace("Added letter paragraph.", "{{unknown}}"),
        SPEC.replace("Delivered 42 new systems.", "Some **bold** text")
            .replace("[build]\n", "[build]\npaper = 'INVALID'\n"),
    ] {
        stage(&work, &spec);
        let result = run(&work, "build", &["--print-spec"]);
        assert_eq!(result.outcome, Outcome::Failure);
        assert!(result.stdout.is_empty());
        assert!(
            result.stderr.contains("spec-fragment-shape")
                || result.stderr.contains("placeholder-unknown")
                || result.stderr.contains("mid-sentence-bold"),
            "{}",
            result.stderr
        );
        assert!(
            result.stderr.contains("incompatible addition placement")
                || result.stderr.contains("not allowed in `add.kind`")
                || result.stderr.contains("not a paper size"),
            "{}",
            result.stderr
        );
    }
}

#[test]
fn outputs_cannot_replace_baseline_or_spec_even_through_a_directory_alias() {
    let work = Workspace::new();
    stage(
        &work,
        &SPEC
            .replace("baseline.md", "position.pdf")
            .replace("Delivered 42 new systems.", "Some **bold** text"),
    );
    assert_ok!(fs::write(work.path().join("position.pdf"), BASE));
    let out = work.path().display().to_string();
    for dry in [false, true] {
        let mut extra = vec!["--fonts", "bundled", "--out", &out];
        if dry {
            extra.push("--dry-run");
        }
        let result = run(&work, "build", &extra);
        assert_eq!(result.outcome, Outcome::Failure, "{}", result.stderr);
        assert!(result.stderr.contains("would replace input"), "{}", result.stderr);
        assert!(result.stderr.contains("mid-sentence-bold"), "{}", result.stderr);
        assert_eq!(assert_ok!(fs::read_to_string(work.path().join("position.pdf"))), BASE);
    }
    #[cfg(unix)]
    {
        let alias = work.path().join("alias");
        assert_ok!(std::os::unix::fs::symlink(work.path(), &alias));
        let result = run(
            &work,
            "build",
            &["--fonts", "bundled", "--out", &alias.display().to_string()],
        );
        assert_eq!(result.outcome, Outcome::Failure);
        assert_eq!(assert_ok!(fs::read_to_string(work.path().join("position.pdf"))), BASE);
    }
}

#[test]
fn additions_participate_in_selection_promotion_core_measurement_and_source_reports() {
    let work = Workspace::new();
    let spec = format!(
        "{}\n[[add]]\nid = 'draft-item'\nkind = 'bullet'\nplace = {{parent = 'summary', at = 'end'}}\ncontent = 'Never publish this draft.'\npriority = 'core'\nstatus = 'draft'\n[select]\ntags = ['new-tag']\npromote = ['new-tag']\n",
        SPEC.replace(
            "priority = 'core'",
            "priority = 'core'\ntags = ['new-tag']\nstatus = 'verify'"
        )
    );
    stage(&work, &spec);
    let out = work.out().display().to_string();
    let result = run(
        &work,
        "build",
        &["--fonts", "bundled", "--out", &out, "--format", "json"],
    );
    assert_eq!(result.outcome, Outcome::Success, "{}", result.stderr);
    let records: Vec<serde_json::Value> = result
        .stdout
        .lines()
        .map(|l| assert_ok!(serde_json::from_str(l)))
        .collect();
    assert!(!records.iter().any(|r| r["code"] == "unknown-tag"));
    let selection = claims::assert_some!(records.iter().find(|r| r["type"] == "selection"));
    assert_eq!(selection["core"]["status"], "measured");
    assert_eq!(selection["counts"]["bullet"]["baseline"], 2);
    assert_eq!(selection["counts"]["bullet"]["kept"], 1);
    assert!(
        selection["verify"][0]["file"]
            .as_str()
            .unwrap()
            .ends_with("position.toml")
    );
    let pdf = assert_ok!(fs::read(work.out().join("position.pdf")));
    let text = common::page_texts(&pdf).join(" ");
    assert!(text.contains("Delivered 42 new systems."));
    assert!(!text.contains("Never publish"));
    // Flattening preserves both products under the same document identity.
    let printed = run(&work, "build", &["--print-spec"]);
    assert_ok!(fs::write(work.path().join("position.toml"), printed.stdout));
    assert_eq!(
        run(&work, "build", &["--fonts", "bundled", "--out", &out]).outcome,
        Outcome::Success
    );
    assert_eq!(assert_ok!(fs::read(work.out().join("position.pdf"))), pdf);
}

#[test]
fn reference_additions_have_identical_outputs_for_lf_crlf_and_flat_print() {
    let inputs = [
        ("Resume.md", include_str!("fixtures/additions/Resume.md")),
        ("Letter.md", include_str!("fixtures/additions/Letter.md")),
        ("Additions.toml", include_str!("fixtures/additions/Additions.toml")),
    ];
    let mut products = Vec::new();
    for crlf in [false, true] {
        let work = Workspace::new();
        for (name, text) in inputs {
            assert_ok!(fs::write(
                work.path().join(name),
                if crlf {
                    text.replace('\n', "\r\n")
                } else {
                    text.to_owned()
                }
            ));
        }
        let spec = work.path().join("Additions.toml").display().to_string();
        let out = work.out().display().to_string();
        let invoke = |args: &[&str]| work.run(&args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>());
        let build = invoke(&["build", "--spec", &spec, "--fonts", "bundled", "--out", &out]);
        assert_eq!(build.outcome, Outcome::Success, "{}", build.stderr);
        let letter = invoke(&["letter", "--spec", &spec, "--fonts", "bundled", "--out", &out]);
        assert_eq!(letter.outcome, Outcome::Success, "{}", letter.stderr);
        let pdf = assert_ok!(fs::read(work.out().join("Additions.pdf")));
        let letter_pdf = assert_ok!(fs::read(work.out().join("Additions-letter.pdf")));
        assert!(!common::page_texts(&pdf).join(" ").contains("draft must never"));
        let printed = invoke(&["build", "--spec", &spec, "--print-spec"]);
        assert_eq!(printed.outcome, Outcome::Success);
        assert_ok!(fs::write(&spec, printed.stdout));
        assert_eq!(
            invoke(&["build", "--spec", &spec, "--fonts", "bundled", "--out", &out]).outcome,
            Outcome::Success
        );
        let flat_letter = invoke(&["letter", "--spec", &spec, "--fonts", "bundled", "--out", &out]);
        assert_eq!(flat_letter.stdout, letter.stdout);
        assert_eq!(assert_ok!(fs::read(work.out().join("Additions.pdf"))), pdf);
        assert_eq!(
            assert_ok!(fs::read(work.out().join("Additions-letter.pdf"))),
            letter_pdf
        );
        products.push((pdf, letter_pdf, letter.stdout));
    }
    assert_eq!(products[0], products[1]);
}

#[test]
fn promotion_reorders_added_bullet_and_core_count_matches_core_only_build() {
    let work = Workspace::new();
    let spec = format!("{SPEC}\n[select]\npromote = ['new-tag']\n")
        .replace("priority = 'core'", "priority = 'core'\ntags = ['new-tag']");
    stage(&work, &spec);
    assert_ok!(fs::write(
        work.path().join("baseline.md"),
        BASE.replace(
            "Baseline paragraph. <!-- &baseline -->",
            "- Baseline first. <!-- &baseline -->"
        )
    ));
    let out = work.out().display().to_string();
    let result = run(
        &work,
        "build",
        &["--fonts", "bundled", "--out", &out, "--format", "json"],
    );
    assert_eq!(result.outcome, Outcome::Success, "{}", result.stderr);
    let records: Vec<serde_json::Value> = result
        .stdout
        .lines()
        .map(|l| assert_ok!(serde_json::from_str(l)))
        .collect();
    let selection = claims::assert_some!(records.iter().find(|r| r["type"] == "selection"));
    assert_eq!(selection["reordered"][0]["moved_up"][0]["from"], 2);
    assert_eq!(selection["reordered"][0]["moved_up"][0]["to"], 1);
    let promoted = &selection["reordered"][0]["moved_up"][0];
    assert!(promoted["file"].as_str().unwrap().ends_with("position.toml"));
    let pdf = assert_ok!(fs::read(work.out().join("position.pdf")));
    let text = common::page_texts(&pdf).join(" ");
    assert!(text.find("Delivered 42").unwrap() < text.find("Baseline first").unwrap());
    assert_ok!(fs::write(
        work.path().join("position.toml"),
        spec.replace("promote = ['new-tag']", "core-only = true")
    ));
    assert_eq!(
        run(&work, "build", &["--fonts", "bundled", "--out", &out]).outcome,
        Outcome::Success
    );
    let core = assert_ok!(fs::read(work.out().join("position.pdf")));
    assert_eq!(
        selection["core"]["pages"].as_u64(),
        Some(common::page_texts(&core).len() as u64)
    );
}

#[test]
fn letter_pdf_collision_and_spec_collision_preserve_every_input() {
    let work = Workspace::new();
    stage(&work, &SPEC.replace("letter.md", "position-letter.pdf"));
    assert_ok!(fs::write(work.path().join("position-letter.pdf"), LETTER));
    let out = work.path().display().to_string();
    for format in ["pdf", "both"] {
        for dry in [false, true] {
            let mut extra = vec![
                "--output-format",
                format,
                "--allow-overrides",
                "--fonts",
                "bundled",
                "--out",
                &out,
            ];
            if dry {
                extra.push("--dry-run");
            }
            let result = run(&work, "letter", &extra);
            assert_eq!(result.outcome, Outcome::Failure, "{}", result.stderr);
            assert!(result.stderr.contains("would replace input"), "{}", result.stderr);
            assert!(result.stdout.is_empty());
            assert_eq!(
                assert_ok!(fs::read_to_string(work.path().join("position-letter.pdf"))),
                LETTER
            );
        }
    }
    // A spec may have a non-TOML extension; its generated PDF must not replace it.
    let spec_path = work.path().join("application.pdf");
    assert_ok!(fs::write(&spec_path, SPEC));
    let result = work.run(&[
        "build".into(),
        "--spec".into(),
        spec_path.display().to_string(),
        "--fonts".into(),
        "bundled".into(),
        "--out".into(),
        out,
    ]);
    assert_eq!(result.outcome, Outcome::Failure, "{}", result.stderr);
    assert!(result.stderr.contains("would replace input"));
    assert_eq!(assert_ok!(fs::read_to_string(&spec_path)), SPEC);
}

#[test]
fn strict_declarations_reject_unknown_fields_bad_placement_and_metadata() {
    for spec in [
        SPEC.replace("kind = 'bullet'", "kind = 'section'"),
        SPEC.replace("priority = 'core'", "priority = 'none'"),
        SPEC.replace("priority = 'core'", "status = 'ready'"),
        SPEC.replace("at = 'end'", "at = 'blurb'"),
        SPEC.replace("{after = 'opening'}", "{after = 'opening', before = 'opening'}"),
        SPEC.replace("[[letter.add]]", "[[letter.add]]\nkind = 'paragraph'"),
        SPEC.replace("priority = 'core'", "tags = ['rust', 'rust']"),
    ] {
        assert_err!(resume::spec::SpecLayer::from_text(
            std::path::Path::new("position.toml"),
            &spec
        ));
    }
}
