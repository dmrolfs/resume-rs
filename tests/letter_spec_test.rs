//! ABOUTME: In-process tests of `resume letter --spec` (rsm-vqr.5): a position spec drives one reproducible letter.
//! ABOUTME: They run the `letter-scopes` fixture through the real pipeline and read what a user would read.

use std::fs;
use std::path::{Path, PathBuf};

use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;
use resume::commands::Outcome;

mod common;

#[path = "common/workspace.rs"]
mod workspace;
use workspace::{Run, Workspace};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/specs")
}

fn copy_tree(from: &Path, to: &Path) {
    assert_ok!(fs::create_dir_all(to));
    for entry in assert_ok!(fs::read_dir(from)) {
        let entry = assert_ok!(entry);
        let target = to.join(entry.file_name());
        if assert_ok!(entry.file_type()).is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            assert_ok!(fs::copy(entry.path(), target));
        }
    }
}

/// The `letter-scopes` fixture and the baseline it names, laid out as a user lays them out.
struct Staged {
    spec: PathBuf,
    root: PathBuf,
}

fn stage(work: &Workspace) -> Staged {
    assert_ok!(fs::copy(
        fixtures().join("baseline.md"),
        work.path().join("baseline.md")
    ));
    let root = work.path().join("letter-scopes");
    copy_tree(&fixtures().join("letter-scopes"), &root);
    Staged {
        spec: root.join("positions/acme.toml"),
        root,
    }
}

/// `resume letter --spec <spec> --fonts bundled --out <out> <extra>`.
fn letter_spec(work: &Workspace, spec: &Path, out: &Path, extra: &[&str]) -> Run {
    let mut args: Vec<String> = ["letter", "--spec"].map(String::from).into();
    args.push(spec.display().to_string());
    args.extend(["--fonts", "bundled", "--out"].map(String::from));
    args.push(out.display().to_string());
    args.extend(extra.iter().map(|a| (*a).to_string()));
    work.run(&args)
}

fn build_spec(work: &Workspace, spec: &Path, out: &Path, extra: &[&str]) -> Run {
    let mut args: Vec<String> = ["build", "--spec"].map(String::from).into();
    args.push(spec.display().to_string());
    args.extend(["--fonts", "bundled", "--out"].map(String::from));
    args.push(out.display().to_string());
    args.extend(extra.iter().map(|a| (*a).to_string()));
    work.run(&args)
}

fn pdf_in(out: &Path, name: &str) -> Vec<u8> {
    assert_ok!(fs::read(out.join(name)), "{name} in {}", out.display())
}

fn page_size(pdf: &[u8]) -> (f64, f64) {
    let doc = assert_ok!(lopdf::Document::load_mem(pdf));
    let (_, id) = assert_some!(doc.get_pages().into_iter().next());
    let page = assert_ok!(doc.get_dictionary(id));
    let media = assert_ok!(assert_ok!(page.get(b"MediaBox")).as_array());
    let number = |i: usize| f64::from(assert_ok!(media[i].as_float()));
    (number(2) - number(0), number(3) - number(1))
}

/// The line `needle` is on, so a failure shows the report and not only that a word is missing.
fn has(run: &Run, needle: &str) {
    assert!(run.stderr.contains(needle), "no `{needle}` in:\n{}", run.stderr);
}

/// A standalone spec (no profiles) over the fixture's letter and the baseline, with `letter` as its letter table.
fn spec_with_letter(work: &Workspace, name: &str, letter: &str) -> PathBuf {
    let dir = work.path().join(name);
    assert_ok!(fs::create_dir_all(dir.join("positions")));
    assert_ok!(fs::create_dir_all(dir.join("letters")));
    assert_ok!(fs::copy(
        fixtures().join("baseline.md"),
        work.path().join("baseline.md")
    ));
    assert_ok!(fs::copy(
        fixtures().join("letter-scopes/letters/base.md"),
        dir.join("letters/base.md")
    ));
    let path = dir.join("positions/acme.toml");
    assert_ok!(fs::write(
        &path,
        format!(
            "[position]\ncompany = \"Acme\"\ntitle = \"Principal Architect\"\n\n[build]\ncontent = \"../../baseline.md\"\n\n{letter}"
        )
    ));
    path
}

// ============================================================================
// What a letter spec says
// ============================================================================

#[test]
fn a_letter_spec_prints_the_selected_text_and_the_pdf_it_names() {
    let work = Workspace::new();
    let staged = stage(&work);
    let run = letter_spec(&work, &staged.spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    // Picks from the profiles and the spec: opening = warm (spec), evidence = short (ai-platform), summary =
    // brief (principal). The letter text is the only thing on stdout.
    assert_eq!(
        run.stdout,
        "Dear Hiring Team,\n\nIt would be a pleasure to join. \n\nMy platform work is brief. \n\n\
         I build things in few words. \n\nRegards,\nAda Lovelace\n"
    );
    let pdf = pdf_in(&work.out(), "acme-letter.pdf");
    // `paper = "a4"` in `[letter]`, and no flag typed, so a default can never have replaced it.
    let (w, h) = page_size(&pdf);
    assert!((w - 595.0).abs() < 2.0 && (h - 842.0).abs() < 2.0, "{w} x {h}");
}

#[test]
fn the_summary_names_the_spec_the_chain_the_letter_and_where_each_setting_came_from() {
    let work = Workspace::new();
    let staged = stage(&work);
    let run = letter_spec(&work, &staged.spec, &work.out(), &[]);
    let sep = std::path::MAIN_SEPARATOR;
    let file = format!("letter-scopes{sep}positions{sep}acme.toml");
    let lines: Vec<&str> = run
        .stderr
        .lines()
        .filter(|l| l.starts_with("  ") && !l.starts_with("  fonts"))
        .collect();
    assert!(!lines.is_empty(), "no spec lines in:\n{}", run.stderr);
    assert_eq!(
        *assert_some!(lines.first()),
        format!(
            "  spec: letter-scopes{0}positions{0}acme.toml",
            std::path::MAIN_SEPARATOR
        )
    );
    assert!(lines.contains(&"  position: Acme; Principal Architect"), "{lines:?}");
    assert!(
        lines.contains(&"  profiles: principal -> ai-platform -> the spec"),
        "{lines:?}"
    );
    let settings = assert_some!(lines.iter().find(|l| l.starts_with("  settings: ")));
    for part in [
        format!("output both ({file}:22:17)"),
        format!("links text-only ({file}:25:9)"),
        format!("max-length 2000 ({file}:23:14)"),
        format!("length-unit scalars ({file}:24:15)"),
        format!("style pragmatic ({file}:20:9)"),
        format!("paper a4 ({file}:21:9)"),
    ] {
        assert!(settings.contains(&part), "{part} in {settings}");
    }
    let fields = assert_some!(lines.iter().find(|l| l.starts_with("  fields: ")));
    assert!(
        fields.contains("company Acme (") && fields.contains("recipient Hiring Team ("),
        "{fields}"
    );
    assert!(fields.contains("date 2026-11-19 ("), "{fields}");
}

#[test]
fn json_output_carries_a_letter_spec_record_only_when_a_pdf_leaves_stdout_free() {
    let work = Workspace::new();
    let staged = stage(&work);
    let run = letter_spec(
        &work,
        &staged.spec,
        &work.out(),
        &["--output-format", "pdf", "--allow-overrides", "--format", "json"],
    );
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let record = assert_some!(run.stdout.lines().find(|l| l.contains("\"type\":\"letter-spec\"")));
    let value: serde_json::Value = assert_ok!(serde_json::from_str(record));
    let keys: Vec<&str> = value
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    // `serde_json` keeps keys sorted, so the set is compared, not the order the record is written in.
    let mut expected = vec![
        "type",
        "file",
        "position",
        "chain",
        "content",
        "output",
        "links",
        "max_length",
        "length_unit",
        "fields",
        "style",
        "paper",
        "drafts",
        "pdf",
        "overrides",
        "environment",
        "version",
    ];
    expected.sort_unstable();
    assert_eq!(keys, expected, "every key is always there");
    assert_eq!(value["output"]["value"], "pdf");
    assert_eq!(value["output"]["from"], "flag");
    assert!(assert_some!(value["pdf"].as_str()).ends_with("acme-letter.pdf"));
    // A letter that prints text cannot share stdout with JSON.
    let text = letter_spec(&work, &staged.spec, &work.out(), &["--format", "json"]);
    assert_eq!(text.outcome, Outcome::Failure);
    assert_eq!(text.stdout, "");
    has(&text, "--format json cannot share stdout with letter text");
}

#[test]
fn json_with_a_spec_that_prints_text_is_an_error_before_anything_is_read_and_a_typed_pdf_is_fine() {
    let work = Workspace::new();
    let spec = spec_with_letter(
        &work,
        "texty",
        "[letter]\ncontent = \"../letters/base.md\"\nrecipient = \"T\"\n",
    );
    let refused = letter_spec(&work, &spec, &work.out(), &["--format", "json"]);
    assert_eq!(refused.outcome, Outcome::Failure);
    assert_eq!(refused.stdout, "");
    has(&refused, "--format json cannot share stdout with letter text");
    let allowed = letter_spec(
        &work,
        &spec,
        &work.out(),
        &["--format", "json", "--allow-overrides", "--output-format", "pdf"],
    );
    assert_eq!(allowed.outcome, Outcome::Success, "{}", allowed.stderr);
}

// ============================================================================
// Flags beside a spec
// ============================================================================

#[test]
fn a_flag_beside_a_spec_is_refused_naming_every_one() {
    let work = Workspace::new();
    let staged = stage(&work);
    let args: Vec<String> = ["letter", "--spec"]
        .map(String::from)
        .into_iter()
        .chain([staged.spec.display().to_string()])
        .chain(["--links", "text-and-url", "--recipient", "Dana", "--date", "2029-01-01"].map(String::from))
        .collect();
    let err = assert_err_text(&args);
    for flag in ["--links", "--date", "--recipient"] {
        assert!(err.contains(flag), "{flag}: {err}");
    }
    assert!(err.contains("--allow-overrides"), "{err}");
}

fn assert_err_text(args: &[String]) -> String {
    let error = claims::assert_err!(resume::cli::Cli::try_parse_validated(
        std::iter::once("resume".to_string()).chain(args.iter().cloned())
    ));
    assert_eq!(error.exit_code(), 2);
    error.to_string()
}

#[test]
fn every_machine_flag_and_drafts_are_allowed_beside_a_spec_without_overrides() {
    let work = Workspace::new();
    let staged = stage(&work);
    let ok = letter_spec(&work, &staged.spec, &work.out(), &["--drafts", "--max-pages", "1"]);
    assert_eq!(ok.outcome, Outcome::Success, "{}", ok.stderr);
    assert!(!ok.stderr.contains("plus overrides"), "{}", ok.stderr);
    assert!(ok.stderr.contains("with drafts"), "{}", ok.stderr);
    assert!(work.out().join("acme-letter.drafts.pdf").is_file());
    assert!(!work.out().join("acme-letter.pdf").exists());
}

#[test]
fn allow_overrides_lists_each_flag_with_the_value_and_the_line_it_replaced() {
    let work = Workspace::new();
    let staged = stage(&work);
    let run = letter_spec(
        &work,
        &staged.spec,
        &work.out(),
        &[
            "--allow-overrides",
            "--links",
            "text-and-url",
            "--recipient",
            "Dana",
            "--tags",
            "platform",
            "--company",
            "Other",
        ],
    );
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    has(&run, "  spec: ");
    has(&run, ", plus overrides");
    // The tag override names the letter's lines (profiles), never the resume's `[select]`.
    has(&run, "override: --tags platform replaces tags ai, platform (");
    assert!(
        run.stderr.contains("profiles") && run.stderr.contains("ai-platform.toml:5:9"),
        "{}",
        run.stderr
    );
    has(&run, "override: --links text-and-url replaces links text-only (");
    has(&run, "override: --recipient Dana replaces recipient Hiring Team (");
    has(&run, "override: --company Other replaces company Acme");
    // The value typed is the one used.
    assert!(run.stdout.starts_with("Dear Dana,\n"), "{}", run.stdout);
}

#[test]
fn json_lists_each_override_with_what_it_set_and_replaced() {
    let work = Workspace::new();
    let staged = stage(&work);
    let run = letter_spec(
        &work,
        &staged.spec,
        &work.out(),
        &[
            "--allow-overrides",
            "--output-format",
            "pdf",
            "--paper",
            "letter",
            "--format",
            "json",
        ],
    );
    let record = assert_some!(run.stdout.lines().find(|l| l.contains("\"type\":\"letter-spec\"")));
    let value: serde_json::Value = assert_ok!(serde_json::from_str(record));
    let overrides = assert_some!(value["overrides"].as_array());
    let flags: Vec<&str> = overrides.iter().map(|o| o["flag"].as_str().expect("flag")).collect();
    assert_eq!(flags, ["--paper", "--output-format"]);
    assert_eq!(overrides[0]["set"], serde_json::json!(["letter"]));
    assert_eq!(overrides[0]["replaced"]["setting"], "paper");
    assert_eq!(overrides[0]["replaced"]["value"], serde_json::json!(["a4"]));
    assert_eq!(value["paper"]["value"], "letter");
    assert_eq!(value["paper"]["from"], "flag");
}

#[test]
fn print_spec_with_a_flag_is_refused_and_without_one_prints_the_same_toml_build_does() {
    let work = Workspace::new();
    let staged = stage(&work);
    let args: Vec<String> = [
        "letter",
        "--spec",
        &staged.spec.display().to_string(),
        "--print-spec",
        "--links",
        "text-only",
    ]
    .map(String::from)
    .into();
    assert!(assert_err_text(&args).contains("--print-spec"));
    let letter = work.run(&["letter", "--spec", &staged.spec.display().to_string(), "--print-spec"].map(String::from));
    let build = work.run(&["build", "--spec", &staged.spec.display().to_string(), "--print-spec"].map(String::from));
    assert_eq!(letter.outcome, Outcome::Success);
    assert_eq!(
        letter.stdout, build.stdout,
        "one spec prints one way, whichever command asks"
    );
    assert!(letter.stdout.contains("[letter.select]"));
    assert!(!work.out().exists(), "nothing is built");
}

// ============================================================================
// A spec and its flags say the same thing
// ============================================================================

#[test]
fn a_spec_without_picks_equals_the_same_letter_built_from_flags_byte_for_byte() {
    let work = Workspace::new();
    let spec = spec_with_letter(
        &work,
        "nopicks",
        "[letter]\ncontent = \"../letters/base.md\"\nrecipient = \"Hiring Team\"\ndate = 2026-11-19\npaper = \"a4\"\n\
         output-format = \"both\"\nmax-length = 2000\nlength-unit = \"scalars\"\nlinks = \"text-only\"\n\
         [letter.select]\ntags = [\"platform\"]\nprefer = [\"formal\", \"short\", \"warm\"]\n",
    );
    let by_spec = work.path().join("by-spec");
    let from_spec = letter_spec(&work, &spec, &by_spec, &[]);
    assert_eq!(from_spec.outcome, Outcome::Success, "{}", from_spec.stderr);
    // The same letter under the spec's stem (`acme`), as the PDF's identity is the stem.
    let copy = work.path().join("flags");
    assert_ok!(fs::create_dir_all(&copy));
    assert_ok!(fs::copy(
        work.path().join("nopicks/letters/base.md"),
        copy.join("acme-letter.md")
    ));
    let by_flags = work.path().join("by-flags");
    let flags: Vec<String> = [
        "letter",
        &copy.join("acme-letter.md").display().to_string(),
        "--fonts",
        "bundled",
        "--out",
        &by_flags.display().to_string(),
        "--company",
        "Acme",
        "--title",
        "Principal Architect",
        "--recipient",
        "Hiring Team",
        "--date",
        "2026-11-19",
        "--paper",
        "a4",
        "--output-format",
        "both",
        "--max-length",
        "2000",
        "--length-unit",
        "scalars",
        "--links",
        "text-only",
        "--tags",
        "platform",
        "--prefer",
        "formal,short,warm",
    ]
    .map(String::from)
    .into();
    let from_flags = work.run(&flags);
    assert_eq!(from_flags.outcome, Outcome::Success, "{}", from_flags.stderr);
    assert_eq!(from_spec.stdout, from_flags.stdout, "the text is the same");
    // A file named `acme-letter.md` has the stem `acme-letter`, which is the identity a spec letter gets, so the
    // two PDFs are the same bytes: a spec says nothing the flags cannot, and adds nothing to the file.
    assert_eq!(
        pdf_in(&by_spec, "acme-letter.pdf"),
        pdf_in(&by_flags, "acme-letter-letter.pdf"),
        "the PDF is the same bytes"
    );
}

// ============================================================================
// Reproducible
// ============================================================================

#[test]
fn two_letter_spec_builds_are_byte_identical_in_text_and_pdf() {
    let work = Workspace::new();
    let staged = stage(&work);
    let (a, b) = (work.path().join("one"), work.path().join("two"));
    let first = letter_spec(&work, &staged.spec, &a, &[]);
    let second = letter_spec(&work, &staged.spec, &b, &[]);
    assert_eq!((first.outcome, second.outcome), (Outcome::Success, Outcome::Success));
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(pdf_in(&a, "acme-letter.pdf"), pdf_in(&b, "acme-letter.pdf"));
}

#[test]
fn the_resume_and_the_letter_of_one_spec_are_two_files_and_two_documents() {
    let work = Workspace::new();
    let staged = stage(&work);
    let out = work.out();
    assert_eq!(build_spec(&work, &staged.spec, &out, &[]).outcome, Outcome::Success);
    assert_eq!(letter_spec(&work, &staged.spec, &out, &[]).outcome, Outcome::Success);
    let (resume, letter) = (pdf_in(&out, "acme.pdf"), pdf_in(&out, "acme-letter.pdf"));
    assert_ne!(resume, letter);
    // One `--out` holds both, and the two do not share a document identity: the first half of the trailer `/ID`.
    let id = |pdf: &[u8]| -> Vec<u8> {
        let doc = assert_ok!(lopdf::Document::load_mem(pdf));
        let ids = assert_ok!(assert_ok!(doc.trailer.get(b"ID")).as_array());
        assert_ok!(ids[0].as_str()).to_vec()
    };
    assert_ne!(id(&resume), id(&letter), "the resume and its letter are two documents");
    // The control: the same letter built again has the same identity, so the comparison can tell them apart.
    let again = work.path().join("again");
    assert_eq!(letter_spec(&work, &staged.spec, &again, &[]).outcome, Outcome::Success);
    assert_eq!(id(&letter), id(&pdf_in(&again, "acme-letter.pdf")));
}

// ============================================================================
// Each command without the other's content
// ============================================================================

#[test]
fn the_letter_builds_when_the_resume_content_does_not_exist_and_gives_the_same_bytes() {
    let work = Workspace::new();
    let staged = stage(&work); // the baseline is present
    let with = letter_spec(&work, &staged.spec, &work.path().join("with"), &[]);
    assert_ok!(fs::remove_file(work.path().join("baseline.md")));
    let without = letter_spec(&work, &staged.spec, &work.path().join("without"), &[]);
    assert_eq!(without.outcome, Outcome::Success, "{}", without.stderr);
    assert_eq!(with.stdout, without.stdout);
    assert_eq!(
        pdf_in(&work.path().join("with"), "acme-letter.pdf"),
        pdf_in(&work.path().join("without"), "acme-letter.pdf")
    );
}

#[test]
fn the_letter_builds_when_the_resume_content_is_a_directory() {
    let work = Workspace::new();
    let staged = stage(&work);
    assert_ok!(fs::remove_file(work.path().join("baseline.md")));
    assert_ok!(fs::create_dir(work.path().join("baseline.md")));
    let run = letter_spec(&work, &staged.spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
}

#[test]
fn the_resume_builds_when_the_letter_content_does_not_exist_or_is_a_directory() {
    let work = Workspace::new();
    let staged = stage(&work);
    let letter = staged.root.join("letters/base.md");
    assert_ok!(fs::remove_file(&letter));
    let missing = build_spec(&work, &staged.spec, &work.path().join("a"), &[]);
    assert_eq!(missing.outcome, Outcome::Success, "{}", missing.stderr);
    assert_ok!(fs::create_dir(&letter));
    let directory = build_spec(&work, &staged.spec, &work.path().join("b"), &[]);
    assert_eq!(directory.outcome, Outcome::Success, "{}", directory.stderr);
    assert_eq!(
        pdf_in(&work.path().join("a"), "acme.pdf"),
        pdf_in(&work.path().join("b"), "acme.pdf")
    );
}

#[test]
fn a_missing_letter_names_the_spec_line_that_named_it() {
    let work = Workspace::new();
    let staged = stage(&work);
    assert_ok!(fs::remove_file(staged.root.join("letters/base.md")));
    let run = letter_spec(&work, &staged.spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    assert_eq!(run.stdout, "");
    has(&run, "acme.toml:17:11");
    has(&run, "the `content` file cannot be used");
}

// ============================================================================
// Resume baseline
// ============================================================================

#[test]
fn a_letter_table_changes_nothing_about_build_spec() {
    let work = Workspace::new();
    let with = stage(&work);
    let plain = work.path().join("plain");
    assert_ok!(fs::create_dir_all(&plain));
    assert_ok!(fs::copy(
        fixtures().join("baseline.md"),
        work.path().join("baseline.md")
    ));
    copy_tree(&fixtures().join("letter-scopes"), &plain);
    // The same spec without its letter tables, in another directory under the same name.
    let plain_spec = plain.join("positions/acme.toml");
    let text = assert_ok!(fs::read_to_string(&plain_spec));
    let cut = assert_some!(text.find("[letter]"));
    assert_ok!(fs::write(&plain_spec, &text[..cut]));
    let strip = |run: &Run| -> String {
        run.stderr
            .lines()
            .filter(|l| {
                !l.contains("built in")
                    && !l.contains("letter-scopes")
                    && !l.contains("plain")
                    && !l.contains("environment:")
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let (a, b) = (work.path().join("a"), work.path().join("b"));
    let first = build_spec(&work, &with.spec, &a, &[]);
    let second = build_spec(&work, &plain_spec, &b, &[]);
    assert_eq!((first.outcome, second.outcome), (Outcome::Success, Outcome::Success));
    assert_eq!(
        pdf_in(&a, "acme.pdf"),
        pdf_in(&b, "acme.pdf"),
        "the resume PDF is unchanged"
    );
    assert_eq!(strip(&first), strip(&second), "so is the report");
}

#[test]
fn a_malformed_letter_table_fails_build_spec_too_so_strictness_is_global() {
    let work = Workspace::new();
    let spec = spec_with_letter(
        &work,
        "typo",
        "[letter]\ncontent = \"../letters/base.md\"\nmax-length = \"ten\"\n",
    );
    let run = build_spec(&work, &spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    has(&run, "max-length");
}

// ============================================================================
// Wrong or missing table, picks, scopes
// ============================================================================

#[test]
fn a_spec_without_a_letter_table_is_an_error_naming_it_and_prints_nothing() {
    let work = Workspace::new();
    let staged = stage(&work);
    let text = assert_ok!(fs::read_to_string(&staged.spec));
    let cut = assert_some!(text.find("[letter]"));
    assert_ok!(fs::write(&staged.spec, &text[..cut]));
    let run = letter_spec(&work, &staged.spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    assert_eq!(run.stdout, "");
    has(&run, "the spec has no `[letter]` table");
    assert!(!work.out().exists());
}

#[test]
fn a_profile_that_only_holds_letter_select_does_not_make_a_letter() {
    let work = Workspace::new();
    let staged = stage(&work);
    let text = assert_ok!(fs::read_to_string(&staged.spec));
    let cut = assert_some!(text.find("[letter]"));
    assert_ok!(fs::write(&staged.spec, &text[..cut]));
    let run = letter_spec(
        &work,
        &staged.spec,
        &work.out(),
        &["--allow-overrides", "--links", "text-only"],
    );
    assert_eq!(run.outcome, Outcome::Failure);
    has(&run, "the spec has no `[letter]` table");
}

#[test]
fn a_misspelled_letter_pick_names_its_letter_line_and_does_not_fail_the_resume() {
    let work = Workspace::new();
    let spec = spec_with_letter(
        &work,
        "badpick",
        "[letter]\ncontent = \"../letters/base.md\"\nrecipient = \"T\"\n[letter.select]\npick = { opening = \"warmm\" }\n",
    );
    let letter = letter_spec(&work, &spec, &work.out(), &[]);
    assert_eq!(letter.outcome, Outcome::Failure);
    assert_eq!(letter.stdout, "");
    has(&letter, "warmm");
    has(&letter, "acme.toml:");
    assert!(!work.out().join("acme-letter.pdf").exists());
    let resume = build_spec(&work, &spec, &work.path().join("r"), &[]);
    assert_eq!(resume.outcome, Outcome::Success, "{}", resume.stderr);
}

#[test]
fn a_resume_pick_the_letter_has_no_group_for_is_not_an_error_for_the_letter() {
    let work = Workspace::new();
    let staged = stage(&work);
    // `gateway = long` is a resume pick from `principal`; the letter has no `gateway` group.
    let run = letter_spec(&work, &staged.spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert!(!run.stderr.contains("gateway"), "{}", run.stderr);
}

#[test]
fn the_same_group_name_is_picked_apart_in_the_resume_and_the_letter() {
    let work = Workspace::new();
    let staged = stage(&work);
    // `summary` is `acme` for the resume (the spec) and `brief` for the letter (principal).
    let letter = letter_spec(&work, &staged.spec, &work.out(), &[]);
    assert!(
        letter.stdout.contains("I build things in few words."),
        "{}",
        letter.stdout
    );
    let resume = build_spec(&work, &staged.spec, &work.path().join("r"), &[]);
    assert_eq!(resume.outcome, Outcome::Success, "{}", resume.stderr);
    let text = common::page_texts(&pdf_in(&work.path().join("r"), "acme.pdf")).join("\n");
    assert!(text.contains("for Acme"), "{text}");
}

#[test]
fn a_picked_draft_wording_names_the_spec_line_that_picked_it() {
    let work = Workspace::new();
    let spec = spec_with_letter(
        &work,
        "draftpick",
        "[letter]\ncontent = \"../letters/draft.md\"\nrecipient = \"T\"\n[letter.select]\npick = { evidence = \"unfinished\" }\n",
    );
    let letters = work.path().join("draftpick/letters");
    assert_ok!(fs::write(
        letters.join("draft.md"),
        "# Ada\n\n## Greeting\n\nHello,\n\n## Body\n\nReady one. <!-- =evidence.default -->\n\nUnfinished one. <!-- =evidence.unfinished @draft -->\n\n## Closing\n\nRegards,\n"
    ));
    let run = letter_spec(&work, &spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Failure, "{}", run.stderr);
    assert_eq!(run.stdout, "");
    has(&run, "chosen at");
    has(&run, "acme.toml:");
    let preview = letter_spec(&work, &spec, &work.out(), &["--drafts"]);
    assert_eq!(preview.outcome, Outcome::Success, "{}", preview.stderr);
}

#[test]
fn a_spec_that_asks_for_core_only_and_tags_in_its_letter_cannot_be_rescued_by_a_flag() {
    let work = Workspace::new();
    let spec = spec_with_letter(
        &work,
        "conflict",
        "[letter]\ncontent = \"../letters/base.md\"\nrecipient = \"T\"\n[letter.select]\ncore-only = true\ntags = [\"platform\"]\n",
    );
    let run = letter_spec(&work, &spec, &work.out(), &["--allow-overrides", "--tags", "platform"]);
    assert_eq!(run.outcome, Outcome::Failure);
    assert_eq!(run.stdout, "");
    has(&run, "`core-only` is true");
    // The control: without the conflict the same spec, flag and recipient build, so the failure above is the conflict.
    let fine = spec_with_letter(
        &work,
        "no-conflict",
        "[letter]\ncontent = \"../letters/base.md\"\nrecipient = \"T\"\n[letter.select]\ntags = [\"platform\"]\n",
    );
    let ok = letter_spec(&work, &fine, &work.out(), &["--allow-overrides", "--tags", "platform"]);
    assert_eq!(ok.outcome, Outcome::Success, "{}", ok.stderr);
}

#[test]
fn a_length_unit_beside_a_spec_counts_against_the_specs_own_limit() {
    let work = Workspace::new();
    let staged = stage(&work);
    let run = letter_spec(
        &work,
        &staged.spec,
        &work.out(),
        &["--allow-overrides", "--length-unit", "words"],
    );
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    has(&run, "override: --length-unit words replaces length-unit scalars (");
    // A limit the spec does not have cannot take a unit.
    let none = spec_with_letter(
        &work,
        "nolimit",
        "[letter]\ncontent = \"../letters/base.md\"\nrecipient = \"T\"\n",
    );
    let refused = letter_spec(
        &work,
        &none,
        &work.out(),
        &["--allow-overrides", "--length-unit", "words"],
    );
    assert_eq!(refused.outcome, Outcome::Failure);
    has(&refused, "--length-unit needs a max-length");
}

#[test]
fn a_tighter_limit_typed_with_overrides_stops_a_letter_the_spec_would_pass() {
    let work = Workspace::new();
    let staged = stage(&work);
    let run = letter_spec(
        &work,
        &staged.spec,
        &work.out(),
        &["--allow-overrides", "--max-length", "10"],
    );
    assert_eq!(run.outcome, Outcome::Failure);
    assert_eq!(run.stdout, "", "no letter body when a limit fails");
    has(&run, "text-limit-exceeded");
}

// ============================================================================
// Names a PDF
// ============================================================================

#[test]
fn a_spec_named_like_the_letter_of_another_is_refused_by_both_commands() {
    let work = Workspace::new();
    let staged = stage(&work);
    let renamed = staged.root.join("positions/acme-letter.toml");
    assert_ok!(fs::rename(&staged.spec, &renamed));
    for run in [
        letter_spec(&work, &renamed, &work.out(), &[]),
        build_spec(&work, &renamed, &work.out(), &[]),
    ] {
        assert_eq!(run.outcome, Outcome::Failure);
        has(&run, "cannot name a PDF");
        has(&run, "-letter");
    }
}

#[test]
fn a_draft_preview_of_the_letter_never_replaces_the_letter_built_to_be_sent() {
    let work = Workspace::new();
    let staged = stage(&work);
    assert_eq!(
        letter_spec(&work, &staged.spec, &work.out(), &[]).outcome,
        Outcome::Success
    );
    let before = pdf_in(&work.out(), "acme-letter.pdf");
    assert_eq!(
        letter_spec(&work, &staged.spec, &work.out(), &["--drafts"]).outcome,
        Outcome::Success
    );
    assert_eq!(pdf_in(&work.out(), "acme-letter.pdf"), before);
    assert!(work.out().join("acme-letter.drafts.pdf").is_file());
}

#[test]
fn text_only_output_writes_no_pdf_and_does_not_need_a_renderer() {
    let work = Workspace::new();
    let spec = spec_with_letter(
        &work,
        "textonly",
        "[letter]\ncontent = \"../letters/base.md\"\nrecipient = \"T\"\nstyle = \"no-such-style\"\n",
    );
    let run = letter_spec(&work, &spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert!(!work.out().exists(), "text mode writes nothing");
    // The same style is a named error once a PDF is wanted, naming the spec line that chose it.
    let pdf = letter_spec(
        &work,
        &spec,
        &work.out(),
        &["--allow-overrides", "--output-format", "pdf"],
    );
    assert_eq!(pdf.outcome, Outcome::Failure);
    has(&pdf, "no-such-style");
    has(&pdf, "chosen at");
}

#[test]
fn the_letter_falls_back_to_the_builds_style_and_paper_only_when_a_pdf_is_made() {
    let work = Workspace::new();
    let spec = spec_with_letter(
        &work,
        "fallback",
        "[letter]\ncontent = \"../letters/base.md\"\nrecipient = \"T\"\n",
    );
    let path = work.path().join("fallback/positions/acme.toml");
    let text = assert_ok!(fs::read_to_string(&path));
    assert_ok!(fs::write(&path, text.replace("[build]\n", "[build]\npaper = \"a4\"\n")));
    let run = letter_spec(
        &work,
        &spec,
        &work.out(),
        &["--allow-overrides", "--output-format", "pdf"],
    );
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    has(&run, "paper a4 (build, ");
    let (w, _) = page_size(&pdf_in(&work.out(), "acme-letter.pdf"));
    assert!((w - 595.0).abs() < 2.0, "{w}");
    // Text mode never reports a style or a paper, which it does not use.
    let text_run = letter_spec(&work, &spec, &work.out(), &[]);
    let settings = assert_some!(text_run.stderr.lines().find(|l| l.starts_with("  settings: ")));
    assert!(
        !settings.contains("paper ") && !settings.contains("style "),
        "{settings}"
    );
}

// ============================================================================
// stdout holds the letter and nothing else
// ============================================================================

#[test]
fn stdout_is_exactly_the_letter_text_with_overrides_and_a_summary_on_stderr() {
    let work = Workspace::new();
    let staged = stage(&work);
    let run = letter_spec(
        &work,
        &staged.spec,
        &work.out(),
        &["--allow-overrides", "--links", "text-and-url"],
    );
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert!(
        run.stdout.starts_with("Dear Hiring Team,\n") && run.stdout.ends_with("Ada Lovelace\n"),
        "{}",
        run.stdout
    );
    for line in ["spec:", "override:", "settings:", "profiles:"] {
        assert!(!run.stdout.contains(line), "{line} on stdout: {}", run.stdout);
    }
    assert!(
        !run.stderr.contains("I build things"),
        "the body is never copied to stderr: {}",
        run.stderr
    );
}

#[test]
fn a_dry_run_prints_the_spec_lines_and_writes_nothing() {
    let work = Workspace::new();
    let staged = stage(&work);
    let run = letter_spec(&work, &staged.spec, &work.out(), &["--dry-run"]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert_eq!(run.stdout, "");
    has(&run, "ok would write letter PDF");
    has(&run, "  spec: ");
    assert!(!work.out().exists());
}

// ============================================================================
// What the review found was not pinned
// ============================================================================

fn json_record(run: &Run) -> serde_json::Value {
    let record = assert_some!(run.stdout.lines().find(|l| l.contains("\"type\":\"letter-spec\"")));
    assert_ok!(serde_json::from_str(record))
}

#[test]
fn a_style_override_names_the_letter_style_line_it_replaced() {
    let work = Workspace::new();
    let staged = stage(&work);
    let run = letter_spec(
        &work,
        &staged.spec,
        &work.out(),
        &[
            "--allow-overrides",
            "--style",
            "pragmatic",
            "--output-format",
            "pdf",
            "--format",
            "json",
        ],
    );
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let value = json_record(&run);
    let overrides = assert_some!(value["overrides"].as_array());
    let style = assert_some!(overrides.iter().find(|o| o["flag"] == "--style"));
    assert_eq!(style["set"], serde_json::json!(["pragmatic"]));
    assert_eq!(style["replaced"]["setting"], "style");
    assert_eq!(style["replaced"]["value"], serde_json::json!(["pragmatic"]));
    let at = style["at"].to_string();
    assert!(at.contains("acme.toml:20:9"), "the letter's `style =` line: {at}");
    assert_eq!(
        value["style"],
        serde_json::json!({"value": "pragmatic", "from": "flag"})
    );
}

#[test]
fn an_unknown_letter_tag_names_its_letter_select_line_and_never_the_resumes() {
    let work = Workspace::new();
    let spec = spec_with_letter(
        &work,
        "unknown",
        "[letter]\ncontent = \"../letters/base.md\"\nrecipient = \"T\"\n[letter.select]\ntags = [\"nosuchtag\"]\nprefer = [\"nosuchwording\"]\n",
    );
    let path = work.path().join("unknown/positions/acme.toml");
    let text = assert_ok!(fs::read_to_string(&path));
    // A resume `[select]` of its own, on an earlier line, so a finding that named it would be wrong.
    assert_ok!(fs::write(
        &path,
        text.replace("[letter]\n", "[select]\ntags = [\"resume-only\"]\n\n[letter]\n")
    ));
    let run = letter_spec(&work, &spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    has(&run, "warning[unknown-tag]: spec tags `nosuchtag` (");
    has(&run, "warning[unknown-wording]: spec wordings `nosuchwording` (");
    let written = assert_ok!(fs::read_to_string(&path));
    let line_of = |key: &str| 1 + assert_some!(written.lines().position(|l| l.starts_with(key)));
    has(&run, &format!("acme.toml:{}:", line_of("tags = [\"nosuchtag")));
    has(&run, &format!("acme.toml:{}:", line_of("prefer = [\"nosuchwording")));
    assert!(!run.stderr.contains("resume-only"), "{}", run.stderr);
    // A name typed as a flag is the flag's, not the spec's: even one the spec also wrote, so the flag is blamed.
    let typed = letter_spec(
        &work,
        &spec,
        &work.out(),
        &["--allow-overrides", "--tags", "nosuchtag", "--prefer", "nosuchwording"],
    );
    has(&typed, "warning[unknown-tag]: --tags `nosuchtag`");
    has(&typed, "warning[unknown-wording]: --prefer `nosuchwording`");
    assert!(!typed.stderr.contains("spec tags"), "{}", typed.stderr);
    assert!(!typed.stderr.contains("spec wordings"), "{}", typed.stderr);
}

#[test]
fn a_name_only_a_profile_wrote_is_a_note_naming_the_profile_line() {
    let work = Workspace::new();
    let staged = stage(&work);
    let run = letter_spec(&work, &staged.spec, &work.out(), &[]);
    // `ai` is written only by the ai-platform profile, and the fixture's letter has no `#ai` item.
    has(&run, "note[unknown-tag]: profile tags `ai` (");
    has(&run, "ai-platform.toml:5:9");
}

#[test]
fn an_unknown_style_the_letter_falls_back_to_names_the_build_line() {
    let work = Workspace::new();
    let spec = spec_with_letter(
        &work,
        "badbuild",
        "[letter]\ncontent = \"../letters/base.md\"\nrecipient = \"T\"\n",
    );
    let path = work.path().join("badbuild/positions/acme.toml");
    let text = assert_ok!(fs::read_to_string(&path));
    assert_ok!(fs::write(
        &path,
        text.replace("[build]\n", "[build]\nstyle = \"no-such-style\"\n")
    ));
    let run = letter_spec(
        &work,
        &spec,
        &work.out(),
        &["--allow-overrides", "--output-format", "pdf"],
    );
    assert_eq!(run.outcome, Outcome::Failure);
    has(&run, "no-such-style");
    has(&run, "chosen at");
    let written = assert_ok!(fs::read_to_string(&path));
    let line = 1 + assert_some!(written.lines().position(|l| l.starts_with("style = ")));
    has(&run, &format!("acme.toml:{line}:"));
    // Text mode never opens a style, so the same spec prints its letter.
    assert_eq!(letter_spec(&work, &spec, &work.out(), &[]).outcome, Outcome::Success);
}

#[test]
fn the_equivalence_sees_tags_links_and_limits_because_the_letter_has_them() {
    let work = Workspace::new();
    let letter = "# Ada\n\n## Greeting\n\nDear {{recipient}},\n\n## Body\n\n\
                  Kept [site](https://example.invalid/a). <!-- #platform -->\n\n\
                  Dropped by tags. <!-- #ai -->\n\n## Closing\n\nRegards,\n";
    let spec = spec_with_letter(
        &work,
        "eq",
        "[letter]\ncontent = \"../letters/tags.md\"\nrecipient = \"Team\"\nlinks = \"text-only\"\nmax-length = 500\n\
         [letter.select]\ntags = [\"platform\"]\n",
    );
    let file = work.path().join("eq/letters/tags.md");
    assert_ok!(fs::write(&file, letter));
    let from_spec = letter_spec(&work, &spec, &work.out(), &[]);
    assert_eq!(from_spec.outcome, Outcome::Success, "{}", from_spec.stderr);
    assert!(from_spec.stdout.contains("Kept site."), "{}", from_spec.stdout);
    assert!(
        !from_spec.stdout.contains("https://"),
        "links text-only: {}",
        from_spec.stdout
    );
    assert!(!from_spec.stdout.contains("Dropped by tags"), "{}", from_spec.stdout);
    let flagged: Vec<String> = [
        "letter",
        &file.display().to_string(),
        "--recipient",
        "Team",
        "--links",
        "text-only",
        "--max-length",
        "500",
        "--tags",
        "platform",
    ]
    .map(String::from)
    .into();
    assert_eq!(
        work.run(&flagged).stdout,
        from_spec.stdout,
        "the spec says what the flags say"
    );
    // The controls: each setting, changed, changes the result, so the equality above could not hold by accident.
    let link = letter_spec(
        &work,
        &spec,
        &work.out(),
        &["--allow-overrides", "--links", "text-and-url"],
    );
    assert!(link.stdout.contains("https://example.invalid/a"), "{}", link.stdout);
    let other = letter_spec(&work, &spec, &work.out(), &["--allow-overrides", "--tags", "ai"]);
    assert!(
        other.stdout.contains("Dropped by tags") && !other.stdout.contains("Kept site"),
        "{}",
        other.stdout
    );
    let tight = letter_spec(&work, &spec, &work.out(), &["--allow-overrides", "--max-length", "5"]);
    assert_eq!(tight.outcome, Outcome::Failure);
}
