//! ABOUTME: In-process tests of `resume build`: real documents through the real pipeline, over injected streams.
//! ABOUTME: They cover the summary, `--dry-run`, exit outcomes, JSON output, and what a failing input does to the rest.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;
use resume::cli::Cli;
use resume::commands::{self, Env, Io, Outcome};
use resume::diagnostics::termcolor::Buffer;

/// A temporary directory standing in for the user's current directory.
struct Workspace {
    dir: tempfile::TempDir,
}

/// What a run printed and how it ended.
struct Run {
    outcome: Outcome,
    stdout: String,
    stderr: String,
}

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    assert_ok!(fs::read_to_string(&path), "{}", path.display())
}

impl Workspace {
    fn new() -> Self {
        Self {
            dir: assert_ok!(tempfile::tempdir()),
        }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Writes `text` to `name` under the workspace and returns its absolute path.
    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.path().join(name);
        if let Some(parent) = path.parent() {
            assert_ok!(fs::create_dir_all(parent));
        }
        assert_ok!(fs::write(&path, text));
        path
    }

    fn out(&self) -> PathBuf {
        self.path().join("out")
    }

    /// Runs `resume <args>` with the workspace as the current directory, over the streams given.
    fn invoke(&self, args: &[String], stdout: &mut dyn io::Write, stderr: &mut Buffer) -> Outcome {
        let cli = assert_ok!(Cli::try_parse_validated(
            std::iter::once("resume".to_string()).chain(args.iter().cloned())
        ));
        let env = Env {
            cwd: self.path().to_path_buf(),
            resume_font_path: None,
        };
        commands::run(&cli, &env, &mut Io { stdout, stderr })
    }

    /// Runs `resume <args>` and collects what it printed; stderr has no color.
    fn run(&self, args: &[String]) -> Run {
        let mut stdout = Vec::new();
        let mut stderr = Buffer::no_color();
        let outcome = self.invoke(args, &mut stdout, &mut stderr);
        Run {
            outcome,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(stderr.as_slice()).into_owned(),
        }
    }

    /// The arguments of `resume build <files> --fonts bundled --out <workspace>/out <extra>`: bundled fonts so
    /// the result does not depend on the machine.
    fn build_args(&self, files: &[&Path], extra: &[&str]) -> Vec<String> {
        let mut args = vec!["build".to_string()];
        args.extend(files.iter().map(|f| f.display().to_string()));
        args.extend(["--fonts", "bundled", "--out"].map(String::from));
        args.push(self.out().display().to_string());
        args.extend(extra.iter().map(|a| (*a).to_string()));
        args
    }

    fn build(&self, files: &[&Path], extra: &[&str]) -> Run {
        self.run(&self.build_args(files, extra))
    }
}

fn shown(parts: &[&str]) -> String {
    parts.iter().collect::<PathBuf>().display().to_string()
}

// ============================================================================
// A clean build
// ============================================================================

#[test]
fn build_writes_pdf_and_prints_summary() {
    let work = Workspace::new();
    let input = work.file("Resume.md", &fixture("full.md"));
    let run = work.build(&[&input], &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let pdf = assert_ok!(fs::read(work.out().join("Resume.pdf")));
    assert!(pdf.starts_with(b"%PDF"), "not a PDF");
    let summary = assert_some!(run.stderr.lines().next());
    let expected_start = format!("ok {}  pragmatic  ", shown(&["out", "Resume.pdf"]));
    assert!(summary.starts_with(&expected_start), "{summary}");
    assert!(
        summary.contains(" pages  ") && summary.contains("  built in "),
        "{summary}"
    );
    assert_eq!(run.stdout, "", "human output must not use stdout");
}

#[test]
fn summary_lists_resolved_fonts() {
    let work = Workspace::new();
    let input = work.file("Resume.md", &fixture("full.md"));
    let run = work.build(&[&input], &[]);
    assert!(run.stderr.contains("fonts: body "), "{}", run.stderr);
    assert!(run.stderr.contains("(fallback for "), "{}", run.stderr);
}

#[test]
fn sparse_last_page_warns() {
    let work = Workspace::new();
    let input = work.file("Resume.md", &fixture("full.md"));
    let run = work.build(&[&input], &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert!(run.stderr.contains("the last page is"), "{}", run.stderr);
    assert!(run.stderr.contains("0 errors, 1 warning"), "{}", run.stderr);
}

#[test]
fn crlf_input_yields_the_same_pdf_as_lf_input() {
    let lf = Workspace::new();
    let crlf = Workspace::new();
    let text = fixture("full.md");
    let a = lf.file("Resume.md", &text.replace("\r\n", "\n"));
    let b = crlf.file("Resume.md", &text.replace("\r\n", "\n").replace('\n', "\r\n"));
    assert_eq!(lf.build(&[&a], &[]).outcome, Outcome::Success);
    assert_eq!(crlf.build(&[&b], &[]).outcome, Outcome::Success);
    assert!(
        assert_ok!(fs::read(lf.out().join("Resume.pdf"))) == assert_ok!(fs::read(crlf.out().join("Resume.pdf"))),
        "the two PDFs differ"
    );
}

// ============================================================================
// --dry-run
// ============================================================================

#[test]
fn build_dry_run_writes_nothing_and_reports_target() {
    let work = Workspace::new();
    let input = work.file("Resume.md", &fixture("full.md"));
    let run = work.build(&[&input], &["--dry-run"]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert!(!work.out().exists(), "a dry run created the output directory");
    let summary = assert_some!(run.stderr.lines().next());
    assert!(
        summary.starts_with(&format!("ok would write {} (", shown(&["out", "Resume.pdf"]))),
        "{summary}"
    );
    assert!(
        summary.contains("creates out") || summary.contains("creates "),
        "{summary}"
    );
    assert!(summary.contains("checked in "), "{summary}");
}

#[test]
fn a_dry_run_says_when_it_would_overwrite() {
    let work = Workspace::new();
    let input = work.file("Resume.md", &fixture("full.md"));
    assert_eq!(work.build(&[&input], &[]).outcome, Outcome::Success);
    let before = assert_ok!(fs::read(work.out().join("Resume.pdf")));
    let run = work.build(&[&input], &["--dry-run"]);
    assert!(run.stderr.contains("overwrites existing"), "{}", run.stderr);
    assert_eq!(assert_ok!(fs::read(work.out().join("Resume.pdf"))), before);
}

#[test]
fn a_dry_run_ends_the_way_a_real_build_would() {
    let work = Workspace::new();
    let input = work.file("Resume.md", &fixture("full.md"));
    let run = work.build(&[&input], &["--dry-run", "--max-pages", "1"]);
    assert_eq!(run.outcome, Outcome::Failure, "{}", run.stderr);
    assert!(!work.out().exists());
}

// ============================================================================
// Failures
// ============================================================================

#[test]
fn max_pages_exceeded_exits_1_and_still_writes_the_pdf() {
    let work = Workspace::new();
    let input = work.file("Resume.md", &fixture("full.md"));
    let run = work.build(&[&input], &["--max-pages", "1"]);
    assert_eq!(run.outcome, Outcome::Failure, "{}", run.stderr);
    assert!(run.stderr.contains("2 pages, limit is 1"), "{}", run.stderr);
    assert!(
        work.out().join("Resume.pdf").is_file(),
        "the PDF is written so it can be looked at"
    );
    assert!(
        assert_some!(run.stderr.lines().next()).starts_with("error "),
        "{}",
        run.stderr
    );
}

#[test]
fn malformed_role_exits_1_with_located_error() {
    let work = Workspace::new();
    let input = work.file("errors-role.md", &fixture("errors-role.md"));
    let run = work.build(&[&input], &[]);
    assert_eq!(run.outcome, Outcome::Failure, "{}", run.stderr);
    assert!(!work.out().join("errors-role.pdf").exists());
    assert!(run.stderr.contains("no PDF written"), "{}", run.stderr);
    assert!(
        run.stderr.contains("errors-role.md:"),
        "the error is located: {}",
        run.stderr
    );
}

#[test]
fn an_error_in_one_file_keeps_the_output_of_the_others() {
    let work = Workspace::new();
    let good = work.file("Resume.md", &fixture("full.md"));
    let bad = work.file("errors-role.md", &fixture("errors-role.md"));
    let run = work.build(&[&good, &bad], &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    assert!(work.out().join("Resume.pdf").is_file());
    let good_at = assert_some!(run.stderr.find("Resume.pdf"));
    let bad_at = assert_some!(run.stderr.find("errors-role.md  "));
    assert!(
        good_at < bad_at,
        "one summary line per file, in the order given: {}",
        run.stderr
    );
}

#[test]
fn an_input_that_cannot_be_read_exits_1_and_says_why() {
    let work = Workspace::new();
    let missing = work.path().join("nothing.md");
    let run = work.build(&[&missing], &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    assert!(run.stderr.contains("cannot read"), "{}", run.stderr);
    assert!(run.stderr.contains("nothing.md"), "{}", run.stderr);
}

#[test]
fn an_output_directory_that_is_a_file_exits_1_and_says_why() {
    let work = Workspace::new();
    let input = work.file("Resume.md", &fixture("full.md"));
    work.file("out", "I am a file");
    let run = work.build(&[&input], &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    // Unix says the path is "Not a directory" when it looks at `out/Resume.pdf`; Windows says the path is
    // missing and fails when it tries to create `out`. Both name the output directory's path.
    assert!(
        run.stderr.contains("Not a directory") || run.stderr.contains("output directory"),
        "{}",
        run.stderr
    );
    assert!(run.stderr.contains("out"), "{}", run.stderr);
    assert!(!run.stderr.contains("panicked"), "{}", run.stderr);
}

// ============================================================================
// Fonts
// ============================================================================

#[test]
fn fallback_font_reports_note_or_warning_by_metric_compatibility() {
    let work = Workspace::new();
    let input = work.file("Resume.md", &fixture("full.md"));

    // The built-in style's fallbacks have the same widths: a note, which a person is not shown (the fonts
    // line already says "fallback for") and which JSON keeps.
    let human = work.build(&[&input], &[]);
    assert!(!human.stderr.contains("same character widths"), "{}", human.stderr);
    assert!(!human.stderr.contains("note"), "{}", human.stderr);
    let json = work.build(&[&input], &["--format", "json"]);
    assert!(json.stdout.contains("font-fallback-metric"), "{}", json.stdout);

    // A fallback with different widths can change the line breaks: a warning, always shown.
    let styles = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/styles");
    let different = [
        "--styles-dir",
        &styles.display().to_string(),
        "--style",
        "different-metrics",
    ];
    let human = work.build(&[&input], &different);
    assert!(human.stderr.contains("so line breaks may change"), "{}", human.stderr);
    assert!(human.stderr.contains("(fallback for Georgia)"), "{}", human.stderr);
    let json = work.build(&[&input], &[&different[..], &["--format", "json"]].concat());
    assert!(json.stdout.contains("font-fallback-layout"), "{}", json.stdout);
}

// ============================================================================
// JSON
// ============================================================================

#[test]
fn json_format_writes_only_diagnostics_to_stdout() {
    let work = Workspace::new();
    let input = work.file("Resume.md", &fixture("full.md"));
    let run = work.build(&[&input], &["--format", "json"]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert!(!run.stdout.is_empty(), "the sparse-page finding is JSON on stdout");
    for line in run.stdout.lines() {
        let value: serde_json::Value = assert_ok!(serde_json::from_str(line), "{line}");
        assert_eq!(value["type"], "diagnostic", "{line}");
    }
    // The metric-compatible notes are hidden from human output but not from JSON.
    assert!(run.stdout.contains("font-fallback-metric"), "{}", run.stdout);
    assert!(
        run.stderr.starts_with("ok "),
        "the summary stays on stderr: {}",
        run.stderr
    );
}

/// A stdout that fails every write with `kind`.
struct FailingStdout(io::ErrorKind);

impl io::Write for FailingStdout {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(self.0.into())
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(self.0.into())
    }
}

#[test]
fn a_closed_stdout_is_a_quiet_stop_not_an_error() {
    let work = Workspace::new();
    let input = work.file("Resume.md", &fixture("full.md"));
    let mut stderr = Buffer::no_color();
    let args = work.build_args(&[&input], &["--format", "json"]);
    let outcome = work.invoke(&args, &mut FailingStdout(io::ErrorKind::BrokenPipe), &mut stderr);
    assert_eq!(outcome, Outcome::Success, "the PDF was built");
    let text = String::from_utf8_lossy(stderr.as_slice()).into_owned();
    assert!(!text.contains("could not write"), "{text}");
    assert!(work.out().join("Resume.pdf").is_file());
}

#[test]
fn a_stdout_that_fails_for_another_reason_is_reported() {
    let work = Workspace::new();
    let input = work.file("Resume.md", &fixture("full.md"));
    let mut stderr = Buffer::no_color();
    let args = work.build_args(&[&input], &["--format", "json"]);
    work.invoke(&args, &mut FailingStdout(io::ErrorKind::PermissionDenied), &mut stderr);
    let text = String::from_utf8_lossy(stderr.as_slice()).into_owned();
    assert!(text.contains("could not write diagnostics"), "{text}");
}

#[test]
fn the_summary_marker_is_green_when_it_worked_and_red_when_it_did_not() {
    let work = Workspace::new();
    let good = work.file("Resume.md", &fixture("full.md"));
    let bad = work.file("errors-role.md", &fixture("errors-role.md"));
    let colored = |input: &Path| {
        let mut stderr = Buffer::ansi();
        work.invoke(&work.build_args(&[input], &[]), &mut Vec::new(), &mut stderr);
        String::from_utf8_lossy(stderr.as_slice()).into_owned()
    };
    let (green, red) = ("\u{1b}[1m\u{1b}[32mok", "\u{1b}[1m\u{1b}[31merror");
    assert!(colored(&good).contains(green), "{:?}", colored(&good));
    assert!(colored(&bad).contains(red), "{:?}", colored(&bad));
}

// ============================================================================
// Structure
// ============================================================================

#[test]
fn commands_never_touch_the_file_system_directly() {
    // Every write goes through `output::Writer`, so `--dry-run` cannot be bypassed by a new command. Reading
    // is fine.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/commands");
    for entry in assert_ok!(fs::read_dir(&dir)) {
        let path = assert_ok!(entry).path();
        let source = assert_ok!(fs::read_to_string(&path));
        for forbidden in [
            "fs::write",
            "File::create",
            "create_dir",
            "OpenOptions",
            "fs::copy",
            "fs::rename",
        ] {
            assert!(
                !source.contains(forbidden),
                "{} writes the file system directly with `{forbidden}`",
                path.display()
            );
        }
    }
}
