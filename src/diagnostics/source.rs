//! ABOUTME: Immutable source snapshots keep diagnostic spans tied to the text that was loaded.
//! ABOUTME: Clones share the text; constructing a span checks byte bounds and UTF-8 boundaries.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{SourceKind, Span};

/// Source ownership of one selectable item. Spec-owned inline offsets refer to decoded text,
/// so all its findings point at the owning TOML literal instead of fabricated byte offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemOrigin {
    Baseline { file: PathBuf, range: Range<usize> },
    SpecContent { file: PathBuf, content: Range<usize> },
}

impl ItemOrigin {
    /// Binds a spec literal to its retained snapshot, checking kind, bounds and UTF-8 boundaries.
    #[must_use]
    pub fn spec(source: &SourceText, content: Range<usize>) -> Option<Self> {
        if source.kind() != SourceKind::Spec {
            return None;
        }
        let span = source.span(content)?;
        Some(Self::SpecContent {
            file: span.file,
            content: span.range,
        })
    }

    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Baseline { file, range } => Span {
                file: file.clone(),
                kind: SourceKind::Markdown,
                range: range.clone(),
            },
            Self::SpecContent { file, content } => Span {
                file: file.clone(),
                kind: SourceKind::Spec,
                range: content.clone(),
            },
        }
    }

    #[must_use]
    pub fn inline_span(&self, range: Range<usize>) -> Span {
        match self {
            Self::Baseline { file, .. } => Span {
                file: file.clone(),
                kind: SourceKind::Markdown,
                range,
            },
            Self::SpecContent { .. } => self.span(),
        }
    }
}

/// Captured bytes and their identity. Equality includes the text; debug output does not disclose it.
#[derive(Clone, PartialEq, Eq)]
pub struct SourceText {
    path: Arc<Path>,
    kind: SourceKind,
    text: Arc<str>,
}

impl std::fmt::Debug for SourceText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceText")
            .field("path", &self.path)
            .field("kind", &self.kind)
            .field("bytes", &self.text.len())
            .finish()
    }
}

impl SourceText {
    /// Captures text without reading or canonicalizing its path.
    #[must_use]
    pub fn new(path: PathBuf, kind: SourceKind, text: impl Into<Arc<str>>) -> Self {
        Self {
            path: path.into(),
            kind,
            text: text.into(),
        }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn kind(&self) -> SourceKind {
        self.kind
    }

    /// Converts a 1-based engine line and Unicode-scalar column into a byte offset.
    /// Handles BOM and all line endings exactly as diagnostic rendering does.
    #[must_use]
    pub fn byte_offset(&self, line: usize, column: usize) -> usize {
        let index = super::lines::LineIndex::new(self.text());
        let Some(range) = index.content(line.saturating_sub(1)) else {
            return self.text.len();
        };
        range.start
            + self.text[range.clone()]
                .char_indices()
                .nth(column.saturating_sub(1))
                .map_or(range.len(), |(offset, _)| offset)
    }

    /// A span into this exact snapshot, or `None` for a reversed, out-of-bounds or split-character range.
    #[must_use]
    pub fn span(&self, range: Range<usize>) -> Option<Span> {
        self.text.get(range.clone())?;
        Some(Span {
            file: self.path.to_path_buf(),
            kind: self.kind,
            range,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_some};
    use pretty_assertions::assert_eq;

    #[test]
    fn engine_coordinates_reverse_to_bytes_with_bom_and_mixed_endings() {
        let source = SourceText::new("p.toml".into(), SourceKind::Spec, "\u{feff}é\r😀x\r\nlast");
        assert_eq!(source.byte_offset(1, 1), 3);
        assert_eq!(source.byte_offset(2, 2), 10);
        assert_eq!(source.byte_offset(3, 2), 14);
        assert_eq!(source.byte_offset(1, 999), 5);
        assert_eq!(source.byte_offset(99, 1), source.text().len());
    }

    #[test]
    fn spec_origins_require_a_spec_snapshot_and_use_whole_literal_spans() {
        let source = SourceText::new("position.toml".into(), SourceKind::Spec, "'é text'");
        let origin = assert_some!(ItemOrigin::spec(&source, 0..source.text().len()));
        assert_eq!(
            origin.inline_span(2..3),
            assert_some!(source.span(0..source.text().len()))
        );
        assert_none!(ItemOrigin::spec(&source, 2..3));
        assert_none!(ItemOrigin::spec(&source, 0..99));
        let baseline = SourceText::new("letter.md".into(), SourceKind::Markdown, "text");
        assert_none!(ItemOrigin::spec(&baseline, 0..4));
    }

    #[test]
    fn spans_use_exact_bytes_and_refuse_invalid_ranges() {
        let source = SourceText::new("unsaved/spec.toml".into(), SourceKind::Spec, "é\r\n中");
        let span = assert_some!(source.span(4..7));
        assert_eq!(span.file, source.path());
        assert_eq!(span.kind, SourceKind::Spec);
        assert_eq!(&source.text()[span.range], "中");
        assert_some!(source.span(7..7));
        assert_none!(source.span(1..2));
        assert_none!(source.span(0..8));
        assert_none!(source.span(Range { start: 4, end: 2 }));
    }

    #[test]
    fn cloned_snapshots_share_text_and_never_read_disk() {
        let source = SourceText::new("does-not-exist.toml".into(), SourceKind::Spec, "unsaved");
        let copy = source.clone();
        assert!(Arc::ptr_eq(&source.text, &copy.text));
        assert!(Arc::ptr_eq(&source.path, &copy.path));
        assert_eq!(copy.text(), "unsaved");
        assert!(!format!("{copy:?}").contains("unsaved"));
    }
}
