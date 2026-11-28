use super::{Content, ContentItem, Outcome, render};
use ::resume::letter::{Letter, LetterPart, LetterReport, LetterSource, SelectedLetter, SourceItem};
use std::collections::{BTreeMap, BTreeSet};

fn origin_key(origin: &::resume::diagnostics::ItemOrigin) -> (&std::path::Path, usize, usize) {
    match origin {
        ::resume::diagnostics::ItemOrigin::Baseline { file, range } => (file, range.start, range.end),
        ::resume::diagnostics::ItemOrigin::SpecContent { file, content } => (file, content.start, content.end),
    }
}

impl Content {
    pub fn letter(source: &LetterSource, assessment: Option<(&SelectedLetter, &LetterReport, &Letter)>) -> Self {
        let resolved_pieces: BTreeMap<_, _> = assessment
            .into_iter()
            .flat_map(|(selected, _, resolved)| {
                std::iter::once((&selected.greeting, &resolved.greeting))
                    .chain(selected.body.iter().zip(&resolved.body))
                    .chain(std::iter::once((&selected.closing, &resolved.closing)))
                    .map(|(p, content)| (origin_key(&p.origin), content))
            })
            .collect();
        let dropped: BTreeMap<_, _> = assessment
            .into_iter()
            .flat_map(|(_, report, _)| &report.dropped)
            .map(|d| (origin_key(&d.item.origin), d.reason))
            .collect();
        let chosen: BTreeMap<_, _> = assessment
            .into_iter()
            .flat_map(|(_, report, _)| &report.chosen)
            .map(|c| (&c.group, c))
            .collect();
        let mut items = Vec::new();
        for (part, group) in std::iter::once((LetterPart::Greeting, &source.greeting))
            .chain(source.body.iter().map(|p| (LetterPart::Body, p)))
            .chain(std::iter::once((LetterPart::Closing, &source.closing)))
        {
            let variants = match group {
                SourceItem::Paragraph(p) => std::slice::from_ref(p),
                SourceItem::Alternates(g) => g.variants(),
            };
            let tags = ::resume::select::rules::GroupFacts::of(variants.iter().map(|v| &v.annotations)).tags;
            for variant in variants {
                let mut item = ContentItem {
                    origin: variant.piece.origin.clone(),
                    annotations: variant.annotations.clone(),
                    group_tags: tags.clone(),
                    inherited_tags: BTreeSet::new(),
                    depth: 0,
                    text: render::letter_inline(&variant.piece.content),
                    outcome: Outcome::Unselected,
                    promoted: None,
                    mandatory: part != LetterPart::Body,
                };
                if assessment.is_some() {
                    let dropped = dropped.get(&origin_key(&variants[0].piece.origin));
                    let chosen = item.annotations.alternate.as_ref().and_then(|a| chosen.get(&a.group));
                    item.outcome = if let Some(reason) = dropped {
                        Outcome::Dropped(*reason)
                    } else if let Some(chosen) = chosen {
                        if item
                            .annotations
                            .alternate
                            .as_ref()
                            .is_some_and(|a| a.variant == chosen.variant)
                        {
                            Outcome::Chosen(chosen.by)
                        } else {
                            Outcome::NotChosen
                        }
                    } else if resolved_pieces.contains_key(&origin_key(&item.origin)) {
                        Outcome::Kept
                    } else {
                        Outcome::Unavailable
                    };
                    if let Some(content) = resolved_pieces.get(&origin_key(&item.origin)) {
                        item.text = render::inline(content);
                    }
                }
                items.push(item);
            }
        }
        let rendered = assessment.map_or_else(
            || {
                let mut parts = vec![
                    format!("# {}", render::literal(source.name.as_ref())),
                    render::inline(&source.contact),
                ];
                parts.extend(items.iter().map(|i| i.text.clone()));
                parts.join("\n\n")
            },
            |(_, _, resolved)| render::letter(resolved),
        );
        Self {
            problems: vec![],
            items,
            rendered: Some(rendered),
            assessed: assessment.is_some(),
        }
    }
}
