//! ABOUTME: `&id` names in the baseline: every command that reads it refuses a bad id with no spec, and a good one changes nothing.
//! ABOUTME: A name is only an address, so output with ids must equal output without, byte for byte.

use std::fs;

use claims::assert_ok;
use pretty_assertions::assert_eq;
use resume::commands::Outcome;

#[path = "common/workspace.rs"]
mod workspace;
use workspace::Workspace;

const PLAIN: &str = "# Ada\n\nLondon\n\n## Summary\n\nEngineer who ships.\n\n## Experience\n\n\
### Acme · Engineer · 2015 – 2017\n\nBuilds engines.\n\n- Cut costs 40% in 2 quarters. <!-- @core #ai -->\n\
- Led a team of 4 through 3 releases. <!-- @extra #lead -->\n";

const NAMED: &str = "# Ada\n\nLondon\n\n## Summary <!-- &summary -->\n\nEngineer who ships. <!-- &intro -->\n\n## Experience\n\n\
### Acme · Engineer · 2015 – 2017 <!-- &acme -->\n\nBuilds engines. <!-- &blurb -->\n\n- Cut costs 40% in 2 quarters. <!-- &costs @core #ai -->\n\
- Led a team of 4 through 3 releases. <!-- &team @extra #lead -->\n";

const DUPLICATE: &str = "# Ada\n\nLondon\n\n## Experience\n\n### Acme · Engineer · 2015 – 2017\n\n\
- Cut costs 40% in 2 quarters. <!-- &same -->\n- Led a team of 4 through 3 releases. <!-- &same -->\n";

fn path_arg(path: &std::path::Path) -> String {
    path.display().to_string()
}

/// What a command printed with the two things that differ between any two runs taken out: the workspace's own
/// temporary directory, and how long the build took.
fn steady(work: &Workspace, text: &str) -> String {
    let text = text.replace(&work.path().display().to_string(), "<work>");
    text.lines()
        .map(|line| match line.find(" built in ") {
            Some(at) => &line[..at],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn every_command_that_reads_the_baseline_refuses_a_duplicate_id_without_a_spec() {
    let work = Workspace::new();
    let file = work.file("Resume.md", DUPLICATE);
    let dir = work.path().join("exported");
    let runs = [
        ("doctor", work.doctor(&[&file], &[])),
        ("check", work.check(&[&file], &[])),
        ("build", work.build(&[&file], &[])),
        ("export", work.export(&file, &dir, &[])),
        ("tags", work.run(&["tags".to_string(), path_arg(&file)])),
    ];
    for (command, run) in runs {
        assert_eq!(run.outcome, Outcome::Failure, "{command}: {}", run.stderr);
        assert!(run.stderr.contains("item-id-reused"), "{command}: {}", run.stderr);
    }
    assert!(
        !dir.join("resume.json").exists(),
        "export wrote nothing from an invalid resume"
    );
    assert!(
        !work.out().join("Resume.pdf").exists(),
        "build wrote no PDF from an invalid resume"
    );
}

#[test]
fn doctor_fix_leaves_ids_alone() {
    // A fix pass rewrites other findings; it must not treat an id comment as removable or invent one.
    let work = Workspace::new();
    let messy =
        "# Ada\n\nLondon\n\n## Experience\n\n### Acme | Engineer | 2015 - 2017\n\n- Led 3 teams. <!-- &teams -->\n";
    let file = work.file("Resume.md", messy);
    work.doctor(&[&file], &["--fix"]);
    let after = assert_ok!(fs::read_to_string(&file));
    assert!(after.contains("<!-- &teams -->"), "{after}");
    assert_eq!(after.matches("&").count(), 1, "no id was added: {after}");
}

#[test]
fn a_baseline_with_ids_exports_the_same_json_as_one_without() {
    let work = Workspace::new();
    let plain = work.file("Plain.md", PLAIN);
    let named = work.file("Named.md", NAMED);
    let (a, b) = (work.path().join("a"), work.path().join("b"));
    assert_eq!(work.export(&plain, &a, &[]).outcome, Outcome::Success);
    assert_eq!(work.export(&named, &b, &[]).outcome, Outcome::Success);
    let json = |dir: &std::path::Path| assert_ok!(fs::read(dir.join("resume.json")));
    assert_eq!(json(&a), json(&b), "ids are not part of what a style reads");
}

#[test]
fn a_baseline_with_ids_builds_the_same_pdf_as_one_without() {
    // Same file name, so the PDF's identity is the same and only content could differ.
    let (plain, named) = (Workspace::new(), Workspace::new());
    let a = plain.file("Resume.md", PLAIN);
    let b = named.file("Resume.md", NAMED);
    assert_eq!(plain.build(&[&a], &[]).outcome, Outcome::Success);
    assert_eq!(named.build(&[&b], &[]).outcome, Outcome::Success);
    let pdf = |w: &Workspace| assert_ok!(fs::read(w.out().join("Resume.pdf")));
    assert!(pdf(&plain) == pdf(&named), "the PDFs differ");
}

#[test]
fn ids_add_no_summary_noise_to_a_build() {
    let (plain, named) = (Workspace::new(), Workspace::new());
    let a = plain.file("Resume.md", PLAIN);
    let b = named.file("Resume.md", NAMED);
    let (ra, rb) = (plain.build(&[&a], &[]), named.build(&[&b], &[]));
    assert_eq!(
        steady(&plain, &ra.stderr),
        steady(&named, &rb.stderr),
        "the same summary, with and without ids"
    );
    assert_eq!(ra.stdout, rb.stdout);
}

#[test]
fn doctor_reports_the_same_findings_with_and_without_ids() {
    let (plain, named) = (Workspace::new(), Workspace::new());
    let a = plain.file("Resume.md", PLAIN);
    let b = named.file("Resume.md", NAMED);
    let (ra, rb) = (plain.doctor(&[&a], &[]), named.doctor(&[&b], &[]));
    assert_eq!(steady(&plain, &ra.stderr), steady(&named, &rb.stderr));
}
