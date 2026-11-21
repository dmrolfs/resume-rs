//! Differential selection properties against the established resume traversal.
use claims::{assert_ok, assert_some};
use proptest::prelude::*;
use resume::{
    letter::{self, LetterSelectError, SourceItem},
    markdown,
    model::{GroupName, VariantName},
    select::{self, Scope, SelectedTags, Selection},
};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn letter_body_decisions_and_reports_match_resume(
        rows in prop::collection::vec((0u8..3,0u8..3,any::<bool>()),1..10),
        scope in 0u8..3, drafts in any::<bool>(), default_draft in any::<bool>(), short_draft in any::<bool>(),
        preference in any::<bool>(), pick in 0u8..4,
    ) {
        let mut paragraphs = Vec::new();
        for (i,(priority,status,tagged)) in rows.iter().enumerate() {
            let p=[""," @core"," @extra"][*priority as usize];
            let s=[""," @draft"," @verify"][*status as usize];
            let t=if *tagged {" #chosen"} else {""};
            let annotation=format!("{p}{s}{t}");
            paragraphs.push(if annotation.is_empty() {format!("Paragraph {i}.")} else {format!("Paragraph {i}. <!--{annotation} -->")});
        }
        paragraphs.push(format!("Default wording. <!-- =g.default #other{} -->",if default_draft {" @draft"} else {""}));
        paragraphs.push(format!("Short wording. <!-- =g.short #chosen{} -->",if short_draft {" @draft"} else {""}));
        let body=paragraphs.join("\n\n");
        let letter_text=format!("# Alex\n\n## Greeting\n\nHello,\n\n## Body\n\n{body}\n\n## Closing\n\nRegards,\n");
        let l=assert_some!(markdown::parse_letter("l.md",&letter_text).into_parts().0);
        let r=assert_some!(markdown::parse("r.md",&format!("# Alex\n\n## Evidence\n\n{body}\n")).into_parts().0);
        let scope=match scope {0=>Scope::All,1=>Scope::CoreOnly,_=>Scope::Tags(assert_some!(SelectedTags::new([assert_ok!(resume::model::Tag::try_new("chosen"))].into())))};
        let mut selection=Selection {scope,drafts,prefer:if preference {vec![assert_ok!(VariantName::try_new("short"))]}else{vec![]},..Selection::default()};
        if pick>0 {selection.picks.insert(assert_ok!(GroupName::try_new(if pick==3 {"unknown"}else{"g"})),assert_ok!(VariantName::try_new(if pick==2 {"missing"}else{"short"})));}
        let letter_result=letter::select(&l,&selection);
        prop_assert_eq!(&letter_result,&letter::select(&l,&selection));
        let resume_result=select::select(&r,&selection);
        match (letter_result,resume_result) {
            (Err(LetterSelectError::PickedDraft{picks:a}),Err(select::SelectError::PickedDraft{picks:b}))=>prop_assert_eq!(a,b),
            (letter_result,Ok((resume,report)))=>{
                let expected:Vec<String>=resume.sections.iter().flat_map(|s| &s.blocks).map(|b|match b {resume::model::Block::Paragraph{content,..}=>resume::model::plain_text(content).trim().to_owned(),_=>panic!("paragraph")}).collect();
                let (selected,lr)=match letter_result {
                    Ok((s,r))=>(Some(s),r),
                    Err(LetterSelectError::EmptyBody{report})=>(None,*report),
                    other=>panic!("unexpected {other:?}"),
                };
                prop_assert_eq!(selected.is_none(),expected.is_empty());
                if let Some(selected)=selected {
                    let actual:Vec<String>=selected.body.iter().map(|p|p.content.iter().map(|n|match n {letter::LetterInline::Text(s)=>s.as_str(),_=>panic!("text")}).collect::<String>().trim().to_owned()).collect();
                    prop_assert_eq!(actual,expected);
                    let originals:Vec<_>=l.body.iter().flat_map(|item|match item {SourceItem::Paragraph(i)=>vec![&i.piece],SourceItem::Alternates(g)=>g.variants().iter().map(|i|&i.piece).collect()}).collect();
                    for p in &selected.body {prop_assert!(originals.contains(&p));}
                    prop_assert!(selected.body.windows(2).all(|w|w[0].range.start<w[1].range.start));
                }
                prop_assert_eq!(lr.chosen,report.chosen);
                prop_assert_eq!(lr.unknown_picks,report.unknown_picks);
                prop_assert_eq!(lr.skipped_drafts,report.skipped_drafts);
                prop_assert_eq!(lr.drafts_left_out,report.drafts_left_out);
                prop_assert_eq!(lr.drafts_included,report.drafts_included);
                prop_assert_eq!(lr.verify.len(),report.verify.len());
                prop_assert_eq!(lr.dropped.iter().map(|d|d.reason).collect::<Vec<_>>(),report.dropped.iter().filter(|d|d.node.kind==resume::model::NodeKind::Paragraph).map(|d|d.reason).collect::<Vec<_>>());
            },
            other=>panic!("different outcomes: {other:?}"),
        }
    }
}
