//! ABOUTME: The typed resume model: the JSON contract (schema version 1) every style reads.
//! ABOUTME: Produced by the parser and serialized to `/resume.json`; never deserialized.

pub mod options;
pub mod period;
pub mod text;

use serde::Serialize;

pub use self::period::{PartialDate, Period, PeriodEnd, Season, Year};
pub use self::text::{Company, EraName, LinkUrl, Name, SectionTitle, StyleName, Title};

/// Version of the `/resume.json` contract.
pub const SCHEMA_VERSION: u32 = 1;

/// A whole resume.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Resume {
    /// Always [`SCHEMA_VERSION`].
    pub schema: u32,
    pub name: Name,
    /// The first paragraph after the name; may be empty.
    pub contact: Vec<Inline>,
    pub sections: Vec<Section>,
}

impl Resume {
    /// Builds a resume stamped with the current [`SCHEMA_VERSION`].
    #[must_use]
    pub fn new(name: Name, contact: Vec<Inline>, sections: Vec<Section>) -> Self {
        Self {
            schema: SCHEMA_VERSION,
            name,
            contact,
            sections,
        }
    }
}

/// A `## ` section and its blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Section {
    pub title: SectionTitle,
    pub blocks: Vec<Block>,
}

/// A block inside a section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Block {
    Paragraph { content: Vec<Inline> },
    Bullets { items: Vec<Bullet> },
    Role(Role),
    PageBreak,
}

/// A `### ` role line and what follows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Role {
    pub company: Company,
    /// `None` for `### Company · Period`.
    pub title: Option<Title>,
    pub period: Period,
    /// The first paragraph after the role line.
    pub blurb: Option<Vec<Inline>>,
    pub body: Vec<RoleBlock>,
}

/// A block inside a role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RoleBlock {
    Era {
        name: EraName,
        period: Period,
        items: Vec<Bullet>,
    },
    Bullets {
        items: Vec<Bullet>,
    },
    Paragraph {
        content: Vec<Inline>,
    },
}

/// A list item, with an optional leading `**Label:**`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Bullet {
    /// The leading label with its colon stripped.
    pub label: Option<Vec<Inline>>,
    pub content: Vec<Inline>,
}

/// An inline run of text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Inline {
    Text { text: String },
    Strong { content: Vec<Inline> },
    Emph { content: Vec<Inline> },
    Link { url: LinkUrl, content: Vec<Inline> },
    Code { text: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_ok;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn text(s: &str) -> Inline {
        Inline::Text { text: s.to_string() }
    }

    fn sample() -> Resume {
        let year = |y| assert_ok!(Year::try_new(y));
        let start = PartialDate::Month {
            year: year(2017),
            month: time::Month::August,
        };
        let period = assert_ok!(Period::try_new("Aug 2017 – Present", start, PeriodEnd::Present));
        let era_period = assert_ok!(Period::try_new(
            "2017 – 2019",
            PartialDate::Year { year: year(2017) },
            PeriodEnd::Date(PartialDate::Year { year: year(2019) }),
        ));
        let bullet = Bullet {
            label: Some(vec![text("Languages")]),
            content: vec![text("Rust")],
        };
        let role = Role {
            company: assert_ok!(Company::try_new("Initech")),
            title: Some(assert_ok!(Title::try_new("VP of Engineering"))),
            period,
            blurb: Some(vec![text("Enterprise software.")]),
            body: vec![RoleBlock::Era {
                name: assert_ok!(EraName::try_new("Early years")),
                period: era_period,
                items: vec![bullet],
            }],
        };
        let url = assert_ok!(url::Url::parse("https://example.com"));
        let section = Section {
            title: assert_ok!(SectionTitle::try_new("Experience")),
            blocks: vec![Block::Role(role), Block::PageBreak],
        };
        Resume::new(
            assert_ok!(Name::try_new("Ada Lovelace")),
            vec![Inline::Link {
                url: assert_ok!(LinkUrl::try_new(url)),
                content: vec![text("site")],
            }],
            vec![section],
        )
    }

    #[test]
    fn role_with_era_json_snapshot() {
        insta::assert_json_snapshot!(sample());
    }

    #[test]
    fn schema_version_is_emitted() {
        let value = assert_ok!(serde_json::to_value(sample()));
        assert_eq!(value["schema"], json!(SCHEMA_VERSION));
        assert_eq!(SCHEMA_VERSION, 1);
    }
}
