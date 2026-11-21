//! ABOUTME: Public text-first letter command: exact product, validation gates and stream failures.
//! ABOUTME: Synthetic files and injected writers keep filesystem and delivery behavior observable.
use claims::{assert_err, assert_ok, assert_some};
use clap::Parser;
use pretty_assertions::assert_eq;
use resume::{
    cli::{Cli, Command},
    commands::{self, Env, Io, Outcome},
    diagnostics::termcolor::Buffer,
};
use std::{
    io::{self, Write},
    path::Path,
};

const SOURCE: &str = "# Alex Example\n\n## Greeting\n\nDear Team,\n\n## Body\n\nI seek {{title}} at {{company}}.\n\n## Closing\n\nSincerely,\n";
const EXPECTED: &str = "Dear Team,\n\nI seek Architect at Acme.\n\nSincerely,\nAlex Example\n";
fn args(path: &Path, extra: &[&str]) -> Cli {
    let mut values = vec![
        "resume".into(),
        "letter".into(),
        path.as_os_str().to_owned(),
        "--company".into(),
        "Acme".into(),
        "--title".into(),
        "Architect".into(),
    ];
    values.extend(extra.iter().map(std::ffi::OsString::from));
    assert_ok!(Cli::try_parse_validated(values))
}
fn execute(cli: &Cli, root: &Path, stdout: &mut dyn Write) -> (Outcome, String) {
    let env = Env {
        cwd: root.to_owned(),
        resume_font_path: Some(root.join("missing-env-fonts").into_os_string()),
    };
    let mut stderr = Buffer::no_color();
    let outcome = commands::run(
        cli,
        &env,
        &mut Io {
            stdout,
            stderr: &mut stderr,
        },
    );
    (outcome, assert_ok!(String::from_utf8(stderr.as_slice().to_vec())))
}
fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = assert_ok!(tempfile::tempdir());
    let path = root.path().join("letter.md");
    assert_ok!(std::fs::write(&path, SOURCE));
    (root, path)
}

#[test]
fn text_product_is_exact_and_render_configuration_is_not_consulted() {
    let (root, path) = fixture();
    let mut stdout = Vec::new();
    let cli = args(
        &path,
        &[
            "--style",
            "absent",
            "--styles-dir",
            "missing-styles",
            "--font-path",
            "missing-fonts",
            "--fonts",
            "auto",
        ],
    );
    let (outcome, stderr) = execute(&cli, root.path(), &mut stdout);
    assert_eq!(outcome, Outcome::Success);
    assert_eq!(stdout, EXPECTED.as_bytes());
    assert_eq!(
        stderr,
        "ok printed letter: 63 utf16, 63 scalars, 63 bytes, 10 words, 6 line breaks\n"
    );
}

#[test]
fn dry_run_checks_content_without_output_or_files() {
    let (root, path) = fixture();
    let mut stdout = Vec::new();
    let (outcome, stderr) = execute(&args(&path, &["--dry-run"]), root.path(), &mut stdout);
    assert_eq!(outcome, Outcome::Success);
    assert!(stdout.is_empty());
    assert!(stderr.contains("would print"));
    assert_eq!(assert_ok!(std::fs::read_dir(root.path())).count(), 1);
    assert_eq!(assert_ok!(std::fs::read_to_string(path)), SOURCE);
}

#[test]
fn every_content_failure_keeps_stdout_empty() {
    let (root, path) = fixture();
    for (input, expected) in [
        (SOURCE.replace("{{title}}", "{{recipient}}"), "placeholder-unresolved"),
        (SOURCE.replace("## Closing", "## Wrong"), "letter-unknown-section"),
    ] {
        assert_ok!(std::fs::write(&path, input));
        let mut stdout = Vec::new();
        let (outcome, stderr) = execute(&args(&path, &[]), root.path(), &mut stdout);
        assert_eq!(outcome, Outcome::Failure);
        assert!(stdout.is_empty());
        assert!(stderr.contains(expected), "{stderr}");
    }
}

#[test]
fn length_gate_accepts_exact_limit_and_rejects_zero_for_every_unit() {
    let (root, path) = fixture();
    for (unit, count) in [
        ("utf16", EXPECTED.encode_utf16().count()),
        ("scalars", EXPECTED.chars().count()),
        ("bytes", EXPECTED.len()),
        ("words", EXPECTED.split_whitespace().count()),
    ] {
        for (max, expected) in [
            (count, Outcome::Success),
            (count - 1, Outcome::Failure),
            (0, Outcome::Failure),
        ] {
            let mut stdout = Vec::new();
            let max = max.to_string();
            let (actual, stderr) = execute(
                &args(&path, &["--max-length", &max, "--length-unit", unit]),
                root.path(),
                &mut stdout,
            );
            assert_eq!(actual, expected);
            assert_eq!(stdout.is_empty(), expected == Outcome::Failure);
            if expected == Outcome::Failure {
                assert!(stderr.contains("text-limit-exceeded"), "{stderr}");
            }
        }
    }
}

#[test]
fn usage_errors_precede_file_reads_in_binary_and_library() {
    for extra in [
        vec!["--format", "json"],
        vec!["--output-format", "pdf"],
        vec!["--output-format", "both"],
        vec!["--length-unit", "words"],
        vec!["--fonts", "bundled", "--font-path", "x"],
    ] {
        let mut argv = vec!["resume", "letter", "missing.md"];
        argv.extend(extra);
        assert_eq!(assert_err!(Cli::try_parse_validated(argv.clone())).exit_code(), 2);
        if let Ok(cli) = Cli::try_parse_from(argv) {
            let mut stdout = Vec::new();
            let (outcome, stderr) = execute(&cli, Path::new("."), &mut stdout);
            assert_eq!(outcome, Outcome::Failure);
            assert!(stdout.is_empty());
            assert!(!stderr.contains("cannot read"));
        }
    }
    assert_cmd::Command::new(env!("CARGO_BIN_EXE_resume"))
        .args(["letter", "missing.md", "--format", "json"])
        .assert()
        .code(2)
        .stdout("");
}

#[test]
fn explicit_settings_are_preserved_and_validated() {
    let cli = assert_ok!(Cli::try_parse_validated(["resume", "letter", "x.md"]));
    let Command::Letter(a) = cli.command else {
        panic!("letter")
    };
    assert!(a.company.is_none() && a.title.is_none() && a.recipient.is_none() && a.date.is_none());
    assert!(a.render.styles_dir.is_none() && a.render.font_path.is_empty());
    assert_eq!(a.effective().links, resume::letter::LinkStyle::TextAndUrl);
    assert_eq!(a.effective().limit, None);
    assert!(a.output_format.is_none() && a.links.is_none() && a.max_length.is_none() && a.length_unit.is_none());
    assert!(
        a.render.style.is_none()
            && a.render.paper.is_none()
            && a.render.fonts.is_none()
            && a.render.max_pages.is_none()
    );
    let cli = args(
        Path::new("x.md"),
        &[
            "--output-format",
            "text",
            "--max-length",
            "0",
            "--length-unit",
            "utf16",
            "--paper",
            "a4",
            "--date",
            "2028-02-29",
        ],
    );
    let Command::Letter(a) = cli.command else {
        panic!("letter")
    };
    assert_some!(a.output_format);
    assert_eq!(a.output_format, Some(resume::letter::LetterOutput::Text));
    assert_eq!(a.length_unit, Some(resume::letter::LengthUnit::Utf16));
    assert_eq!(a.max_length, Some(0));
    assert_some!(a.render.paper);
    assert_some!(a.date);
    for (flag, value) in [
        ("--company", ""),
        ("--company", "Acme\nCo"),
        ("--date", "2027-02-29"),
        ("--style", "9 bad"),
        ("--length-unit", "letters"),
    ] {
        assert_err!(Cli::try_parse_validated(["resume", "letter", "x.md", flag, value]));
    }
}

struct Broken {
    kind: io::ErrorKind,
    on_flush: bool,
}
impl Write for Broken {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.on_flush {
            Ok(bytes.len())
        } else {
            Err(self.kind.into())
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.on_flush { Err(self.kind.into()) } else { Ok(()) }
    }
}
#[test]
fn write_and_flush_failures_including_broken_pipe_never_report_success() {
    let (root, path) = fixture();
    for kind in [io::ErrorKind::BrokenPipe, io::ErrorKind::PermissionDenied] {
        for on_flush in [false, true] {
            let (outcome, stderr) = execute(&args(&path, &[]), root.path(), &mut Broken { kind, on_flush });
            assert_eq!(outcome, Outcome::Failure);
            assert!(!stderr.contains("ok "), "{stderr}");
            assert!(stderr.contains("not fully written"), "{stderr}");
        }
    }
}

#[test]
fn input_is_bounded_and_utf8_errors_keep_typed_read_context() {
    let (root, path) = fixture();
    let multibyte = format!("{}€€€€", "x".repeat(resume::markdown::MAX_INPUT_BYTES - 1));
    for (bytes, expected) in [
        (multibyte.into_bytes(), "input-limit"),
        (vec![b'x'; resume::markdown::MAX_INPUT_BYTES + 1], "input-limit"),
        (vec![0xff], "as UTF-8"),
    ] {
        assert_ok!(std::fs::write(&path, bytes));
        let mut stdout = Vec::new();
        let (outcome, stderr) = execute(&args(&path, &[]), root.path(), &mut stdout);
        assert_eq!(outcome, Outcome::Failure);
        assert!(stdout.is_empty());
        assert!(stderr.contains(expected), "{stderr}");
    }
}

#[test]
fn unicode_units_and_default_are_discriminating_at_cli_boundary() {
    let (root, path) = fixture();
    for (unit, count) in [("utf16", 66), ("scalars", 65), ("bytes", 69), ("words", 11)] {
        for max in [count - 1, count] {
            let mut cli = args(&path, &["--max-length", &max.to_string(), "--length-unit", unit]);
            let Command::Letter(a) = &mut cli.command else {
                unreachable!()
            };
            a.company = Some(assert_ok!(resume::letter::FieldValue::try_new("Acmé 🚀")));
            let mut stdout = Vec::new();
            let (outcome, _) = execute(&cli, root.path(), &mut stdout);
            assert_eq!(
                outcome,
                if max == count {
                    Outcome::Success
                } else {
                    Outcome::Failure
                }
            );
        }
    }
    let mut cli = args(&path, &["--max-length", "65"]);
    let Command::Letter(a) = &mut cli.command else {
        unreachable!()
    };
    a.company = Some(assert_ok!(resume::letter::FieldValue::try_new("Acmé 🚀")));
    assert_eq!(execute(&cli, root.path(), &mut Vec::new()).0, Outcome::Failure);
}

#[test]
fn recipient_links_and_literal_fields_reach_the_product() {
    let (root, path) = fixture();
    assert_ok!(std::fs::write(
        &path,
        SOURCE.replace("Dear Team,", "Dear {{recipient}},").replace(
            "I seek {{title}} at {{company}}.",
            "[Site](https://example.invalid/) at {{company}}."
        )
    ));
    for (extra, link) in [
        (vec!["--recipient", "Taylor"], "Site (https://example.invalid/)"),
        (vec!["--recipient", "Taylor", "--links", "text-only"], "Site"),
    ] {
        let mut cli = args(&path, &extra);
        let Command::Letter(a) = &mut cli.command else {
            unreachable!()
        };
        a.company = Some(assert_ok!(resume::letter::FieldValue::try_new("*Acme* {{title}}")));
        let mut stdout = Vec::new();
        let (outcome, _) = execute(&cli, root.path(), &mut stdout);
        assert_eq!(outcome, Outcome::Success);
        assert_eq!(
            String::from_utf8_lossy(&stdout),
            format!("Dear Taylor,\n\n{link} at *Acme* {{{{title}}}}.\n\nSincerely,\nAlex Example\n")
        );
    }
}

#[test]
fn dry_run_rejects_bad_content_and_limits_and_missing_files() {
    let (root, path) = fixture();
    for extra in [vec!["--dry-run", "--max-length", "0"], vec!["--dry-run"]] {
        if extra.len() == 1 {
            assert_ok!(std::fs::write(&path, SOURCE.replace("{{company}}", "{{recipient}}")));
        }
        let mut stdout = Vec::new();
        let (outcome, stderr) = execute(&args(&path, &extra), root.path(), &mut stdout);
        assert_eq!(outcome, Outcome::Failure);
        assert!(stdout.is_empty());
        assert!(!stderr.contains("would print"));
    }
    let (outcome, stderr) = execute(&args(&root.path().join("missing"), &[]), root.path(), &mut Vec::new());
    assert_eq!(outcome, Outcome::Failure);
    assert!(stderr.contains("cannot read"));
    assert!(!stderr.contains("as UTF-8"));
    let padded = format!(
        "{}{}",
        SOURCE,
        " ".repeat(resume::markdown::MAX_INPUT_BYTES - SOURCE.len())
    );
    assert_ok!(std::fs::write(&path, padded));
    let (_, stderr) = execute(&args(&path, &[]), root.path(), &mut Vec::new());
    assert!(!stderr.contains("input-limit"));
}

#[test]
fn existing_resume_only_style_and_zero_write_are_handled() {
    let (root, path) = fixture();
    let style = root.path().join("styles/only");
    assert_ok!(std::fs::create_dir_all(&style));
    assert_ok!(std::fs::write(style.join("style.typ"), "invalid Typst"));
    let mut stdout = Vec::new();
    let cli = args(
        &path,
        &[
            "--styles-dir",
            root.path().join("styles").to_str().expect("temp path"),
            "--style",
            "only",
        ],
    );
    assert_eq!(execute(&cli, root.path(), &mut stdout).0, Outcome::Success);
    assert_eq!(stdout, EXPECTED.as_bytes());
    struct Zero;
    impl Write for Zero {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Ok(0)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let (outcome, stderr) = execute(&cli, root.path(), &mut Zero);
    assert_eq!(outcome, Outcome::Failure);
    assert!(stderr.contains("not fully written"));
    assert_cmd::Command::new(env!("CARGO_BIN_EXE_resume"))
        .args(["letter", "missing.md", "--output-format", "pdf"])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicates::str::contains("not available yet"));
}
