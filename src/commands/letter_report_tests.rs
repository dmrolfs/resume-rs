use super::*;
use crate::diagnostics::{ItemOrigin, SourceKind, SourceText, Sources};
use crate::letter::{LetterPart, SourceItem};
use claims::{assert_err, assert_ok, assert_some};
use pretty_assertions::assert_eq;
use std::path::Path;

#[test]
fn spec_pieces_keep_locations_through_selection_reporting_and_refusal() {
    let baseline = "# Ada\n\n## Greeting\n\nHello\n\n## Body\n\nBaseline\n\n## Closing\n\nThanks\n";
    let parsed = crate::markdown::parse_letter("letter.md", baseline);
    let mut source = assert_some!(parsed.valid()).clone();
    let snapshot = SourceText::new(
        "position.toml".into(),
        SourceKind::Spec,
        "# spec\ncontent = '{{company}}'\n",
    );
    let mut added = match source.body[0].clone() {
        SourceItem::Paragraph(item) => item,
        _ => panic!("paragraph"),
    };
    added.piece.content = vec![crate::letter::LetterInline::Placeholder {
        key: crate::letter::Placeholder::Company,
        range: 0..11,
    }];
    added.piece.origin = assert_some!(ItemOrigin::spec(&snapshot, 17..30));
    added.annotations.status = Some(crate::model::Status::Draft);
    added.annotation_range = None;
    source.body.push(SourceItem::Paragraph(added));
    let refusal = assert_err!(crate::letter::SelectedLetter::try_from(source.clone()));
    assert_eq!(refusal.span.kind, SourceKind::Spec);
    assert_eq!(refusal.span.range, 17..30);
    let (selected, report) = assert_ok!(crate::letter::select(&source, &Selection::default()));
    assert_ok!(crate::letter::resolve(
        selected,
        &crate::letter::LetterFields::default()
    ));
    assert_eq!(report.dropped[0].item.part, LetterPart::Body);
    let mut sources = Sources::new(None);
    sources.insert("letter.md", baseline);
    sources.insert_text(&snapshot);
    let record = LetterSelectionRecord::new(
        "letter.md".into(),
        Path::new("letter.md"),
        &sources,
        &report,
        &Selection::default(),
    );
    let json = assert_ok!(serde_json::to_value(&record));
    assert_eq!(json["dropped"][0]["file"], "position.toml");
    assert_eq!(json["dropped"][0]["line"], 2);
    assert_eq!(json["dropped"][0]["column"], 11);
    assert!(
        record
            .lines(&Selection::default())
            .iter()
            .any(|line| line.contains("position.toml:2:11"))
    );
    assert!(
        record
            .lines(&Selection::default())
            .iter()
            .any(|line| line == "kept: body at line 9")
    );
}

#[test]
#[should_panic(expected = "selected letter origins have registered sources")]
fn missing_source_mapping_is_not_silently_reported_as_line_zero() {
    let report = LetterReport {
        kept: vec![LetterRef {
            part: LetterPart::Body,
            origin: ItemOrigin::SpecContent {
                file: "missing.toml".into(),
                content: 0..1,
            },
        }],
        ..Default::default()
    };
    LetterSelectionRecord::new(
        "letter.md".into(),
        Path::new("letter.md"),
        &Sources::new(None),
        &report,
        &Selection::default(),
    );
}
