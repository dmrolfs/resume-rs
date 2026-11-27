//! Canonical addition output retains authored Markdown and declaration order, never reverse-renders ASTs.
use super::additions::*;
use serde::Serialize;

#[derive(Serialize)]
pub(super) struct PrintedAddition<'a> {
    id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    content: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<Vec<&'a str>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    priority: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    place: Place<'a>,
}
#[derive(Serialize, Default)]
struct Place<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    before: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    after: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    at: Option<&'static str>,
}
impl<'a> PrintedAddition<'a> {
    pub(super) fn resume(a: &'a ResumeAddition) -> Self {
        let place = match a.place() {
            ResumePlace::Blurb { role } => Place {
                parent: Some(role.value.as_ref()),
                at: Some("blurb"),
                ..Default::default()
            },
            ResumePlace::Bullet(p) | ResumePlace::Paragraph(p) => match p {
                LeafPlace::Before(id) => Place {
                    before: Some(id.value.as_ref()),
                    ..Default::default()
                },
                LeafPlace::After(id) => Place {
                    after: Some(id.value.as_ref()),
                    ..Default::default()
                },
                LeafPlace::Start(id) => Place {
                    parent: Some(id.value.as_ref()),
                    at: Some("start"),
                    ..Default::default()
                },
                LeafPlace::End(id) => Place {
                    parent: Some(id.value.as_ref()),
                    at: Some("end"),
                    ..Default::default()
                },
            },
        };
        Self::new(
            a.id().value.as_ref(),
            Some(a.place().kind().to_string()),
            a.content().markdown(),
            a.metadata(),
            place,
        )
    }
    pub(super) fn letter(a: &'a LetterAddition) -> Self {
        let place = match a.place() {
            LetterPlace::Before(id) => Place {
                before: Some(id.value.as_ref()),
                ..Default::default()
            },
            LetterPlace::After(id) => Place {
                after: Some(id.value.as_ref()),
                ..Default::default()
            },
            LetterPlace::Start(_) => Place {
                at: Some("start"),
                ..Default::default()
            },
            LetterPlace::End(_) => Place {
                at: Some("end"),
                ..Default::default()
            },
        };
        Self::new(a.id().value.as_ref(), None, a.content().markdown(), a.metadata(), place)
    }
    fn new(id: &'a str, kind: Option<String>, content: &'a str, m: &'a AdditionMetadata, place: Place<'a>) -> Self {
        Self {
            id,
            kind,
            content,
            place,
            tags: m.tags.as_ref().map(|s| s.value.iter().map(AsRef::as_ref).collect()),
            priority: m.priority.as_ref().map(|s| s.value.to_string()),
            status: m.status.as_ref().map(|s| s.value.to_string()),
        }
    }
}
