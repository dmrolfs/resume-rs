//! ABOUTME: Converts Typst diagnostics into the crate's `Diagnostic`, located in style files by byte range.
//! ABOUTME: Spans in the in-memory resume or outside the style files become unlocated diagnostics.

use std::path::Path;

use typst::WorldExt;
use typst::diag::{Severity as TypstSeverity, SourceDiagnostic};
use typst::syntax::DiagSpan;

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity, SourceKind, Span};
use crate::typeset::world::ResumeWorld;

/// Maps Typst diagnostics to ours, keeping Typst's order.
///
/// Without a `world` no span can be located, so every diagnostic is unlocated.
pub(crate) fn convert(
    world: Option<&ResumeWorld<'_>>,
    diagnostics: impl IntoIterator<Item = SourceDiagnostic>,
) -> Vec<Diagnostic> {
    diagnostics.into_iter().map(|d| convert_one(world, &d)).collect()
}

fn convert_one(world: Option<&ResumeWorld<'_>>, diagnostic: &SourceDiagnostic) -> Diagnostic {
    let (severity, code) = match diagnostic.severity {
        TypstSeverity::Error => (Severity::Error, DiagnosticCode::TypstError),
        TypstSeverity::Warning => (Severity::Warning, DiagnosticCode::TypstWarning),
    };
    let located = world.and_then(|w| locate(w, diagnostic.span));
    let hints = diagnostic
        .hints
        .iter()
        .map(|hint| match world.and_then(|w| describe(w, hint.span)) {
            Some(at) => format!("{} (at {at})", hint.v),
            None => hint.v.to_string(),
        })
        .chain(diagnostic.trace.iter().map(
            |frame| match world.and_then(|w| describe(w, DiagSpan::from(frame.span))) {
                Some(at) => format!("{} ({at})", frame.v),
                None => frame.v.to_string(),
            },
        ))
        .collect();
    Diagnostic {
        severity,
        code,
        message: diagnostic.message.to_string(),
        span: located.map(|l| Span {
            file: l.file,
            kind: SourceKind::Style,
            range: l.range,
        }),
        hints,
        fix: None,
    }
}

struct Located {
    file: std::path::PathBuf,
    range: std::ops::Range<usize>,
}

/// The style file and byte range a span points at, if it points into a style file.
fn locate(world: &ResumeWorld<'_>, span: DiagSpan) -> Option<Located> {
    let file = world.display_path(span.id()?)?;
    let range = world.range(span)?;
    Some(Located { file, range })
}

/// `file:line` for a span in a style file, with a 1-based line.
fn describe(world: &ResumeWorld<'_>, span: DiagSpan) -> Option<String> {
    let id = span.id()?;
    let Located { file, range } = locate(world, span)?;
    let text = typst::World::source(world, id).ok()?;
    let line = text.text().get(..range.start)?.matches('\n').count() + 1;
    Some(format!("{}:{line}", display(&file)))
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::StyleName;
    use crate::model::options::Paper;
    use crate::style::StyleSource;
    use crate::typeset::fonts::FontSet;
    use claims::{assert_none, assert_ok, assert_some};
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;
    use std::sync::Arc;
    use typst::World;
    use typst::diag::Tracepoint;
    use typst::ecow::EcoVec;
    use typst::syntax::{Span as TypstSpan, Spanned};

    fn style() -> StyleSource {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/styles");
        assert_ok!(StyleSource::directory(root, assert_ok!(StyleName::try_new("minimal"))))
    }

    fn world(style: &StyleSource) -> ResumeWorld<'_> {
        ResumeWorld::new(style, Arc::new(FontSet::bundled()), b"{}".to_vec(), Paper::Letter)
    }

    #[test]
    fn error_maps_to_style_span_with_typst_error_code() {
        let style = style();
        let world = world(&style);
        let source = assert_ok!(world.source(world.main()));
        let span = source.root().span();
        let [mapped] = convert(Some(&world), [SourceDiagnostic::error(span, "boom")])
            .try_into()
            .unwrap();
        assert_eq!(mapped.severity, Severity::Error);
        assert_eq!(mapped.code, DiagnosticCode::TypstError);
        assert_eq!(mapped.message, "boom");
        let located = assert_some!(mapped.span);
        assert_eq!(located.kind, SourceKind::Style);
        assert_eq!(located.range, 0..source.text().len());
        assert!(located.file.ends_with("minimal/style.typ"), "{:?}", located.file);
        assert_none!(mapped.fix);
    }

    #[test]
    fn warning_maps_with_warning_severity_and_typst_warning_code() {
        let [mapped] = convert(None, [SourceDiagnostic::warning(TypstSpan::detached(), "careful")])
            .try_into()
            .unwrap();
        assert_eq!(mapped.severity, Severity::Warning);
        assert_eq!(mapped.code, DiagnosticCode::TypstWarning);
    }

    #[test]
    fn detached_span_has_no_span() {
        let style = style();
        let world = world(&style);
        let [mapped] = convert(Some(&world), [SourceDiagnostic::error(TypstSpan::detached(), "x")])
            .try_into()
            .unwrap();
        assert_none!(mapped.span);
    }

    #[test]
    fn spans_in_the_in_memory_resume_have_no_span() {
        let style = style();
        let world = world(&style);
        let resume = typst::syntax::RootedPath::new(
            typst::syntax::VirtualRoot::Project,
            assert_ok!(typst::syntax::VirtualPath::new("/resume.json")),
        )
        .intern();
        let span = DiagSpan::from_range(resume, 0..1);
        let [mapped] = convert(Some(&world), [SourceDiagnostic::error(span, "bad json")])
            .try_into()
            .unwrap();
        assert_none!(mapped.span);
    }

    #[test]
    fn hints_and_trace_become_hint_strings() {
        let style = style();
        let world = world(&style);
        let source = assert_ok!(world.source(world.main()));
        let span = source.root().span();
        let mut diagnostic = SourceDiagnostic::error(span, "boom");
        diagnostic.hints = EcoVec::from([
            Spanned::new("generic hint".into(), DiagSpan::from(TypstSpan::detached())),
            Spanned::new("located hint".into(), DiagSpan::from(span)),
        ]);
        diagnostic.trace = EcoVec::from([Spanned::new(Tracepoint::Call(Some("f".into())), span)]);
        let [mapped] = convert(Some(&world), [diagnostic]).try_into().unwrap();
        assert_eq!(mapped.hints.len(), 3);
        assert_eq!(mapped.hints[0], "generic hint");
        assert!(mapped.hints[1].starts_with("located hint (at "), "{}", mapped.hints[1]);
        assert!(mapped.hints[1].ends_with("style.typ:1)"), "{}", mapped.hints[1]);
        assert!(
            mapped.hints[2].starts_with("while calling `f` ("),
            "{}",
            mapped.hints[2]
        );
    }

    #[test]
    fn order_is_preserved() {
        let inputs = [
            SourceDiagnostic::error(TypstSpan::detached(), "first"),
            SourceDiagnostic::warning(TypstSpan::detached(), "second"),
        ];
        let messages: Vec<String> = convert(None, inputs).into_iter().map(|d| d.message).collect();
        assert_eq!(messages, ["first", "second"]);
    }
}
