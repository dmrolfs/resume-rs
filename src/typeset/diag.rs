//! ABOUTME: Converts Typst diagnostics into the crate's `Diagnostic`, located in style files by byte range.
//! ABOUTME: Spans in the in-memory resume or outside the style files become unlocated diagnostics.

use typst::World;
use typst::WorldExt;
use typst::diag::{Severity as TypstSeverity, SourceDiagnostic};
use typst::syntax::DiagSpan;

use crate::diagnostics::{Diagnostic, DiagnosticCode, Severity, SourceKind, Span};
use crate::typeset::world::StyleWorld;

/// Maps Typst diagnostics to ours, keeping Typst's order.
pub(crate) fn convert(world: &StyleWorld, diagnostics: impl IntoIterator<Item = SourceDiagnostic>) -> Vec<Diagnostic> {
    diagnostics.into_iter().map(|d| convert_one(world, &d)).collect()
}

fn convert_one(world: &StyleWorld, diagnostic: &SourceDiagnostic) -> Diagnostic {
    let (severity, code) = match diagnostic.severity {
        TypstSeverity::Error => (Severity::Error, DiagnosticCode::TypstError),
        TypstSeverity::Warning => (Severity::Warning, DiagnosticCode::TypstWarning),
    };
    let hints = diagnostic
        .hints
        .iter()
        .map(|hint| match describe(world, hint.span) {
            Some(at) => format!("{} (at {at})", hint.v),
            None => hint.v.to_string(),
        })
        .chain(
            diagnostic
                .trace
                .iter()
                .map(|frame| match describe(world, DiagSpan::from(frame.span)) {
                    Some(at) => format!("{} ({at})", frame.v),
                    None => frame.v.to_string(),
                }),
        )
        .collect();
    Diagnostic {
        severity,
        code,
        message: diagnostic.message.to_string(),
        span: locate(world, diagnostic.span),
        hints,
        fix: None,
    }
}

/// The style file and byte range a span points at, if it points into a style file.
fn locate(world: &StyleWorld, span: DiagSpan) -> Option<Span> {
    Some(Span {
        file: world.display_path(span.id()?)?,
        kind: SourceKind::Style,
        range: world.range(span)?,
    })
}

/// `file:line` for a span in a style file, with a 1-based line as Typst counts lines.
fn describe(world: &StyleWorld, span: DiagSpan) -> Option<String> {
    let located = locate(world, span)?;
    let source = world.source(span.id()?).ok()?;
    let line = source.lines().byte_to_line(located.range.start)? + 1;
    Some(format!("{}:{line}", located.file.display()))
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
    use typst::diag::Tracepoint;
    use typst::ecow::EcoVec;
    use typst::syntax::{Span as TypstSpan, Spanned};

    fn world_in(root: &std::path::Path, name: &str) -> StyleWorld {
        let style = assert_ok!(StyleSource::directory(root, assert_ok!(StyleName::try_new(name))));
        StyleWorld::new(style, Arc::new(FontSet::bundled()), b"{}".to_vec(), Paper::Letter)
    }

    fn fixtures_world() -> StyleWorld {
        world_in(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/styles"),
            "minimal",
        )
    }

    fn one(mut mapped: Vec<Diagnostic>) -> Diagnostic {
        assert_eq!(mapped.len(), 1, "{mapped:?}");
        mapped.remove(0)
    }

    #[test]
    fn error_maps_to_style_span_with_typst_error_code() {
        let world = fixtures_world();
        let source = assert_ok!(world.source(world.main()));
        let mapped = one(convert(&world, [SourceDiagnostic::error(source.root().span(), "boom")]));
        assert_eq!(mapped.severity, Severity::Error);
        assert_eq!(mapped.code, DiagnosticCode::TypstError);
        assert_eq!(mapped.message, "boom");
        let located = assert_some!(mapped.span);
        assert_eq!(located.kind, SourceKind::Style);
        assert_eq!(located.range, 0..source.text().len());
        let expected = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/styles/minimal/style.typ");
        assert_eq!(located.file, expected);
        assert_none!(mapped.fix);
    }

    #[test]
    fn warning_maps_with_warning_severity_and_typst_warning_code() {
        let world = fixtures_world();
        let mapped = one(convert(
            &world,
            [SourceDiagnostic::warning(TypstSpan::detached(), "careful")],
        ));
        assert_eq!(mapped.severity, Severity::Warning);
        assert_eq!(mapped.code, DiagnosticCode::TypstWarning);
    }

    #[test]
    fn detached_span_has_no_span() {
        let world = fixtures_world();
        assert_none!(one(convert(&world, [SourceDiagnostic::error(TypstSpan::detached(), "x")])).span);
    }

    #[test]
    fn spans_in_the_in_memory_resume_have_no_span() {
        let world = fixtures_world();
        let resume = typst::syntax::RootedPath::new(
            typst::syntax::VirtualRoot::Project,
            assert_ok!(typst::syntax::VirtualPath::new("/resume.json")),
        )
        .intern();
        let span = DiagSpan::from_range(resume, 0..1);
        assert_none!(one(convert(&world, [SourceDiagnostic::error(span, "bad json")])).span);
    }

    #[test]
    fn hint_and_trace_lines_are_counted_not_constant() {
        let tmp = assert_ok!(tempfile::tempdir());
        assert_ok!(std::fs::create_dir(tmp.path().join("s")));
        assert_ok!(std::fs::write(
            tmp.path().join("s/style.typ"),
            "line one\nline two\nline three\n"
        ));
        let world = world_in(tmp.path(), "s");
        let source = assert_ok!(world.source(world.main()));
        let start = assert_some!(source.text().find("line three"));
        let span = DiagSpan::from_range(world.main(), start..start + 4);
        let mut diagnostic = SourceDiagnostic::error(TypstSpan::detached(), "boom");
        diagnostic.hints = EcoVec::from([
            Spanned::new("generic hint".into(), DiagSpan::from(TypstSpan::detached())),
            Spanned::new("located hint".into(), span),
        ]);
        let mapped = one(convert(&world, [diagnostic]));
        assert_eq!(mapped.hints[0], "generic hint");
        let expected = format!(
            "located hint (at {}:3)",
            tmp.path().join("s").join("style.typ").display()
        );
        assert_eq!(mapped.hints[1], expected);
    }

    #[test]
    fn trace_frames_become_hints_with_their_location() {
        let world = fixtures_world();
        let source = assert_ok!(world.source(world.main()));
        let mut diagnostic = SourceDiagnostic::error(TypstSpan::detached(), "boom");
        diagnostic.trace = EcoVec::from([Spanned::new(Tracepoint::Call(Some("f".into())), source.root().span())]);
        let mapped = one(convert(&world, [diagnostic]));
        assert_eq!(mapped.hints.len(), 1);
        assert!(
            mapped.hints[0].starts_with("while calling `f` ("),
            "{}",
            mapped.hints[0]
        );
        assert!(mapped.hints[0].ends_with("style.typ:1)"), "{}", mapped.hints[0]);
    }

    #[test]
    fn crlf_and_lone_cr_count_as_one_line_break_each() {
        let tmp = assert_ok!(tempfile::tempdir());
        assert_ok!(std::fs::create_dir(tmp.path().join("s")));
        assert_ok!(std::fs::write(tmp.path().join("s/style.typ"), "a\r\nb\rc\nTARGET"));
        let world = world_in(tmp.path(), "s");
        let source = assert_ok!(world.source(world.main()));
        let start = assert_some!(source.text().find("TARGET"));
        let mut diagnostic = SourceDiagnostic::error(TypstSpan::detached(), "boom");
        diagnostic.hints = EcoVec::from([Spanned::new(
            "h".into(),
            DiagSpan::from_range(world.main(), start..start + 1),
        )]);
        let mapped = one(convert(&world, [diagnostic]));
        assert!(mapped.hints[0].ends_with("style.typ:4)"), "{}", mapped.hints[0]);
    }

    #[test]
    fn order_is_preserved() {
        let world = fixtures_world();
        let inputs = [
            SourceDiagnostic::error(TypstSpan::detached(), "first"),
            SourceDiagnostic::warning(TypstSpan::detached(), "second"),
        ];
        let messages: Vec<String> = convert(&world, inputs).into_iter().map(|d| d.message).collect();
        assert_eq!(messages, ["first", "second"]);
    }
}
