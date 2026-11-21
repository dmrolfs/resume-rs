//! ABOUTME: The inventory of a baseline's annotations: tags, keywords, and alternate groups, with their lines.
//! ABOUTME: What a position spec or the advisor can name; read from the whole baseline, before any selection.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::model::{NodeKind, NodeView, Priority, Resume, Status};

/// What a baseline's annotations offer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Inventory {
    pub tags: Vec<TagUse>,
    /// The four keywords, always all four, in the order `core`, `extra`, `draft`, `verify`.
    pub keywords: Vec<KeywordUse>,
    pub groups: Vec<GroupUse>,
}

/// One tag: the nodes that carry it themselves, and those that carry it only through a section, role, or era.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TagUse {
    pub name: String,
    /// Source lines of the nodes that carry the tag themselves.
    pub own: Vec<usize>,
    /// How many nodes carry it only because a container above them does.
    pub inherited: usize,
}

/// One keyword and the lines of the nodes that carry it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeywordUse {
    pub name: &'static str,
    pub lines: Vec<usize>,
}

/// One alternate group and its wordings, in document order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GroupUse {
    pub name: String,
    pub kind: NodeKind,
    pub variants: Vec<VariantUse>,
}

/// One wording of a group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VariantUse {
    pub name: String,
    pub line: usize,
    pub default: bool,
    /// Marked `@draft` itself, or under a `@draft` section, role, or era.
    pub draft: bool,
    /// The wording's first words.
    pub text: String,
}

impl Inventory {
    /// The inventory of `resume`, whose nodes start at `lines` (in [`Resume::nodes`] order, as
    /// `Parsed::node_lines` gives them).
    #[must_use]
    pub fn of(resume: &Resume, lines: &[usize]) -> Self {
        let nodes = resume.nodes();
        let line = |index: usize| lines.get(index).copied().unwrap_or(0);
        let mut tags: BTreeMap<String, TagUse> = BTreeMap::new();
        let mut keywords: [(&'static str, Vec<usize>); 4] = [
            ("core", Vec::new()),
            ("extra", Vec::new()),
            ("draft", Vec::new()),
            ("verify", Vec::new()),
        ];
        let mut groups: Vec<GroupUse> = Vec::new();
        // The containers open above the current node, by depth: their tags and whether one is a draft.
        let mut open: Vec<(Vec<String>, bool)> = Vec::new();
        for (index, node) in nodes.iter().enumerate() {
            open.truncate(node.depth);
            let annotations = node.annotations;
            let under_draft = open.iter().any(|(_, draft)| *draft);
            // A set: a tag on both a section and its role still makes one inheriting node, not two.
            let inherited: std::collections::BTreeSet<&String> = open.iter().flat_map(|(t, _)| t).collect();
            for tag in &annotations.tags {
                let entry = tags.entry(tag.to_string()).or_insert_with(|| TagUse {
                    name: tag.to_string(),
                    own: Vec::new(),
                    inherited: 0,
                });
                entry.own.push(line(index));
            }
            for name in inherited
                .iter()
                .filter(|t| !annotations.tags.iter().any(|own| own.as_ref() == t.as_str()))
            {
                if let Some(entry) = tags.get_mut(name.as_str()) {
                    entry.inherited += 1;
                }
            }
            let flags = [
                annotations.priority == Some(Priority::Core),
                annotations.priority == Some(Priority::Extra),
                annotations.status == Some(Status::Draft),
                annotations.status == Some(Status::Verify),
            ];
            for ((_, at), set) in keywords.iter_mut().zip(flags) {
                if set {
                    at.push(line(index));
                }
            }
            if let Some(alternate) = &annotations.alternate {
                let variant = VariantUse {
                    name: alternate.variant.to_string(),
                    line: line(index),
                    default: alternate.variant.is_default(),
                    draft: under_draft || annotations.status == Some(Status::Draft),
                    text: first_words(node),
                };
                match groups.iter_mut().find(|g| g.name == alternate.group.as_ref()) {
                    Some(group) => group.variants.push(variant),
                    None => groups.push(GroupUse {
                        name: alternate.group.to_string(),
                        kind: node.kind,
                        variants: vec![variant],
                    }),
                }
            }
            if node.heading.is_some() {
                let own = annotations.tags.iter().map(ToString::to_string).collect();
                open.push((own, under_draft || annotations.status == Some(Status::Draft)));
            }
        }
        Self {
            tags: tags.into_values().collect(),
            keywords: keywords
                .into_iter()
                .map(|(name, lines)| KeywordUse { name, lines })
                .collect(),
            groups,
        }
    }
}

/// A node's first words, for a listing: at most eight.
fn first_words(node: &NodeView<'_>) -> String {
    let text = node.content.map(crate::model::plain_text).unwrap_or_default();
    let mut words = text.split_whitespace();
    let mut out: Vec<&str> = words.by_ref().take(8).collect();
    if words.next().is_some() {
        out.push("…");
    }
    out.join(" ")
}
