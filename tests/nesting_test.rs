//! ABOUTME: Deeply nested styles must end in a diagnostic or a PDF, never a stack overflow that aborts.
//! ABOUTME: Each case runs in a child process, because an overflow aborts the whole test harness.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use claims::assert_ok;
use resume::model::StyleName;
use resume::style::StyleSource;
use resume::typeset::fonts::FontSet;
use resume::typeset::{RenderOptions, Typesetter};

const DEPTH_ENV: &str = "RESUME_TEST_NEST_DEPTH";
const OUT_ENV: &str = "RESUME_TEST_NEST_OUT";

/// A style whose body nests `depth` blocks.
fn nested_style(depth: usize) -> String {
    format!(
        "#set page(paper: sys.inputs.paper)\n{}x{}\n",
        "#block(inset: 1pt)[".repeat(depth),
        "]".repeat(depth)
    )
}

/// Child entry point: compiles a style nested to `$RESUME_TEST_NEST_DEPTH` and records the outcome.
#[test]
#[ignore = "child process entry point; run by the nesting tests"]
fn child_nesting() {
    let depth: usize = assert_ok!(std::env::var(DEPTH_ENV)).parse().expect("a depth");
    let out = assert_ok!(std::env::var(OUT_ENV));
    let tmp = assert_ok!(tempfile::tempdir());
    assert_ok!(std::fs::create_dir(tmp.path().join("nested")));
    assert_ok!(std::fs::write(tmp.path().join("nested/style.typ"), nested_style(depth)));
    let style = assert_ok!(StyleSource::directory(
        tmp.path(),
        assert_ok!(StyleName::try_new("nested"))
    ));
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/full.md");
    let text = assert_ok!(std::fs::read_to_string(&path));
    let resume = resume::markdown::parse(&path, &text).into_parts().0.expect("a resume");
    let outcome =
        match Typesetter::new(Arc::new(FontSet::bundled())).compile(&resume, &style, &RenderOptions::default()) {
            Ok(_) => "compiled".to_string(),
            Err(resume::error::Error::Compile { diagnostics, .. }) => {
                format!("diagnostic: {}", diagnostics.first().map_or("", |d| d.message.as_str()))
            }
            Err(other) => format!("error: {other}"),
        };
    assert_ok!(std::fs::write(out, outcome));
}

struct Outcome {
    exited_normally: bool,
    recorded: String,
}

fn nest_in_child(depth: usize, stack_env: Option<&str>) -> Outcome {
    let dir = assert_ok!(tempfile::tempdir());
    let out = dir.path().join("outcome.txt");
    let mut command = Command::new(assert_ok!(std::env::current_exe()));
    command
        .args(["--ignored", "--exact", "child_nesting", "--test-threads=1"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .env(DEPTH_ENV, depth.to_string())
        .env(OUT_ENV, &out);
    if let Some(bytes) = stack_env {
        command.env("RUST_MIN_STACK", bytes);
    }
    let status = assert_ok!(command.status());
    Outcome {
        exited_normally: status.success(),
        recorded: std::fs::read_to_string(&out).unwrap_or_default(),
    }
}

/// Typst's layout depth cap, as measured with this nesting shape: 34 compiles and 36 does not.
/// Pinned so a Typst upgrade that moves the cap shows up here, not as a user's crash.
const LARGEST_DEPTH_UNDER_LAYOUT_CAP: usize = 34;
const A_QUARTER_OF_THE_DEFAULT_WORKER_STACK: &str = "524288";

#[test]
fn nesting_just_under_the_layout_cap_compiles() {
    let o = nest_in_child(LARGEST_DEPTH_UNDER_LAYOUT_CAP, None);
    assert!(o.exited_normally);
    assert_eq!(o.recorded, "compiled");
}

#[test]
fn nesting_just_under_the_cap_survives_quarter_size_worker_stacks() {
    let o = nest_in_child(
        LARGEST_DEPTH_UNDER_LAYOUT_CAP,
        Some(A_QUARTER_OF_THE_DEFAULT_WORKER_STACK),
    );
    assert!(o.exited_normally, "the child crashed with a small worker stack");
    assert_eq!(o.recorded, "compiled");
}

#[test]
fn nesting_beyond_the_layout_cap_is_a_diagnostic_not_a_crash() {
    for depth in [LARGEST_DEPTH_UNDER_LAYOUT_CAP + 2, 100] {
        let o = nest_in_child(depth, None);
        assert!(o.exited_normally, "depth {depth} crashed the child");
        assert_eq!(o.recorded, "diagnostic: maximum layout depth exceeded", "depth {depth}");
    }
}

#[test]
fn nesting_beyond_the_parse_cap_is_a_diagnostic_not_a_crash() {
    let o = nest_in_child(1000, None);
    assert!(o.exited_normally);
    assert_eq!(o.recorded, "diagnostic: maximum parsing depth exceeded");
}
