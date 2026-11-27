//! ABOUTME: A located outline of the resume, recorded by the parser in the same pass that builds the model.
//! ABOUTME: `doctor` lints it; the model stays span-free, and `build` never sees it.

use std::ops::Range;

use super::tree::{InlineNode, TextNode};
use crate::model::Period;

/// What the parser kept of a resume, with where each part sits in the source.
///
/// Recorded next to the model by the same walk, so the two agree on which sections, roles, and bullets exist.
/// Not serialized and not part of the model's JSON contract.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Outline {
    pub sections: Vec<SectionOutline>,
    /// The visible text of each variant of each valid alternate group, in document order.
    pub groups: Vec<Vec<Located>>,
    /// The source range of each node of the model, in the order [`crate::model::Resume::nodes`] visits them, so a
    /// report about a node can name its line and a view can mark its text. A node's range is its own block: a
    /// heading line, a paragraph, or a bullet's first paragraph, with any annotation comment written inside it.
    pub ranges: Vec<Range<usize>>,
}

/// A kept `##` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SectionOutline {
    pub title: String,
    /// The `## ` heading.
    pub heading: Range<usize>,
    /// Paragraphs directly in the section.
    pub prose: Vec<Located>,
    /// Bullets directly in the section, outside any role.
    pub bullets: Vec<BulletOutline>,
    pub roles: Vec<RoleOutline>,
}

/// A kept `###` role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RoleOutline {
    /// The role line's text.
    pub line: Range<usize>,
    pub company: String,
    pub period: Period,
    /// The periods of the role's eras, in order.
    pub eras: Vec<Period>,
    pub blurb: Option<Located>,
    /// Paragraphs after the blurb.
    pub paragraphs: Vec<Located>,
    /// Role bullets and era bullets, in document order.
    pub bullets: Vec<BulletOutline>,
}

/// A kept bullet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BulletOutline {
    /// The item's first paragraph.
    pub item: Range<usize>,
    pub label: Option<Located>,
    pub content: Located,
    /// The bullet's annotation comment, where findings about its annotation point.
    pub annotation: Option<Range<usize>>,
    /// Where the parent bullet starts, for a nested bullet; it names the parent as a container of alternates.
    pub nested_in: Option<usize>,
}

/// Visible text with a map back to the source.
///
/// The text is what a reader sees: link text but not the address, code text, and a break as one space; comments,
/// images, and raw HTML contribute nothing. Smart punctuation has been applied, so an apostrophe may be `'` or `’`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Located {
    text: TextNode,
    /// Byte ranges of the text that came from code spans.
    code: Vec<Range<usize>>,
}

impl Located {
    /// The visible text of `nodes`.
    pub fn of(nodes: &[InlineNode]) -> Self {
        let mut located = Self::default();
        located.add(nodes);
        located
    }

    fn add(&mut self, nodes: &[InlineNode]) {
        for node in nodes {
            match node {
                InlineNode::Text(text) => self.text.append(text),
                InlineNode::Code { range, text } => {
                    let start = self.text.text.len();
                    self.text.push(text, range.clone(), false);
                    self.code.push(start..self.text.text.len());
                }
                InlineNode::Break { range, .. } => self.text.push(" ", range.clone(), false),
                InlineNode::Strong { children, .. }
                | InlineNode::Emph { children, .. }
                | InlineNode::Link { children, .. } => self.add(children),
                InlineNode::Html { .. } | InlineNode::Image { .. } | InlineNode::Unsupported { .. } => {}
            }
        }
    }

    /// Appends `other`, separated by one space.
    pub fn append_with_space(&mut self, other: &Self, gap: Range<usize>) {
        self.text.push(" ", gap, false);
        let offset = self.text.text.len();
        self.text.append(&other.text);
        self.code
            .extend(other.code.iter().map(|r| r.start + offset..r.end + offset));
    }

    /// The visible text, as stored: it can keep the space that followed a label or a comment.
    #[cfg(test)]
    pub fn text(&self) -> &str {
        &self.text.text
    }

    /// The visible text without leading and trailing whitespace, as the model keeps it.
    pub fn trimmed(&self) -> &str {
        self.text.text.trim()
    }

    /// The visible text with each code span blanked to spaces of the same byte length, so word matching never
    /// looks inside code and every offset still indexes [`Located::text`].
    pub fn matched(&self) -> String {
        let text = &self.text.text;
        let mut out = String::with_capacity(text.len());
        for (at, c) in text.char_indices() {
            if self.code.iter().any(|r| r.contains(&at)) {
                out.extend(std::iter::repeat_n(' ', c.len_utf8()));
            } else {
                out.push(c);
            }
        }
        out
    }

    /// The source range of `text_range`: exact inside verbatim runs, the whole run otherwise.
    pub fn source_range(&self, text_range: Range<usize>) -> Range<usize> {
        self.text.source_range(text_range)
    }
}

#[cfg(test)]
mod tests {
    use crate::markdown::parse;
    use claims::assert_some;
    use pretty_assertions::assert_eq;

    const DOC: &str = "# Ada\n\nLondon\n\n## Summary\n\nBuilt `x` things.\n\n- **Languages:** Rust\n\n## Experience\n\n### Acme · Engineer · 2019 – 2020\n\nMakes tools.\n\n- Cut costs 40%.\n\n**Early · 2019**\n\n- Joined.\n";

    #[test]
    fn the_outline_has_the_sections_roles_and_bullets_the_model_has() {
        let parsed = parse("r.md", DOC);
        let outline = assert_some!(parsed.outline());
        let resume = assert_some!(parsed.best_effort());
        assert_eq!(outline.sections.len(), resume.sections.len());
        let titles: Vec<&str> = outline.sections.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(titles, ["Summary", "Experience"]);
        let role = &outline.sections[1].roles[0];
        assert_eq!(role.company, "Acme");
        assert_eq!(role.bullets.len(), 2, "role and era bullets");
        assert_eq!(role.eras.len(), 1);
        assert_eq!(assert_some!(role.blurb.as_ref()).text(), "Makes tools.");
    }

    #[test]
    fn a_label_is_split_from_the_content() {
        let parsed = parse("r.md", DOC);
        let outline = assert_some!(parsed.outline());
        let bullet = &outline.sections[0].bullets[0];
        assert_eq!(assert_some!(bullet.label.as_ref()).text(), "Languages:");
        assert_eq!(bullet.content.trimmed(), "Rust");
    }

    #[test]
    fn code_is_blanked_in_the_matched_text_but_kept_in_the_visible_text() {
        let parsed = parse("r.md", DOC);
        let outline = assert_some!(parsed.outline());
        let prose = &outline.sections[0].prose[0];
        assert_eq!(prose.text(), "Built x things.");
        assert_eq!(prose.matched(), "Built   things.");
    }

    #[test]
    fn a_section_with_a_bad_title_is_not_in_the_outline_just_as_it_is_not_in_the_model() {
        let doc = "# Ada\n\nLondon\n\n## \n\n- gone\n\n## Real\n\n- kept\n";
        let parsed = parse("r.md", doc);
        let outline = assert_some!(parsed.outline());
        assert_eq!(
            outline.sections.len(),
            assert_some!(parsed.best_effort()).sections.len()
        );
        assert_eq!(outline.sections.len(), 1);
    }

    #[test]
    fn there_is_no_outline_without_a_name() {
        let parsed = parse("r.md", "## Experience\n\n- x\n");
        assert!(parsed.outline().is_none());
    }

    #[test]
    fn a_phrase_maps_to_its_own_source_bytes() {
        let parsed = parse("r.md", DOC);
        let outline = assert_some!(parsed.outline());
        let bullet = &outline.sections[1].roles[0].bullets[0];
        let at = assert_some!(bullet.content.text().find("40%"));
        let range = bullet.content.source_range(at..at + 3);
        assert_eq!(&DOC[range], "40%");
    }
}
