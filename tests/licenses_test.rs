//! ABOUTME: In-process tests of `resume licenses`: the notices every distributed binary must carry, on stdout.
//! ABOUTME: The text itself is checked in `fonts_license_test.rs`; here it is that the command prints all of it.

use claims::assert_ok;
use pretty_assertions::assert_eq;
use resume::commands::Outcome;
use resume::diagnostics::termcolor::Buffer;
use resume::typeset::fonts::bundled_font_notices;

#[path = "common/workspace.rs"]
mod workspace;
use workspace::{FailingStdout, Workspace};

fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_string()).collect()
}

#[test]
fn licenses_prints_every_notice_on_stdout_and_exits_0() {
    let work = Workspace::new();
    let run = work.run(&args(&["licenses"]));
    assert_eq!(run.outcome, Outcome::Success, "{}", run.stderr);
    assert_eq!(run.stderr, "");
    assert_eq!(run.stdout, format!("{}\n", bundled_font_notices()));
}

#[test]
fn licenses_prints_each_ofl_verbatim() {
    let work = Workspace::new();
    let run = work.run(&args(&["licenses"]));
    let fonts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fonts");
    for family in ["carlito", "gelasio", "arimo"] {
        let ofl = assert_ok!(std::fs::read_to_string(fonts.join(family).join("OFL.txt")));
        assert!(run.stdout.contains(&ofl), "{family}'s OFL.txt is not printed in full");
    }
}

#[test]
fn a_dry_run_changes_nothing_about_licenses() {
    let work = Workspace::new();
    let plain = work.run(&args(&["licenses"]));
    let dry = work.run(&args(&["--dry-run", "licenses"]));
    assert_eq!(dry.stdout, plain.stdout);
}

#[test]
fn licenses_on_a_closed_stdout_stops_quietly() {
    let work = Workspace::new();
    let mut stderr = Buffer::no_color();
    let outcome = work.invoke(
        &args(&["licenses"]),
        &mut FailingStdout(std::io::ErrorKind::BrokenPipe),
        &mut stderr,
    );
    assert_eq!(outcome, Outcome::Success);
    assert_eq!(stderr.as_slice(), b"");
}
