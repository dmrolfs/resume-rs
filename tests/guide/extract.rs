//! ABOUTME: Reads the structure of a Markdown guide: headings, fenced blocks, tables, links, and `guide:` markers.
//! ABOUTME: Checks compare these hooks with the code, never the wording, so a prose edit changes nothing here.

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use regex::Regex;

/// A heading and the text that belongs to it, up to the next heading of the same or a higher level.
#[derive(Debug, Clone)]
pub struct Heading {
    pub level: usize,
    pub text: String,
    pub slug: String,
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

/// A fenced code block. `info` is everything after the opening fence.
#[derive(Debug, Clone)]
pub struct Fence {
    pub info: String,
    pub body: String,
    pub line: usize,
    pub start: usize,
    /// The targets of the links in the paragraph that ends right before the fence, if one does.
    pub links_before: Vec<String>,
}

/// A table: the header cells, and each row's cells with code spans written between backticks.
#[derive(Debug, Clone)]
pub struct Table {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub start: usize,
}

/// `<!-- guide:KIND ARGS -->`
#[derive(Debug, Clone)]
pub struct Marker {
    pub kind: String,
    pub args: String,
    pub line: usize,
    pub start: usize,
}

#[derive(Debug, Clone)]
pub struct Link {
    pub url: String,
    pub line: usize,
    pub start: usize,
}

#[derive(Debug, Clone)]
pub struct Guide {
    pub text: String,
    pub headings: Vec<Heading>,
    pub fences: Vec<Fence>,
    pub tables: Vec<Table>,
    pub markers: Vec<Marker>,
    pub links: Vec<Link>,
    /// `<a id="...">` anchors.
    pub anchors: Vec<String>,
}

/// The line number, from 1, of byte `offset` in `text`.
pub fn line_of(text: &str, offset: usize) -> usize {
    text[..offset.min(text.len())].matches('\n').count() + 1
}

/// The anchor GitHub gives a heading: lowercase letters and digits, spaces and hyphens as hyphens.
pub fn slug(heading: &str) -> String {
    heading
        .chars()
        .filter_map(|c| match c {
            c if c.is_alphanumeric() => Some(c.to_ascii_lowercase()),
            ' ' | '-' => Some('-'),
            '_' => Some('_'),
            _ => None,
        })
        .collect()
}

impl Guide {
    pub fn parse(text: &str) -> Self {
        let marker = Regex::new(r"(?s)<!--\s*guide:(\S+)\s*(.*?)\s*-->").expect("a valid regex");
        let anchor = Regex::new(r#"<a id="([^"]+)">"#).expect("a valid regex");
        let mut guide = Self {
            text: text.to_string(),
            headings: Vec::new(),
            fences: Vec::new(),
            tables: Vec::new(),
            markers: Vec::new(),
            links: Vec::new(),
            anchors: anchor.captures_iter(text).map(|c| c[1].to_string()).collect(),
        };

        let mut heading: Option<(usize, usize, String)> = None; // level, start, text so far
        let mut fence: Option<(String, usize, String)> = None; // info, start, body so far
        let mut table: Option<Table> = None;
        let mut row: Option<Vec<String>> = None;
        let mut cell: Option<String> = None;
        let mut in_head = false;
        let mut paragraph_links: Vec<String> = Vec::new();
        let mut links_before_next: Vec<String> = Vec::new();
        let mut just_after_paragraph = false;

        for (event, range) in Parser::new_ext(text, Options::ENABLE_TABLES).into_offset_iter() {
            match event {
                Event::Start(Tag::Heading { level, .. }) => {
                    heading = Some((level as usize, range.start, String::new()));
                    just_after_paragraph = false;
                }
                Event::End(TagEnd::Heading(_)) => {
                    if let Some((level, start, text_so_far)) = heading.take() {
                        guide.headings.push(Heading {
                            level,
                            slug: slug(&text_so_far),
                            text: text_so_far,
                            line: line_of(text, start),
                            start,
                            end: text.len(),
                        });
                    }
                }
                Event::Start(Tag::Paragraph) => paragraph_links.clear(),
                Event::End(TagEnd::Paragraph) => {
                    links_before_next = std::mem::take(&mut paragraph_links);
                    just_after_paragraph = true;
                }
                Event::Start(Tag::CodeBlock(kind)) => {
                    let info = match kind {
                        CodeBlockKind::Fenced(info) => info.to_string(),
                        CodeBlockKind::Indented => String::new(),
                    };
                    fence = Some((info, range.start, String::new()));
                    if !just_after_paragraph {
                        links_before_next.clear();
                    }
                    just_after_paragraph = false;
                }
                Event::End(TagEnd::CodeBlock) => {
                    if let Some((info, start, body)) = fence.take() {
                        guide.fences.push(Fence {
                            info,
                            body,
                            line: line_of(text, start),
                            start,
                            links_before: std::mem::take(&mut links_before_next),
                        });
                    }
                }
                Event::Start(Tag::Table(_)) => {
                    table = Some(Table {
                        header: Vec::new(),
                        rows: Vec::new(),
                        start: range.start,
                    });
                    just_after_paragraph = false;
                }
                Event::End(TagEnd::Table) => guide.tables.extend(table.take()),
                Event::Start(Tag::TableHead) => {
                    in_head = true;
                    row = Some(Vec::new());
                }
                Event::End(TagEnd::TableHead) => {
                    if let (Some(t), Some(r)) = (table.as_mut(), row.take()) {
                        t.header = r;
                    }
                    in_head = false;
                }
                Event::Start(Tag::TableRow) => row = Some(Vec::new()),
                Event::End(TagEnd::TableRow) => {
                    if let (Some(t), Some(r), false) = (table.as_mut(), row.take(), in_head) {
                        t.rows.push(r);
                    }
                }
                Event::Start(Tag::TableCell) => cell = Some(String::new()),
                Event::End(TagEnd::TableCell) => {
                    if let (Some(r), Some(c)) = (row.as_mut(), cell.take()) {
                        r.push(c.trim().to_string());
                    }
                }
                Event::Start(Tag::Link { dest_url, .. }) => {
                    paragraph_links.push(dest_url.to_string());
                    guide.links.push(Link {
                        url: dest_url.to_string(),
                        line: line_of(text, range.start),
                        start: range.start,
                    });
                }
                Event::Code(code) => {
                    if let Some((_, _, t)) = heading.as_mut() {
                        t.push_str(&code);
                    }
                    if let Some(c) = cell.as_mut() {
                        c.push('`');
                        c.push_str(&code);
                        c.push('`');
                    }
                }
                Event::Text(t) => {
                    if let Some((_, _, body)) = fence.as_mut() {
                        body.push_str(&t);
                    } else if let Some((_, _, h)) = heading.as_mut() {
                        h.push_str(&t);
                    } else if let Some(c) = cell.as_mut() {
                        c.push_str(&t);
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    if let Some(c) = cell.as_mut() {
                        c.push(' ');
                    }
                }
                Event::Html(html) => {
                    for m in marker.captures_iter(&html) {
                        let at = range.start + m.get(0).map_or(0, |g| g.start());
                        guide.markers.push(Marker {
                            kind: m[1].to_string(),
                            args: m[2].to_string(),
                            line: line_of(text, at),
                            start: at,
                        });
                    }
                }
                _ => {}
            }
        }

        // A section runs to the next heading of the same or a higher level.
        let ends: Vec<usize> = (0..guide.headings.len())
            .map(|i| {
                let level = guide.headings[i].level;
                guide.headings[i + 1..]
                    .iter()
                    .find(|h| h.level <= level)
                    .map_or(text.len(), |h| h.start)
            })
            .collect();
        for (heading, end) in guide.headings.iter_mut().zip(ends) {
            heading.end = end;
        }
        guide
    }

    pub fn heading(&self, level: usize, text: &str) -> Option<&Heading> {
        self.headings.iter().find(|h| h.level == level && h.text == text)
    }

    pub fn body(&self, heading: &Heading) -> &str {
        &self.text[heading.start..heading.end]
    }

    pub fn range(heading: &Heading) -> Range<usize> {
        heading.start..heading.end
    }

    pub fn fences_in(&self, range: Range<usize>) -> impl Iterator<Item = &Fence> {
        self.fences.iter().filter(move |f| range.contains(&f.start))
    }

    pub fn tables_in(&self, range: Range<usize>) -> impl Iterator<Item = &Table> {
        self.tables.iter().filter(move |t| range.contains(&t.start))
    }

    pub fn markers_in(&self, range: Range<usize>) -> impl Iterator<Item = &Marker> {
        self.markers.iter().filter(move |m| range.contains(&m.start))
    }

    /// The text of a section without its fenced blocks, which hold examples and not the guide's own words.
    pub fn prose(&self, heading: &Heading) -> String {
        let mut out = String::new();
        let mut at = heading.start;
        for fence in self.fences_in(Self::range(heading)) {
            out.push_str(&self.text[at..fence.start]);
            let after = self.text[fence.start..]
                .find("\n```")
                .map_or(fence.start, |i| fence.start + i);
            at = (after + 4).min(heading.end);
        }
        out.push_str(&self.text[at..heading.end]);
        out
    }
}

/// The text of a table cell without its backticks: ``` `spacing.section` ``` is `spacing.section`.
pub fn plain(cell: &str) -> String {
    cell.replace('`', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Title\n\n## One\n\nSee [`role-separator`](#code-role-separator) for it.\n\n```markdown resume expect=none\n# A\n```\n\n### Sub\n\n| Key | What |\n|---|---|\n| `spacing.section` | gap |\n| `size.body` | text |\n\n<!-- guide:planned rsm-141.22 witness=build:--tags -->\n\n## Two\n\n<a id=\"code-x\"></a> text\n";

    #[test]
    fn headings_have_levels_slugs_lines_and_sections_that_end_at_the_next_equal_or_higher_heading() {
        let guide = Guide::parse(SAMPLE);
        let texts: Vec<_> = guide
            .headings
            .iter()
            .map(|h| (h.level, h.text.as_str(), h.slug.as_str()))
            .collect();
        assert_eq!(
            texts,
            [
                (1, "Title", "title"),
                (2, "One", "one"),
                (3, "Sub", "sub"),
                (2, "Two", "two")
            ]
        );
        let one = guide.heading(2, "One").expect("heading One");
        assert_eq!(one.line, 3);
        assert!(
            guide.body(one).contains("### Sub"),
            "a section includes its subsections"
        );
        assert!(
            !guide.body(one).contains("## Two"),
            "and stops at the next heading of its level"
        );
    }

    #[test]
    fn a_fence_has_its_info_body_line_and_the_links_of_the_paragraph_right_before_it() {
        let guide = Guide::parse(SAMPLE);
        assert_eq!(guide.fences.len(), 1);
        let fence = &guide.fences[0];
        assert_eq!(fence.info, "markdown resume expect=none");
        assert_eq!(fence.body, "# A\n");
        assert_eq!(fence.line, 7);
        assert_eq!(fence.links_before, ["#code-role-separator"]);
    }

    #[test]
    fn a_fence_that_does_not_follow_a_paragraph_has_no_links_before_it() {
        let guide = Guide::parse("See [x](#x).\n\n## H\n\n```text\nbody\n```\n");
        assert!(guide.fences[0].links_before.is_empty());
    }

    #[test]
    fn a_table_has_its_header_and_rows_with_code_spans_between_backticks() {
        let guide = Guide::parse(SAMPLE);
        assert_eq!(guide.tables.len(), 1);
        assert_eq!(guide.tables[0].header, ["Key", "What"]);
        let firsts: Vec<_> = guide.tables[0].rows.iter().map(|r| plain(&r[0])).collect();
        assert_eq!(firsts, ["spacing.section", "size.body"]);
        assert_eq!(guide.tables[0].rows[0][0], "`spacing.section`");
    }

    #[test]
    fn markers_anchors_and_links_are_found() {
        let guide = Guide::parse(SAMPLE);
        assert_eq!(guide.markers.len(), 1);
        assert_eq!(
            (guide.markers[0].kind.as_str(), guide.markers[0].args.as_str()),
            ("planned", "rsm-141.22 witness=build:--tags")
        );
        assert_eq!(guide.anchors, ["code-x"]);
        assert_eq!(guide.links[0].url, "#code-role-separator");
    }

    #[test]
    fn a_marker_inside_a_fence_is_not_a_marker() {
        let guide = Guide::parse("```text\n<!-- guide:planned x -->\n```\n");
        assert!(guide.markers.is_empty());
    }

    #[test]
    fn prose_leaves_out_fenced_blocks() {
        let guide = Guide::parse(SAMPLE);
        let one = guide.heading(2, "One").expect("heading One");
        let prose = guide.prose(one);
        assert!(prose.contains("See [`role-separator`]") && !prose.contains("# A"));
    }
}
