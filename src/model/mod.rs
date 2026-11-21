//! ABOUTME: The typed resume model: the JSON contract (schema version 1) every style reads.
//! ABOUTME: Produced by the parser and serialized to `/resume.json`; never deserialized.

pub mod annotation;
pub mod options;
pub mod period;
pub mod text;

use serde::Serialize;

pub use self::annotation::{Alternate, Annotations, GroupName, Priority, Status, Tag, VariantName};
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
    /// The annotation on the `## ` line; selection reads it, styles never see it.
    #[serde(skip)]
    pub annotations: Annotations,
}

/// A block inside a section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Block {
    Paragraph {
        content: Vec<Inline>,
        #[serde(skip)]
        annotations: Annotations,
    },
    Bullets {
        items: Vec<Bullet>,
    },
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
    /// The annotation on the `### ` line.
    #[serde(skip)]
    pub annotations: Annotations,
    /// The annotation on the blurb; empty without a blurb.
    #[serde(skip)]
    pub blurb_annotations: Annotations,
}

/// A block inside a role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RoleBlock {
    Era {
        name: EraName,
        period: Period,
        items: Vec<Bullet>,
        /// The annotation on the era line.
        #[serde(skip)]
        annotations: Annotations,
    },
    Bullets {
        items: Vec<Bullet>,
    },
    Paragraph {
        content: Vec<Inline>,
        #[serde(skip)]
        annotations: Annotations,
    },
}

/// A list item, with an optional leading `**Label:**`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Bullet {
    /// The leading label with its colon stripped.
    pub label: Option<Vec<Inline>>,
    pub content: Vec<Inline>,
    #[serde(skip)]
    pub annotations: Annotations,
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

impl Resume {
    /// Every annotation in the resume, in document order; for tests that set, clear, or count them all.
    ///
    /// The one place that lists every annotated node, so a test cannot miss a field that a new node adds.
    #[cfg(test)]
    pub(crate) fn annotations_mut(&mut self) -> Vec<&mut Annotations> {
        let mut out = Vec::new();
        for section in &mut self.sections {
            out.push(&mut section.annotations);
            for block in &mut section.blocks {
                match block {
                    Block::Paragraph { annotations, .. } => out.push(annotations),
                    Block::Bullets { items } => out.extend(items.iter_mut().map(|b| &mut b.annotations)),
                    Block::Role(role) => {
                        out.push(&mut role.annotations);
                        out.push(&mut role.blurb_annotations);
                        for part in &mut role.body {
                            match part {
                                RoleBlock::Era { items, annotations, .. } => {
                                    out.push(annotations);
                                    out.extend(items.iter_mut().map(|b| &mut b.annotations));
                                }
                                RoleBlock::Bullets { items } => {
                                    out.extend(items.iter_mut().map(|b| &mut b.annotations))
                                }
                                RoleBlock::Paragraph { annotations, .. } => out.push(annotations),
                            }
                        }
                    }
                    Block::PageBreak => {}
                }
            }
        }
        out
    }
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
            annotations: Annotations::default(),
        };
        let role = Role {
            company: assert_ok!(Company::try_new("Initech")),
            title: Some(assert_ok!(Title::try_new("VP of Engineering"))),
            period,
            blurb: Some(vec![text("Enterprise software.")]),
            annotations: Annotations::default(),
            blurb_annotations: Annotations::default(),
            body: vec![RoleBlock::Era {
                name: assert_ok!(EraName::try_new("Early years")),
                period: era_period,
                items: vec![bullet],
                annotations: Annotations::default(),
            }],
        };
        let url = assert_ok!(url::Url::parse("https://example.com"));
        let section = Section {
            title: assert_ok!(SectionTitle::try_new("Experience")),
            blocks: vec![Block::Role(role), Block::PageBreak],
            annotations: Annotations::default(),
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

    /// `resume` with a paragraph added to each section and role, and every annotation set to `annotations`.
    fn annotate_everything(mut resume: Resume, annotations: &Annotations) -> Resume {
        for section in &mut resume.sections {
            section.blocks.push(Block::Paragraph {
                content: vec![text("Prose.")],
                annotations: Annotations::default(),
            });
            for block in &mut section.blocks {
                if let Block::Role(role) = block {
                    role.body.push(RoleBlock::Paragraph {
                        content: vec![text("Role prose.")],
                        annotations: Annotations::default(),
                    });
                }
            }
        }
        for slot in resume.annotations_mut() {
            *slot = annotations.clone();
        }
        resume
    }

    #[test]
    fn the_visitor_reaches_every_kind_of_annotated_node() {
        // Section, role, blurb, era, era bullet, section paragraph, role paragraph.
        let mut resume = annotate_everything(sample(), &Annotations::default());
        assert_eq!(resume.annotations_mut().len(), 7);
    }

    #[test]
    fn annotations_never_reach_the_json() {
        let annotations = Annotations {
            priority: Some(Priority::Core),
            status: Some(Status::Verify),
            tags: std::collections::BTreeSet::from([assert_ok!(Tag::try_new("ai"))]),
            alternate: Some(Alternate {
                group: assert_ok!(GroupName::try_new("gateway")),
                variant: assert_ok!(VariantName::try_new("short")),
            }),
        };
        let plain = annotate_everything(sample(), &Annotations::default());
        let annotated = annotate_everything(sample(), &annotations);
        assert_ne!(plain, annotated, "the annotations are set");
        assert_eq!(
            assert_ok!(serde_json::to_string(&annotated)),
            assert_ok!(serde_json::to_string(&plain))
        );
    }

    #[test]
    fn schema_version_is_emitted() {
        let value = assert_ok!(serde_json::to_value(sample()));
        assert_eq!(value["schema"], json!(SCHEMA_VERSION));
        assert_eq!(SCHEMA_VERSION, 1);
    }
}
