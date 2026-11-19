//! ABOUTME: Measures where content sits on each laid-out page: the page body and how far the content reaches.
//! ABOUTME: The layout report's checks read these numbers; probes and this module are the only readers of page frames.

use typst::foundations::{Content, StyleChain};
use typst::introspection::{Location, Tag};
use typst::layout::{Abs, Frame, FrameItem};
use typst::pdf::{ArtifactElem, ArtifactKind};
use typst_layout::PagedDocument;

/// What the layout checks need from one laid-out page, in points from the top of the page.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct PageGeometry {
    /// The height of the page.
    pub height: f64,
    /// The top of the page body: the smallest top of any body item in the whole document.
    pub body_top: f64,
    /// The top of the bottom artifact (the footer), or the page height when the page has none.
    pub body_bottom: f64,
    /// The lowest body text baseline, shape, or image on the page; `None` when the page has no body content.
    pub content_bottom: Option<f64>,
}

/// What one page frame holds, before the document-wide body top is known.
#[derive(Debug, Clone, Copy, Default)]
struct Measured {
    height: f64,
    /// The smallest top of a body block.
    top: Option<f64>,
    /// The top of the bottom artifact.
    footer_top: Option<f64>,
    /// The lowest body leaf.
    content_bottom: Option<f64>,
}

/// Measures every page of `document`.
///
/// Typst flattens the page body into the page frame, so the body is not one inner frame: the frame's
/// top-level items are the blocks, and a block that can break (a paragraph of plain text) is inlined as bare
/// text. The header, footer, and background are artifacts, and Typst marks them with an artifact start tag
/// and its end tag. Whether the artifact is one group or inlined items depends on its content (a footer made
/// of a grid is a group, a footer of plain text is inlined), so both forms are read. The body top is taken
/// over the whole document, because content always starts at the region top on some page (weak spacing is
/// dropped at the start of a region).
///
/// Limits: a group's transform is ignored, so rotated, scaled, or skewed content is measured by its unrotated
/// position (`move` shifts the frame's position, not its transform, and is honored); a style that sets `page(foreground: ..)` has its foreground counted as body, because
/// Typst does not mark the foreground as an artifact; an inlined footer is bounded by its first baseline.
pub(crate) fn page_geometry(document: &PagedDocument) -> Vec<PageGeometry> {
    let measured: Vec<Measured> = document.pages().iter().map(|page| measure(&page.frame)).collect();
    let body_top = measured.iter().filter_map(|m| m.top).reduce(f64::min).unwrap_or(0.0);
    measured
        .iter()
        .map(|m| PageGeometry {
            height: m.height,
            body_top,
            body_bottom: m.footer_top.unwrap_or(m.height),
            content_bottom: m.content_bottom,
        })
        .collect()
}

fn measure(frame: &Frame) -> Measured {
    let mut measured = Measured {
        height: frame.height().to_pt(),
        ..Measured::default()
    };
    // The artifact whose items are being read, while Typst has inlined it between its start and end tags.
    let mut open: Option<(Location, ArtifactKind)> = None;
    for (position, item) in frame.items() {
        match item {
            FrameItem::Tag(Tag::Start(content, _)) => {
                if let (Some(kind), Some(location)) = (artifact_kind(content), content.location()) {
                    open = Some((location, kind));
                }
            }
            FrameItem::Tag(Tag::End(location, ..)) => {
                if open.is_some_and(|(opened, _)| opened == *location) {
                    open = None;
                }
            }
            _ => match open.map(|(_, kind)| kind).or_else(|| wrapped_artifact(item)) {
                Some(kind) => measured.add_artifact(kind, position.y),
                None => measured.add_body(item, position.y),
            },
        }
    }
    measured
}

impl Measured {
    /// Only the footer bounds the body; a header, background, or watermark does not.
    fn add_artifact(&mut self, kind: ArtifactKind, y: Abs) {
        if kind == ArtifactKind::Footer {
            let y = y.to_pt();
            self.footer_top = Some(self.footer_top.map_or(y, |top| top.min(y)));
        }
    }

    fn add_body(&mut self, item: &FrameItem, y: Abs) {
        if let Some(top) = top_of(item, y) {
            self.top = Some(self.top.map_or(top, |t| t.min(top)));
        }
        if let Some(bottom) = bottom_of(item, y) {
            self.content_bottom = Some(self.content_bottom.map_or(bottom, |b| b.max(bottom)));
        }
    }
}

fn artifact_kind(content: &Content) -> Option<ArtifactKind> {
    // The element is fully materialized by now (Typst's own PDF export reads it the same way), so the default
    // style chain gives its value.
    content
        .to_packed::<ArtifactElem>()
        .map(|artifact| artifact.kind.get(StyleChain::default()))
}

/// The kind of artifact a top-level group wraps: its first item is the artifact's start tag.
fn wrapped_artifact(item: &FrameItem) -> Option<ArtifactKind> {
    let FrameItem::Group(group) = item else { return None };
    match group.frame.items().next() {
        Some((_, FrameItem::Tag(Tag::Start(content, _)))) => artifact_kind(content),
        _ => None,
    }
}

/// The top of a body item, from its position. For bare text that is the baseline, which is a little below
/// the true top, so the body top can be a point or so low when the body has no block at all.
fn top_of(item: &FrameItem, y: Abs) -> Option<f64> {
    match item {
        FrameItem::Group(_) | FrameItem::Text(_) | FrameItem::Shape(..) | FrameItem::Image(..) => Some(y.to_pt()),
        FrameItem::Link(..) | FrameItem::Tag(_) => None,
    }
}

/// The lowest point a leaf reaches: a text baseline, the bottom of a shape or an image. Groups are walked
/// rather than measured by their frame, because a frame that cannot break is clamped to the page body and
/// hides content that runs past it.
fn bottom_of(item: &FrameItem, y: Abs) -> Option<f64> {
    match item {
        FrameItem::Group(group) => group
            .frame
            .items()
            .filter_map(|(position, inner)| bottom_of(inner, y + position.y))
            .reduce(f64::max),
        FrameItem::Text(_) => Some(y.to_pt()),
        FrameItem::Shape(shape, _) => Some((y + shape.bbox(false).max.y).to_pt()),
        FrameItem::Image(_, size, _) => Some((y + size.y).to_pt()),
        FrameItem::Link(..) | FrameItem::Tag(_) => None,
    }
}
