//! Letter command tailoring agrees with the shared flags and library pipeline.
use claims::{assert_err, assert_ok, assert_some};
use pretty_assertions::assert_eq;
use resume::{
    cli::{Cli, Command},
    commands::{self, Env, Io, Outcome},
    diagnostics::termcolor::Buffer,
    letter,
    markdown::parse_letter,
};
use std::{ffi::OsString, path::Path};
const SOURCE: &str = "# Alex\n\n## Greeting\n\nHello,\n\n## Body\n\nCore. <!-- @core @verify -->\n\nGeneral. <!-- =g.default #platform -->\n\nFor {{recipient}}. <!-- =g.personal #platform -->\n\nDraft. <!-- @draft -->\n\nExtra. <!-- @extra #other -->\n\n## Closing\n\nRegards,\n";
fn cli(path: &Path, flags: &[&str]) -> Cli {
    let mut args = vec![OsString::from("resume"), "letter".into(), path.as_os_str().to_owned()];
    args.extend(flags.iter().map(OsString::from));
    assert_ok!(Cli::try_parse_validated(args))
}
fn run(cli: &Cli, root: &Path) -> (Outcome, String, String) {
    let mut stdout = Vec::new();
    let mut stderr = Buffer::no_color();
    let outcome = commands::run(
        cli,
        &Env {
            cwd: root.to_owned(),
            resume_font_path: None,
        },
        &mut Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );
    (
        outcome,
        assert_ok!(String::from_utf8(stdout)),
        assert_ok!(String::from_utf8(stderr.into_inner())),
    )
}
#[test]
fn shared_flags_produce_exact_library_output_and_private_reports() {
    let root = assert_ok!(tempfile::tempdir());
    let path = root.path().join("letter.md");
    assert_ok!(std::fs::write(&path, SOURCE));
    for flags in [
        vec![],
        vec!["--tags", "platform"],
        vec!["--core-only"],
        vec!["--drafts"],
        vec!["--prefer", "personal", "--recipient", "Team"],
        vec![
            "--tags",
            "platform",
            "--prefer",
            "personal",
            "--drafts",
            "--recipient",
            "Team",
        ],
    ] {
        let cli = cli(&path, &flags);
        let Command::Letter(args) = &cli.command else {
            panic!("letter")
        };
        let source = assert_some!(parse_letter(&path, SOURCE).into_parts().0);
        let (selected, _) = assert_ok!(letter::select(&source, &args.select.selection()));
        let resolved = assert_ok!(letter::resolve(selected, &args.effective().fields));
        let expected = letter::to_plain(&resolved, args.effective().links);
        let (outcome, stdout, stderr) = run(&cli, root.path());
        assert_eq!(outcome, Outcome::Success);
        assert_eq!(stdout, expected.as_str());
        assert!(stderr.contains("selection:"));
        assert!(stderr.contains("to verify:"));
        assert!(!stderr.contains("For Team"));
        assert!(!stderr.contains("Core."));
    }
}
#[test]
fn flags_parse_identically_and_usage_conflicts_still_fail() {
    for flags in [
        vec!["--tags", "a,b", "--tags", "c"],
        vec!["--prefer", "short,long", "--prefer", "other"],
        vec!["--core-only", "--drafts"],
    ] {
        let parse = |command| {
            let mut a = vec!["resume", command, "letter.md"];
            a.extend(&flags);
            assert_ok!(Cli::try_parse_validated(a))
        };
        let Command::Letter(letter) = parse("letter").command else {
            panic!("letter")
        };
        let Command::Build(build) = parse("build").command else {
            panic!("build")
        };
        assert_eq!(letter.select.selection(), build.select.selection());
    }
    for flags in [
        vec!["--tags", "Upper"],
        vec!["--prefer", ""],
        vec!["--tags", "a", "--core-only"],
        vec!["--format", "json", "--tags", "a"],
    ] {
        let mut args = vec!["resume", "letter", "missing.md"];
        args.extend(flags);
        assert_eq!(assert_err!(Cli::try_parse_validated(args)).exit_code(), 2);
    }
}
#[test]
fn all_modes_select_before_fields_and_json_explains_only_locations() {
    let root = assert_ok!(tempfile::tempdir());
    let path = root.path().join("letter.md");
    assert_ok!(std::fs::write(&path, SOURCE));
    let out = root.path().join("out");
    let out_arg = assert_some!(out.to_str());
    for mode in ["text", "pdf", "both"] {
        let (result, stdout, stderr) = run(
            &cli(
                &path,
                &["--output-format", mode, "--fonts", "bundled", "--out", out_arg],
            ),
            root.path(),
        );
        assert_eq!(result, Outcome::Success);
        assert!(!stdout.contains("recipient"));
        assert!(!stderr.contains("placeholder-unresolved"));
        let target = out.join("letter-letter.pdf");
        assert_ok!(std::fs::create_dir_all(&out));
        assert_ok!(std::fs::write(&target, b"prior"));
        let (result, stdout, stderr) = run(
            &cli(
                &path,
                &["--output-format", mode, "--prefer", "personal", "--out", out_arg],
            ),
            root.path(),
        );
        assert_eq!(result, Outcome::Failure);
        assert!(stdout.is_empty());
        assert!(stderr.contains("placeholder-unresolved"));
        assert_eq!(assert_ok!(std::fs::read(&target)), b"prior");
    }
    let (result, stdout, _) = run(
        &cli(
            &path,
            &[
                "--output-format",
                "pdf",
                "--format",
                "json",
                "--fonts",
                "bundled",
                "--dry-run",
                "--tags",
                "typo",
            ],
        ),
        root.path(),
    );
    assert_eq!(result, Outcome::Success);
    let records: Vec<serde_json::Value> = stdout
        .lines()
        .map(|line| assert_ok!(serde_json::from_str(line)))
        .collect();
    assert_eq!(records[0]["code"], "unknown-tag");
    let record = assert_some!(records.iter().find(|r| r["type"] == "letter-selection"));
    assert_eq!(record["scope"], "tags");
    assert!(record.get("kind").is_none());
    assert_eq!(record["drafts_left_out"], 1);
    assert_eq!(record["file"], "letter.md");
    assert_eq!(record["kept"].as_array().map(Vec::len), Some(3));
    assert_eq!(record["verify"][0]["part"], "body");
    assert_eq!(record["verify"][0]["line"], 9);
    assert_eq!(record["dropped"].as_array().map(Vec::len), Some(3));
    assert!(!stdout.contains("Core."));
    assert!(!stdout.contains("{{recipient}}"));
}

#[test]
fn malformed_unselected_text_fails_before_reports_and_empty_selection_keeps_prior_pdf() {
    let root = assert_ok!(tempfile::tempdir());
    let path = root.path().join("letter.md");
    assert_ok!(std::fs::write(&path, SOURCE.replace("{{recipient}}", "{{unknown}}")));
    let (result, stdout, stderr) = run(&cli(&path, &["--core-only"]), root.path());
    assert_eq!(result, Outcome::Failure);
    assert!(stdout.is_empty());
    assert!(stderr.contains("placeholder-unknown"));
    assert!(!stderr.contains("selection:"));
    assert_ok!(std::fs::write(&path, SOURCE.replace("@core @verify", "#other")));
    let out = root.path().join("out");
    assert_ok!(std::fs::create_dir(&out));
    let target = out.join("letter-letter.pdf");
    assert_ok!(std::fs::write(&target, b"prior"));
    let (result, stdout, stderr) = run(
        &cli(
            &path,
            &[
                "--core-only",
                "--output-format",
                "both",
                "--out",
                assert_some!(out.to_str()),
            ],
        ),
        root.path(),
    );
    assert_eq!(result, Outcome::Failure);
    assert!(stdout.is_empty());
    assert!(stderr.contains("letter-body-empty"));
    assert_eq!(assert_ok!(std::fs::read(target)), b"prior");
}

#[test]
fn silent_defaults_and_draft_fallback_reports_have_stable_contracts() {
    let root = assert_ok!(tempfile::tempdir());
    let path = root.path().join("letter.md");
    let plain = "# Alex\n\n## Greeting\n\nHello,\n\n## Body\n\nBody.\n\n## Closing\n\nRegards,\n";
    assert_ok!(std::fs::write(&path, plain));
    let (result, stdout, stderr) = run(
        &cli(
            &path,
            &[
                "--output-format",
                "pdf",
                "--format",
                "json",
                "--fonts",
                "bundled",
                "--dry-run",
            ],
        ),
        root.path(),
    );
    assert_eq!(result, Outcome::Success);
    assert!(!stdout.contains("letter-selection"));
    assert!(!stderr.contains("selection:"));
    let source = plain.replace(
        "Body.",
        "Draft. <!-- =g.default @draft #draft-only -->\n\nReady. <!-- =g.short -->",
    );
    assert_ok!(std::fs::write(&path, &source));
    let (_, _, stderr) = run(
        &cli(&path, &["--tags", "draft-only", "--prefer", "default"]),
        root.path(),
    );
    assert!(!stderr.contains("unknown-"));
    assert!(stderr.contains("wording g.short: first-ready"));
    assert!(stderr.contains("skipped draft wording: g.default"));
    assert!(stderr.contains("drafts: 1 left out"));
    let (_, _, stderr) = run(&cli(&path, &["--drafts"]), root.path());
    assert!(stderr.contains("drafts: 1 included for preview"));
    let (_, stdout, _) = run(
        &cli(
            &path,
            &[
                "--output-format",
                "pdf",
                "--format",
                "json",
                "--fonts",
                "bundled",
                "--dry-run",
            ],
        ),
        root.path(),
    );
    let records: Vec<serde_json::Value> = stdout
        .lines()
        .map(|line| assert_ok!(serde_json::from_str(line)))
        .collect();
    let record = assert_some!(records.iter().find(|r| r["type"] == "letter-selection"));
    assert_eq!(record["chosen"][0]["by"], "first-ready");
    assert_eq!(record["chosen"][0]["variant"], "short");
    assert_eq!(record["drafts_left_out"], 1);
    let mut keys: Vec<_> = assert_some!(record.as_object()).keys().map(String::as_str).collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "chosen",
            "drafts",
            "drafts_included",
            "drafts_left_out",
            "dropped",
            "file",
            "kept",
            "prefer",
            "scope",
            "skipped_drafts",
            "tags",
            "type",
            "verify"
        ]
    );
    assert_ok!(std::fs::write(
        &path,
        SOURCE.replace(
            "Hello,",
            "Hello, <!-- =greeting.default -->\n\nWelcome, <!-- =greeting.frame-only -->"
        )
    ));
    let (result, _, stderr) = run(
        &cli(&path, &["--core-only", "--prefer", "personal,frame-only"]),
        root.path(),
    );
    assert_eq!(result, Outcome::Success);
    assert!(!stderr.contains("unknown-wording"));
}

#[test]
fn unknown_names_warn_but_do_not_pollute_product_or_offer_resume_only_command() {
    let root = assert_ok!(tempfile::tempdir());
    let path = root.path().join("letter.md");
    assert_ok!(std::fs::write(&path, SOURCE));
    let (outcome, stdout, stderr) = run(&cli(&path, &["--tags", "typo", "--prefer", "absent"]), root.path());
    assert_eq!(outcome, Outcome::Success);
    assert_eq!(stdout, "Hello,\n\nCore. \n\nRegards,\nAlex\n");
    assert!(stderr.contains("unknown-tag"));
    assert!(stderr.contains("unknown-wording"));
    assert!(!stderr.contains("resume tags"));
    assert!(!stderr.contains("the resume"));
    let (outcome, stdout, stderr) = run(&cli(&path, &["--core-only", "--dry-run"]), root.path());
    assert_eq!(outcome, Outcome::Success);
    assert!(stdout.is_empty());
    assert!(stderr.contains("dropped:"));
    assert!(!root.path().join("out").exists());
}
