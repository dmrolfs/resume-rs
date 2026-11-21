//! ABOUTME: In-process tests of `resume build --spec`: a position spec drives one reproducible, traceable build.
//! ABOUTME: They run the `two-profiles` fixture through the real pipeline and read what a user would read.

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

/// The `two-profiles` fixture and the baseline it names, laid out as a user lays them out.
struct Staged {
    spec: PathBuf,
    profiles: PathBuf,
}

fn stage(work: &Workspace) -> Staged {
    assert_ok!(fs::copy(
        fixtures().join("baseline.md"),
        work.path().join("baseline.md")
    ));
    let root = work.path().join("two-profiles");
    copy_tree(&fixtures().join("two-profiles"), &root);
    Staged {
        spec: root.join("positions/acme.toml"),
        profiles: root.join("profiles"),
    }
}

/// `resume build --spec <spec> --fonts bundled --out <out> <extra>`.
fn build_spec(work: &Workspace, spec: &Path, out: &Path, extra: &[&str]) -> Run {
    let mut args: Vec<String> = ["build", "--spec"].map(String::from).into();
    args.push(spec.display().to_string());
    args.extend(["--fonts", "bundled", "--out"].map(String::from));
    args.push(out.display().to_string());
    args.extend(extra.iter().map(|a| (*a).to_string()));
    work.run(&args)
}

fn shown(parts: &[&str]) -> String {
    parts.iter().collect::<PathBuf>().display().to_string()
}

fn pdf_in(out: &Path, name: &str) -> Vec<u8> {
    assert_ok!(fs::read(out.join(name)), "{name} in {}", out.display())
}

/// The width and height in points of the first page.
fn page_size(pdf: &[u8]) -> (f64, f64) {
    let doc = assert_ok!(lopdf::Document::load_mem(pdf));
    let (_, id) = assert_some!(doc.get_pages().into_iter().next());
    let page = assert_ok!(doc.get_dictionary(id));
    let media = assert_ok!(assert_ok!(page.get(b"MediaBox")).as_array());
    let number = |i: usize| f64::from(assert_ok!(media[i].as_float()));
    (number(2) - number(0), number(3) - number(1))
}

// ============================================================================
// What a spec build applies
// ============================================================================

#[test]
fn a_spec_build_applies_the_content_the_style_the_paper_and_the_merged_selection() {
    let work = Workspace::new();
    let staged = stage(&work);
    let run = build_spec(&work, &staged.spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let pdf = pdf_in(&work.out(), "acme.pdf");
    assert!(pdf.starts_with(b"%PDF"));
    let first = assert_some!(run.stderr.lines().next());
    assert!(
        first.starts_with(&format!("ok {}  mono  ", shown(&["out", "acme.pdf"]))),
        "the spec's style, and the PDF named after the spec: {first}"
    );
    let (width, height) = page_size(&pdf);
    assert!(
        (width - 595.0).abs() < 1.0 && (height - 842.0).abs() < 1.0,
        "A4: {width} x {height}"
    );
    assert!(
        run.stderr
            .contains("selection: tags ai, iceberg, leadership, platform; prefer short, business, technical"),
        "{}",
        run.stderr
    );
    let text = common::page_texts(&pdf).join(" ");
    assert!(
        text.contains("Ada Lovelace"),
        "the content is the baseline the spec names"
    );
    assert!(
        text.contains("Principal engineer who led 12 people"),
        "the spec's own pick of the summary: {text}"
    );
    assert_eq!(run.stdout, "", "human output must not use stdout");
    assert!(
        !work.out().join("baseline.pdf").exists(),
        "named after the spec, not the baseline"
    );
}

#[test]
fn the_summary_names_the_position_the_chain_and_the_content() {
    let work = Workspace::new();
    let staged = stage(&work);
    let run = build_spec(&work, &staged.spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let spec = shown(&["two-profiles", "positions", "acme.toml"]);
    let lines: Vec<&str> = run.stderr.lines().collect();
    assert_eq!(lines[1], format!("  spec: {spec}"), "{}", run.stderr);
    assert_eq!(
        lines[2],
        "  position: Acme; Principal Architect, AI Platform; level principal; domain AI platform; \
         audience technical; dated 2026-11-19; posting https://example.com/jobs/123; \
         emphasis business low, org medium, technical high"
    );
    assert_eq!(lines[3], "  profiles: principal -> ai-platform -> the spec");
    assert_eq!(
        lines[4], "  content: baseline.md",
        "shown without the `..` that led there"
    );
}

#[test]
fn json_output_carries_a_spec_record_with_every_key_in_order() {
    let work = Workspace::new();
    let staged = stage(&work);
    let run = build_spec(&work, &staged.spec, &work.out(), &["--format", "json"]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let records: Vec<serde_json::Value> = run
        .stdout
        .lines()
        .map(|line| assert_ok!(serde_json::from_str(line), "{line}"))
        .collect();
    let spec = assert_some!(records.iter().find(|r| r["type"] == "spec"));
    // `serde_json::Value` sorts its keys, so the order is read from the line itself.
    let line = assert_some!(run.stdout.lines().find(|l| l.contains("\"type\":\"spec\"")));
    let keys = [
        "type",
        "file",
        "position",
        "chain",
        "content",
        "pdf",
        "style",
        "paper",
        "drafts",
        "overrides",
        "environment",
        "version",
    ];
    let mut last = 0;
    for key in keys {
        assert!(spec.get(key).is_some(), "`{key}` is always present");
        let at = assert_some!(line.find(&format!("\"{key}\":")), "{key} in {line}");
        assert!(at >= last, "`{key}` is out of order in {line}");
        last = at;
    }
    assert_eq!(assert_some!(spec.as_object()).len(), keys.len(), "no other key: {line}");
    assert_eq!(spec["style"], "mono");
    assert_eq!(spec["paper"], "a4");
    assert_eq!(spec["drafts"], false);
    assert_eq!(spec["overrides"], serde_json::json!([]));
    assert_eq!(spec["position"]["company"], "Acme");
    assert_eq!(spec["position"]["emphasis"]["technical"], "high");
    let chain: Vec<(&serde_json::Value, &str)> = assert_some!(spec["chain"].as_array())
        .iter()
        .map(|link| (&link["name"], assert_some!(link["file"].as_str())))
        .collect();
    assert_eq!(chain.len(), 3);
    assert_eq!(chain[0].0.as_str(), Some("principal"));
    assert_eq!(chain[1].0.as_str(), Some("ai-platform"));
    assert!(chain[2].0.is_null(), "the spec itself has no profile name");
    assert!(chain[2].1.ends_with("acme.toml"), "{chain:?}");
    assert!(assert_some!(spec["pdf"].as_str()).ends_with("acme.pdf"));
    assert_eq!(spec["environment"]["fonts"], "bundled");
    let at = |kind: &str| records.iter().position(|r| r["type"] == kind);
    assert!(
        at("spec") < at("selection"),
        "the spec record comes first: {:?}",
        run.stdout
    );
}

// ============================================================================
// Reproducibility
// ============================================================================

#[test]
fn two_builds_of_one_spec_are_byte_identical() {
    let work = Workspace::new();
    let staged = stage(&work);
    let (first, second) = (work.path().join("one"), work.path().join("two"));
    for out in [&first, &second] {
        let run = build_spec(&work, &staged.spec, out, &[]);
        assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    }
    assert!(pdf_in(&first, "acme.pdf") == pdf_in(&second, "acme.pdf"));
}

/// A spec with no profiles and no picks, which flags can say completely, beside the baseline.
fn flag_equivalent_spec(work: &Workspace, name: &str, select: &str) -> PathBuf {
    assert_ok!(fs::copy(
        fixtures().join("baseline.md"),
        work.path().join("baseline.md")
    ));
    work.file(
        &format!("positions/{name}.toml"),
        &format!("[build]\ncontent = \"../baseline.md\"\nstyle = \"mono\"\npaper = \"a4\"\n\n[select]\n{select}\n"),
    )
}

const SELECT: &str = "tags = [\"ai\", \"platform\"]\nprefer = [\"short\"]";

#[test]
fn a_spec_build_equals_the_flag_build_of_a_copy_named_like_the_spec() {
    let work = Workspace::new();
    let spec = flag_equivalent_spec(&work, "acme", SELECT);
    let by_spec = work.path().join("by-spec");
    let run = build_spec(&work, &spec, &by_spec, &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);

    let copy = work.file(
        "flags/acme.md",
        &assert_ok!(fs::read_to_string(work.path().join("baseline.md"))),
    );
    let by_flags = work.path().join("by-flags");
    let flags = [
        "--tags",
        "ai,platform",
        "--prefer",
        "short",
        "--style",
        "mono",
        "--paper",
        "a4",
    ];
    let run = work.run(&flag_args(&copy, &by_flags, &flags));
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert!(
        pdf_in(&by_spec, "acme.pdf") == pdf_in(&by_flags, "acme.pdf"),
        "same content, same choices, same identity: the same bytes"
    );

    // Negative controls: the comparison above would also pass if the spec were ignored.
    let other = flag_equivalent_spec(&work, "other", "tags = [\"leadership\"]\nprefer = [\"short\"]");
    let other_out = work.path().join("other");
    assert_eq!(build_spec(&work, &other, &other_out, &[]).outcome, Outcome::Success);
    assert!(
        pdf_in(&by_spec, "acme.pdf") != pdf_in(&other_out, "other.pdf"),
        "other choices, other bytes"
    );
    let renamed = flag_equivalent_spec(&work, "renamed", SELECT);
    let renamed_out = work.path().join("renamed");
    assert_eq!(build_spec(&work, &renamed, &renamed_out, &[]).outcome, Outcome::Success);
    assert!(
        pdf_in(&by_spec, "acme.pdf") != pdf_in(&renamed_out, "renamed.pdf"),
        "the spec's stem is the PDF's identity"
    );
}

fn flag_args(file: &Path, out: &Path, flags: &[&str]) -> Vec<String> {
    let mut args = vec!["build".to_string(), file.display().to_string()];
    args.extend(["--fonts", "bundled", "--out"].map(String::from));
    args.push(out.display().to_string());
    args.extend(flags.iter().map(|f| (*f).to_string()));
    args
}

#[test]
fn position_fields_and_profile_names_never_change_the_pdf() {
    let work = Workspace::new();
    let staged = stage(&work);
    let first = work.path().join("first");
    assert_eq!(build_spec(&work, &staged.spec, &first, &[]).outcome, Outcome::Success);
    let baseline = pdf_in(&first, "acme.pdf");

    // Every `[position]` field changes, a comment is added, and a profile is renamed.
    let text = assert_ok!(fs::read_to_string(&staged.spec));
    let edited = text
        .replace("company = \"Acme\"", "company = \"ZzSentinelCompany\"")
        .replace("Principal Architect, AI Platform", "ZzSentinelTitle")
        .replace("level = \"principal\"", "level = \"zzsentinel-level\"")
        .replace("domain = \"AI platform\"", "domain = \"zzsentinel domain\"")
        .replace("audience = \"technical\"", "audience = \"zzsentinel audience\"")
        .replace("business = \"low\"", "business = \"high\"")
        .replace("org = \"medium\"", "org = \"low\"")
        .replace("technical = \"high\"", "technical = \"low\"")
        .replace("date = 2026-11-19", "date = 2031-01-02")
        .replace("https://example.com/jobs/123", "https://example.com/zzsentinel")
        .replace(
            "extends = [\"principal\", \"ai-platform\"]",
            "# a comment\nextends = [\"zzsentinel-first\", \"zzsentinel-second\"]",
        );
    assert!(edited != text, "the edit must change the spec");
    assert_ok!(fs::rename(
        staged.profiles.join("principal.toml"),
        staged.profiles.join("zzsentinel-first.toml")
    ));
    assert_ok!(fs::rename(
        staged.profiles.join("ai-platform.toml"),
        staged.profiles.join("zzsentinel-second.toml")
    ));
    assert_ok!(fs::write(&staged.spec, edited));
    let second = work.path().join("second");
    let run = build_spec(&work, &staged.spec, &second, &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let after = pdf_in(&second, "acme.pdf");
    assert!(
        baseline == after,
        "nothing a spec says about the position reaches the PDF"
    );
    for sentinel in [&b"ZzSentinel"[..], b"zzsentinel", b"ai-platform"] {
        assert!(
            !after.windows(sentinel.len()).any(|w| w == sentinel),
            "{} appears in the PDF",
            String::from_utf8_lossy(sentinel)
        );
    }
}

// ============================================================================
// Specs written for the tests
// ============================================================================

/// A spec at `positions/<name>.toml` beside `baseline.md`, with `extra` after its `[build]` table.
fn spec_with(work: &Workspace, name: &str, extra: &str) -> PathBuf {
    let baseline = work.path().join("baseline.md");
    if !baseline.exists() {
        assert_ok!(fs::copy(fixtures().join("baseline.md"), baseline));
    }
    work.file(
        &format!("positions/{name}.toml"),
        &format!("[build]\ncontent = \"../baseline.md\"\n{extra}\n"),
    )
}

fn stderr_has(run: &Run, needle: &str) {
    assert!(run.stderr.contains(needle), "{needle:?} not in:\n{}", run.stderr);
}

// ============================================================================
// Flags beside a spec
// ============================================================================

#[test]
fn allow_overrides_says_spec_plus_overrides_and_lists_each_with_what_it_replaced() {
    let work = Workspace::new();
    let staged = stage(&work);
    let plain = work.path().join("plain");
    assert_eq!(build_spec(&work, &staged.spec, &plain, &[]).outcome, Outcome::Success);
    let out = work.path().join("over");
    let run = build_spec(
        &work,
        &staged.spec,
        &out,
        &[
            "--allow-overrides",
            "--tags",
            "ai",
            "--prefer",
            "technical",
            "--paper",
            "letter",
        ],
    );
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let spec = shown(&["two-profiles", "positions", "acme.toml"]);
    stderr_has(&run, &format!("  spec: {spec}, plus overrides\n"));
    let principal = shown(&["two-profiles", "profiles", "principal.toml"]);
    let platform = shown(&["two-profiles", "profiles", "ai-platform.toml"]);
    let tags = assert_some!(run.stderr.lines().find(|l| l.starts_with("  override: --tags ai ")));
    assert!(
        tags.contains("replaces tags ai, iceberg, leadership, platform ("),
        "{tags}"
    );
    let at = |name: &str| assert_some!(tags.find(name), "{name} in {tags}");
    assert!(
        at(&principal) < at(&platform) && at(&platform) < at(&spec),
        "least specific first: {tags}"
    );
    stderr_has(
        &run,
        &format!("  override: --paper letter replaces paper a4 ({platform}:5:9)\n"),
    );
    let prefer = assert_some!(
        run.stderr
            .lines()
            .find(|l| l.starts_with("  override: --prefer technical "))
    );
    assert!(
        prefer.contains("replaces prefer short, business, technical ("),
        "{prefer}"
    );
    let at_prefer = |name: &str| assert_some!(prefer.find(name), "{name} in {prefer}");
    assert!(
        at_prefer(&principal) < at_prefer(&platform) && at_prefer(&platform) < at_prefer(&spec),
        "one place for each file, least specific first: {prefer}"
    );
    assert_eq!(prefer.matches(".toml:").count(), 3, "each file once: {prefer}");
    stderr_has(&run, "selection: tags ai; prefer technical");
    let (width, height) = page_size(&pdf_in(&out, "acme.pdf"));
    assert!(
        (width - 612.0).abs() < 1.0 && (height - 792.0).abs() < 1.0,
        "letter: {width} x {height}"
    );
    assert!(
        pdf_in(&out, "acme.pdf") != pdf_in(&plain, "acme.pdf"),
        "overrides change the build"
    );
}

#[test]
fn json_lists_each_override_with_what_it_set_and_replaced() {
    let work = Workspace::new();
    let staged = stage(&work);
    let run = build_spec(
        &work,
        &staged.spec,
        &work.out(),
        &[
            "--format",
            "json",
            "--allow-overrides",
            "--core-only",
            "--style",
            "pragmatic",
        ],
    );
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let records: Vec<serde_json::Value> = run
        .stdout
        .lines()
        .map(|l| assert_ok!(serde_json::from_str(l)))
        .collect();
    let spec = assert_some!(records.iter().find(|r| r["type"] == "spec"));
    let overrides = assert_some!(spec["overrides"].as_array());
    let flags: Vec<&str> = overrides.iter().map(|o| assert_some!(o["flag"].as_str())).collect();
    assert_eq!(flags, ["--core-only", "--style"]);
    assert_eq!(overrides[0]["set"], serde_json::json!([]), "a switch sets no value");
    assert_eq!(
        overrides[0]["replaced"],
        serde_json::json!({"setting": "tags", "value": ["ai", "iceberg", "leadership", "platform"]})
    );
    assert_eq!(
        assert_some!(overrides[0]["at"].as_array()).len(),
        3,
        "one place for each of the three files"
    );
    assert_eq!(overrides[1]["set"], serde_json::json!(["pragmatic"]));
    assert_eq!(
        overrides[1]["replaced"],
        serde_json::json!({"setting": "style", "value": ["mono"]})
    );
    assert_eq!(assert_some!(overrides[1]["at"].as_array()).len(), 1);
    assert_eq!(spec["style"], "pragmatic", "the record says what was built");
}

#[test]
fn drafts_beside_a_spec_write_a_preview_that_never_replaces_the_pdf_built_to_be_sent() {
    let work = Workspace::new();
    let staged = stage(&work);
    assert_eq!(
        build_spec(&work, &staged.spec, &work.out(), &[]).outcome,
        Outcome::Success
    );
    let sent = pdf_in(&work.out(), "acme.pdf");
    let run = build_spec(&work, &staged.spec, &work.out(), &["--drafts"]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let first = assert_some!(run.stderr.lines().next());
    assert!(
        first.starts_with(&format!("ok {}  mono  ", shown(&["out", "acme.drafts.pdf"]))),
        "{first}"
    );
    stderr_has(&run, ", with drafts\n");
    assert!(
        pdf_in(&work.out(), "acme.pdf") == sent,
        "the PDF built to be sent is untouched"
    );
    assert!(work.out().join("acme.drafts.pdf").is_file());
    let json = build_spec(
        &work,
        &staged.spec,
        &work.path().join("j"),
        &["--drafts", "--format", "json"],
    );
    let record: serde_json::Value = assert_ok!(serde_json::from_str(assert_some!(
        json.stdout.lines().find(|l| l.contains("\"type\":\"spec\""))
    )));
    assert_eq!(record["drafts"], true);
    assert!(assert_some!(record["pdf"].as_str()).ends_with("acme.drafts.pdf"));
}

#[test]
fn a_spec_named_like_a_preview_cannot_build() {
    let work = Workspace::new();
    let spec = spec_with(&work, "acme.drafts", "");
    let run = build_spec(&work, &spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    stderr_has(&run, "cannot name a PDF: its name ends in `.drafts`");
    assert!(!work.out().exists());
}

// ============================================================================
// Printing the spec
// ============================================================================

#[test]
fn print_spec_prints_the_resolved_toml_on_stdout_and_builds_nothing() {
    let work = Workspace::new();
    let staged = stage(&work);
    // The content file is never read, so it need not exist.
    assert_ok!(fs::remove_file(work.path().join("baseline.md")));
    for format in [&[][..], &["--format", "json"][..]] {
        let mut extra = vec!["--print-spec"];
        extra.extend(format);
        let run = build_spec(&work, &staged.spec, &work.out(), &extra);
        assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
        assert!(
            run.stdout
                .starts_with("# Resolved from: principal, ai-platform, the spec\n"),
            "{}",
            run.stdout
        );
        assert!(
            run.stdout
                .contains("tags = [\"ai\", \"iceberg\", \"leadership\", \"platform\"]")
        );
        assert!(run.stdout.contains("[select.pick]\ngateway = \"short\""));
        assert_eq!(run.stderr, "", "nothing but the spec is printed");
    }
    assert!(!work.out().exists(), "nothing was built or written");
}

#[test]
fn what_print_spec_prints_resolves_to_the_same_build() {
    let work = Workspace::new();
    let staged = stage(&work);
    let printed = build_spec(&work, &staged.spec, &work.out(), &["--print-spec"]).stdout;
    // Saved beside the original it is the same spec with no profiles to find.
    let flat = work.path().join("two-profiles/positions/flat.toml");
    assert_ok!(fs::write(&flat, printed));
    let (one, two) = (work.path().join("one"), work.path().join("two"));
    assert_eq!(build_spec(&work, &staged.spec, &one, &[]).outcome, Outcome::Success);
    assert_eq!(build_spec(&work, &flat, &two, &[]).outcome, Outcome::Success);
    // The names differ, so the bytes do; the text is what must agree.
    let text = |out: &Path, name: &str| common::page_texts(&pdf_in(out, name));
    assert_eq!(text(&one, "acme.pdf"), text(&two, "flat.pdf"));
}

// ============================================================================
// What goes wrong, and what it says
// ============================================================================

#[test]
fn an_unresolvable_spec_is_an_error_on_stderr_with_nothing_on_stdout() {
    let work = Workspace::new();
    let staged = stage(&work);
    assert_ok!(fs::remove_dir_all(&staged.profiles));
    for format in [&[][..], &["--format", "json"][..]] {
        let run = build_spec(&work, &staged.spec, &work.out(), format);
        assert_eq!(run.outcome, Outcome::Failure);
        stderr_has(&run, "has no directory of profiles");
        assert_eq!(run.stdout, "");
    }
    assert!(!work.out().exists());
}

#[test]
fn profiles_can_be_given_and_are_then_the_only_place_looked() {
    let work = Workspace::new();
    let staged = stage(&work);
    let moved = work.path().join("elsewhere");
    assert_ok!(fs::rename(&staged.profiles, &moved));
    let missing = build_spec(&work, &staged.spec, &work.out(), &[]);
    assert_eq!(missing.outcome, Outcome::Failure);
    let searched = shown(&["two-profiles", "profiles"]);
    stderr_has(
        &missing,
        &format!("looked for {}", work.path().join(&searched).display()),
    );
    let found = build_spec(
        &work,
        &staged.spec,
        &work.out(),
        &["--profiles", &moved.display().to_string()],
    );
    assert_eq!(found.outcome, Outcome::Success, "{}", found.stderr);
    stderr_has(&found, "profiles: principal -> ai-platform -> the spec");
}

#[test]
fn a_missing_content_file_names_the_spec_line_that_named_it() {
    let work = Workspace::new();
    let spec = spec_with(&work, "acme", "");
    assert_ok!(fs::remove_file(work.path().join("baseline.md")));
    let run = build_spec(&work, &spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    let at = spec.display();
    stderr_has(
        &run,
        &format!("error: {at}:2:11: the `content` file cannot be used: cannot read baseline.md"),
    );
    stderr_has(&run, &format!("  spec: {}", shown(&["positions", "acme.toml"])));
    assert!(!work.out().exists(), "nothing is written");
}

#[test]
fn a_content_directory_is_refused_unread() {
    let work = Workspace::new();
    let spec = work.file("positions/acme.toml", "[build]\ncontent = \"sub\"\n");
    assert_ok!(fs::create_dir_all(work.path().join("positions/sub")));
    let run = build_spec(&work, &spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    stderr_has(&run, "is not a regular file");
    stderr_has(&run, "the `content` file cannot be used");
}

#[test]
fn a_plain_build_of_a_directory_is_refused_the_same_way() {
    let work = Workspace::new();
    let dir = work.path().join("a-directory.md");
    assert_ok!(fs::create_dir_all(&dir));
    let run = work.build(&[&dir], &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    stderr_has(&run, "is not a regular file");
}

#[test]
fn unknown_picks_name_their_lines_and_write_no_pdf() {
    let work = Workspace::new();
    let spec = spec_with(&work, "acme", "[select]\npick = { nope = \"x\", gateway = \"huge\" }");
    let run = build_spec(&work, &spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    let at = spec.display();
    stderr_has(&run, "2 picks name nothing in the content file");
    stderr_has(&run, &format!("{at}:4:17: the content has no group `nope`"));
    stderr_has(&run, &format!("{at}:4:32: the group `gateway` has no wording `huge`"));
    assert!(!work.out().exists(), "no PDF was written");
}

#[test]
fn a_picked_draft_wording_names_the_line_that_picked_it() {
    let work = Workspace::new();
    let text = assert_ok!(fs::read_to_string(fixtures().join("baseline.md")))
        .replace("<!-- =gateway.short -->", "<!-- =gateway.short @draft -->");
    work.file("baseline.md", &text);
    let spec = spec_with(&work, "acme", "[select]\npick = { gateway = \"short\" }");
    let run = build_spec(&work, &spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Failure, "{}", run.stderr);
    let at = spec.display();
    stderr_has(&run, "`gateway.short`");
    stderr_has(&run, &format!("(chosen at {at}:4:"));
    assert!(!work.out().exists());
    let preview = build_spec(&work, &spec, &work.out(), &["--drafts"]);
    assert_eq!(
        preview.outcome,
        Outcome::Success,
        "a preview may pick a draft: {}",
        preview.stderr
    );
}

#[test]
fn a_spec_with_promote_is_refused_naming_the_line_and_print_spec_still_prints_it() {
    let work = Workspace::new();
    let spec = spec_with(&work, "acme", "[select]\npromote = [\"ai\"]");
    let run = build_spec(&work, &spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    let at = spec.display();
    stderr_has(
        &run,
        &format!("`promote` is not applied by this version: `ai` ({at}:4:12)"),
    );
    assert!(!work.out().exists());
    let printed = build_spec(&work, &spec, &work.out(), &["--print-spec"]);
    assert_eq!(printed.outcome, Outcome::Success);
    assert!(printed.stdout.contains("promote = [\"ai\"]"), "{}", printed.stdout);
}

#[test]
fn a_spec_that_chooses_core_only_and_tags_cannot_be_rescued_by_a_flag() {
    let work = Workspace::new();
    let spec = spec_with(&work, "acme", "[select]\ntags = [\"ai\"]\ncore-only = true");
    for extra in [&[][..], &["--allow-overrides", "--tags", "ai"][..]] {
        let run = build_spec(&work, &spec, &work.out(), extra);
        assert_eq!(run.outcome, Outcome::Failure, "{extra:?}");
        stderr_has(&run, "`core-only` is true");
    }
    assert!(!work.out().exists());
}

#[test]
fn an_unknown_style_in_a_spec_names_the_line_but_one_from_a_flag_does_not() {
    let work = Workspace::new();
    let spec = spec_with(&work, "acme", "style = \"nope\"");
    let run = build_spec(&work, &spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    let at = spec.display();
    stderr_has(&run, "unknown style `nope`");
    stderr_has(&run, &format!("(chosen at {at}:3:9)"));
    let flagged = build_spec(&work, &spec, &work.out(), &["--allow-overrides", "--style", "nope2"]);
    assert_eq!(flagged.outcome, Outcome::Failure);
    stderr_has(&flagged, "unknown style `nope2`");
    assert!(!flagged.stderr.contains("chosen at"), "{}", flagged.stderr);
}

// ============================================================================
// Names a spec or a profile uses that the resume does not
// ============================================================================

#[test]
fn a_name_the_spec_wrote_that_the_resume_lacks_is_a_warning_naming_its_line() {
    let work = Workspace::new();
    let spec = spec_with(
        &work,
        "acme",
        "[select]\ntags = [\"nosuchtag\"]\nprefer = [\"nosuchwording\"]",
    );
    let run = build_spec(&work, &spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let at = shown(&["positions", "acme.toml"]);
    stderr_has(
        &run,
        &format!("warning[unknown-tag]: spec tags `nosuchtag` ({at}:4:9): not used in baseline.md"),
    );
    stderr_has(
        &run,
        &format!("warning[unknown-wording]: spec wordings `nosuchwording` ({at}:5:11)"),
    );
    assert!(!run.stderr.contains("--tags"), "no flag was typed: {}", run.stderr);
}

#[test]
fn a_name_only_a_profile_wrote_is_a_note_so_a_shared_profile_does_not_cry_wolf() {
    let work = Workspace::new();
    let staged = stage(&work);
    let run = build_spec(&work, &staged.spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert!(
        assert_some!(run.stderr.lines().next()).contains("0 errors, 0 warnings, 1 note"),
        "{}",
        run.stderr
    );
    let platform = shown(&["two-profiles", "profiles", "ai-platform.toml"]);
    let principal = shown(&["two-profiles", "profiles", "principal.toml"]);
    stderr_has(
        &run,
        &format!(
            "note[unknown-wording]: profile wordings `business` ({platform}:9:11), `technical` ({principal}:9:11): not used in baseline.md"
        ),
    );
    let json = build_spec(&work, &staged.spec, &work.path().join("j"), &["--format", "json"]);
    let note = assert_some!(json.stdout.lines().find(|l| l.contains("\"unknown-wording\"")));
    assert!(note.contains("\"severity\":\"note\""), "{note}");
}

#[test]
fn a_flag_typed_beside_a_spec_is_still_judged_as_a_flag() {
    let work = Workspace::new();
    let staged = stage(&work);
    let run = build_spec(
        &work,
        &staged.spec,
        &work.out(),
        &["--allow-overrides", "--tags", "nosuchtag"],
    );
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    stderr_has(
        &run,
        "warning[unknown-tag]: --tags `nosuchtag`: not used in baseline.md",
    );
}

// ============================================================================
// The summary says what was chosen
// ============================================================================

#[test]
fn the_selection_line_and_record_list_the_picks() {
    let work = Workspace::new();
    let staged = stage(&work);
    let run = build_spec(&work, &staged.spec, &work.out(), &[]);
    stderr_has(&run, "; picks gateway.short, summary.acme\n");
    let json = build_spec(&work, &staged.spec, &work.path().join("j"), &["--format", "json"]);
    let record: serde_json::Value = assert_ok!(serde_json::from_str(assert_some!(
        json.stdout.lines().find(|l| l.contains("\"type\":\"selection\""))
    )));
    assert_eq!(
        record["picks"],
        serde_json::json!({"gateway": "short", "summary": "acme"})
    );
}

#[test]
fn environment_flags_are_echoed_and_do_not_count_as_overrides() {
    let work = Workspace::new();
    let staged = stage(&work);
    let out = work.path().join("pdfs");
    let run = build_spec(&work, &staged.spec, &out, &["--max-pages", "3"]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    stderr_has(
        &run,
        &format!("  environment: --fonts bundled; --max-pages 3; -o {}\n", out.display()),
    );
    assert!(!run.stderr.contains("plus overrides"));
}

#[test]
fn a_dry_run_prints_the_spec_lines_and_writes_nothing() {
    let work = Workspace::new();
    let staged = stage(&work);
    let run = build_spec(&work, &staged.spec, &work.out(), &["--dry-run"]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    stderr_has(&run, "  profiles: principal -> ai-platform -> the spec\n");
    assert!(!work.out().exists());
}

// ============================================================================
// Profiles are shared
// ============================================================================

#[test]
fn two_specs_sharing_a_profile_both_change_when_it_changes_and_an_unrelated_profile_changes_neither() {
    let work = Workspace::new();
    assert_ok!(fs::copy(
        fixtures().join("baseline.md"),
        work.path().join("baseline.md")
    ));
    work.file("profiles/shared.toml", "[select]\ntags = [\"ai\"]\n");
    work.file("profiles/unrelated.toml", "[select]\ntags = [\"leadership\"]\n");
    let one = work.file(
        "positions/one.toml",
        "extends = [\"shared\"]\n[build]\ncontent = \"../baseline.md\"\n",
    );
    let two = work.file(
        "positions/two.toml",
        "extends = [\"shared\"]\n[build]\ncontent = \"../baseline.md\"\n[select]\nprefer = [\"short\"]\n",
    );
    let build_both = |out: &Path| {
        for spec in [&one, &two] {
            let run = build_spec(&work, spec, out, &[]);
            assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
        }
    };
    let (before, after, untouched) = (
        work.path().join("before"),
        work.path().join("after"),
        work.path().join("untouched"),
    );
    build_both(&before);
    assert_ok!(fs::write(
        work.path().join("profiles/unrelated.toml"),
        "[select]\ntags = [\"platform\"]\n"
    ));
    build_both(&untouched);
    for name in ["one.pdf", "two.pdf"] {
        assert!(
            pdf_in(&before, name) == pdf_in(&untouched, name),
            "{name}: an unrelated profile changed it"
        );
    }
    assert_ok!(fs::write(
        work.path().join("profiles/shared.toml"),
        "[select]\ntags = [\"leadership\"]\n"
    ));
    build_both(&after);
    for name in ["one.pdf", "two.pdf"] {
        assert!(
            pdf_in(&before, name) != pdf_in(&after, name),
            "{name}: the shared profile did not reach it"
        );
    }
}

// ============================================================================
// From the independent reviews (history/rsm-141.23.2_code-review.md, _test-review.md)
// ============================================================================

#[test]
fn a_flag_that_repeats_a_bad_spec_name_is_judged_as_the_flag() {
    let work = Workspace::new();
    let spec = spec_with(
        &work,
        "acme",
        "[select]\ntags = [\"nosuchtag\"]\nprefer = [\"nosuchword\"]",
    );
    let run = build_spec(
        &work,
        &spec,
        &work.out(),
        &["--allow-overrides", "--tags", "nosuchtag", "--prefer", "nosuchword"],
    );
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    stderr_has(
        &run,
        "warning[unknown-tag]: --tags `nosuchtag`: not used in baseline.md",
    );
    stderr_has(
        &run,
        "warning[unknown-wording]: --prefer `nosuchword`: not used in baseline.md",
    );
    assert!(
        !run.stderr.contains("spec tags") && !run.stderr.contains("spec wordings"),
        "{}",
        run.stderr
    );
}

#[test]
fn a_promote_only_a_profile_wrote_is_refused_naming_that_profile() {
    let work = Workspace::new();
    work.file("profiles/shared.toml", "[select]\npromote = [\"ai\"]\n");
    let spec = spec_with(&work, "acme", "");
    assert_ok!(fs::write(
        &spec,
        "extends = [\"shared\"]\n[build]\ncontent = \"../baseline.md\"\n"
    ));
    let run = build_spec(&work, &spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    let profile = work.path().join("profiles").join("shared.toml");
    stderr_has(&run, &format!("`ai` ({}:2:12)", profile.display()));
    assert!(!work.out().exists());
}

#[test]
fn the_json_record_is_written_even_when_no_pdf_is_and_says_so() {
    let work = Workspace::new();
    let spec = spec_with(&work, "acme", "");
    assert_ok!(fs::remove_file(work.path().join("baseline.md")));
    let run = build_spec(&work, &spec, &work.out(), &["--format", "json"]);
    assert_eq!(run.outcome, Outcome::Failure);
    let record: serde_json::Value = assert_ok!(serde_json::from_str(assert_some!(
        run.stdout.lines().find(|l| l.contains("\"type\":\"spec\""))
    )));
    assert!(record["pdf"].is_null(), "no PDF was written: {record}");
    assert!(
        record["position"].is_null(),
        "a spec with no [position] has none: {record}"
    );
    assert_eq!(record["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn a_dry_run_names_the_pdf_it_would_write_in_the_json_record() {
    let work = Workspace::new();
    let staged = stage(&work);
    let run = build_spec(&work, &staged.spec, &work.out(), &["--dry-run", "--format", "json"]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let record: serde_json::Value = assert_ok!(serde_json::from_str(assert_some!(
        run.stdout.lines().find(|l| l.contains("\"type\":\"spec\""))
    )));
    assert!(assert_some!(record["pdf"].as_str()).ends_with("acme.pdf"), "{record}");
    assert!(!work.out().exists(), "nothing was written");
}

#[test]
fn every_environment_option_is_echoed_in_both_formats_and_only_when_it_differs() {
    let work = Workspace::new();
    let staged = stage(&work);
    let fonts = work.path().join("my-fonts");
    assert_ok!(fs::create_dir_all(&fonts));
    // `--fonts auto` is where a font path means something, so it is the one case without `--fonts bundled`.
    let mut args: Vec<String> = ["build", "--spec"].map(String::from).into();
    args.push(staged.spec.display().to_string());
    args.extend(["--out".to_string(), work.out().display().to_string()]);
    args.extend(["--font-path".to_string(), fonts.display().to_string()]);
    args.extend([
        "--styles-dir".to_string(),
        work.path().join("styles").display().to_string(),
    ]);
    args.extend(["--max-pages".to_string(), "2".to_string()]);
    assert_ok!(fs::create_dir_all(work.path().join("styles")));
    let human = work.run(&args);
    assert_eq!(human.outcome, Outcome::Success, "{}", human.stderr);
    stderr_has(&human, &format!("--font-path {}", fonts.display()));
    stderr_has(
        &human,
        &format!("--styles-dir {}", work.path().join("styles").display()),
    );
    stderr_has(&human, "--max-pages 2");
    assert!(
        !human.stderr.contains("--fonts"),
        "--fonts auto is the default, so it is not said"
    );
    args.extend(["--format".to_string(), "json".to_string()]);
    let json = work.run(&args);
    let record: serde_json::Value = assert_ok!(serde_json::from_str(assert_some!(
        json.stdout.lines().find(|l| l.contains("\"type\":\"spec\""))
    )));
    let env = &record["environment"];
    assert_eq!(env["fonts"], "auto");
    assert_eq!(env["font_path"], serde_json::json!([fonts.display().to_string()]));
    assert_eq!(env["styles_dir"], work.path().join("styles").display().to_string());
    assert_eq!(env["max_pages"], 2);
}

#[test]
fn nothing_about_the_machine_is_echoed_when_nothing_about_it_changed() {
    let work = Workspace::new();
    let staged = stage(&work);
    // The helper adds `--fonts bundled` and `--out`, which are changes; this run uses neither.
    let run = work.run(&[
        "build".to_string(),
        "--spec".to_string(),
        staged.spec.display().to_string(),
        "--dry-run".to_string(),
    ]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert!(!run.stderr.contains("environment:"), "{}", run.stderr);
}

#[test]
fn a_pick_error_still_shows_the_findings_found_before_it_and_counts_them() {
    let work = Workspace::new();
    let spec = spec_with(
        &work,
        "acme",
        "[select]\ntags = [\"nosuchtag\"]\npick = { nope = \"x\" }",
    );
    let run = build_spec(&work, &spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    let first = assert_some!(run.stderr.lines().next());
    assert!(first.contains("1 error, 1 warning"), "{first}");
    stderr_has(&run, "warning[unknown-tag]: spec tags `nosuchtag`");
    stderr_has(&run, "the content has no group `nope`");
    assert!(!work.out().exists());
}

#[test]
fn a_style_directory_without_a_main_file_is_traced_to_the_spec_that_chose_the_style() {
    let work = Workspace::new();
    assert_ok!(fs::create_dir_all(work.path().join("styles/mine")));
    let spec = spec_with(&work, "acme", "style = \"mine\"");
    let styles = work.path().join("styles");
    let run = build_spec(
        &work,
        &spec,
        &work.out(),
        &["--styles-dir", &styles.display().to_string()],
    );
    assert_eq!(run.outcome, Outcome::Failure);
    stderr_has(&run, "style.typ");
    stderr_has(&run, &format!("(chosen at {}:3:9)", spec.display()));
}

#[test]
fn the_oversize_message_states_the_limit_and_not_a_length_that_was_cut() {
    let work = Workspace::new();
    let big = work.file("big.md", &"a".repeat(2 * 1024 * 1024));
    let run = work.build(&[&big], &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    stderr_has(&run, "input is over the limit of 1048576 bytes");
    assert!(!run.stderr.contains("input is 10"), "{}", run.stderr);
}

#[cfg(unix)]
#[test]
fn a_link_to_a_file_is_read_and_a_link_to_a_directory_is_refused() {
    let work = Workspace::new();
    let spec = spec_with(&work, "acme", "");
    assert_ok!(std::os::unix::fs::symlink(
        work.path().join("baseline.md"),
        work.path().join("linked.md")
    ));
    assert_ok!(fs::write(&spec, "[build]\ncontent = \"../linked.md\"\n"));
    let run = build_spec(&work, &spec, &work.out(), &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert_ok!(std::os::unix::fs::symlink(
        work.path().join("positions"),
        work.path().join("dir-link")
    ));
    assert_ok!(fs::write(&spec, "[build]\ncontent = \"../dir-link\"\n"));
    let refused = build_spec(&work, &spec, &work.out(), &[]);
    assert_eq!(refused.outcome, Outcome::Failure);
    stderr_has(&refused, "is not a regular file");
}
