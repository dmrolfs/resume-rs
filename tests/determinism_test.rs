//! ABOUTME: Proves PDF output is a pure function of the input: across repeats, caches, threads, and processes.
//! ABOUTME: Cross-process cases re-run this test binary with an `#[ignore]` child entry point.

mod common;

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use claims::assert_ok;
use lopdf::Document;
use resume::model::options::Paper;
use resume::model::{Resume, StyleName};
use resume::style::StyleSource;
use resume::typeset::fonts::FontSet;
use resume::typeset::{RenderOptions, Typesetter};
use typst::syntax::{RootedPath, VirtualPath, VirtualRoot};

const OUT_ENV: &str = "RESUME_TEST_OUT";
const PREINTERN_ENV: &str = "RESUME_TEST_PREINTERN";
const STYLE: &str = "content";
const CHILD_TIMEOUT: Duration = Duration::from_secs(120);

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
    let compiled = assert_ok!(typesetter.compile(resume, &style, RenderOptions::new(Paper::Letter)));
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

/// Entry point of the child process: renders once, writes the PDF to `$RESUME_TEST_OUT` and the size of
/// rayon's global pool next to it, so the parent can check that `RAYON_NUM_THREADS` really arrived.
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
    let out = PathBuf::from(std::env::var(OUT_ENV).expect("the parent sets RESUME_TEST_OUT"));
    assert_ok!(std::fs::write(&out, render_full("fixture")));
    assert_ok!(std::fs::write(
        out.with_extension("threads"),
        rayon::current_num_threads().to_string()
    ));
}

struct ChildRender {
    pdf: Vec<u8>,
    rayon_threads: usize,
}

/// Renders in a fresh process of this test binary with the given environment and nothing else inherited
/// from the knobs this file turns.
fn render_in_child(env: &[(&str, &str)]) -> ChildRender {
    let dir = assert_ok!(tempfile::tempdir());
    let out = dir.path().join("out.pdf");
    let mut command = Command::new(assert_ok!(std::env::current_exe()));
    command
        .args(["--ignored", "--exact", "child_render", "--test-threads=1"])
        .env_remove(PREINTERN_ENV)
        .env_remove("RAYON_NUM_THREADS")
        .env(OUT_ENV, &out)
        .envs(env.iter().copied());
    let run = common::run_child(command, CHILD_TIMEOUT);
    assert!(run.success, "the child render failed ({env:?}):\n{}", run.stderr);
    ChildRender {
        pdf: assert_ok!(std::fs::read(&out)),
        rayon_threads: assert_ok!(assert_ok!(std::fs::read_to_string(out.with_extension("threads"))).parse()),
    }
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
fn the_content_fixture_has_several_pages_for_layout_to_parallelize() {
    let doc = assert_ok!(Document::load_mem(&render_full("fixture")));
    assert!(doc.get_pages().len() >= 5, "only {} pages", doc.get_pages().len());
}

#[test]
fn cold_and_warm_comemo_cache_agree() {
    let warm = render_full("fixture");
    Typesetter::evict_cache(0);
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
    let doc = assert_ok!(Document::load_mem(&pdf));
    let info = assert_ok!(assert_ok!(doc.trailer.get(b"Info")).as_reference());
    let info = assert_ok!(doc.get_dictionary(info));
    assert!(info.get(b"CreationDate").is_err() && info.get(b"ModDate").is_err());
}

#[test]
fn rayon_thread_count_does_not_change_bytes() {
    let one = render_in_child(&[("RAYON_NUM_THREADS", "1")]);
    let two = render_in_child(&[("RAYON_NUM_THREADS", "2")]);
    let eight = render_in_child(&[("RAYON_NUM_THREADS", "8")]);
    assert_eq!(
        (one.rayon_threads, two.rayon_threads, eight.rayon_threads),
        (1, 2, 8),
        "RAYON_NUM_THREADS did not reach the child's pool"
    );
    assert_same_bytes(&one.pdf, &two.pdf, "1 vs 2 threads");
    assert_same_bytes(&one.pdf, &eight.pdf, "1 vs 8 threads");
}

#[test]
fn file_id_numbering_does_not_change_bytes() {
    let clean = render_in_child(&[]);
    let shifted = render_in_child(&[(PREINTERN_ENV, "500")]);
    assert_same_bytes(&clean.pdf, &shifted.pdf, "with 500 file ids interned first");
}

#[test]
fn a_fresh_process_matches_this_warm_one() {
    assert_same_bytes(&render_in_child(&[]).pdf, &render_full("fixture"), "child vs parent");
}

/// Writes the reference PDF that CI compares across operating systems (rsm-141.15, rung 1).
///
/// Ignored by default; CI runs it with `RESUME_DETERMINISM_OUT` set on each OS and an `ubuntu-latest`
/// job compares the files byte for byte. It uses only the fonts embedded in typst-kit, so installed
/// fonts cannot reach the output, and a fixed ident, so the file name cannot either.
#[test]
#[ignore = "writes the cross-OS reference PDF; run by CI with RESUME_DETERMINISM_OUT set"]
fn write_determinism_pdf() {
    let out =
        PathBuf::from(std::env::var("RESUME_DETERMINISM_OUT").expect("set RESUME_DETERMINISM_OUT to the output path"));
    if let Some(parent) = out.parent() {
        assert_ok!(std::fs::create_dir_all(parent));
    }
    assert_ok!(std::fs::write(&out, render_full("determinism")));
}
