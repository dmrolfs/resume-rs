//! ABOUTME: Reads the structure of a Markdown guide: headings, fenced blocks, tables, links, and `guide:` markers.
//! ABOUTME: Checks compare these hooks with the code, never the wording, so a prose edit changes nothing here.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::LazyLock;

use claims::assert_ok;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use regex::Regex;

static MARKER: LazyLock<Regex> = LazyLock::new(|| assert_ok!(Regex::new(r"(?s)<!--\s*guide:(\S+)\s*(.*?)\s*-->")));
static ANCHOR: LazyLock<Regex> = LazyLock::new(|| assert_ok!(Regex::new(r#"<a id="([^"]+)">"#)));

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

impl Fence {
    /// The info string's words, lowercase.
    pub fn words(&self) -> Vec<String> {
        self.info.split_whitespace().map(str::to_lowercase).collect()
    }

    /// The language: the first word of the info string, lowercase, up to a comma. Empty when there is none.
    pub fn language(&self) -> String {
        let first = self.words().into_iter().next().unwrap_or_default();
        first.split(',').next().unwrap_or_default().to_string()
    }
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

#[derive(Debug, Clone, Default)]
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

/// The anchor GitHub gives a heading, before it makes duplicates unique: lowercase, letters and digits kept
/// (in any script), spaces and hyphens as hyphens, other punctuation dropped.
pub fn slug(heading: &str) -> String {
    heading
        .chars()
        .flat_map(|c| match c {
            c if c.is_alphanumeric() => c.to_lowercase().collect::<Vec<_>>(),
            ' ' | '-' => vec!['-'],
            '_' => vec!['_'],
            _ => Vec::new(),
        })
        .collect()
}

/// What is open while the events of a document go by.
#[derive(Default)]
struct Open {
    heading: Option<(usize, usize, String)>, // level, start, text so far
    fence: Option<(String, usize, String)>,  // info, start, body so far
    table: Option<Table>,
    row: Option<Vec<String>>,
    cell: Option<String>,
    in_head: bool,
    paragraph_links: Vec<String>,
    links_before_next: Vec<String>,
    just_after_paragraph: bool,
}

/// Reads a document's events into a [`Guide`].
struct Collector<'t> {
    text: &'t str,
    guide: Guide,
    open: Open,
    slugs: HashMap<String, usize>,
}

impl<'t> Collector<'t> {
    fn new(text: &'t str) -> Self {
        Self {
            text,
            guide: Guide {
                text: text.to_string(),
                anchors: ANCHOR.captures_iter(text).map(|c| c[1].to_string()).collect(),
                ..Guide::default()
            },
            open: Open::default(),
            slugs: HashMap::new(),
        }
    }

    fn event(&mut self, event: Event<'_>, range: Range<usize>) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                self.open.heading = Some((level as usize, range.start, String::new()));
                self.open.just_after_paragraph = false;
            }
            Event::End(TagEnd::Heading(_)) => self.end_heading(),
            Event::Start(Tag::Paragraph) => self.open.paragraph_links.clear(),
            Event::End(TagEnd::Paragraph) => {
                self.open.links_before_next = std::mem::take(&mut self.open.paragraph_links);
                self.open.just_after_paragraph = true;
            }
            Event::Start(Tag::CodeBlock(kind)) => self.start_fence(&kind, range.start),
            Event::End(TagEnd::CodeBlock) => self.end_fence(),
            Event::Start(tag @ (Tag::Table(_) | Tag::TableHead | Tag::TableRow | Tag::TableCell)) => {
                self.start_table_part(&tag, range.start);
            }
            Event::End(tag @ (TagEnd::Table | TagEnd::TableHead | TagEnd::TableRow | TagEnd::TableCell)) => {
                self.end_table_part(tag);
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                self.open.paragraph_links.push(dest_url.to_string());
                self.guide.links.push(Link {
                    url: dest_url.to_string(),
                    line: line_of(self.text, range.start),
                    start: range.start,
                });
            }
            Event::Code(code) => {
                if let Some((_, _, t)) = self.open.heading.as_mut() {
                    t.push_str(&code);
                }
                if let Some(c) = self.open.cell.as_mut() {
                    c.push('`');
                    c.push_str(&code);
                    c.push('`');
                }
            }
            Event::Text(t) => self.text_event(&t),
            Event::SoftBreak | Event::HardBreak => {
                if let Some(c) = self.open.cell.as_mut() {
                    c.push(' ');
                }
            }
            Event::Html(html) => self.html_event(&html, range.start),
            _ => {}
        }
    }

    fn text_event(&mut self, t: &str) {
        if let Some((_, _, body)) = self.open.fence.as_mut() {
            body.push_str(t);
        } else if let Some((_, _, h)) = self.open.heading.as_mut() {
            h.push_str(t);
        } else if let Some(c) = self.open.cell.as_mut() {
            c.push_str(t);
        }
    }

    fn html_event(&mut self, html: &str, start: usize) {
        for m in MARKER.captures_iter(html) {
            let at = start + m.get(0).map_or(0, |g| g.start());
            self.guide.markers.push(Marker {
                kind: m[1].to_string(),
                args: m[2].to_string(),
                line: line_of(self.text, at),
                start: at,
            });
        }
    }

    fn end_heading(&mut self) {
        let Some((level, start, text)) = self.open.heading.take() else {
            return;
        };
        // GitHub makes a repeated heading's anchor unique with `-1`, `-2`, and so on.
        let base = slug(&text);
        let seen = self.slugs.entry(base.clone()).or_insert(0);
        let unique = if *seen == 0 { base } else { format!("{base}-{seen}") };
        *seen += 1;
        self.guide.headings.push(Heading {
            level,
            slug: unique,
            text,
            line: line_of(self.text, start),
            start,
            end: self.text.len(),
        });
    }

    fn start_fence(&mut self, kind: &CodeBlockKind<'_>, start: usize) {
        let info = match kind {
            CodeBlockKind::Fenced(info) => info.to_string(),
            CodeBlockKind::Indented => String::new(),
        };
        self.open.fence = Some((info, start, String::new()));
        if !self.open.just_after_paragraph {
            self.open.links_before_next.clear();
        }
        self.open.just_after_paragraph = false;
    }

    fn end_fence(&mut self) {
        if let Some((info, start, body)) = self.open.fence.take() {
            self.guide.fences.push(Fence {
                info,
                body,
                line: line_of(self.text, start),
                start,
                links_before: std::mem::take(&mut self.open.links_before_next),
            });
        }
    }

    fn start_table_part(&mut self, tag: &Tag<'_>, start: usize) {
        match tag {
            Tag::Table(_) => {
                self.open.table = Some(Table {
                    header: Vec::new(),
                    rows: Vec::new(),
                    start,
                });
                self.open.just_after_paragraph = false;
            }
            Tag::TableHead => {
                self.open.in_head = true;
                self.open.row = Some(Vec::new());
            }
            Tag::TableRow => self.open.row = Some(Vec::new()),
            Tag::TableCell => self.open.cell = Some(String::new()),
            _ => {}
        }
    }

    fn end_table_part(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Table => self.guide.tables.extend(self.open.table.take()),
            TagEnd::TableHead => {
                if let (Some(t), Some(r)) = (self.open.table.as_mut(), self.open.row.take()) {
                    t.header = r;
                }
                self.open.in_head = false;
            }
            TagEnd::TableRow => {
                if let (Some(t), Some(r), false) = (self.open.table.as_mut(), self.open.row.take(), self.open.in_head) {
                    t.rows.push(r);
                }
            }
            TagEnd::TableCell => {
                if let (Some(r), Some(c)) = (self.open.row.as_mut(), self.open.cell.take()) {
                    r.push(c.trim().to_string());
                }
            }
            _ => {}
        }
    }

    /// A section runs to the next heading of the same or a higher level.
    fn close_sections(mut self) -> Guide {
        let ends: Vec<usize> = (0..self.guide.headings.len())
            .map(|i| {
                let level = self.guide.headings[i].level;
                self.guide.headings[i + 1..]
                    .iter()
                    .find(|h| h.level <= level)
                    .map_or(self.text.len(), |h| h.start)
            })
            .collect();
        for (heading, end) in self.guide.headings.iter_mut().zip(ends) {
            heading.end = end;
        }
        self.guide
    }
}

impl Guide {
    /// Reads `text`, with Windows line endings read as `\n`, so a guide edited on Windows is the same guide.
    pub fn parse(text: &str) -> Self {
        let text = text.replace("\r\n", "\n");
        let mut collector = Collector::new(&text);
        for (event, range) in Parser::new_ext(&text, Options::ENABLE_TABLES).into_offset_iter() {
            collector.event(event, range);
        }
        collector.close_sections()
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
}

/// The text of a table cell without its backticks: ``` `spacing.section` ``` is `spacing.section`.
pub fn plain(cell: &str) -> String {
    cell.replace('`', "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::assert_some;

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
        let one = assert_some!(guide.heading(2, "One"));
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
    fn a_guide_with_windows_line_endings_is_the_same_guide() {
        let crlf = SAMPLE.replace('\n', "\r\n");
        let (a, b) = (Guide::parse(SAMPLE), Guide::parse(&crlf));
        assert_eq!(a.text, b.text);
        assert_eq!(a.fences[0].body, b.fences[0].body);
        assert_eq!(a.tables[0].rows, b.tables[0].rows);
    }

    #[test]
    fn a_heading_slug_follows_github_for_other_scripts_and_repeats() {
        assert_eq!(slug("Café: a Guide, 2 – Ü"), "café-a-guide-2--ü");
        let guide = Guide::parse("## Same\n\n## Same\n\n## Same\n");
        let slugs: Vec<_> = guide.headings.iter().map(|h| h.slug.as_str()).collect();
        assert_eq!(slugs, ["same", "same-1", "same-2"]);
    }

    #[test]
    fn a_tilde_fence_is_a_fence_and_its_language_is_lowercase() {
        let guide = Guide::parse("~~~Markdown,Resume expect=none\nx\n~~~\n");
        assert_eq!(guide.fences.len(), 1);
        assert_eq!(guide.fences[0].language(), "markdown");
        assert_eq!(guide.fences[0].words(), ["markdown,resume", "expect=none"]);
    }
}
