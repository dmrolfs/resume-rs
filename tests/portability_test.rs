//! ABOUTME: Guards byte-exact fixtures against line-ending conversion by git on Windows checkouts.
//! ABOUTME: Expected bytes are hardcoded escapes, so this file's own line endings never matter.

use std::{fs, io, path::PathBuf};

const CRLF_DOC: &[u8] = b"# Jane Doe\r\n\r\n- one\r\n- two\r\n";
const MIXED_DOC: &[u8] = b"a\r\nb\nc\r\n";
const LF_DOC: &[u8] = b"# Jane Doe\n\n- one\n- two\n";

/// Builds a fixture path from components so no separator is hardcoded.
fn fixture(components: &[&str]) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests");
    path.push("fixtures");
    path.extend(components);
    path
}

#[test]
fn crlf_fixture_keeps_every_byte() -> io::Result<()> {
    let bytes = fs::read(fixture(&["crlf", "crlf.md"]))?;
    assert_eq!(bytes, CRLF_DOC, "git changed the CRLF fixture; check .gitattributes");
    Ok(())
}

#[test]
fn mixed_fixture_keeps_every_byte() -> io::Result<()> {
    let bytes = fs::read(fixture(&["crlf", "mixed.md"]))?;
    assert_eq!(
        bytes, MIXED_DOC,
        "git changed the mixed line-ending fixture; check .gitattributes"
    );
    Ok(())
}

#[test]
fn lf_fixture_has_no_carriage_returns() -> io::Result<()> {
    let bytes = fs::read(fixture(&["lf", "lf.md"]))?;
    assert_eq!(
        bytes, LF_DOC,
        "git converted an LF fixture to CRLF; check .gitattributes eol=lf"
    );
    Ok(())
}
