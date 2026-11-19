//! ABOUTME: In-process tests of `resume styles` and `resume export-style` over real directories and styles.
//! ABOUTME: They cover the listing, a directory style that wins over a built-in, and exporting without losing edits.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use claims::assert_ok;
use pretty_assertions::assert_eq;
use resume::commands::Outcome;
use resume::model::options::BuiltinStyle;

#[path = "common/workspace.rs"]
mod workspace;
use workspace::{FailingStdout, Run, Workspace, fixture};

fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_string()).collect()
}

fn minimal_style() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/styles/minimal/style.typ");
    assert_ok!(fs::read_to_string(path))
}

/// Every file under `dir`, as paths relative to it with `/` separators.
fn files_under(dir: &Path) -> BTreeSet<String> {
    fn walk(dir: &Path, base: &Path, found: &mut BTreeSet<String>) {
        for entry in assert_ok!(fs::read_dir(dir)) {
            let path = assert_ok!(entry).path();
            if path.is_dir() {
                walk(&path, base, found);
            } else {
                let relative = assert_ok!(path.strip_prefix(base));
                found.insert(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut found = BTreeSet::new();
    walk(dir, dir, &mut found);
    found
}

fn repo_styles() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("styles")
}

// ============================================================================
// resume styles
// ============================================================================

#[test]
fn styles_lists_every_embedded_builtin() {
    let work = Workspace::new();
    let run = work.run(&args(&["styles"]));
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert_eq!(run.stderr, "");
    let names: Vec<&str> = run
        .stdout
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    assert!(names.contains(&"pragmatic"), "{}", run.stdout);
    for name in &names {
        assert!(
            name.parse::<BuiltinStyle>().is_ok(),
            "{name} is listed but is not a known built-in style"
        );
    }
    assert!(run.stdout.contains("built-in"), "{}", run.stdout);
}

#[test]
fn styles_dir_overrides_builtin() {
    let work = Workspace::new();
    work.file("styles/mine/style.typ", &minimal_style());
    work.file("styles/pragmatic/style.typ", &minimal_style());
    let dir = work.path().join("styles").display().to_string();

    let listed = work.run(&args(&["styles", "--styles-dir", &dir]));
    assert_eq!(listed.outcome, Outcome::Success, "{}", listed.stderr);
    assert!(listed.stdout.contains("(overrides the built-in)"), "{}", listed.stdout);
    assert!(
        listed.stdout.lines().any(|line| line.starts_with("mine")),
        "{}",
        listed.stdout
    );

    // The directory's `pragmatic` is the one used: the one-line minimal style gives one page, the built-in two.
    let input = work.file("Resume.md", &fixture("full.md"));
    let overridden = work.build(&[&input], &["--styles-dir", &dir]);
    assert!(overridden.stderr.contains("  1 page  "), "{}", overridden.stderr);
    let built_in = work.build(&[&input], &[]);
    assert!(built_in.stderr.contains("  2 pages  "), "{}", built_in.stderr);
}

#[test]
fn styles_on_a_closed_stdout_stops_quietly_and_on_a_failing_one_says_so() {
    let work = Workspace::new();
    let mut stderr = resume::diagnostics::termcolor::Buffer::no_color();
    let closed = work.invoke(
        &args(&["styles"]),
        &mut FailingStdout(std::io::ErrorKind::BrokenPipe),
        &mut stderr,
    );
    assert_eq!(closed, Outcome::Success);
    assert_eq!(stderr.as_slice(), b"", "a closed pipe is the reader's choice");

    let mut stderr = resume::diagnostics::termcolor::Buffer::no_color();
    work.invoke(
        &args(&["styles"]),
        &mut FailingStdout(std::io::ErrorKind::PermissionDenied),
        &mut stderr,
    );
    let text = String::from_utf8_lossy(stderr.as_slice()).into_owned();
    assert!(text.contains("cannot write to stdout"), "{text}");
}

#[test]
fn styles_marks_invalid_directory_styles() {
    let work = Workspace::new();
    work.file("styles/Bad Name/style.typ", "x");
    work.file("styles/half/readme.txt", "x");
    work.file("styles/mine/style.typ", "x");
    work.file("styles/lib/shared.typ", "x");
    let dir = work.path().join("styles").display().to_string();
    let run = work.run(&args(&["styles", "--styles-dir", &dir]));
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert!(
        run.stdout.contains("(invalid: not a valid style name)"),
        "{}",
        run.stdout
    );
    assert!(run.stdout.contains("(invalid: no style.typ)"), "{}", run.stdout);
    assert!(
        !run.stdout.lines().any(|line| line.starts_with("lib")),
        "the shared lib is not a style"
    );
}

#[test]
fn a_styles_dir_that_cannot_be_read_exits_1() {
    let work = Workspace::new();
    let missing = work.path().join("nowhere").display().to_string();
    let run = work.run(&args(&["styles", "--styles-dir", &missing]));
    assert_eq!(run.outcome, Outcome::Failure);
    assert!(run.stderr.contains("cannot open styles directory"), "{}", run.stderr);
    assert_eq!(run.stdout, "");
}

#[test]
fn unknown_style_exits_1_listing_styles() {
    let work = Workspace::new();
    let input = work.file("Resume.md", &fixture("full.md"));
    let run = work.build(&[&input], &["--style", "nothing-like-it"]);
    assert_eq!(run.outcome, Outcome::Failure, "{}", run.stderr);
    assert!(run.stderr.contains("unknown style `nothing-like-it`"), "{}", run.stderr);
    assert!(run.stderr.contains("available styles: pragmatic"), "{}", run.stderr);
    assert!(!work.out().exists(), "nothing is built for an unknown style");
}

// ============================================================================
// resume export-style
// ============================================================================

/// `resume export-style pragmatic <dest>` with `extra` arguments after it.
fn export(work: &Workspace, extra: &[&str]) -> Run {
    let dest = work.path().join("mine").display().to_string();
    let mut all = args(&["export-style", "pragmatic", &dest]);
    all.extend(args(extra));
    work.run(&all)
}

fn expected_files() -> BTreeSet<String> {
    let mut expected = files_under(&repo_styles().join("pragmatic"))
        .into_iter()
        .map(|file| format!("pragmatic/{file}"))
        .collect::<BTreeSet<_>>();
    expected.extend(
        files_under(&repo_styles().join("lib"))
            .into_iter()
            .map(|file| format!("lib/{file}")),
    );
    expected
}

#[test]
fn export_style_copies_style_and_lib() {
    let work = Workspace::new();
    let run = export(&work, &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let dest = work.path().join("mine");
    assert_eq!(
        files_under(&dest),
        expected_files(),
        "exactly the style and the lib it shares"
    );
    for file in expected_files() {
        assert_eq!(
            assert_ok!(fs::read(dest.join(&file))),
            assert_ok!(fs::read(repo_styles().join(&file))),
            "{file}"
        );
    }
    assert_eq!(
        run.stderr.matches("created ").count(),
        expected_files().len(),
        "{}",
        run.stderr
    );
    assert_eq!(run.stdout, "");
}

#[test]
fn export_style_dry_run_lists_files() {
    let work = Workspace::new();
    let dest = work.path().join("mine").display().to_string();
    let run = work.run(&args(&["--dry-run", "export-style", "pragmatic", &dest]));
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert!(!work.path().join("mine").exists(), "a dry run created the directory");
    assert_eq!(
        run.stderr.matches("would create ").count(),
        expected_files().len(),
        "{}",
        run.stderr
    );
}

#[test]
fn export_style_refuses_to_overwrite_without_force() {
    let work = Workspace::new();
    assert_eq!(export(&work, &[]).outcome, Outcome::Success);
    let dest = work.path().join("mine");
    let edited = dest.join("pragmatic").join("theme.typ");
    assert_ok!(fs::write(&edited, "my edits"));
    assert_ok!(fs::remove_file(dest.join("lib").join("components.typ")));

    let run = export(&work, &[]);
    assert_eq!(run.outcome, Outcome::Failure, "{}", run.stderr);
    assert_eq!(assert_ok!(fs::read_to_string(&edited)), "my edits", "an edit was lost");
    assert!(
        !dest.join("lib").join("components.typ").exists(),
        "one existing file must stop every write, not only its own"
    );
    assert!(run.stderr.contains("already exists"), "{}", run.stderr);
    assert!(run.stderr.contains("--force"), "{}", run.stderr);
    assert!(run.stderr.contains("theme.typ"), "{}", run.stderr);
}

#[test]
fn export_style_force_replaces_existing_files() {
    let work = Workspace::new();
    assert_eq!(export(&work, &[]).outcome, Outcome::Success);
    let edited = work.path().join("mine").join("pragmatic").join("theme.typ");
    assert_ok!(fs::write(&edited, "my edits"));
    let run = export(&work, &["--force"]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert_eq!(
        assert_ok!(fs::read(&edited)),
        assert_ok!(fs::read(repo_styles().join("pragmatic").join("theme.typ")))
    );
    assert!(run.stderr.contains("replaced "), "{}", run.stderr);
}

#[test]
fn a_dry_run_with_force_says_it_would_replace() {
    let work = Workspace::new();
    assert_eq!(export(&work, &[]).outcome, Outcome::Success);
    let dest = work.path().join("mine").display().to_string();
    let run = work.run(&args(&["--dry-run", "export-style", "pragmatic", &dest, "--force"]));
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert_eq!(
        run.stderr.matches("would replace ").count(),
        expected_files().len(),
        "{}",
        run.stderr
    );
}

#[test]
fn export_style_of_an_unknown_style_exits_1_listing_styles() {
    let work = Workspace::new();
    let dest = work.path().join("mine").display().to_string();
    for name in ["nothing-like-it", "lib"] {
        let run = work.run(&args(&["export-style", name, &dest]));
        assert_eq!(run.outcome, Outcome::Failure, "{name}: {}", run.stderr);
        assert!(
            run.stderr.contains("available styles: pragmatic"),
            "{name}: {}",
            run.stderr
        );
    }
    assert!(!work.path().join("mine").exists());
}

#[test]
fn exported_style_builds_the_same_pdf_as_the_builtin() {
    let work = Workspace::new();
    assert_eq!(export(&work, &[]).outcome, Outcome::Success);
    let input = work.file("Resume.md", &fixture("full.md"));
    let pdf = work.out().join("Resume.pdf");
    let styles = work.path().join("mine").display().to_string();

    assert_eq!(work.build(&[&input], &[]).outcome, Outcome::Success);
    let built_in = assert_ok!(fs::read(&pdf));
    let exported = work.build(&[&input], &["--styles-dir", &styles, "--style", "pragmatic"]);
    assert_eq!(exported.outcome, Outcome::Success, "{}", exported.stderr);
    assert!(
        assert_ok!(fs::read(&pdf)) == built_in,
        "the exported copy builds a different PDF than the built-in style"
    );
}
