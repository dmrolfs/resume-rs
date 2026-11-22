//! Public letter PDF, mixed-document and output-order contracts.
mod common;

use claims::{assert_err, assert_ok, assert_some};
use pretty_assertions::assert_eq;
use resume::{
    cli::Cli,
    commands::{self, Env, Io, Outcome},
    diagnostics::termcolor::Buffer,
    letter::{self, FieldValue, Letter, LetterFields, SelectedLetter},
    model::options::BuiltinStyle,
    style::StyleSource,
    typeset::{RenderOptions, Typesetter},
};
use std::{io, path::Path};

const SOURCE: &str = "# Alex Example\n\n[alex@example.invalid](mailto:alex@example.invalid)\n\n## Greeting\n\nDear {{recipient}},\n\n## Body\n\nI seek {{title}} at {{company}}.\n\nMy distinctive letter paragraph.\n\n## Closing\n\nSincerely,\n";

fn letter() -> Letter {
    let parsed = resume::markdown::parse_letter("neutral.md", SOURCE);
    let fields = LetterFields {
        company: Some(assert_ok!(FieldValue::try_new("Acme"))),
        title: Some(assert_ok!(FieldValue::try_new("Architect"))),
        recipient: Some(assert_ok!(FieldValue::try_new("Team"))),
        date: Some(assert_ok!("2028-02-29".parse())),
    };
    assert_ok!(letter::resolve(
        assert_ok!(SelectedLetter::try_from(assert_some!(parsed.into_parts().0))),
        &fields,
        Path::new("neutral.md")
    ))
}

fn run(root: &Path, extra: &[&str], stdout: &mut dyn io::Write) -> (Outcome, String) {
    let file = root.join("neutral.md");
    let out = root.join("out");
    let cli = assert_ok!(Cli::try_parse_validated(
        [
            "resume",
            "letter",
            assert_some!(file.to_str()),
            "--out",
            assert_some!(out.to_str()),
            "--fonts",
            "bundled",
            "--company",
            "Acme",
            "--title",
            "Architect",
            "--recipient",
            "Team",
        ]
        .into_iter()
        .chain(extra.iter().copied())
    ));
    let mut stderr = Buffer::no_color();
    let env = Env {
        cwd: root.to_owned(),
        resume_font_path: None,
    };
    let result = commands::run(
        &cli,
        &env,
        &mut Io {
            stdout,
            stderr: &mut stderr,
        },
    );
    (result, String::from_utf8_lossy(stderr.as_slice()).into_owned())
}

#[test]
fn one_page_letter_content_date_and_repeated_mixed_compiles() {
    let ts = Typesetter::new(common::bundled_fonts());
    let style = assert_ok!(StyleSource::builtin(BuiltinStyle::Pragmatic));
    let resume = common::full_resume();
    let before = assert_ok!(assert_ok!(ts.compile(&resume, &style, RenderOptions::default())).export_pdf("neutral"));
    let compiled = assert_ok!(ts.compile_letter(&letter(), &style, RenderOptions::default()));
    assert_eq!(compiled.page_count(), 1);
    let pdf = assert_ok!(compiled.export_pdf("neutral"));
    let texts = common::page_texts(&pdf);
    let text = &texts[0];
    let parts = [
        "Alex Example",
        "alex@example.invalid",
        "February 29, 2028",
        "Team",
        "Acme",
        "Dear Team,",
        "I seek Architect at Acme.",
        "My distinctive letter paragraph.",
        "Sincerely,",
        "Alex Example",
    ];
    let mut rest = text.as_str();
    for part in parts {
        let pos = common::position(rest, part);
        rest = &rest[pos + part.len()..];
    }
    assert_eq!(text.matches("My distinctive letter paragraph.").count(), 1);
    let repeat = assert_ok!(
        assert_ok!(Typesetter::new(common::bundled_fonts()).compile_letter(
            &letter(),
            &style,
            RenderOptions::default()
        ))
        .export_pdf("neutral")
    );
    assert_eq!(pdf, repeat);
    let after = assert_ok!(assert_ok!(ts.compile(&resume, &style, RenderOptions::default())).export_pdf("neutral"));
    assert_eq!(before, after);
    assert!(!common::page_texts(&after).join(" ").contains("distinctive letter"));
}

#[test]
fn missing_capability_is_only_a_pdf_error() {
    let root = assert_ok!(tempfile::tempdir());
    assert_ok!(std::fs::write(root.path().join("neutral.md"), SOURCE));
    for mode in ["text", "pdf", "both"] {
        let mut stdout = Vec::new();
        let (result, err) = run(root.path(), &["--output-format", mode, "--style", "mono"], &mut stdout);
        if mode == "text" {
            assert_eq!(result, Outcome::Success);
            assert!(!stdout.is_empty());
        } else {
            assert_eq!(result, Outcome::Failure);
            assert!(stdout.is_empty());
            assert!(err.contains("letter.typ"), "{err}");
        }
    }
}

#[test]
fn limits_fail_before_replacing_pdf_and_stdout_failure_keeps_committed_pdf() {
    let root = assert_ok!(tempfile::tempdir());
    assert_ok!(std::fs::write(root.path().join("neutral.md"), SOURCE));
    assert_ok!(std::fs::create_dir(root.path().join("out")));
    let target = root.path().join("out/neutral-letter.pdf");
    assert_ok!(std::fs::write(&target, b"prior PDF"));
    for mode in ["pdf", "both"] {
        let mut stdout = Vec::new();
        assert_eq!(
            run(
                root.path(),
                &["--output-format", mode, "--max-length", "0"],
                &mut stdout
            )
            .0,
            Outcome::Failure
        );
        assert!(stdout.is_empty());
        assert_eq!(assert_ok!(std::fs::read(&target)), b"prior PDF");
    }
    struct Fail;
    impl io::Write for Fail {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let (result, err) = run(root.path(), &["--output-format", "both"], &mut Fail);
    assert_eq!(result, Outcome::Failure);
    assert!(err.contains("PDF remains"), "{err}");
    assert_eq!(common::page_texts(&assert_ok!(std::fs::read(&target))).len(), 1);
}

#[test]
fn page_compile_and_destination_failures_preserve_prior_output() {
    let root = assert_ok!(tempfile::tempdir());
    let target = root.path().join("out/neutral-letter.pdf");
    assert_ok!(std::fs::create_dir(root.path().join("out")));
    assert_ok!(std::fs::write(&target, b"prior"));
    let long = SOURCE.replace(
        "My distinctive letter paragraph.",
        &"A paragraph with enough words to fill a page. ".repeat(300),
    );
    assert_ok!(std::fs::write(root.path().join("neutral.md"), long));
    for extra in [
        vec!["--output-format", "both"],
        vec!["--output-format", "both", "--dry-run"],
    ] {
        let mut stdout = Vec::new();
        let (result, err) = run(root.path(), &extra, &mut stdout);
        assert_eq!(result, Outcome::Failure);
        assert!(err.contains("page-limit-exceeded"), "{err}");
        assert!(stdout.is_empty());
        assert_eq!(assert_ok!(std::fs::read(&target)), b"prior");
    }
    assert_eq!(
        run(
            root.path(),
            &["--output-format", "pdf", "--max-pages", "20"],
            &mut Vec::new()
        )
        .0,
        Outcome::Success
    );
    assert!(common::page_texts(&assert_ok!(std::fs::read(&target))).len() > 1);
    assert_ok!(std::fs::write(root.path().join("neutral.md"), SOURCE));
    assert_ok!(std::fs::write(&target, b"prior"));
    let styles = root.path().join("styles");
    assert_ok!(std::fs::create_dir_all(styles.join("broken")));
    assert_ok!(std::fs::write(styles.join("broken/style.typ"), ""));
    assert_ok!(std::fs::write(
        styles.join("broken/letter.typ"),
        "#panic(\"broken letter\")"
    ));
    let mut stdout = Vec::new();
    let (result, err) = run(
        root.path(),
        &[
            "--output-format",
            "both",
            "--styles-dir",
            assert_some!(styles.to_str()),
            "--style",
            "broken",
        ],
        &mut stdout,
    );
    assert_eq!(result, Outcome::Failure);
    assert!(err.contains("broken letter"));
    assert!(stdout.is_empty());
    assert_eq!(assert_ok!(std::fs::read(&target)), b"prior");
    let mut permissions = assert_ok!(std::fs::metadata(&target)).permissions();
    permissions.set_readonly(true);
    assert_ok!(std::fs::set_permissions(&target, permissions));
    let (result, err) = run(root.path(), &["--output-format", "both"], &mut stdout);
    assert_eq!(result, Outcome::Failure);
    assert!(err.contains("read-only"));
    assert!(stdout.is_empty());
    assert_eq!(assert_ok!(std::fs::read(&target)), b"prior");
}

#[test]
fn metadata_is_neutral_and_optional_fields_can_be_absent() {
    let ts = Typesetter::new(common::bundled_fonts());
    let style = assert_ok!(StyleSource::builtin(BuiltinStyle::Pragmatic));
    let mut first = letter();
    first.contact.clear();
    first.date = None;
    first.recipient = None;
    let mut second = first.clone();
    second.company = Some(assert_ok!(FieldValue::try_new("Other Company")));
    let mut metadata = Vec::new();
    for letter in [first, second] {
        let pdf =
            assert_ok!(assert_ok!(ts.compile_letter(&letter, &style, RenderOptions::default())).export_pdf("neutral"));
        let text = common::page_texts(&pdf).join(" ");
        assert!(!text.contains("February") && !text.contains("alex@example.invalid"));
        let doc = assert_ok!(lopdf::Document::load_mem(&pdf));
        let info_id = assert_ok!(assert_ok!(doc.trailer.get(b"Info")).as_reference());
        let info = assert_ok!(doc.get_dictionary(info_id));
        let title = assert_ok!(lopdf::decode_text_string(assert_ok!(info.get(b"Title"))));
        let author = assert_ok!(lopdf::decode_text_string(assert_ok!(info.get(b"Author"))));
        assert_eq!(title, "Alex Example - Cover letter");
        assert_eq!(author, "Alex Example");
        metadata.push((title, author));
    }
    assert_eq!(metadata[0], metadata[1]);
}

#[test]
fn exported_letter_style_and_explicit_render_defaults() {
    let root = assert_ok!(tempfile::tempdir());
    let style = assert_ok!(StyleSource::builtin(BuiltinStyle::Pragmatic));
    let files = assert_ok!(style.files());
    assert!(files.iter().any(|(path, _)| path == Path::new("pragmatic/letter.typ")));
    for (path, bytes) in files {
        let path = root.path().join(path);
        assert_ok!(std::fs::create_dir_all(assert_some!(path.parent())));
        assert_ok!(std::fs::write(path, bytes));
    }
    let copied = assert_ok!(StyleSource::directory(root.path(), style.name().clone()));
    let ts = Typesetter::new(common::bundled_fonts());
    let a =
        assert_ok!(assert_ok!(ts.compile_letter(&letter(), &style, RenderOptions::default())).export_pdf("neutral"));
    let b =
        assert_ok!(assert_ok!(ts.compile_letter(&letter(), &copied, RenderOptions::default())).export_pdf("neutral"));
    assert_eq!(a, b);
    for explicit in [false, true] {
        let mut args = vec!["resume", "letter", "neutral.md"];
        if explicit {
            args.extend([
                "--style",
                "pragmatic",
                "--paper",
                "letter",
                "--max-pages",
                "1",
                "--out",
                "out",
            ]);
        }
        let cli = assert_ok!(Cli::try_parse_validated(args));
        let resume::cli::Command::Letter(args) = cli.command else {
            panic!("letter command")
        };
        assert_eq!(args.render.style.is_some(), explicit);
        assert_eq!(args.render.paper.is_some(), explicit);
        assert_eq!(args.render.max_pages.is_some(), explicit);
        assert_eq!(args.out.is_some(), explicit);
        assert_eq!(
            args.effective_render().max_pages,
            Some(assert_ok!(resume::model::options::PageLimit::try_new(1)))
        );
    }
}

#[test]
fn letter_entry_enforces_package_and_cross_payload_sandbox() {
    let root = assert_ok!(tempfile::tempdir());
    assert_ok!(std::fs::create_dir(root.path().join("s")));
    assert_ok!(std::fs::write(root.path().join("s/style.typ"), ""));
    assert_ok!(std::fs::write(
        root.path().join("resume.json"),
        "\"stale private resume\""
    ));
    let style = assert_ok!(StyleSource::directory(
        root.path(),
        assert_ok!(resume::model::StyleName::try_new("s"))
    ));
    let ts = Typesetter::new(common::bundled_fonts());
    for source in ["#json(\"/resume.json\")", "#import \"@preview/example:0.1.0\": *"] {
        assert_ok!(std::fs::write(root.path().join("s/letter.typ"), source));
        assert_err!(ts.compile_letter(&letter(), &style, RenderOptions::default()));
    }
    #[cfg(unix)]
    {
        let outside = assert_ok!(tempfile::NamedTempFile::new());
        assert_ok!(std::fs::remove_file(root.path().join("s/letter.typ")));
        assert_ok!(std::os::unix::fs::symlink(
            outside.path(),
            root.path().join("s/letter.typ")
        ));
        let err = assert_err!(ts.compile_letter(&letter(), &style, RenderOptions::default()));
        assert!(matches!(
            err,
            resume::error::Error::StyleFile {
                source: typst::diag::FileError::AccessDenied,
                ..
            }
        ));
    }
}

#[test]
fn both_product_matches_text_and_pdf_is_committed_at_first_stdout_write() {
    let root = assert_ok!(tempfile::tempdir());
    assert_ok!(std::fs::write(root.path().join("neutral.md"), SOURCE));
    let mut expected = Vec::new();
    assert_eq!(run(root.path(), &[], &mut expected).0, Outcome::Success);
    let target = root.path().join("out").join("neutral-letter.pdf");
    assert_ok!(std::fs::create_dir(root.path().join("out")));
    struct Observe<'a> {
        target: &'a Path,
        bytes: Vec<u8>,
        fail_write: bool,
        fail_flush: bool,
    }
    impl io::Write for Observe<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let text = common::page_texts(&assert_ok!(std::fs::read(self.target))).join(" ");
            for paragraph in ["I seek Architect at Acme.", "My distinctive letter paragraph."] {
                assert_eq!(text.matches(paragraph).count(), 1);
            }
            if self.fail_write {
                return Err(io::ErrorKind::BrokenPipe.into());
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            if self.fail_flush {
                Err(io::ErrorKind::BrokenPipe.into())
            } else {
                Ok(())
            }
        }
    }
    for (fail_write, fail_flush) in [(false, false), (true, false), (false, true)] {
        assert_ok!(std::fs::write(&target, b"prior PDF"));
        let mut observer = Observe {
            target: &target,
            bytes: Vec::new(),
            fail_write,
            fail_flush,
        };
        let (result, err) = run(root.path(), &["--output-format", "both", "--drafts"], &mut observer);
        if fail_write || fail_flush {
            assert_eq!(result, Outcome::Failure);
            assert!(err.contains("PDF remains"));
            assert!(err.contains(assert_some!(target.to_str())));
        } else {
            assert_eq!(result, Outcome::Success);
            assert_eq!(observer.bytes, expected);
        }
    }
}

#[test]
fn empty_selected_body_has_a_located_error_before_output() {
    let root = assert_ok!(tempfile::tempdir());
    let target = root.path().join("out").join("neutral-letter.pdf");
    assert_ok!(std::fs::create_dir(root.path().join("out")));
    assert_ok!(std::fs::write(&target, b"prior"));
    for comment in ["<!-- @draft -->", "<!-- =body.default @draft -->"] {
        let source = SOURCE.replace("I seek {{title}} at {{company}}.\n\n", "").replace(
            "My distinctive letter paragraph.",
            &format!("My distinctive letter paragraph. {comment}"),
        );
        let start = assert_some!(source.find("My distinctive letter paragraph."));
        assert_ok!(std::fs::write(root.path().join("neutral.md"), &source));
        let mut stdout = Vec::new();
        let (outcome, _) = run(
            root.path(),
            &["--output-format", "pdf", "--format", "json"],
            &mut stdout,
        );
        assert_eq!(outcome, Outcome::Failure);
        let records: Vec<serde_json::Value> = String::from_utf8_lossy(&stdout)
            .lines()
            .map(|line| assert_ok!(serde_json::from_str(line)))
            .collect();
        let finding = assert_some!(
            records.iter().find(|record| record["code"] == "letter-body-empty"),
            "{records:?}"
        );
        assert_eq!(finding["severity"], "error");
        let prefix = &source[..start];
        let line = prefix.bytes().filter(|&byte| byte == b'\n').count() + 1;
        let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
        assert_eq!(finding["line"], line);
        assert_eq!(finding["column"], column);
        assert_eq!(finding["end_line"], line);
        assert_eq!(
            finding["end_column"],
            column + "My distinctive letter paragraph. ".chars().count() + comment.chars().count()
        );
        assert_eq!(finding["file"], "neutral.md");
        for extra in [
            vec!["--output-format", "text"],
            vec!["--output-format", "both"],
            vec!["--output-format", "both", "--dry-run"],
        ] {
            let mut stdout = Vec::new();
            let (outcome, stderr) = run(root.path(), &extra, &mut stdout);
            assert_eq!(outcome, Outcome::Failure);
            assert!(stdout.is_empty());
            assert!(stderr.contains("letter-body-empty"), "{stderr}");
            assert_eq!(assert_ok!(std::fs::read(&target)), b"prior");
        }
    }
    let source = SOURCE
        .replace("I seek {{title}} at {{company}}.\n\n", "")
        .replace("Dear {{recipient}},", "Dear {{recipient}}, <!-- =greeting.default -->")
        .replace(
            "My distinctive letter paragraph.",
            "My distinctive letter paragraph. <!-- @draft -->",
        );
    assert_ok!(std::fs::write(root.path().join("neutral.md"), source));
    let input = root.path().join("neutral.md");
    let cli = assert_ok!(Cli::try_parse_validated([
        "resume",
        "letter",
        assert_some!(input.to_str()),
        "--format",
        "json",
        "--output-format",
        "pdf"
    ]));
    let mut stdout = Vec::new();
    let mut stderr = Buffer::no_color();
    let outcome = commands::run(
        &cli,
        &Env {
            cwd: root.path().to_owned(),
            resume_font_path: None,
        },
        &mut Io {
            stdout: &mut stdout,
            stderr: &mut stderr,
        },
    );
    assert_eq!(outcome, Outcome::Failure);
    let records: Vec<serde_json::Value> = String::from_utf8_lossy(&stdout)
        .lines()
        .map(|line| assert_ok!(serde_json::from_str(line)))
        .collect();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["type"], "letter-selection");
    assert_eq!(records[1]["code"], "letter-body-empty");
    assert_eq!(records[1]["line"], 11);
    assert_eq!(records[1]["column"], 1);
    assert_eq!(assert_ok!(std::fs::read(&target)), b"prior");
}

#[test]
fn pdf_json_findings_and_pdf_only_dry_run_are_not_text_products() {
    let root = assert_ok!(tempfile::tempdir());
    assert_ok!(std::fs::write(root.path().join("neutral.md"), SOURCE));
    let mut stdout = Vec::new();
    let (result, err) = run(root.path(), &["--output-format", "pdf", "--dry-run"], &mut stdout);
    assert_eq!(result, Outcome::Success);
    assert!(err.contains("would write"));
    assert!(!err.contains("would print"));
    assert!(stdout.is_empty());
    assert!(!root.path().join("out").exists());
    assert_ok!(std::fs::create_dir(root.path().join("out")));
    let target = root.path().join("out/neutral-letter.pdf");
    assert_ok!(std::fs::write(&target, b"prior"));
    let (result, _) = run(
        root.path(),
        &["--output-format", "pdf", "--format", "json", "--max-length", "0"],
        &mut stdout,
    );
    assert_eq!(result, Outcome::Failure);
    let records: Vec<serde_json::Value> = String::from_utf8_lossy(&stdout)
        .lines()
        .map(|line| assert_ok!(serde_json::from_str(line)))
        .collect();
    assert!(
        records.iter().any(|record| record["code"] == "text-limit-exceeded"),
        "{records:?}"
    );
    assert_eq!(assert_ok!(std::fs::read(&target)), b"prior");
}

#[test]
fn pdf_field_strings_remain_literal_and_metadata_ignores_application_fields() {
    let ts = Typesetter::new(common::bundled_fonts());
    let style = assert_ok!(StyleSource::builtin(BuiltinStyle::Pragmatic));
    for (company, title, recipient, date) in [
        (
            "*Acme* {{title}} #panic(\"literal\")",
            "Architect",
            "Team",
            "2028-02-29",
        ),
        ("Other Company", "Manager", "Hiring Committee", "2029-03-04"),
    ] {
        let source = assert_some!(resume::markdown::parse_letter("neutral.md", SOURCE).into_parts().0);
        let fields = LetterFields {
            company: Some(assert_ok!(FieldValue::try_new(company))),
            title: Some(assert_ok!(FieldValue::try_new(title))),
            recipient: Some(assert_ok!(FieldValue::try_new(recipient))),
            date: Some(assert_ok!(date.parse())),
        };
        let letter = assert_ok!(letter::resolve(
            assert_ok!(SelectedLetter::try_from(source)),
            &fields,
            Path::new("neutral.md")
        ));
        let pdf =
            assert_ok!(assert_ok!(ts.compile_letter(&letter, &style, RenderOptions::default())).export_pdf("neutral"));
        let text = common::page_texts(&pdf).join(" ");
        assert!(text.contains(company), "{text}");
        assert!(text.contains(title));
        assert!(text.contains(recipient));
        let doc = assert_ok!(lopdf::Document::load_mem(&pdf));
        let info = assert_ok!(doc.get_dictionary(assert_ok!(assert_ok!(doc.trailer.get(b"Info")).as_reference())));
        assert_eq!(
            assert_ok!(lopdf::decode_text_string(assert_ok!(info.get(b"Title")))),
            "Alex Example - Cover letter"
        );
        assert_eq!(
            assert_ok!(lopdf::decode_text_string(assert_ok!(info.get(b"Author")))),
            "Alex Example"
        );
    }
}

#[test]
fn dry_run_and_json_pdf_and_schema_validation() {
    let root = assert_ok!(tempfile::tempdir());
    assert_ok!(std::fs::write(root.path().join("neutral.md"), SOURCE));
    let mut stdout = Vec::new();
    assert_eq!(
        run(root.path(), &["--output-format", "both", "--dry-run"], &mut stdout).0,
        Outcome::Success
    );
    assert!(stdout.is_empty());
    assert!(!root.path().join("out").exists());
    assert_eq!(
        run(
            root.path(),
            &["--output-format", "pdf", "--format", "json"],
            &mut stdout
        )
        .0,
        Outcome::Success
    );
    for line in String::from_utf8_lossy(&stdout).lines() {
        assert_ok!(serde_json::from_str::<serde_json::Value>(line));
    }
    let mut bad = letter();
    bad.schema = 2;
    let style = assert_ok!(StyleSource::builtin(BuiltinStyle::Pragmatic));
    assert_err!(Typesetter::new(common::bundled_fonts()).compile_letter(&bad, &style, RenderOptions::default()));
}
