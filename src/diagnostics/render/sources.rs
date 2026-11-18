//! ABOUTME: The source texts findings point into, keyed by path, with one lazily built line index per file.
//! ABOUTME: Resolving a span snaps it to the text and locates it, or says why it cannot be located.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::excerpt::sanitize;
use crate::diagnostics::Span;
use crate::diagnostics::lines::{LineIndex, Location};

/// Source text by path, with a lazily built line index per file.
///
/// The renderer does no input I/O: every caller registers the text its spans index into, under the
/// same path the spans carry.
#[derive(Debug)]
pub struct Sources<'a> {
    base_dir: Option<PathBuf>,
    files: HashMap<PathBuf, SourceFile<'a>>,
}

#[derive(Debug)]
struct SourceFile<'a> {
    text: &'a str,
    index: OnceLock<LineIndex>,
}

/// Where a finding points, after snapping.
#[derive(Debug)]
pub(super) enum Resolved<'s> {
    /// The finding has no span.
    NoSpan,
    /// The finding has a span the renderer cannot show.
    Unlocated { name: String, reason: String },
    /// The span, snapped to the text.
    Located(Located<'s>),
}

/// A located span with its true start and end.
#[derive(Debug)]
pub(super) struct Located<'s> {
    pub name: String,
    pub text: &'s str,
    pub index: &'s LineIndex,
    pub range: Range<usize>,
    pub start: Location,
    pub end: Location,
}

impl<'a> Sources<'a> {
    /// An empty database; paths under `base_dir` are shown relative to it.
    #[must_use]
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        Self {
            base_dir,
            files: HashMap::new(),
        }
    }

    /// Registers the text that spans into `path` index; a second call for a path replaces the first.
    pub fn insert(&mut self, path: impl Into<PathBuf>, text: &'a str) {
        self.files.insert(
            path.into(),
            SourceFile {
                text,
                index: OnceLock::new(),
            },
        );
    }

    /// Snaps and locates a span, or says why it cannot be located.
    pub(super) fn resolve(&self, span: Option<&Span>) -> Resolved<'_> {
        let Some(span) = span else {
            return Resolved::NoSpan;
        };
        let name = display_path(&span.file, self.base_dir.as_deref());
        let Some(file) = self.files.get(&span.file) else {
            return Resolved::Unlocated {
                name,
                reason: "source not available".into(),
            };
        };
        let text = file.text;
        if span.range.start > text.len() {
            let Range { start, end } = span.range;
            return Resolved::Unlocated {
                name,
                reason: format!("span {start}..{end} is past the end ({} bytes)", text.len()),
            };
        }
        let index = file.index.get_or_init(|| LineIndex::new(text));
        let range = index.snap(text, span.range.clone());
        Resolved::Located(Located {
            name,
            text,
            index,
            start: index.location(text, range.start),
            end: index.location(text, range.end),
            range,
        })
    }
}

/// Shows `path` relative to `base` when it is absolute and under it; never canonicalizes.
///
/// Canonicalizing would turn a Windows path into `\\?\C:\...`, which no longer compares equal to `base`.
pub(super) fn display_path(path: &Path, base: Option<&Path>) -> String {
    let relative = base
        .filter(|_| path.is_absolute())
        .and_then(|base| path.strip_prefix(base).ok())
        .filter(|relative| !relative.as_os_str().is_empty());
    sanitize(&relative.unwrap_or(path).to_string_lossy(), false).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::SourceKind;
    use pretty_assertions::assert_eq;

    fn span(file: &str, range: Range<usize>) -> Span {
        Span {
            file: file.into(),
            kind: SourceKind::Markdown,
            range,
        }
    }

    fn sources_with<'a>(text: &'a str) -> Sources<'a> {
        let mut sources = Sources::new(None);
        sources.insert("t.md", text);
        sources
    }

    fn located<'s>(resolved: Resolved<'s>) -> Located<'s> {
        match resolved {
            Resolved::Located(located) => located,
            other => panic!("expected a located span, got {other:?}"),
        }
    }

    #[test]
    fn no_span_is_no_span() {
        let sources = Sources::new(None);
        assert!(matches!(sources.resolve(None), Resolved::NoSpan));
    }

    #[test]
    fn missing_source_is_unlocated() {
        let sources = Sources::new(None);
        let resolved = sources.resolve(Some(&span("x.md", 0..3)));
        let Resolved::Unlocated { name, reason } = resolved else {
            panic!("expected unlocated, got {resolved:?}");
        };
        assert_eq!(name, "x.md");
        assert_eq!(reason, "source not available");
    }

    #[test]
    fn a_span_past_the_end_is_unlocated_not_clamped() {
        let sources = sources_with("abc");
        let resolved = sources.resolve(Some(&span("t.md", 5..9)));
        let Resolved::Unlocated { name, reason } = resolved else {
            panic!("expected unlocated, got {resolved:?}");
        };
        assert_eq!(name, "t.md");
        assert_eq!(reason, "span 5..9 is past the end (3 bytes)");
    }

    #[test]
    fn a_span_starting_at_the_end_is_located() {
        let sources = sources_with("abc");
        let found = located(sources.resolve(Some(&span("t.md", 3..3))));
        assert_eq!(found.range, 3..3);
        assert_eq!(found.start, Location { line: 1, column: 4 });
        assert_eq!(found.end, found.start);
    }

    #[test]
    fn a_located_span_reports_its_true_start_and_exclusive_end() {
        let text = "ab\ncd é\n";
        let sources = sources_with(text);
        let found = located(sources.resolve(Some(&span("t.md", 6..8))));
        assert_eq!(&text[found.range.clone()], "é");
        assert_eq!(found.start, Location { line: 2, column: 4 });
        assert_eq!(found.end, Location { line: 2, column: 5 });
    }

    #[test]
    fn a_span_inside_a_character_is_widened_to_it() {
        let sources = sources_with("é");
        let found = located(sources.resolve(Some(&span("t.md", 1..1))));
        assert_eq!(found.range, 0..2);
        assert_eq!(found.start, Location { line: 1, column: 1 });
        assert_eq!(found.end, Location { line: 1, column: 2 });
    }

    #[test]
    fn a_span_inside_the_bom_starts_after_it() {
        let sources = sources_with("\u{FEFF}x");
        let found = located(sources.resolve(Some(&span("t.md", 0..4))));
        assert_eq!(found.range, 3..4);
        assert_eq!(found.start, Location { line: 1, column: 1 });
    }

    #[test]
    fn a_trailing_terminator_does_not_make_the_span_multi_line() {
        let sources = sources_with("ab\ncd");
        let found = located(sources.resolve(Some(&span("t.md", 0..3))));
        assert_eq!(found.range, 0..2);
        assert_eq!(found.end, Location { line: 1, column: 3 });
    }

    #[test]
    fn a_reversed_span_becomes_empty_at_its_start() {
        let sources = sources_with("abcdef");
        let reversed = Range { start: 4, end: 1 };
        let found = located(sources.resolve(Some(&span("t.md", reversed))));
        assert_eq!(found.range, 4..4);
    }

    #[test]
    fn findings_in_one_file_share_one_line_index() {
        let sources = sources_with("a\nb\nc");
        let first = located(sources.resolve(Some(&span("t.md", 0..1))));
        let second = located(sources.resolve(Some(&span("t.md", 4..5))));
        assert!(std::ptr::eq(first.index, second.index));
        assert_eq!(second.start.line, 3);
    }

    #[test]
    fn inserting_a_path_again_replaces_its_text() {
        let mut sources = Sources::new(None);
        sources.insert("t.md", "old");
        sources.insert("t.md", "newer");
        let found = located(sources.resolve(Some(&span("t.md", 0..5))));
        assert_eq!(found.text, "newer");
    }

    #[test]
    fn the_name_is_the_display_path_under_the_base() {
        let base = std::env::temp_dir().join("proj");
        let mut sources = Sources::new(Some(base.clone()));
        let file = base.join("resume.md");
        sources.insert(file.clone(), "abc");
        let found = located(sources.resolve(Some(&Span {
            file,
            kind: SourceKind::Markdown,
            range: 0..1,
        })));
        assert_eq!(found.name, "resume.md");
    }

    #[test]
    fn display_path_relativizes_under_base() {
        let base = std::env::temp_dir().join("proj");
        assert_eq!(display_path(&base.join("resume.md"), Some(&base)), "resume.md");
        let nested = Path::new("docs").join("a.md");
        assert_eq!(display_path(&base.join(&nested), Some(&base)), nested.to_string_lossy());
    }

    #[test]
    fn display_path_keeps_relative_outside_and_unbased_paths() {
        let base = std::env::temp_dir().join("proj");
        let relative = Path::new("docs").join("a.md");
        assert_eq!(display_path(&relative, Some(&base)), relative.to_string_lossy());
        let outside = std::env::temp_dir().join("other").join("x.md");
        assert_eq!(display_path(&outside, Some(&base)), outside.to_string_lossy());
        assert_eq!(
            display_path(&base.join("a.md"), None),
            base.join("a.md").to_string_lossy()
        );
    }

    #[test]
    fn display_path_never_empties_the_name() {
        let base = std::env::temp_dir().join("proj");
        assert_eq!(display_path(&base, Some(&base)), base.to_string_lossy());
    }

    #[test]
    fn display_path_escapes_control_characters() {
        assert_eq!(display_path(Path::new("a\u{1b}[31m.md"), None), "a\\u{1b}[31m.md");
    }
}
