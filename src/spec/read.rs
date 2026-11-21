//! ABOUTME: Reads a spec or profile file: bounded, UTF-8 only, strict TOML, with every error located.
//! ABOUTME: The text of a toml error is read in one place, so an upgrade that changes it fails one test.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_path_to_error::Segment;
use snafu::ResultExt;

use super::error::{Located, MAX_FILE_BYTES, NotAFileSnafu, NotUtf8Snafu, ReadSnafu, SpecError, TooLargeSnafu, shown};
use super::raw::RawFile;
use crate::diagnostics::lines::LineIndex;

/// A file's text with what is needed to turn a byte offset into `file:line:column`.
#[derive(Debug)]
pub(super) struct Source {
    pub path: PathBuf,
    shared_path: Arc<Path>,
    pub text: String,
    lines: LineIndex,
}

impl Source {
    pub(super) fn new(path: PathBuf, text: String) -> Self {
        let lines = LineIndex::new(&text);
        Self {
            shared_path: path.as_path().into(),
            path,
            text,
            lines,
        }
    }

    /// The location of byte `offset`, as an editor would show it.
    pub(super) fn at(&self, offset: usize) -> Located {
        let location = self.lines.location(&self.text, offset);
        Located {
            path: Arc::clone(&self.shared_path),
            line: location.line,
            column: location.column,
        }
    }
}

/// Reads `path`: a regular file of at most [`MAX_FILE_BYTES`] bytes of UTF-8.
pub(super) fn read_source(path: &Path) -> Result<Source, SpecError> {
    read_source_shown(path, path)
}

/// Reads the file at `open`, and calls it `path` in every error and location: the file that was checked is the file
/// that is read, and the user sees the name they know it by.
pub(super) fn read_source_shown(open: &Path, path: &Path) -> Result<Source, SpecError> {
    // Asked before the file is opened, because opening a FIFO waits for a writer. `metadata` follows links, as
    // `open` does, so a link to a regular file is a regular file.
    let meta = std::fs::metadata(open).context(ReadSnafu { path })?;
    if !meta.is_file() {
        return NotAFileSnafu { path }.fail();
    }
    if meta.len() > MAX_FILE_BYTES {
        return TooLargeSnafu { path, len: meta.len() }.fail();
    }
    let file = std::fs::File::open(open).context(ReadSnafu { path })?;
    let mut bytes = Vec::new();
    // The file may have grown since `metadata`; the read is capped, and a longer file is refused all the same.
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .context(ReadSnafu { path })?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return TooLargeSnafu {
            path,
            len: bytes.len() as u64,
        }
        .fail();
    }
    let text = String::from_utf8(bytes).map_err(|e| {
        NotUtf8Snafu {
            path,
            valid_up_to: e.utf8_error().valid_up_to(),
        }
        .build()
    })?;
    Ok(Source::new(path.to_path_buf(), text))
}

/// The segment `Spanned` adds to a key path; it is not a key.
const SPANNED_SEGMENT: &str = "$__serde_spanned_private_value";

/// Parses `source` strictly: an unknown key, a wrong type, or bad syntax is a located error.
pub(super) fn parse(source: &Source) -> Result<RawFile, SpecError> {
    let deserializer = toml::Deserializer::new(&source.text);
    serde_path_to_error::deserialize::<_, RawFile>(deserializer).map_err(|e| classify(source, &e))
}

/// The names along the key path of `error`, without `Spanned`'s own segment: `["select", "pick", "gateway"]`.
fn key_segments(error: &serde_path_to_error::Error<toml::de::Error>) -> Vec<String> {
    error
        .path()
        .iter()
        .filter_map(|segment| match segment {
            Segment::Map { key } | Segment::Enum { variant: key } if key == SPANNED_SEGMENT => None,
            Segment::Map { key } => Some(key.clone()),
            Segment::Enum { variant } => Some(variant.clone()),
            Segment::Seq { index } => Some(format!("[{index}]")),
            Segment::Unknown => Some("?".to_string()),
        })
        .collect()
}

/// `select.pick.gateway`, or `extends[0]`, escaped for printing.
fn dotted(segments: &[String]) -> String {
    let mut out = String::new();
    for segment in segments {
        if !out.is_empty() && !segment.starts_with('[') {
            out.push('.');
        }
        out.push_str(segment);
    }
    shown(&out)
}

fn classify(source: &Source, error: &serde_path_to_error::Error<toml::de::Error>) -> SpecError {
    let segments = key_segments(error);
    let inner = error.inner();
    let at = source.at(inner.span().map_or(0, |range| range.start));
    let message = inner.message().lines().collect::<Vec<_>>().join("; ");
    if let Some(expected) = unknown_field_allowed(&message) {
        // The unknown key is the last segment of the path, which holds it exactly as written.
        let (unknown, table) = segments
            .split_last()
            .map_or(("", &[][..]), |(last, rest)| (last.as_str(), rest));
        let suggestion = closest(unknown, &expected).map(|name| dotted(&[table, &[name]].concat()));
        return SpecError::UnknownKey {
            at,
            key: dotted(&segments),
            expected,
            suggestion,
        };
    }
    if segments.is_empty() {
        return SpecError::Syntax { at, message };
    }
    SpecError::WrongType {
        at,
        key: dotted(&segments),
        message,
    }
}

/// The allowed keys named by toml's "unknown field `a`, expected one of `b`, `c`" message, or `None` for any other
/// message. This is the one place that reads toml's wording; a test pins it.
///
/// The unknown key itself is quoted in the message too, and a hostile key can hold a backtick or the words
/// ", expected ", so only the text after the last "`, expected " is read: it holds the allowed names, which are
/// fixed ASCII.
fn unknown_field_allowed(message: &str) -> Option<Vec<String>> {
    message.strip_prefix("unknown field `")?;
    let Some((_, allowed)) = message.rsplit_once("`, expected ") else {
        return Some(Vec::new()); // "there are no fields": nothing is allowed here
    };
    Some(allowed.split('`').skip(1).step_by(2).map(str::to_string).collect())
}

fn closest(key: &str, expected: &[String]) -> Option<String> {
    expected
        .iter()
        .map(|candidate| (strsim::jaro_winkler(key, candidate), candidate))
        .filter(|(score, _)| *score >= 0.8)
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, candidate)| candidate.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok, assert_some};
    use pretty_assertions::assert_eq;

    fn parse_text(text: &str) -> Result<RawFile, SpecError> {
        parse(&Source::new(PathBuf::from("spec.toml"), text.to_string()))
    }

    fn error_of(text: &str) -> SpecError {
        assert_err!(parse_text(text))
    }

    #[test]
    fn toml_words_an_unknown_field_the_way_this_module_reads_it() {
        // If a toml upgrade rewords the message, this fails here and nowhere else.
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Table {
            #[allow(dead_code)]
            known: Option<u8>,
            #[allow(dead_code)]
            other_known: Option<u8>,
        }
        let message = assert_some!(toml::from_str::<Table>("stray = 1").err())
            .message()
            .to_string();
        assert_eq!(
            unknown_field_allowed(&message),
            Some(vec!["known".to_string(), "other_known".to_string()])
        );
    }

    #[test]
    fn an_unknown_field_message_is_read_by_its_last_expected_clause_so_a_hostile_key_cannot_confuse_it() {
        assert_eq!(
            unknown_field_allowed("unknown field `tag`, expected one of `tags`, `pick`, `core-only`"),
            Some(vec!["tags".to_string(), "pick".to_string(), "core-only".to_string()])
        );
        assert_eq!(
            unknown_field_allowed("unknown field `a`b`, expected `c`, expected one of `x`, `y`"),
            Some(vec!["x".to_string(), "y".to_string()])
        );
        assert_eq!(
            unknown_field_allowed("unknown field `a`, expected `b`"),
            Some(vec!["b".to_string()])
        );
        assert_eq!(
            unknown_field_allowed("unknown field `a`, there are no fields"),
            Some(vec![])
        );
        assert_eq!(unknown_field_allowed("invalid type: string \"a\""), None);
    }

    #[test]
    fn an_empty_file_is_an_empty_file() {
        let raw = assert_ok!(parse_text(""));
        assert!(raw.extends.is_none() && raw.select.is_none() && raw.build.is_none());
    }

    #[test]
    fn a_misspelled_key_is_located_named_and_the_right_one_suggested() {
        match error_of("# c\n\n[select]\ntag = [\"ai\"]\n") {
            SpecError::UnknownKey {
                at,
                key,
                expected,
                suggestion,
            } => {
                assert_eq!((at.line, at.column), (4, 1));
                assert_eq!(key, "select.tag");
                assert!(expected.contains(&"tags".to_string()), "{expected:?}");
                assert_eq!(suggestion.as_deref(), Some("select.tags"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_misspelled_top_level_table_has_no_table_name() {
        match error_of("[selct]\n") {
            SpecError::UnknownKey { key, suggestion, .. } => {
                assert_eq!(key, "selct");
                assert_eq!(suggestion.as_deref(), Some("select"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_hostile_key_is_escaped_and_not_mangled() {
        // An escape character, a backtick, a dot, and the words of toml's own message, all in key names.
        for (text, shown_key) in [
            ("[build]\n\"x\\u001b[31mred\" = 1\n", "build.x\\u{1b}[31mred"),
            ("[build]\n\"a`b`c\" = 1\n", "build.a`b`c"),
            ("[select]\n\"a.b\" = 3\n", "select.a.b"),
            ("[select]\n\"q`, expected `tags\" = 3\n", "select.q`, expected `tags"),
        ] {
            match error_of(text) {
                SpecError::UnknownKey { key, expected, .. } => {
                    assert_eq!(key, shown_key, "{text:?}");
                    assert!(!key.contains('\u{1b}'));
                    assert!(
                        expected
                            .iter()
                            .all(|e| e.chars().all(|c| c.is_ascii_lowercase() || c == '-')),
                        "{expected:?}"
                    );
                }
                other => panic!("{text:?}: {other:?}"),
            }
        }
    }

    #[test]
    fn a_wrong_type_names_its_key_path_without_the_spanned_segment() {
        match error_of("[select]\ncore-only = \"yes\"\n") {
            SpecError::WrongType { at, key, .. } => {
                assert_eq!(key, "select.core-only");
                assert_eq!(at.line, 2);
            }
            other => panic!("{other:?}"),
        }
        match error_of("[select]\ntags = \"ai\"\n") {
            SpecError::WrongType { key, message, .. } => {
                assert_eq!(key, "select.tags");
                assert!(message.contains("expected a sequence"), "{message}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn wrong_types_name_their_key_and_have_a_position() {
        for (text, key, line) in [
            ("extends = [1]", "extends[0]", 1),
            ("extends = \"x\"", "extends", 1),
            ("position = 5", "position", 1),
            ("[build]\ncontent = 5", "build.content", 2),
            ("[select.pick]\ngateway = 3", "select.pick.gateway", 2),
        ] {
            match error_of(text) {
                SpecError::WrongType { key: got, at, .. } => assert_eq!((got.as_str(), at.line), (key, line), "{text}"),
                other => panic!("{text}: {other:?}"),
            }
        }
    }

    #[test]
    fn a_quoted_date_is_refused_and_a_bare_date_is_read() {
        assert_ok!(parse_text("[position]\ndate = 2026-11-19\n"));
        match error_of("[position]\ndate = \"2026-11-19\"\n") {
            SpecError::WrongType { key, .. } => assert_eq!(key, "position.date"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn every_valid_toml_spelling_of_the_position_table_is_read() {
        for text in [
            "[position.emphasis]\nbusiness = \"low\"\n",
            "position.company = \"A\"\n",
            "[position]\nemphasis.business = \"low\"\n",
            "[position]\nemphasis = { business = \"low\" }\n",
        ] {
            assert_ok!(parse_text(text), "{text:?}");
        }
    }

    #[test]
    fn the_start_of_a_position_table_is_its_earliest_value_and_an_empty_table_has_none() {
        let raw = assert_ok!(parse_text("\n\n[position.emphasis]\nbusiness = \"low\"\n"));
        let at = assert_some!(assert_some!(raw.position).first_offset());
        assert_eq!(
            Source::new(
                PathBuf::from("f"),
                "\n\n[position.emphasis]\nbusiness = \"low\"\n".into()
            )
            .at(at)
            .line,
            4
        );
        assert_eq!(
            assert_some!(assert_ok!(parse_text("[position]\n")).position).first_offset(),
            None
        );
    }

    #[test]
    fn every_table_refuses_unknown_keys_and_the_documented_non_keys() {
        for (text, key) in [
            ("typo = 1", "typo"),
            ("drafts = true", "drafts"),
            ("[letter]\nx = 1", "letter"),
            ("[position]\ncompny = \"x\"", "position.compny"),
            (
                "[position]\nemphasis = { busines = \"low\" }",
                "position.emphasis.busines",
            ),
            ("[build]\ncontnet = \"a.md\"", "build.contnet"),
            ("[build]\ndrafts = true", "build.drafts"),
            ("[select]\ndrafts = true", "select.drafts"),
            ("[select]\ncore_only = true", "select.core_only"),
        ] {
            match error_of(text) {
                SpecError::UnknownKey { key: got, .. } => assert_eq!(got, key, "{text}"),
                other => panic!("{text}: {other:?}"),
            }
        }
    }

    #[test]
    fn a_nested_unknown_key_is_named_by_its_full_path_and_suggests_a_full_path() {
        match error_of("[position]\ncompany = \"A\"\n[position.emphasis]\nbusines = \"low\"\n") {
            SpecError::UnknownKey { key, suggestion, .. } => {
                assert_eq!(key, "position.emphasis.busines");
                assert_eq!(suggestion.as_deref(), Some("position.emphasis.business"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_key_far_from_every_allowed_key_gets_no_suggestion() {
        match error_of("[select]\nzzzzz = 1\n") {
            SpecError::UnknownKey {
                suggestion, expected, ..
            } => {
                assert_eq!(suggestion, None);
                assert_eq!(expected.len(), 5);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_closest_of_two_close_keys_is_the_suggestion() {
        let expected = ["taggs", "tags"].map(String::from);
        assert_eq!(closest("tag", &expected).as_deref(), Some("tags"));
    }

    #[test]
    fn bad_syntax_a_duplicate_table_and_a_deep_nest_are_syntax_errors_with_a_line() {
        for text in [
            "[select\n",
            "[select]\n[select]\n",
            &format!("a = {}1{}", "[".repeat(200), "]".repeat(200)),
        ] {
            match error_of(text) {
                SpecError::Syntax { at, message } => {
                    assert!(at.line >= 1 && !message.is_empty(), "{message}");
                }
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn a_byte_order_mark_does_not_move_the_columns() {
        match error_of("\u{feff}[select]\ntag = []\n") {
            SpecError::UnknownKey { at, .. } => assert_eq!((at.line, at.column), (2, 1)),
            other => panic!("{other:?}"),
        }
        match error_of("\u{feff}[selct]\n") {
            SpecError::UnknownKey { at, .. } => assert_eq!((at.line, at.column), (1, 2)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn crlf_line_endings_give_the_same_lines() {
        match error_of("[select]\r\ntag = []\r\n") {
            SpecError::UnknownKey { at, .. } => assert_eq!((at.line, at.column), (2, 1)),
            other => panic!("{other:?}"),
        }
        assert_ok!(parse_text("[select]\r\ntags = [\"a\"]\r\n"));
    }

    #[test]
    fn a_column_counts_characters_not_bytes() {
        match error_of("[select]\ntags = [\"é\"]\ntaag = 1\n") {
            SpecError::UnknownKey { at, key, .. } => {
                assert_eq!((at.line, at.column, key.as_str()), (3, 1, "select.taag"))
            }
            other => panic!("{other:?}"),
        }
    }

    fn write(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        assert_ok!(std::fs::write(&path, bytes));
        path
    }

    #[test]
    fn a_file_is_read_whole_if_it_is_utf8_and_exactly_the_limit_or_less() {
        let dir = assert_ok!(tempfile::tempdir());
        assert_ok!(read_source(&write(&dir, "ok.toml", b"extends = []\n")));
        let limit = usize::try_from(MAX_FILE_BYTES).unwrap_or(0);
        assert_eq!(limit, 65_536, "the limit is 64 KiB");
        assert_ok!(read_source(&write(&dir, "max.toml", "#".repeat(limit).as_bytes())));
        assert!(matches!(
            read_source(&write(&dir, "big.toml", "#".repeat(limit + 1).as_bytes())),
            Err(SpecError::TooLarge { .. })
        ));
        assert!(matches!(
            read_source(&write(&dir, "bad.toml", b"a = \"\xff\"")),
            Err(SpecError::NotUtf8 { valid_up_to: 5, .. })
        ));
    }

    #[test]
    fn a_too_large_file_reports_its_real_size() {
        let dir = assert_ok!(tempfile::tempdir());
        match assert_err!(read_source(&write(&dir, "f.toml", "#".repeat(70_000).as_bytes()))) {
            SpecError::TooLarge { len, .. } => assert_eq!(len, 70_000),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_missing_file_keeps_the_error_the_system_gave() {
        let dir = assert_ok!(tempfile::tempdir());
        match assert_err!(read_source(&dir.path().join("missing.toml"))) {
            SpecError::Read { source, .. } => assert_eq!(source.kind(), std::io::ErrorKind::NotFound),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_directory_is_not_a_file_on_every_system() {
        let dir = assert_ok!(tempfile::tempdir());
        assert!(matches!(read_source(dir.path()), Err(SpecError::NotAFile { .. })));
    }

    #[cfg(unix)]
    #[test]
    fn a_device_and_a_fifo_are_not_files_and_a_fifo_does_not_hang_the_loader() {
        assert!(matches!(
            read_source(Path::new("/dev/null")),
            Err(SpecError::NotAFile { .. })
        ));
        let dir = assert_ok!(tempfile::tempdir());
        let fifo = dir.path().join("f.toml");
        assert!(assert_ok!(std::process::Command::new("mkfifo").arg(&fifo).status()).success());
        // Opening a FIFO waits for a writer, so a loader that opened it first would hang: ask from another thread.
        let (sender, receiver) = std::sync::mpsc::channel();
        let path = fifo.clone();
        std::thread::spawn(move || {
            let _ = sender.send(matches!(read_source(&path), Err(SpecError::NotAFile { .. })));
        });
        let answer = receiver.recv_timeout(std::time::Duration::from_secs(5));
        assert_eq!(answer, Ok(true), "the loader must refuse a FIFO without opening it");
    }
}
