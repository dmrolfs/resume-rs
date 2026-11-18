//! ABOUTME: Proves PDF output is a pure function of the input: across repeats, caches, threads, and processes.
//! ABOUTME: Cross-process cases re-run this test binary with an `#[ignore]` child entry point.

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use claims::assert_ok;
use resume::model::options::Paper;
use resume::model::{Resume, StyleName};
use resume::style::StyleSource;
use resume::typeset::fonts::FontSet;
use resume::typeset::{RenderOptions, Typesetter};
use typst::syntax::{RootedPath, VirtualPath, VirtualRoot};

const OUT_ENV: &str = "RESUME_TEST_OUT";
const PREINTERN_ENV: &str = "RESUME_TEST_PREINTERN";
const STYLE: &str = "content";

fn crate_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative)
}

fn resume_from(file: &str) -> Resume {
    let path = crate_path(file);
    let text = assert_ok!(std::fs::read_to_string(&path));
    let (resume, diagnostics) = resume::markdown::parse(&path, &text).into_parts();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    resume.expect("a resume")
}

fn render(resume: &Resume, ident: &str) -> Vec<u8> {
    let name = assert_ok!(StyleName::try_new(STYLE));
    let style = assert_ok!(StyleSource::directory(crate_path("tests/fixtures/styles"), name));
    let typesetter = Typesetter::new(Arc::new(FontSet::bundled()));
    let compiled = assert_ok!(typesetter.compile(resume, &style, &RenderOptions { paper: Paper::Letter }));
    assert_ok!(compiled.export_pdf(ident))
}

fn render_full(ident: &str) -> Vec<u8> {
    render(&resume_from("tests/fixtures/full.md"), ident)
}

/// Fails with the first differing offset instead of dumping kilobytes.
fn assert_same_bytes(left: &[u8], right: &[u8], what: &str) {
    if left == right {
        return;
    }
    let at = left.iter().zip(right).position(|(a, b)| a != b);
    panic!(
        "{what}: outputs differ (lengths {} vs {}, first difference at {at:?})",
        left.len(),
        right.len()
    );
}

/// Entry point of the child process: renders once and writes the bytes to `$RESUME_TEST_OUT`.
#[test]
#[ignore = "child process entry point; run by the cross-process tests"]
fn child_render() {
    if let Ok(count) = std::env::var(PREINTERN_ENV) {
        let count: usize = count.parse().expect("a number");
        for i in 0..count {
            let path = assert_ok!(VirtualPath::new(format!("/junk/{i}.typ")));
            RootedPath::new(VirtualRoot::Project, path).intern();
        }
    }
    let out = std::env::var(OUT_ENV).expect("the parent sets RESUME_TEST_OUT");
    assert_ok!(std::fs::write(out, render_full("fixture")));
}

/// Renders in a fresh process of this test binary with extra environment variables.
fn render_in_child(env: &[(&str, &str)]) -> Vec<u8> {
    let dir = assert_ok!(tempfile::tempdir());
    let out = dir.path().join("out.pdf");
    let status = Command::new(assert_ok!(std::env::current_exe()))
        .args(["--ignored", "--exact", "child_render", "--test-threads=1"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .env(OUT_ENV, &out)
        .envs(env.iter().copied())
        .status();
    assert!(assert_ok!(status).success(), "the child render failed ({env:?})");
    assert_ok!(std::fs::read(&out))
}

#[test]
fn repeated_in_process_renders_are_identical() {
    let first = render_full("fixture");
    assert!(first.len() > 2000, "a vacuously small PDF proves nothing");
    for round in 1..20 {
        assert_same_bytes(&first, &render_full("fixture"), &format!("render {round}"));
    }
}

#[test]
fn cold_and_warm_comemo_cache_agree() {
    let warm = render_full("fixture");
    typst::comemo::evict(0);
    let cold = render_full("fixture");
    assert_same_bytes(&warm, &cold, "after evicting the memoization cache");
}

#[test]
fn different_content_gives_different_bytes() {
    let mut other = resume_from("tests/fixtures/full.md");
    other.name = assert_ok!(resume::model::Name::try_new("Charles Babbage"));
    assert!(
        render(&other, "fixture") != render_full("fixture"),
        "the negative control did not differ"
    );
}

#[test]
fn a_different_ident_changes_the_bytes_and_the_same_ident_does_not() {
    assert!(render_full("one") != render_full("two"));
    assert_same_bytes(&render_full("one"), &render_full("one"), "the same ident");
}

#[test]
fn pdf_has_no_dates() {
    let pdf = render_full("fixture");
    let text = String::from_utf8_lossy(&pdf);
    for marker in ["CreationDate", "ModDate", "xmp:CreateDate", "xmp:ModifyDate"] {
        assert!(!text.contains(marker), "the PDF contains {marker}");
    }
}

#[test]
fn rayon_thread_count_does_not_change_bytes() {
    let one = render_in_child(&[("RAYON_NUM_THREADS", "1")]);
    let two = render_in_child(&[("RAYON_NUM_THREADS", "2")]);
    let eight = render_in_child(&[("RAYON_NUM_THREADS", "8")]);
    assert_same_bytes(&one, &two, "1 vs 2 threads");
    assert_same_bytes(&one, &eight, "1 vs 8 threads");
}

#[test]
fn file_id_numbering_does_not_change_bytes() {
    let clean = render_in_child(&[]);
    let shifted = render_in_child(&[(PREINTERN_ENV, "500")]);
    assert_same_bytes(&clean, &shifted, "with 500 file ids interned first");
}

#[test]
fn a_fresh_process_matches_this_warm_one() {
    assert_same_bytes(&render_in_child(&[]), &render_full("fixture"), "child vs parent");
}
