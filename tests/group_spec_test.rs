//! Exact group specs cross loading, merging, selection, overrides and build reporting.
use claims::{assert_err, assert_ok, assert_some};
use pretty_assertions::assert_eq;
use resume::{
    commands::Outcome,
    model::GroupName,
    spec::{self, ChoiceKey},
};
use std::fs;
#[path = "common/workspace.rs"]
mod workspace;
use workspace::Workspace;

const BASE: &str = "# Ada Example\n\nLondon · ada@example.com\n\n## Summary\n\n- Keep. <!-- =keep.default #same -->\n- Omit. <!-- =omit.default #same @core -->\n\nGeneral.\n";

fn setup(work: &Workspace, select: &str) -> std::path::PathBuf {
    assert_ok!(fs::write(work.path().join("base.md"), BASE));
    let spec = work.path().join("job.toml");
    assert_ok!(fs::write(
        &spec,
        format!("[build]\ncontent = \"base.md\"\n[select]\n{select}\n")
    ));
    spec
}

fn group(name: &str) -> GroupName {
    assert_ok!(GroupName::try_new(name))
}

#[test]
fn group_lists_replace_inherit_clear_and_round_trip_with_correct_origins() {
    let work = Workspace::new();
    let profiles = work.path().join("profiles");
    assert_ok!(fs::create_dir(&profiles));
    assert_ok!(fs::write(
        profiles.join("parent.toml"),
        "[select]\ngroups = [\"omit\"]\ntags = [\"same\"]\n"
    ));
    let path = setup(&work, "groups = [\"keep\"]");
    for (field, expected) in [
        ("", vec!["omit"]),
        ("groups = [\"keep\"]", vec!["keep"]),
        ("groups = []", vec![]),
    ] {
        assert_ok!(fs::write(
            &path,
            format!("extends = [\"parent\"]\n[build]\ncontent = \"base.md\"\n[select]\n{field}\n")
        ));
        let resolved = assert_ok!(spec::resolve(&path, Some(&profiles)));
        let names = assert_some!(resolved.choices.select.groups.as_ref());
        assert_eq!(names.iter().map(|g| g.as_ref()).collect::<Vec<_>>(), expected);
        let at = assert_some!(resolved.origin_of(&ChoiceKey::Groups));
        assert_eq!(
            at.path.file_name().and_then(|p| p.to_str()),
            Some(if field.is_empty() { "parent.toml" } else { "job.toml" })
        );
        if !field.is_empty() {
            assert!(resolved.origin_of(&ChoiceKey::Group(group("omit"))).is_none());
        }
        let printed = resolved.to_toml();
        assert!(printed.contains("groups ="));
        let reload = work.path().join("printed.toml");
        assert_ok!(fs::write(&reload, printed));
        let again = assert_ok!(spec::resolve(&reload, Some(&profiles)));
        assert_eq!(again.choices.select.groups, resolved.choices.select.groups);
    }
}

#[test]
fn invalid_duplicate_and_letter_group_fields_are_rejected() {
    let work = Workspace::new();
    for field in [
        "groups = [\"keep\", \"keep\"]",
        "groups = [\"bad name\"]",
        "groups = \"keep\"",
    ] {
        let path = setup(&work, field);
        let error = assert_err!(spec::resolve(&path, None));
        assert!(error.to_string().contains("groups"), "{error}");
    }
    let path = setup(&work, "");
    assert_ok!(fs::write(
        &path,
        "[build]\ncontent = \"base.md\"\n[letter]\ncontent = \"letter.md\"\n[letter.select]\ngroups = []\n"
    ));
    assert!(assert_err!(spec::resolve(&path, None)).to_string().contains("groups"));
}

#[test]
fn real_build_reports_exact_groups_and_scope_override_clearing() {
    let work = Workspace::new();
    let path = setup(
        &work,
        "groups = [\"keep\"]\ntags = [\"same\"]\n[select.pick]\nkeep = \"default\"",
    );
    for (extra, expected, cleared) in [
        (vec![], vec!["keep"], false),
        (vec!["--allow-overrides", "--tags", "same"], vec!["keep", "omit"], true),
        (vec!["--allow-overrides", "--core-only"], vec!["omit"], true),
        (vec!["--allow-overrides", "--prefer", "default"], vec!["keep"], false),
    ] {
        let mut args = vec![
            "build",
            "--spec",
            assert_some!(path.to_str()),
            "--fonts",
            "bundled",
            "--dry-run",
            "--format",
            "json",
        ];
        args.extend(extra);
        let run = work.run(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
        let rows: Vec<serde_json::Value> = run
            .stdout
            .lines()
            .map(|s| assert_ok!(serde_json::from_str(s)))
            .collect();
        let selection = assert_some!(rows.iter().find(|r| r["type"] == "selection"));
        let chosen = assert_some!(selection["chosen"].as_array());
        assert_eq!(
            chosen
                .iter()
                .map(|v| assert_some!(v["group"].as_str()))
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(selection["groups"].is_null(), cleared);
        let report = assert_some!(rows.iter().find(|r| r["type"] == "spec"));
        let overrides = assert_some!(report["overrides"].as_array());
        assert_eq!(
            overrides
                .iter()
                .any(|v| v["cleared_groups"]["names"] == serde_json::json!(["keep"])),
            cleared
        );
    }
    assert_err!(resume::cli::Cli::try_parse_validated([
        "resume",
        "build",
        "--spec",
        assert_some!(path.to_str()),
        "--core-only"
    ]));
}

#[test]
fn unknown_and_draft_requested_groups_fail_with_spec_origin_and_no_pdf() {
    let work = Workspace::new();
    for (groups, draft, needle) in [
        ("missing", false, "unknown requested groups"),
        ("keep", true, "cannot render"),
    ] {
        let path = setup(&work, &format!("groups = [\"{groups}\"]"));
        if draft {
            assert_ok!(fs::write(
                work.path().join("base.md"),
                BASE.replace("## Summary", "## Summary <!-- @draft -->")
            ));
        }
        let run = work.run(&["build", "--spec", assert_some!(path.to_str()), "--fonts", "bundled"].map(String::from));
        assert_ne!(run.outcome, Outcome::Success);
        assert!(run.stderr.contains(needle), "{}", run.stderr);
        assert!(run.stderr.contains("job.toml:4:"), "{}", run.stderr);
        if !draft {
            assert!(
                !run.stderr.contains("--drafts"),
                "unknown names need correction, not preview"
            );
        }
        assert!(!work.out().join("job.pdf").exists());
    }
}

#[test]
fn empty_list_clearing_and_core_measurement_preserve_their_meaning() {
    let work = Workspace::new();
    let path = setup(&work, "groups = []\ntags = [\"same\"]");
    let run = work.run(
        &[
            "build",
            "--spec",
            assert_some!(path.to_str()),
            "--fonts",
            "bundled",
            "--dry-run",
            "--format",
            "json",
            "--allow-overrides",
            "--core-only",
        ]
        .map(String::from),
    );
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let rows: Vec<serde_json::Value> = run
        .stdout
        .lines()
        .map(|s| assert_ok!(serde_json::from_str(s)))
        .collect();
    let spec = assert_some!(rows.iter().find(|r| r["type"] == "spec"));
    assert_eq!(spec["overrides"][0]["cleared_groups"]["names"], serde_json::json!([]));
    assert!(assert_some!(spec["overrides"][0]["cleared_groups"]["at"].as_str()).contains("job.toml:4:"));
    assert_eq!(spec["overrides"][0]["replaced"]["setting"], "tags");
    let path = setup(&work, "groups = [\"keep\"]\ncore-only = true");
    let run = work.run(
        &[
            "build",
            "--spec",
            assert_some!(path.to_str()),
            "--fonts",
            "bundled",
            "--dry-run",
            "--format",
            "json",
        ]
        .map(String::from),
    );
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let rows: Vec<serde_json::Value> = run
        .stdout
        .lines()
        .map(|s| assert_ok!(serde_json::from_str(s)))
        .collect();
    let selection = assert_some!(rows.iter().find(|r| r["type"] == "selection"));
    assert_eq!(selection["chosen"][0]["group"], "keep");
    assert_eq!(selection["core"]["status"], "measured");
}

#[test]
fn inherited_requests_and_clearing_point_at_the_winning_profile() {
    let work = Workspace::new();
    let profiles = work.path().join("profiles");
    assert_ok!(fs::create_dir(&profiles));
    assert_ok!(fs::write(
        profiles.join("parent.toml"),
        "[select]\ngroups = [\"missing\"]\n"
    ));
    let path = setup(&work, "");
    assert_ok!(fs::write(
        &path,
        "extends = [\"parent\"]\n[build]\ncontent = \"base.md\"\n"
    ));
    let common = [
        "build",
        "--spec",
        assert_some!(path.to_str()),
        "--profiles",
        assert_some!(profiles.to_str()),
        "--fonts",
        "bundled",
        "--dry-run",
        "--format",
        "json",
    ];
    let run = work.run(&common.map(String::from));
    assert_ne!(run.outcome, Outcome::Success);
    assert!(format!("{}{}", run.stdout, run.stderr).contains("parent.toml:2:"));
    let mut args = common.map(String::from).to_vec();
    args.extend(["--allow-overrides", "--core-only"].map(String::from));
    let run = work.run(&args);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let rows: Vec<serde_json::Value> = run
        .stdout
        .lines()
        .map(|s| assert_ok!(serde_json::from_str(s)))
        .collect();
    let spec = assert_some!(rows.iter().find(|r| r["type"] == "spec"));
    assert!(assert_some!(spec["overrides"][0]["cleared_groups"]["at"].as_str()).contains("parent.toml:2:"));
    assert_ok!(fs::write(
        profiles.join("parent.toml"),
        "[select]\ngroups = [\"missing\"]\npick = { missing = \"default\" }\n"
    ));
    assert_ne!(work.run(&args).outcome, Outcome::Success);
    assert_ok!(fs::write(
        profiles.join("parent.toml"),
        "[select]\ngroups = [\"keep\", \"keep\"]\n"
    ));
    let run = work.run(&args);
    assert_ne!(run.outcome, Outcome::Success);
    assert!(format!("{}{}", run.stdout, run.stderr).contains("more than once"));
}

#[test]
fn letter_public_api_refuses_even_empty_membership() {
    let text = "# Ada\n\n## Greeting\n\nHello,\n\n## Body\n\nWork.\n\n## Closing\n\nRegards,\n";
    let parsed = resume::markdown::parse_letter("letter.md", text);
    let source = assert_some!(parsed.into_parts().0);
    let selection = resume::select::Selection {
        groups: Some(Default::default()),
        ..Default::default()
    };
    let error = assert_err!(resume::letter::select(&source, &selection));
    assert!(error.to_string().contains("only for resumes"));
}
