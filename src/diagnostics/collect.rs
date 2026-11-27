//! ABOUTME: One bounded diagnostic budget shared across all source files in a document.
//! ABOUTME: Suppressed errors remain errors in the final summary; callers choose finding order.

use super::{Diagnostic, DiagnosticCode, Severity, SourceKind, Span};

pub(crate) struct Collector {
    limit: usize,
    diagnostics: Vec<Diagnostic>,
    suppressed: usize,
    errors: usize,
    worst: Option<Severity>,
}

impl Collector {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            limit,
            diagnostics: Vec::new(),
            suppressed: 0,
            errors: 0,
            worst: None,
        }
    }

    pub(crate) fn push(&mut self, diagnostic: Diagnostic) {
        if self.diagnostics.len() < self.limit {
            self.diagnostics.push(diagnostic);
        } else {
            self.suppressed += 1;
            self.errors += usize::from(diagnostic.severity == Severity::Error);
            self.worst = self.worst.max(Some(diagnostic.severity));
        }
    }

    /// Sort baseline runs by byte range; spec literal findings retain prepared-document order.
    pub(crate) fn finish(mut self, summary_span: Option<Span>) -> Vec<Diagnostic> {
        let mut start = 0;
        while start < self.diagnostics.len() {
            let file = self.diagnostics[start].span.as_ref().map(|s| (&s.file, s.kind));
            let end = start
                + self.diagnostics[start..]
                    .iter()
                    .take_while(|d| d.span.as_ref().map(|s| (&s.file, s.kind)) == file)
                    .count();
            if self.diagnostics[start]
                .span
                .as_ref()
                .is_some_and(|s| s.kind == SourceKind::Markdown)
            {
                self.diagnostics[start..end].sort_by_key(|d| d.span.as_ref().map(|s| (s.range.start, s.range.end)));
            }
            start = end;
        }
        if let Some(severity) = self.worst {
            self.diagnostics.push(Diagnostic {
                severity,
                code: DiagnosticCode::InputLimit,
                message: format!("{} more findings ({} errors) not shown", self.suppressed, self.errors),
                span: summary_span,
                hints: vec![format!(
                    "only the first {} findings are kept; fix these and rerun",
                    self.limit
                )],
                fix: None,
            });
        }
        self.diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_some;
    use pretty_assertions::assert_eq;

    fn finding(file: &str, kind: SourceKind, at: usize, message: &str) -> Diagnostic {
        Diagnostic {
            severity: Severity::Warning,
            code: DiagnosticCode::HardBreak,
            message: message.into(),
            span: Some(Span {
                file: file.into(),
                kind,
                range: at..at,
            }),
            hints: vec![],
            fix: None,
        }
    }

    #[test]
    fn sorts_baseline_runs_but_preserves_spec_placement_and_file_boundaries() {
        let mut collector = Collector::new(10);
        for (file, kind, at, message) in [
            ("a", SourceKind::Markdown, 8, "later"),
            ("a", SourceKind::Markdown, 2, "first"),
            ("b", SourceKind::Spec, 90, "placed-first"),
            ("b", SourceKind::Spec, 10, "placed-second"),
            ("a", SourceKind::Markdown, 1, "back-to-a"),
        ] {
            collector.push(finding(file, kind, at, message));
        }
        let got = collector.finish(None);
        assert_eq!(
            got.iter().map(|d| d.message.as_str()).collect::<Vec<_>>(),
            vec!["first", "later", "placed-first", "placed-second", "back-to-a"]
        );
    }

    #[test]
    fn ties_and_unlocated_findings_keep_their_order() {
        let mut collector = Collector::new(10);
        collector.push(finding("a", SourceKind::Markdown, 2, "first"));
        collector.push(finding("a", SourceKind::Markdown, 2, "second"));
        let mut unlocated = finding("a", SourceKind::Markdown, 0, "unlocated");
        unlocated.span = None;
        collector.push(unlocated);
        collector.push(finding("a", SourceKind::Markdown, 0, "last"));
        assert_eq!(
            collector
                .finish(None)
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second", "unlocated", "last"]
        );
    }

    #[test]
    fn zero_budget_and_warning_overflow_preserve_summary_severity_and_location() {
        assert!(Collector::new(0).finish(None).is_empty());
        let mut collector = Collector::new(0);
        collector.push(finding("a", SourceKind::Markdown, 1, "warning"));
        let summary_span = Span {
            file: "a".into(),
            kind: SourceKind::Markdown,
            range: 0..0,
        };
        let got = collector.finish(Some(summary_span.clone()));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].severity, Severity::Warning);
        assert_eq!(assert_some!(got[0].span.as_ref()), &summary_span);
    }
}
