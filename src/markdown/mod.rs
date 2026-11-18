//! ABOUTME: Strict Markdown resume parser: events -> located tree -> typed model plus located diagnostics.
//! ABOUTME: Parsing never aborts on content errors; every finding carries a byte range into the original text.
//!
//! Parsing runs in two phases. Phase 1 (`tree`) lowers pulldown-cmark offset events into a small
//! located syntax tree, iteratively and with depth caps. Phase 2 (`blocks`, with `role`, `inline`,
//! `period`, and `lint`) interprets that tree into the [`Resume`] model and
//! reports every convention violation with its source range, a hint, and an [`Edit`] when the fix is
//! mechanical.

mod blocks;
mod inline;
mod lint;
mod period;
mod role;
mod tree;

use std::borrow::Cow;
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::diagnostics::{Diagnostic, DiagnosticCode, Edit, Severity, SourceKind, Span};
use crate::model::Resume;

/// Largest accepted input; larger inputs yield one `input-limit` error and no model.
pub const MAX_INPUT_BYTES: usize = 1 << 20;
/// Deepest nesting of emphasis, strong, and links kept; deeper runs are flattened.
pub(crate) const MAX_INLINE_DEPTH: usize = 8;
/// Deepest nesting of lists kept; deeper lists are skipped.
pub(crate) const MAX_LIST_DEPTH: usize = 8;
/// Most diagnostics kept; the rest are summarized in one `input-limit` finding.
pub(crate) const MAX_DIAGNOSTICS: usize = 500;
/// Longest role line (in bytes) that separator recovery will examine.
pub(crate) const MAX_ROLE_LINE_BYTES: usize = 512;
/// Byte length of a UTF-8 byte order mark.
const BOM: char = '\u{FEFF}';

/// The outcome of parsing one resume.
///
/// `valid()` is `Some` only when no error-severity diagnostic was reported, so `build` and `export`
/// cannot render through an error by accident. `best_effort()` is `Some` whenever a name was found,
/// so `doctor` can still lint the content of a resume that has format errors.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct Parsed {
    resume: Option<Resume>,
    diagnostics: Vec<Diagnostic>,
}

impl Parsed {
    fn new(resume: Option<Resume>, diagnostics: Vec<Diagnostic>) -> Self {
        Self { resume, diagnostics }
    }

    /// The model, only when there is no error-severity diagnostic.
    #[must_use]
    pub fn valid(&self) -> Option<&Resume> {
        self.resume.as_ref().filter(|_| !self.has_errors())
    }

    /// The best-effort model, whenever a name heading was found.
    #[must_use]
    pub fn best_effort(&self) -> Option<&Resume> {
        self.resume.as_ref()
    }

    /// All findings, sorted by source position.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// True when any finding has [`Severity::Error`].
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }

    /// Splits into the best-effort model and the findings.
    #[must_use]
    pub fn into_parts(self) -> (Option<Resume>, Vec<Diagnostic>) {
        (self.resume, self.diagnostics)
    }
}

/// Parses a Markdown resume; `path` only labels the spans of the findings.
///
/// Never panics and never aborts on content errors: every problem becomes a [`Diagnostic`] whose span
/// is a byte range into `text` (on UTF-8 character boundaries, for LF, CRLF, and mixed line endings).
pub fn parse(path: impl AsRef<Path>, text: &str) -> Parsed {
    let path = path.as_ref();
    let mut sink = Sink::new(path);
    if text.len() > MAX_INPUT_BYTES {
        let message = format!("input is {} bytes; the limit is {MAX_INPUT_BYTES}", text.len());
        sink.report(Finding::new(DiagnosticCode::InputLimit, 0..0, message).hint("split the resume or remove content"));
        return Parsed::new(None, sink.finish());
    }
    let base = if text.starts_with(BOM) { BOM.len_utf8() } else { 0 };
    let normalized = normalize_lone_cr(&text[base..]);
    let doc = tree::lower(&normalized, base, &mut sink);
    let src = Source { text };
    let resume = blocks::interpret(&doc, &src, &mut sink);
    Parsed::new(resume, sink.finish())
}

/// Replaces each lone `\r` with `\n`, byte for byte, so every offset still indexes the original.
///
/// CommonMark treats a lone `\r` as a line ending, but pulldown-cmark's HTML-block scan only looks for
/// `\n`, so a leading `<!-- comment -->` would swallow the whole file. `\r\n` pairs are left alone.
fn normalize_lone_cr(text: &str) -> Cow<'_, str> {
    let bytes = text.as_bytes();
    let is_lone = |i: usize| bytes[i] == b'\r' && bytes.get(i + 1) != Some(&b'\n');
    if !(0..bytes.len()).any(is_lone) {
        return Cow::Borrowed(text);
    }
    let fixed: Vec<u8> = (0..bytes.len())
        .map(|i| if is_lone(i) { b'\n' } else { bytes[i] })
        .collect();
    // Only ASCII bytes were replaced by ASCII bytes, so this always succeeds; falling back is for the type.
    String::from_utf8(fixed).map_or(Cow::Borrowed(text), Cow::Owned)
}

/// The original text; every range indexes it.
pub(crate) struct Source<'a> {
    pub text: &'a str,
}

impl<'a> Source<'a> {
    /// Slices the original text; an out-of-range or non-boundary range yields an empty string.
    pub fn slice(&self, range: &Range<usize>) -> &'a str {
        self.text.get(range.clone()).unwrap_or("")
    }
}

/// One finding before it becomes a [`Diagnostic`].
#[derive(Debug, Clone)]
pub(crate) struct Finding {
    code: DiagnosticCode,
    range: Range<usize>,
    message: String,
    hints: Vec<String>,
    fix: Option<Edit>,
}

impl Finding {
    /// Starts a finding with its code, source range, and occurrence-specific message.
    pub fn new(code: DiagnosticCode, range: Range<usize>, message: impl Into<String>) -> Self {
        Self {
            code,
            range,
            message: message.into(),
            hints: Vec::new(),
            fix: None,
        }
    }

    /// Adds an occurrence-specific hint.
    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hints.push(hint.into());
        self
    }

    /// Attaches a mechanical, meaning-preserving edit.
    pub fn fix(mut self, range: Range<usize>, replacement: impl Into<String>) -> Self {
        self.fix = Some(Edit {
            range,
            replacement: replacement.into(),
        });
        self
    }
}

/// Collects findings, caps their volume, and sorts them deterministically.
pub(crate) struct Sink<'a> {
    path: &'a Path,
    diagnostics: Vec<Diagnostic>,
    suppressed: usize,
    suppressed_errors: usize,
    suppressed_max: Option<Severity>,
}

impl<'a> Sink<'a> {
    pub fn new(path: &'a Path) -> Self {
        Self {
            path,
            diagnostics: Vec::new(),
            suppressed: 0,
            suppressed_errors: 0,
            suppressed_max: None,
        }
    }

    /// Records a finding with its code's severity, or counts it once the cap is reached.
    pub fn report(&mut self, finding: Finding) {
        let severity = severity_of(finding.code);
        if self.diagnostics.len() >= MAX_DIAGNOSTICS {
            self.suppressed += 1;
            self.suppressed_errors += usize::from(severity == Severity::Error);
            self.suppressed_max = self.suppressed_max.max(Some(severity));
            return;
        }
        self.diagnostics.push(Diagnostic {
            severity,
            code: finding.code,
            message: finding.message,
            span: Some(Span {
                file: self.path.to_path_buf(),
                kind: SourceKind::Markdown,
                range: finding.range,
            }),
            hints: finding.hints,
            fix: finding.fix,
        });
    }

    /// Sorts by position, then appends the truncation summary, which takes the worst suppressed severity.
    pub fn finish(mut self) -> Vec<Diagnostic> {
        self.diagnostics
            .sort_by_key(|d| d.span.as_ref().map(|s| (s.range.start, s.range.end)));
        if let Some(severity) = self.suppressed_max {
            let message = format!(
                "{} more findings ({} errors) not shown",
                self.suppressed, self.suppressed_errors
            );
            self.diagnostics.push(Diagnostic {
                severity,
                code: DiagnosticCode::InputLimit,
                message,
                span: Some(Span {
                    file: PathBuf::from(self.path),
                    kind: SourceKind::Markdown,
                    range: 0..0,
                }),
                hints: vec![format!(
                    "only the first {MAX_DIAGNOSTICS} findings are kept; fix these and rerun"
                )],
                fix: None,
            });
        }
        self.diagnostics
    }
}

/// Severity of each Format code: errors block `build`; warnings do not.
pub(crate) const fn severity_of(code: DiagnosticCode) -> Severity {
    use DiagnosticCode::*;
    match code {
        MissingName | DuplicateName | ContentBeforeSection | RoleLineShape | RoleSeparator | EmptyRolePart
        | EraShape | EraOutsideRole | PeriodUnparseable | PeriodEndBeforeStart | UnsupportedHtml | InvalidLink
        | UnsupportedLink | UnsupportedMarkdown | TextTooLong | EmptyHeading | InputLimit => Severity::Error,
        // Warnings, and every non-Format code (never reported by the parser).
        _ => Severity::Warning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_none, assert_some};
    use pretty_assertions::assert_eq;

    fn parsed(text: &str) -> Parsed {
        parse("resume.md", text)
    }

    fn codes(parsed: &Parsed) -> Vec<DiagnosticCode> {
        parsed.diagnostics().iter().map(|d| d.code).collect()
    }

    #[test]
    fn empty_input_is_missing_name() {
        let parsed = parsed("");
        assert_eq!(codes(&parsed), vec![DiagnosticCode::MissingName]);
        assert_none!(parsed.best_effort());
        assert_eq!(assert_some!(parsed.diagnostics()[0].span.as_ref()).range, 0..0);
    }

    #[test]
    fn oversized_input_is_input_limit_error() {
        let parsed = parsed(&"a".repeat(MAX_INPUT_BYTES + 1));
        assert_eq!(codes(&parsed), vec![DiagnosticCode::InputLimit]);
        assert!(parsed.has_errors());
        assert_none!(parsed.best_effort());
    }

    #[test]
    fn bom_is_ignored_and_spans_shift() {
        let plain = "# Ada\n\nHere\n\n## S\n\n### Acme · Engineer - 2015 – 2017\n";
        let with_bom = format!("\u{FEFF}{plain}");
        let (plain_parsed, bom_parsed) = (parsed(plain), parsed(&with_bom));
        assert_eq!(
            plain_parsed.best_effort(),
            bom_parsed.best_effort(),
            "the BOM is not part of the content"
        );
        assert_eq!(codes(&plain_parsed), codes(&bom_parsed));
        for (a, b) in plain_parsed.diagnostics().iter().zip(bom_parsed.diagnostics()) {
            let (sa, sb) = (assert_some!(a.span.as_ref()), assert_some!(b.span.as_ref()));
            assert_eq!(
                sb.range,
                sa.range.start + 3..sa.range.end + 3,
                "spans index the original, BOM included"
            );
            let (fa, fb) = (assert_some!(a.fix.as_ref()), assert_some!(b.fix.as_ref()));
            assert_eq!(fb.range, fa.range.start + 3..fa.range.end + 3);
            assert_eq!(&with_bom[sb.range.clone()], &plain[sa.range.clone()]);
        }
        assert!(
            !plain_parsed.diagnostics().is_empty(),
            "the fixture must produce findings to compare"
        );
    }

    #[test]
    fn lone_cr_is_a_line_ending_that_keeps_offsets() {
        assert_eq!(normalize_lone_cr("a\rb\r\nc\r"), "a\nb\r\nc\n");
        assert!(
            matches!(normalize_lone_cr("a\r\nb\n"), Cow::Borrowed(_)),
            "no allocation without lone CRs"
        );
        let text = "<!-- c -->\r# Ada\r\rHere\r\r## S\r\r### Acme · 2015 - 2017\r";
        let parsed = parsed(text);
        assert_eq!(codes(&parsed), vec![DiagnosticCode::PeriodHyphen]);
        let fix = assert_some!(parsed.diagnostics()[0].fix.as_ref());
        assert_eq!(&text[fix.range.clone()], " - ", "spans index the original text");
    }

    #[test]
    fn a_bom_in_the_middle_is_ordinary_text() {
        let parsed = parsed("# Ada\n\nHe\u{FEFF}re\n");
        assert_eq!(codes(&parsed), vec![]);
    }

    #[test]
    fn input_at_the_limit_is_parsed() {
        let parsed = parsed(&"a".repeat(MAX_INPUT_BYTES));
        assert!(!codes(&parsed).contains(&DiagnosticCode::InputLimit));
    }

    #[test]
    fn valid_is_none_when_errors_exist() {
        let parsed = parsed("# Ada\n\n## Experience\n\n### Broken heading\n");
        assert!(parsed.has_errors());
        assert_some!(parsed.best_effort());
        assert_none!(parsed.valid());
    }

    #[test]
    fn diagnostics_are_capped_with_summary_severity() {
        let mut sink = Sink::new(Path::new("r.md"));
        for i in 0..MAX_DIAGNOSTICS {
            sink.report(Finding::new(DiagnosticCode::Image, i..i, "image"));
        }
        sink.report(Finding::new(DiagnosticCode::Image, 0..0, "warning beyond the cap"));
        sink.report(Finding::new(DiagnosticCode::InvalidLink, 0..0, "error beyond the cap"));
        let diagnostics = sink.finish();
        assert_eq!(diagnostics.len(), MAX_DIAGNOSTICS + 1);
        let summary = assert_some!(diagnostics.last());
        assert_eq!(summary.code, DiagnosticCode::InputLimit);
        assert_eq!(summary.severity, Severity::Error);
        assert_eq!(summary.message, "2 more findings (1 errors) not shown");
    }

    #[test]
    fn capped_warnings_summarize_as_a_warning() {
        let mut sink = Sink::new(Path::new("r.md"));
        for _ in 0..=MAX_DIAGNOSTICS {
            sink.report(Finding::new(DiagnosticCode::Image, 0..0, "image"));
        }
        let diagnostics = sink.finish();
        assert_eq!(assert_some!(diagnostics.last()).severity, Severity::Warning);
    }

    #[test]
    fn diagnostics_are_sorted_by_position() {
        let mut sink = Sink::new(Path::new("r.md"));
        sink.report(Finding::new(DiagnosticCode::Image, 9..10, "late"));
        sink.report(Finding::new(DiagnosticCode::Image, 1..2, "early"));
        let messages: Vec<_> = sink.finish().into_iter().map(|d| d.message).collect();
        assert_eq!(messages, vec!["early", "late"]);
    }

    #[test]
    fn severity_of_matches_the_documented_split() {
        assert_eq!(severity_of(DiagnosticCode::MissingName), Severity::Error);
        assert_eq!(severity_of(DiagnosticCode::PeriodHyphen), Severity::Warning);
        assert_eq!(severity_of(DiagnosticCode::TooManyContactItems), Severity::Warning);
        assert_eq!(severity_of(DiagnosticCode::InputLimit), Severity::Error);
    }
}
