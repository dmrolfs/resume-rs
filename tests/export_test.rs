//! ABOUTME: `resume export`: resume.json plus the style and lib in a directory a Typst preview can open.
//! ABOUTME: The Typst World serves /resume.json from memory, so the file on disk is checked against the model directly.

use std::fs;
use std::path::Path;

use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;
use resume::commands::Outcome;
use resume::markdown::parse;
use resume::style::StyleSource;
use serde_json::Value;

#[path = "common/workspace.rs"]
mod workspace;
use workspace::{Workspace, fixture};

fn read(path: &Path) -> Vec<u8> {
    assert_ok!(fs::read(path), "{}", path.display())
}

fn names_under(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(here) = stack.pop() {
        for entry in assert_ok!(fs::read_dir(&here)) {
            let path = assert_ok!(entry).path();
            if path.is_dir() {
                stack.push(path);
            } else {
                found.push(
                    path.strip_prefix(dir)
                        .expect("under dir")
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    found.sort();
    found
}

fn setup() -> (Workspace, std::path::PathBuf, std::path::PathBuf) {
    let work = Workspace::new();
    let file = work.file("Resume.md", &fixture("full.md"));
    let dir = work.path().join("preview");
    (work, file, dir)
}

#[test]
fn export_writes_resume_json_matching_model() {
    let (work, file, dir) = setup();
    let run = work.export(&file, &dir, &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let text = fixture("full.md");
    let parsed = parse("Resume.md", &text);
    let model = assert_some!(parsed.valid());
    let bytes = read(&dir.join("resume.json"));
    let on_disk: Value = assert_ok!(serde_json::from_slice(&bytes));
    assert_eq!(on_disk, assert_ok!(serde_json::to_value(model)));
    assert!(bytes.ends_with(b"}\n"), "a file a person opens ends with a newline");
    assert!(!bytes.contains(&b'\r'), "the same bytes on every platform");
}

#[test]
fn the_exported_files_are_the_style_and_the_lib_and_resume_json() {
    let (work, file, dir) = setup();
    work.export(&file, &dir, &[]);
    let names = names_under(&dir);
    assert!(names.contains(&"resume.json".to_string()), "{names:?}");
    assert!(names.contains(&"pragmatic/style.typ".to_string()), "{names:?}");
    assert!(names.contains(&"lib/components.typ".to_string()), "{names:?}");
    assert!(
        names
            .iter()
            .all(|n| n == "resume.json" || n.starts_with("pragmatic/") || n.starts_with("lib/")),
        "{names:?}"
    );
}

#[test]
fn exported_dir_compiles_with_world() {
    // The Typst World answers /resume.json from memory, so this proves the style files are complete; the data
    // file is proved by `export_writes_resume_json_matching_model`.
    let (work, file, dir) = setup();
    work.export(&file, &dir, &[]);
    let text = fixture("full.md");
    let parsed = parse("Resume.md", &text);
    let model = assert_some!(parsed.valid());
    let fonts = std::sync::Arc::new(assert_ok!(
        resume::typeset::fonts::FontSet::builder()
            .mode(resume::model::options::FontsMode::Bundled)
            .build()
    ));
    let typesetter = resume::typeset::Typesetter::new(fonts);
    let options = resume::typeset::RenderOptions::default();
    let name = assert_ok!(resume::model::StyleName::try_new("pragmatic"));
    let exported = assert_ok!(StyleSource::directory(&dir, name.clone()));
    let built_in = assert_ok!(resume::style::resolve(&name, None));
    let pdf = |style: &StyleSource| {
        let compiled = assert_ok!(typesetter.compile(model, style, options));
        assert_ok!(compiled.export_pdf("Resume"))
    };
    let (from_export, from_built_in) = (pdf(&exported), pdf(&built_in));
    assert_eq!(
        from_export.len(),
        from_built_in.len(),
        "the exported style builds a different PDF"
    );
    assert!(
        from_export == from_built_in,
        "the exported style builds a different PDF"
    );
}

#[test]
fn export_dry_run_lists_files() {
    let (work, file, dir) = setup();
    let run = work.export(&file, &dir, &["--dry-run"]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert!(!dir.exists(), "a dry run creates nothing, not even the directory");
    assert!(run.stderr.contains("would create "), "{}", run.stderr);
    assert!(
        run.stderr.contains("preview"),
        "the hint is shown in a dry run too: {}",
        run.stderr
    );
    assert!(run.stderr.contains("ok would export pragmatic to "), "{}", run.stderr);
}

#[test]
fn a_second_export_refreshes_resume_json_and_keeps_an_edited_style_file() {
    let (work, file, dir) = setup();
    work.export(&file, &dir, &[]);
    let theme = dir.join("pragmatic/theme.typ");
    assert_ok!(fs::write(&theme, "// my edit\n"));
    let edited = fixture("full.md").replace("Ada Lovelace", "Ada Byron");
    assert_ok!(fs::write(&file, &edited));
    let run = work.export(&file, &dir, &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert_eq!(
        read(&theme),
        b"// my edit\n",
        "an edited style file survives a re-export"
    );
    let json: Value = assert_ok!(serde_json::from_slice(&read(&dir.join("resume.json"))));
    assert!(json.to_string().contains("Ada Byron"), "resume.json is refreshed");
    assert!(
        run.stderr.contains(&format!(
            "kept {}",
            Path::new("preview").join("pragmatic/theme.typ").display()
        )),
        "{}",
        run.stderr
    );
    assert!(run.stderr.contains("replaced "), "{}", run.stderr);
    assert!(run.stderr.contains("use --force"), "{}", run.stderr);
}

#[test]
fn force_replaces_the_style_files_too() {
    let (work, file, dir) = setup();
    work.export(&file, &dir, &[]);
    let theme = dir.join("pragmatic/theme.typ");
    assert_ok!(fs::write(&theme, "// my edit\n"));
    let run = work.export(&file, &dir, &["--force"]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert_ne!(read(&theme), b"// my edit\n");
    assert!(!run.stderr.contains("kept"), "{}", run.stderr);
}

#[test]
fn a_dry_run_says_what_a_real_run_would_keep_and_replace() {
    let (work, file, dir) = setup();
    work.export(&file, &dir, &[]);
    let run = work.export(&file, &dir, &["--dry-run"]);
    assert!(run.stderr.contains("would keep "), "{}", run.stderr);
    assert!(run.stderr.contains("would replace "), "{}", run.stderr);
    assert!(!run.stderr.contains("would create "), "{}", run.stderr);
}

#[test]
fn a_resume_with_errors_exports_nothing_and_exits_1() {
    let work = Workspace::new();
    let file = work.file("Bad.md", "## Experience\n\n- x\n");
    let dir = work.path().join("preview");
    let run = work.export(&file, &dir, &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    assert!(run.stderr.contains("nothing exported"), "{}", run.stderr);
    assert!(
        run.stderr.contains("missing-name"),
        "the finding is shown: {}",
        run.stderr
    );
    assert!(!dir.exists());
}

#[test]
fn a_warning_does_not_stop_the_export() {
    let work = Workspace::new();
    let file = work.file("Warn.md", &fixture("warnings-fixable.md"));
    let dir = work.path().join("preview");
    let run = work.export(&file, &dir, &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert!(dir.join("resume.json").is_file());
}

#[test]
fn an_unknown_style_and_a_missing_input_exit_1_and_write_nothing() {
    let (work, file, dir) = setup();
    let run = work.export(&file, &dir, &["--style", "nope"]);
    assert_eq!(run.outcome, Outcome::Failure);
    assert!(run.stderr.contains("unknown style `nope`"), "{}", run.stderr);
    assert!(!dir.exists());
    let run = work.export(&work.path().join("missing.md"), &dir, &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    assert!(run.stderr.contains("cannot read"), "{}", run.stderr);
}

#[test]
fn a_target_that_is_a_directory_stops_the_export_before_anything_is_written() {
    let (work, file, dir) = setup();
    assert_ok!(fs::create_dir_all(dir.join("resume.json")));
    let run = work.export(&file, &dir, &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    assert!(run.stderr.contains("is a directory"), "{}", run.stderr);
    assert!(!dir.join("pragmatic").exists(), "nothing was written");
}

#[test]
fn a_directory_style_is_exported_with_its_own_files_and_the_lib() {
    let (work, file, dir) = setup();
    let styles = work.path().join("styles");
    work.export(&file, &styles, &[]);
    // Edit the exported style, then export it again from the styles directory.
    assert_ok!(fs::write(styles.join("pragmatic/theme.typ"), "// custom\n"));
    assert_ok!(fs::write(styles.join("pragmatic/.DS_Store"), "junk"));
    let run = work.export(&file, &dir, &["--styles-dir", &styles.display().to_string()]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert_eq!(read(&dir.join("pragmatic/theme.typ")), b"// custom\n");
    assert!(!dir.join("pragmatic/.DS_Store").exists(), "dot-files are left out");
    assert!(dir.join("lib/components.typ").is_file());
}

#[cfg(unix)]
#[test]
fn a_style_file_that_links_outside_the_styles_directory_fails_the_export_with_nothing_written() {
    let (work, file, _dir) = setup();
    let styles = work.path().join("styles");
    work.export(&file, &styles, &[]);
    let outside = work.file("secret.txt", "private");
    assert_ok!(fs::remove_file(styles.join("pragmatic/theme.typ")));
    assert_ok!(std::os::unix::fs::symlink(&outside, styles.join("pragmatic/theme.typ")));
    let out = work.path().join("copy");
    let run = work.export(&file, &out, &["--styles-dir", &styles.display().to_string()]);
    assert_eq!(run.outcome, Outcome::Failure);
    assert!(run.stderr.contains("cannot read the style file"), "{}", run.stderr);
    assert!(!out.exists(), "nothing was written");
}

#[test]
fn json_format_gives_findings_as_json_lines_and_file_lines_on_stderr() {
    let work = Workspace::new();
    let file = work.file("Warn.md", &fixture("warnings-fixable.md"));
    let dir = work.path().join("preview");
    let run = work.run(&[
        "--format".into(),
        "json".into(),
        "export".into(),
        file.display().to_string(),
        dir.display().to_string(),
    ]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert!(
        !run.stdout.trim().is_empty(),
        "the findings are on stdout: {}",
        run.stderr
    );
    for line in run.stdout.lines() {
        let finding: Value = assert_ok!(serde_json::from_str(line), "{line}");
        assert!(finding.get("code").is_some(), "{line}");
    }
    assert!(run.stderr.contains("created "), "{}", run.stderr);
}

#[test]
fn the_preview_hint_names_the_typst_command_with_the_paths_as_typed() {
    let (work, file, dir) = setup();
    let run = work.export(&file, &dir, &[]);
    let hint = assert_some!(run.stderr.lines().find(|l| l.starts_with("preview: ")));
    let shown = Path::new("preview");
    assert_eq!(
        hint,
        format!(
            "preview: typst watch --root {} {}  (add `--input paper=a4` for A4)",
            shown.display(),
            shown.join("pragmatic").join("style.typ").display()
        )
    );
}

#[test]
fn the_closing_line_counts_every_file_and_resume_json_is_written_last() {
    let (work, file, dir) = setup();
    let first = work.export(&file, &dir, &[]);
    let total = names_under(&dir).len();
    let created: Vec<&str> = first.stderr.lines().filter(|l| l.starts_with("created ")).collect();
    assert_eq!(created.len(), total);
    assert!(
        assert_some!(created.last()).ends_with("resume.json"),
        "{}",
        first.stderr
    );
    assert!(
        first.stderr.contains(&format!("({total} files: {total} created)")),
        "{}",
        first.stderr
    );
    assert!(!first.stderr.contains("use --force"), "{}", first.stderr);
    let again = work.export(&file, &dir, &[]);
    assert!(
        again
            .stderr
            .contains(&format!("({total} files: 1 replaced, {} kept); use --force", total - 1)),
        "{}",
        again.stderr
    );
    let forced = work.export(&file, &dir, &["--force"]);
    assert!(
        forced.stderr.contains(&format!("({total} files: {total} replaced)")),
        "{}",
        forced.stderr
    );
    assert!(!forced.stderr.contains("use --force"), "{}", forced.stderr);
}

#[test]
fn a_dry_run_reports_exactly_what_the_real_run_then_does() {
    let (work, file, dir) = setup();
    work.export(&file, &dir, &[]);
    assert_ok!(fs::remove_file(dir.join("pragmatic/README.md")));
    let dry = work.export(&file, &dir, &["--dry-run"]);
    let real = work.export(&file, &dir, &[]);
    let plain = |text: &str| -> Vec<String> {
        text.lines()
            .map(|line| {
                line.replace("would create", "created")
                    .replace("would replace", "replaced")
                    .replace("would keep", "kept")
                    .replace("would export", "exported")
            })
            .collect()
    };
    assert_eq!(plain(&dry.stderr), plain(&real.stderr));
}

#[test]
fn a_dir_that_is_a_file_fails_with_nothing_written() {
    let (work, file, _dir) = setup();
    let dir = work.file("preview", "i am a file");
    let run = work.export(&file, &dir, &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    assert_eq!(read(&dir), b"i am a file");
    assert!(run.stderr.contains("error:"), "{}", run.stderr);
}

#[test]
fn a_read_only_resume_json_stops_the_export_before_any_style_file_is_written() {
    let (work, file, dir) = setup();
    assert_ok!(fs::create_dir_all(&dir));
    let json = dir.join("resume.json");
    assert_ok!(fs::write(&json, "{}"));
    let mut permissions = assert_ok!(fs::metadata(&json)).permissions();
    permissions.set_readonly(true);
    assert_ok!(fs::set_permissions(&json, permissions));
    let run = work.export(&file, &dir, &[]);
    let mut writable = assert_ok!(fs::metadata(&json)).permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    writable.set_readonly(false);
    assert_ok!(fs::set_permissions(&json, writable));
    assert_eq!(run.outcome, Outcome::Failure);
    assert!(run.stderr.contains("is read-only"), "{}", run.stderr);
    assert!(!dir.join("pragmatic").exists(), "nothing was written");
}

#[test]
fn a_style_other_than_the_default_is_exported_and_named_in_the_closing_line() {
    let (work, file, dir) = setup();
    let run = work.export(&file, &dir, &["-s", "mono"]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert!(dir.join("mono/style.typ").is_file());
    assert!(!dir.join("pragmatic").exists());
    assert!(run.stderr.contains("ok exported mono to preview"), "{}", run.stderr);
}

#[test]
fn lib_is_not_a_style() {
    let (work, file, dir) = setup();
    let run = work.export(&file, &dir, &["-s", "lib"]);
    assert_eq!(run.outcome, Outcome::Failure);
    assert!(run.stderr.contains("unknown style `lib`"), "{}", run.stderr);
    assert!(!dir.exists());
}

#[test]
fn export_writes_the_resume_a_build_would_compile_without_its_drafts() {
    let work = Workspace::new();
    let text = "# Ada Lovelace\n\nLondon · ada@example.com\n\n## Experience\n\n### Acme · Engineer · 2015 – 2017\n\n\
        - Shipped the ready result.\n- Wrote the unfinished claim. <!-- @draft -->\n";
    let file = work.file("Resume.md", text);
    let dir = work.path().join("preview");
    let run = work.export(&file, &dir, &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let json = String::from_utf8(read(&dir.join("resume.json"))).expect("UTF-8");
    assert!(json.contains("Shipped the ready result."), "{json}");
    assert!(
        !json.contains("unfinished claim"),
        "a draft reached the preview: {json}"
    );
}
