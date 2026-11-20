//! ABOUTME: In-process tests of `resume doctor` and `resume check`: the grouped report, --explain, and --fix.
//! ABOUTME: Real files in a temporary directory, so what is written (and what is not) is what a user would see.

use std::fs;

use claims::{assert_ok, assert_some};
use pretty_assertions::assert_eq;
use resume::commands::Outcome;

#[path = "common/workspace.rs"]
mod workspace;
use workspace::{Run, Workspace, fixture};

fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_string()).collect()
}

/// A resume with fixable format findings (a role separator, a period hyphen and `Current`, a hard break).
const MESSY: &str =
    "# Ada\n\nLondon\n\n## Experience\n\n### Acme | Engineer | 2015 - Current\n\n- Led 3 teams.  \n  More.\n";

/// A resume with no findings of any category.
const CLEAN: &str =
    "# Ada\n\nLondon\n\n## Experience\n\n### Acme · Engineer · 2015 – 2017\n\n- Cut costs 40% in 2 quarters.\n";

fn headers(run: &Run) -> Vec<&str> {
    run.stderr
        .lines()
        .filter(|line| {
            ["Format ", "Content", "Layout ", "Setup  "]
                .iter()
                .any(|h| line.starts_with(h))
        })
        .collect()
}

// ============================================================================
// The grouped report
// ============================================================================

#[test]
fn doctor_groups_findings_by_category_in_a_fixed_order_with_why_and_fix() {
    let work = Workspace::new();
    let file = work.file("Resume.md", MESSY);
    let run = work.doctor(&[&file], &[]);
    let order: Vec<&str> = headers(&run)
        .iter()
        .map(|h| h.split_whitespace().next().unwrap_or(""))
        .collect();
    assert_eq!(order, ["Format", "Content", "Layout", "Setup"], "{}", run.stderr);
    assert!(run.stderr.contains("= why: "), "{}", run.stderr);
    assert!(run.stderr.contains("= fix: "), "{}", run.stderr);
    assert!(run.stderr.contains("error[role-separator]"), "{}", run.stderr);
    assert!(run.stderr.contains("warning[period-hyphen]"), "{}", run.stderr);
    assert_eq!(run.stdout, "", "human output must not use stdout");
}

#[test]
fn a_clean_resume_reports_ok_everywhere_and_exits_0() {
    let work = Workspace::new();
    let file = work.file("Resume.md", CLEAN);
    let run = work.doctor(&[&file], &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let found = headers(&run);
    assert!(
        found[0].ends_with("ok") && found[1].ends_with("ok") && found[2].ends_with("ok"),
        "{found:?}"
    );
    assert!(found[3].starts_with("Setup   ok (style pragmatic"), "{found:?}");
    assert!(
        run.stderr.lines().any(|l| l.starts_with("ok 0 errors, 0 warnings")),
        "{}",
        run.stderr
    );
}

#[test]
fn the_advice_is_printed_in_full_once_per_code_and_pointed_to_after() {
    let work = Workspace::new();
    let file = work.file(
        "Resume.md",
        "# Ada\n\nLondon\n\n## Experience\n\n### Acme · 2015 – 2017\n\n- Built a thing.\n- Built another thing.\n",
    );
    let run = work.doctor(&[&file], &[]);
    assert_eq!(
        run.stderr.matches("warning[unquantified-bullet]").count(),
        2,
        "{}",
        run.stderr
    );
    let why = "The Tech Resume Inside Out says to quantify impact";
    assert_eq!(
        run.stderr.matches(why).count(),
        1,
        "the advice must print once: {}",
        run.stderr
    );
    assert!(
        run.stderr
            .contains("the why and fix are under the first `unquantified-bullet` above"),
        "{}",
        run.stderr
    );
}

#[test]
fn exit_is_1_only_for_errors_not_for_warnings() {
    let work = Workspace::new();
    let warnings = work.file(
        "Warn.md",
        "# Ada\n\nLondon\n\n## Experience\n\n### Acme · 2015 - 2017\n\n- Built a thing.\n",
    );
    let run = work.doctor(&[&warnings], &[]);
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    let error = work.file("Err.md", MESSY);
    assert_eq!(work.doctor(&[&error], &[]).outcome, Outcome::Failure);
}

#[test]
fn the_role_map_is_under_layout_for_a_resume_of_several_pages() {
    let work = Workspace::new();
    let file = work.file("Resume.md", &fixture("full.md"));
    let run = work.doctor(&[&file], &[]);
    let line = assert_some!(run.stderr.lines().find(|l| l.trim_start().starts_with("= roles: ")));
    assert_eq!(
        line.trim(),
        "= roles: Analytical Engines Ltd p1; Babbage & Co p1; Consulting and Early-Stage Companies p2; University of London p2",
        "{}",
        run.stderr
    );
}

#[test]
fn a_page_limit_overrun_is_a_layout_error_with_a_role_map() {
    let work = Workspace::new();
    let file = work.file("Resume.md", &fixture("full.md"));
    let run = work.doctor(&[&file], &["--max-pages", "1"]);
    assert_eq!(run.outcome, Outcome::Failure);
    assert!(run.stderr.contains("error[page-limit-exceeded]"), "{}", run.stderr);
    assert!(
        headers(&run)
            .iter()
            .any(|h| h.starts_with("Layout ") && h.ends_with("finding")),
        "{}",
        run.stderr
    );
}

#[test]
fn doctor_never_writes_a_pdf() {
    let work = Workspace::new();
    let file = work.file("Resume.md", CLEAN);
    work.doctor(&[&file], &[]);
    assert!(!work.out().exists(), "doctor created the output directory");
}

#[test]
fn several_files_each_get_a_report_and_the_worst_outcome_wins() {
    let work = Workspace::new();
    let good = work.file("Good.md", CLEAN);
    let bad = work.file("Bad.md", MESSY);
    let run = work.doctor(&[&good, &bad], &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    let good_at = run.stderr.find("Good.md").unwrap_or(usize::MAX);
    let bad_at = run.stderr.find("Bad.md").unwrap_or(usize::MAX);
    assert!(good_at < bad_at, "{}", run.stderr);
    assert_eq!(headers(&run).len(), 8);
}

#[test]
fn an_unreadable_file_is_an_error_and_the_others_are_still_reported() {
    let work = Workspace::new();
    let good = work.file("Good.md", CLEAN);
    let missing = work.path().join("Nothing.md");
    let run = work.doctor(&[&missing, &good], &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    assert!(run.stderr.contains("cannot read"), "{}", run.stderr);
    assert!(run.stderr.contains("Good.md"), "{}", run.stderr);
}

// ============================================================================
// Setup
// ============================================================================

#[test]
fn an_unknown_style_is_a_setup_error_reported_beside_the_other_findings() {
    let work = Workspace::new();
    let file = work.file("Resume.md", MESSY);
    let run = work.doctor(&[&file], &["--style", "nope"]);
    assert_eq!(run.outcome, Outcome::Failure);
    assert!(run.stderr.contains("error[unknown-style]"), "{}", run.stderr);
    assert!(
        run.stderr.contains("error[role-separator]"),
        "the format findings are still reported: {}",
        run.stderr
    );
    assert!(
        headers(&run)
            .iter()
            .any(|h| h.starts_with("Setup   ") && h.ends_with("finding")),
        "{}",
        run.stderr
    );
}

#[test]
fn an_unknown_style_alone_fails_the_run_even_for_a_clean_resume() {
    let work = Workspace::new();
    let file = work.file("Resume.md", CLEAN);
    let run = work.doctor(&[&file], &["--style", "nope"]);
    assert_eq!(
        run.outcome,
        Outcome::Failure,
        "an unknown style must not exit 0: {}",
        run.stderr
    );
}

#[test]
fn an_unusable_styles_dir_is_a_setup_error() {
    let work = Workspace::new();
    let file = work.file("Resume.md", CLEAN);
    let missing = work.path().join("no-such-dir").display().to_string();
    let run = work.doctor(&[&file], &["--styles-dir", &missing]);
    assert_eq!(run.outcome, Outcome::Failure);
    assert!(run.stderr.contains("error[styles-dir-unusable]"), "{}", run.stderr);
}

#[test]
fn an_unusable_font_path_is_a_setup_error() {
    let work = Workspace::new();
    let file = work.file("Resume.md", CLEAN);
    let missing = work.path().join("no-fonts").display().to_string();
    let mut run_args = args(&["doctor"]);
    run_args.push(file.display().to_string());
    run_args.extend(args(&["--font-path", &missing]));
    let run = work.run(&run_args);
    assert_eq!(run.outcome, Outcome::Failure);
    assert!(run.stderr.contains("error[font-path-unusable]"), "{}", run.stderr);
}

#[test]
fn an_output_directory_that_is_a_file_is_a_setup_error() {
    let work = Workspace::new();
    let file = work.file("Resume.md", CLEAN);
    work.file("out", "I am a file");
    let run = work.doctor(&[&file], &[]);
    assert_eq!(run.outcome, Outcome::Failure);
    assert!(run.stderr.contains("error[output-not-writable]"), "{}", run.stderr);
    assert_eq!(assert_ok!(fs::read_to_string(work.out())), "I am a file");
}

#[test]
fn the_setup_line_names_the_style_the_fonts_and_the_output() {
    let work = Workspace::new();
    let file = work.file("Resume.md", CLEAN);
    let run = work.doctor(&[&file], &[]);
    let setup = assert_some!(headers(&run).into_iter().find(|h| h.starts_with("Setup")));
    assert!(setup.contains("style pragmatic"), "{setup}");
    assert!(setup.contains("fonts: body "), "{setup}");
    assert!(setup.contains("would be created"), "{setup}");
}

// ============================================================================
// --explain
// ============================================================================

#[test]
fn explain_prints_the_advice_for_a_code_on_stdout_and_exits_0() {
    let work = Workspace::new();
    let run = work.run(&args(&["doctor", "--explain", "period-hyphen"]));
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert!(run.stdout.starts_with("period-hyphen (format)\n"), "{}", run.stdout);
    for part in [
        "Why: ",
        "Fix: ",
        "Example:",
        "  - 2015 - 2017",
        "  + 2015 \u{2013} 2017",
    ] {
        assert!(run.stdout.contains(part), "{part}: {}", run.stdout);
    }
    assert_eq!(run.stderr, "");
}

#[test]
fn explain_works_for_every_code_and_prints_its_own_advice() {
    use strum::IntoEnumIterator;
    let work = Workspace::new();
    for code in resume::diagnostics::DiagnosticCode::iter() {
        let run = work.run(&args(&["doctor", "--explain", &code.to_string()]));
        assert_eq!(run.outcome, Outcome::Success, "{code}");
        let advice = code.advice();
        assert!(
            run.stdout
                .starts_with(&format!("{code} ({})\n\n{}\n", code.category(), advice.summary)),
            "{code}: {}",
            run.stdout
        );
        assert!(
            run.stdout.contains(&format!("\nWhy: {}\n", advice.why)),
            "{code}: {}",
            run.stdout
        );
        assert!(
            run.stdout.contains(&format!("\nFix: {}\n", advice.fix)),
            "{code}: {}",
            run.stdout
        );
    }
}

#[test]
fn explain_ignores_the_format_option_like_the_other_text_commands() {
    let work = Workspace::new();
    let plain = work.run(&args(&["doctor", "--explain", "long-bullet"]));
    let json = work.run(&args(&["--format", "json", "doctor", "--explain", "long-bullet"]));
    assert_eq!(json.stdout, plain.stdout);
}

// ============================================================================
// check
// ============================================================================

#[test]
fn check_prints_the_same_report_as_doctor_and_the_same_exit() {
    let work = Workspace::new();
    let file = work.file("Resume.md", MESSY);
    let doctor = work.doctor(&[&file], &[]);
    let check = work.check(&[&file], &[]);
    assert_eq!(check.outcome, doctor.outcome);
    assert_eq!(check.stderr, doctor.stderr);
}

#[test]
fn check_never_writes_the_resume_or_anything_else() {
    let work = Workspace::new();
    let file = work.file("Resume.md", MESSY);
    let before = assert_ok!(fs::read(&file));
    let modified = assert_ok!(fs::metadata(&file)).modified();
    work.check(&[&file], &[]);
    assert_eq!(assert_ok!(fs::read(&file)), before);
    assert_eq!(assert_ok!(fs::metadata(&file)).modified().ok(), modified.ok());
    assert!(!work.out().exists());
    let entries: Vec<_> = assert_ok!(fs::read_dir(work.path())).collect();
    assert_eq!(entries.len(), 1, "check left something behind");
}

#[test]
fn check_says_fix_is_available_through_doctor() {
    let work = Workspace::new();
    let file = work.file("Resume.md", MESSY);
    let run = work.check(&[&file], &[]);
    assert!(
        run.stderr.contains("fixable with `resume doctor --fix`"),
        "{}",
        run.stderr
    );
}

// ============================================================================
// --fix
// ============================================================================

const FIXED: &str =
    "# Ada\n\nLondon\n\n## Experience\n\n### Acme · Engineer · 2015 \u{2013} Present\n\n- Led 3 teams.\n  More.\n";

#[test]
fn fix_applies_the_edits_prints_the_diff_and_reports_what_is_left() {
    let work = Workspace::new();
    let file = work.file("Resume.md", MESSY);
    let run = work.doctor(&[&file], &["--fix"]);
    assert_eq!(assert_ok!(fs::read_to_string(&file)), FIXED);
    assert_eq!(
        run.outcome,
        Outcome::Success,
        "nothing left but warnings: {}",
        run.stderr
    );
    assert!(run.stderr.contains("fixed 4 findings in"), "{}", run.stderr);
    assert!(
        run.stderr.contains("--- a/Resume.md\n+++ b/Resume.md\n"),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr.contains("-### Acme | Engineer | 2015 - Current"),
        "{}",
        run.stderr
    );
    assert!(
        run.stderr.contains("+### Acme · Engineer · 2015 \u{2013} Present"),
        "{}",
        run.stderr
    );
}

#[test]
fn fix_dry_run_prints_the_diff_and_leaves_the_file_alone() {
    let work = Workspace::new();
    let file = work.file("Resume.md", MESSY);
    let modified = assert_ok!(fs::metadata(&file)).modified().ok();
    let run = work.run(&{
        let mut a = work.assess_args("doctor", &[&file], &["--fix"]);
        a.insert(0, "--dry-run".to_string());
        a
    });
    assert_eq!(assert_ok!(fs::read_to_string(&file)), MESSY);
    assert_eq!(assert_ok!(fs::metadata(&file)).modified().ok(), modified);
    assert!(run.stderr.contains("would fix 4 findings in"), "{}", run.stderr);
    assert!(run.stderr.contains("+### Acme · Engineer"), "{}", run.stderr);
    assert_eq!(
        run.outcome,
        Outcome::Success,
        "the dry run ends the way the real run would: {}",
        run.stderr
    );
}

#[test]
fn fix_is_idempotent() {
    let work = Workspace::new();
    let file = work.file("Resume.md", MESSY);
    work.doctor(&[&file], &["--fix"]);
    let once = assert_ok!(fs::read(&file));
    let modified = assert_ok!(fs::metadata(&file)).modified().ok();
    let second = work.doctor(&[&file], &["--fix"]);
    assert_eq!(assert_ok!(fs::read(&file)), once);
    assert_eq!(
        assert_ok!(fs::metadata(&file)).modified().ok(),
        modified,
        "a second run rewrote the file"
    );
    assert!(!second.stderr.contains("--- a/"), "{}", second.stderr);
    assert!(!second.stderr.contains("fixed "), "{}", second.stderr);
}

#[test]
fn fix_preserves_crlf_line_endings() {
    let work = Workspace::new();
    let crlf = MESSY.replace('\n', "\r\n");
    let file = work.file("Resume.md", &crlf);
    work.doctor(&[&file], &["--fix"]);
    let after = assert_ok!(fs::read(&file));
    let text = String::from_utf8_lossy(&after);
    assert_eq!(
        text.matches('\n').count(),
        text.matches("\r\n").count(),
        "a bare LF appeared"
    );
    assert_eq!(text, FIXED.replace('\n', "\r\n"));
}

#[test]
fn fix_does_not_touch_content_advice() {
    let work = Workspace::new();
    let text = "# Ada\n\nLondon\n\n## Experience\n\n### Acme · 2015 – 2017\n\n- Helped with various things, etc.\n";
    let file = work.file("Resume.md", text);
    let run = work.doctor(&[&file], &["--fix"]);
    assert_eq!(assert_ok!(fs::read_to_string(&file)), text);
    assert!(run.stderr.contains("warning[weak-opener]"), "{}", run.stderr);
}

#[test]
fn fix_of_a_file_with_an_error_and_no_edit_fixes_what_it_can_and_still_fails() {
    let work = Workspace::new();
    let text = "# Ada\n\nLondon\n\n## Experience\n\n### Acme · 2015 - 2017\n\n### Broken\n\n- Led 3 teams.\n";
    let file = work.file("Resume.md", text);
    let run = work.doctor(&[&file], &["--fix"]);
    assert_eq!(run.outcome, Outcome::Failure, "{}", run.stderr);
    let after = assert_ok!(fs::read_to_string(&file));
    assert!(
        after.contains("2015 \u{2013} 2017") && after.contains("### Broken"),
        "{after}"
    );
}

#[test]
fn fix_refuses_a_file_without_a_name_and_changes_nothing() {
    let work = Workspace::new();
    let text = "## Experience\n\n### Acme · 2015 - 2017\n\n- Led 3 teams.\n";
    let file = work.file("Resume.md", text);
    let run = work.doctor(&[&file], &["--fix"]);
    assert_eq!(run.outcome, Outcome::Failure);
    assert_eq!(assert_ok!(fs::read_to_string(&file)), text);
    assert!(
        run.stderr.contains("not fixed: the file has no name heading"),
        "{}",
        run.stderr
    );
}

#[test]
fn fix_refuses_a_read_only_file_in_both_modes_and_changes_nothing() {
    let work = Workspace::new();
    let file = work.file("Resume.md", MESSY);
    let mut permissions = assert_ok!(fs::metadata(&file)).permissions();
    permissions.set_readonly(true);
    assert_ok!(fs::set_permissions(&file, permissions));
    let _writable = ReadOnlyGuard(file.clone());
    for dry in [false, true] {
        let mut a = work.assess_args("doctor", &[&file], &["--fix"]);
        if dry {
            a.insert(0, "--dry-run".to_string());
        }
        let run = work.run(&a);
        assert_eq!(run.outcome, Outcome::Failure, "dry={dry}: {}", run.stderr);
        assert!(run.stderr.contains("is read-only"), "dry={dry}: {}", run.stderr);
    }
    assert_eq!(assert_ok!(fs::read_to_string(&file)), MESSY);
}

#[cfg(unix)]
#[test]
fn fix_keeps_the_files_permissions_and_a_symlink() {
    use std::os::unix::fs::PermissionsExt;
    let work = Workspace::new();
    let real = work.file("real.md", MESSY);
    assert_ok!(fs::set_permissions(&real, fs::Permissions::from_mode(0o600)));
    let link = work.path().join("Resume.md");
    assert_ok!(std::os::unix::fs::symlink(&real, &link));
    work.doctor(&[&link], &["--fix"]);
    assert!(
        assert_ok!(fs::symlink_metadata(&link)).file_type().is_symlink(),
        "the link was replaced"
    );
    assert_eq!(assert_ok!(fs::read_to_string(&real)), FIXED);
    assert_eq!(assert_ok!(fs::metadata(&real)).permissions().mode() & 0o777, 0o600);
}

#[test]
fn fix_reports_the_state_after_the_fix_not_before() {
    let work = Workspace::new();
    let file = work.file("Resume.md", MESSY);
    let run = work.doctor(&[&file], &["--fix"]);
    assert!(
        !run.stderr.contains("role-separator"),
        "a fixed finding is still reported: {}",
        run.stderr
    );
    assert!(!run.stderr.contains("fixable"), "{}", run.stderr);
}

// ============================================================================
// The fix, exactly
// ============================================================================

#[test]
fn the_diff_is_exactly_the_changed_lines() {
    let work = Workspace::new();
    let file = work.file("Resume.md", MESSY);
    let run = work.doctor(&[&file], &["--fix"]);
    let start = assert_some!(run.stderr.find("--- a/Resume.md"));
    // The report follows the diff directly, so the diff ends at the first line that is not part of it.
    let diff: Vec<&str> = run.stderr[start..]
        .lines()
        .take_while(|l| l.starts_with(['-', '+', '@', ' ']))
        .collect();
    assert_eq!(
        diff,
        [
            "--- a/Resume.md",
            "+++ b/Resume.md",
            "@@ -4,7 +4,7 @@",
            " ",
            " ## Experience",
            " ",
            "-### Acme | Engineer | 2015 - Current",
            "+### Acme \u{b7} Engineer \u{b7} 2015 \u{2013} Present",
            " ",
            "-- Led 3 teams.  ",
            "+- Led 3 teams.",
            "   More.",
        ]
    );
}

#[test]
fn fix_keeps_a_lone_cr_and_mixed_endings_byte_for_byte() {
    let work = Workspace::new();
    let lone_cr = "# Ada\r\rLondon\r\r## Experience\r\r### Acme \u{b7} 2015 - 2017\r\r- Led 3 teams.\r";
    let file = work.file("Lone.md", lone_cr);
    work.doctor(&[&file], &["--fix"]);
    assert_eq!(
        assert_ok!(fs::read(&file)),
        lone_cr.replace("2015 - 2017", "2015 \u{2013} 2017").into_bytes()
    );

    let mixed =
        "# Ada\r\n\nLondon\r\n\n## Experience\n\r\n### Acme \u{b7} 2015 - 2017\n\n- Led 3 teams.  \r\n  More.\n";
    let file = work.file("Mixed.md", mixed);
    work.doctor(&[&file], &["--fix"]);
    let expected = mixed
        .replace("2015 - 2017", "2015 \u{2013} 2017")
        .replace("teams.  \r\n", "teams.\r\n");
    assert_eq!(assert_ok!(fs::read(&file)), expected.into_bytes());
}

#[test]
fn a_crlf_fix_reports_the_same_findings_and_exit_as_the_lf_one() {
    let work = Workspace::new();
    let lf = work.file("Lf.md", MESSY);
    let crlf = work.file("Crlf.md", &MESSY.replace('\n', "\r\n"));
    let a = work.doctor(&[&lf], &["--fix"]);
    let b = work.doctor(&[&crlf], &["--fix"]);
    assert_eq!(a.outcome, b.outcome);
    assert!(b.stderr.contains("fixed 4 findings in Crlf.md"), "{}", b.stderr);
}

#[test]
fn a_second_fix_run_does_not_rewrite_the_file() {
    let work = Workspace::new();
    let file = work.file("Resume.md", MESSY);
    work.doctor(&[&file], &["--fix"]);
    let first = assert_ok!(fs::metadata(&file));
    work.doctor(&[&file], &["--fix"]);
    let second = assert_ok!(fs::metadata(&file));
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // The Writer renames a new file over the old one, so a rewrite changes the inode; mtime can be equal.
        assert_eq!(first.ino(), second.ino(), "a second run rewrote the file");
    }
    assert_eq!(first.modified().ok(), second.modified().ok());
}

#[test]
fn a_refused_fix_says_so_once_and_the_closing_line_does_not_say_remaining() {
    let work = Workspace::new();
    let text = "## Experience\n\n### Acme \u{b7} 2015 - 2017\n\n- Led 3 teams.\n";
    let file = work.file("Resume.md", text);
    let run = work.doctor(&[&file], &["--fix"]);
    assert_eq!(run.stderr.matches("not fixed: ").count(), 1, "{}", run.stderr);
    assert!(!run.stderr.contains("remaining"), "{}", run.stderr);
}

#[test]
fn a_fix_that_cannot_be_written_is_reported_once() {
    let work = Workspace::new();
    let file = work.file("Resume.md", MESSY);
    let mut permissions = assert_ok!(fs::metadata(&file)).permissions();
    permissions.set_readonly(true);
    assert_ok!(fs::set_permissions(&file, permissions));
    let _restore = ReadOnlyGuard(file.clone());
    let run = work.doctor(&[&file], &["--fix"]);
    assert_eq!(run.stderr.matches("is read-only").count(), 1, "{}", run.stderr);
}

/// Restores write permission so the temporary directory can be removed on every platform.
struct ReadOnlyGuard(std::path::PathBuf);

impl Drop for ReadOnlyGuard {
    fn drop(&mut self) {
        if let Ok(meta) = fs::metadata(&self.0) {
            let mut permissions = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            let _ = fs::set_permissions(&self.0, permissions);
        }
    }
}

// ============================================================================
// check equals doctor
// ============================================================================

#[test]
fn check_matches_doctor_on_a_clean_a_warning_and_an_error_resume_and_in_json() {
    let work = Workspace::new();
    let cases = [
        ("Clean.md", CLEAN),
        (
            "Warn.md",
            "# Ada\n\nLondon\n\n## Experience\n\n### Acme \u{b7} 2015 - 2017\n\n- Built a thing.\n",
        ),
        ("Error.md", MESSY),
    ];
    for (name, text) in cases {
        let file = work.file(name, text);
        for json in [false, true] {
            let prefix: Vec<String> = if json { args(&["--format", "json"]) } else { Vec::new() };
            let run = |command: &str| {
                let mut a = work.assess_args(command, &[&file], &[]);
                a.splice(0..0, prefix.clone());
                work.run(&a)
            };
            let (doctor, check) = (run("doctor"), run("check"));
            assert_eq!(check.outcome, doctor.outcome, "{name} json={json}");
            assert_eq!(check.stderr, doctor.stderr, "{name} json={json}");
            assert_eq!(check.stdout, doctor.stdout, "{name} json={json}");
        }
    }
}

#[test]
fn two_files_with_the_same_code_each_carry_the_advice_in_full() {
    let work = Workspace::new();
    let text = "# Ada\n\nLondon\n\n## Experience\n\n### Acme \u{b7} 2015 \u{2013} 2017\n\n- Built a thing.\n";
    let one = work.file("One.md", text);
    let two = work.file("Two.md", text);
    let run = work.doctor(&[&one, &two], &[]);
    let why = "The Tech Resume Inside Out says to quantify impact";
    assert_eq!(run.stderr.matches(why).count(), 2, "{}", run.stderr);
}

// ============================================================================
// --format json
// ============================================================================

#[test]
fn json_has_one_record_per_finding_with_its_category_and_nothing_else_on_stdout() {
    let work = Workspace::new();
    let file = work.file(
        "Resume.md",
        "# Ada\n\nLondon\n\n## Experience\n\n### Acme | Engineer | 2015 - 2017\n\n- Helped with various things.\n",
    );
    let run = work.run(&{
        let mut a = work.assess_args("doctor", &[&file], &[]);
        a.splice(0..0, args(&["--format", "json"]));
        a
    });
    assert!(!run.stdout.is_empty());
    let mut categories = std::collections::BTreeSet::new();
    for line in run.stdout.lines() {
        let value: serde_json::Value = assert_ok!(serde_json::from_str(line), "{line}");
        assert_eq!(value["type"], "diagnostic", "{line}");
        categories.insert(value["category"].as_str().unwrap_or("").to_string());
    }
    assert!(
        categories.contains("format") && categories.contains("content"),
        "{categories:?}; stdout={:?}; stderr={}",
        run.stdout,
        run.stderr
    );
}
