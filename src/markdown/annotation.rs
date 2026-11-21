//! ABOUTME: Shared annotation recognition for resumes and letters: the shape predicate and the token parser.
//! ABOUTME: Every token error is located inside the comment; an ordinary comment is never an annotation.

use std::ops::Range;
use std::str::FromStr;

use crate::diagnostics::DiagnosticCode;
use crate::model::{Alternate, Annotations, GroupName, Priority, Status, Tag, VariantName};

use super::tree::{BlockNode, InlineNode};
use super::{Finding, Sink, Source};

/// The text between `<!--` and `-->`, with its offset in `comment`; `None` unless `comment` is one whole comment.
fn body(comment: &str) -> Option<(usize, &str)> {
    // Exactly one comment, with nothing around it; `<!-->` and `<!--->` are complete, empty comments.
    let found = comments(comment)?;
    let [only] = found.as_slice() else { return None };
    let inner = comment.get(only.start + 4..only.end.checked_sub(3)?)?;
    (*only == (0..comment.len())).then_some((4, inner))
}

/// True when `comment` is one whole HTML comment whose first word is `@`, `#`, or `=` followed by a letter.
///
/// The one predicate both document parsers use: an annotation-shaped comment is never dropped as ordinary.
/// The letter keeps a commented-out heading (`<!-- ## Hobbies -->`), a banner (`<!-- ===== -->`), and a note
/// such as `<!-- #1 priority -->` ordinary, while a misspelled token (`#AI`, `@Draft`) is still an error.
pub(crate) fn is_annotation(comment: &str) -> bool {
    body(comment)
        .and_then(|(_, inner)| inner.split_whitespace().next())
        .is_some_and(|first| {
            let mut chars = first.chars();
            matches!(chars.next(), Some('@' | '#' | '=')) && chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        })
}

/// An annotation and the range of the comment it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Marked {
    pub annotations: Annotations,
    pub range: Range<usize>,
}

/// The annotations of `marked`, or empty ones when there is no annotation.
pub(crate) fn annotations_of(marked: Option<Marked>) -> Annotations {
    marked.map(|m| m.annotations).unwrap_or_default()
}

/// Reads the annotation that ends an inline run, the inline placement: `- Did X. <!-- #ai -->`.
///
/// It annotates the run's own line. Every other annotation-shaped comment in the run (mid-line, inside bold or a
/// link, or a second one at the end) is a located error, and so is one with nothing before it to annotate.
pub(crate) fn trailing(nodes: &[InlineNode], sink: &mut Sink<'_>) -> Option<Marked> {
    let mut end = nodes.len();
    while end > 0 && is_space(&nodes[end - 1]) {
        end -= 1;
    }
    let mut cluster = Vec::new();
    while end > 0 {
        match &nodes[end - 1] {
            InlineNode::Html { text, range } if is_annotation(text) => cluster.push((text, range.clone())),
            // An ordinary comment shows nothing, so an annotation before it still ends the line.
            node if is_space(node)
                || matches!(node, InlineNode::Html { text, .. } if super::inline::is_comment(text)) => {}
            _ => break,
        }
        end -= 1;
    }
    reject_in(&nodes[..end], sink);
    let (text, range) = cluster.pop()?;
    for (_, extra) in cluster.into_iter().rev() {
        sink.report(duplicate(extra));
    }
    if !nodes[..end].iter().any(super::inline::is_visible) {
        sink.report(misplaced(
            range,
            "there is nothing on this line for the annotation to mark",
        ));
        return None;
    }
    Some(Marked {
        annotations: parse(text, range.start, sink),
        range,
    })
}

/// One item's annotation from both placements; two are never merged, so the second is a duplicate.
pub(crate) fn combine(above: Option<Marked>, trailing: Option<Marked>, sink: &mut Sink<'_>) -> Option<Marked> {
    match (above, trailing) {
        (Some(above), Some(trailing)) => {
            sink.report(duplicate(trailing.range).hint("this item also has an annotation on the line above it"));
            Some(above)
        }
        (above, trailing) => above.or(trailing),
    }
}

/// Reports an annotation that reached something that cannot carry one; `why` says what that is.
pub(crate) fn unclaimed(marked: Option<Marked>, why: &str, sink: &mut Sink<'_>) {
    if let Some(marked) = marked {
        sink.report(misplaced(marked.range, why));
    }
}

/// [`unclaimed`]'s reason for a line that shows nothing once comments and images are dropped.
pub(crate) const SHOWS_NOTHING: &str = "the line it marks shows nothing, so there is nothing to annotate";
/// [`unclaimed`]'s reason for a block that is not a section, role, era, paragraph, or bullet.
pub(crate) const NOT_ANNOTATABLE: &str = "only sections, roles, eras, paragraphs, and bullets take annotations";

/// The full-line placement: for each block, the annotation written alone on the line directly above it.
///
/// A comment block annotates the block whose first line comes right after the comment's last line, in the same
/// sequence (the document root, or one list item). Anything less clear is an error: a blank line or another
/// comment in between, nothing after it, a target that takes no annotation, or a comment that would change how
/// the text around it parses. `last_hint` is the caller's advice for an annotation with nothing after it.
pub(crate) fn leading(
    blocks: &[BlockNode],
    src: &Source<'_>,
    last_hint: &str,
    sink: &mut Sink<'_>,
) -> Vec<Option<Marked>> {
    let mut above = vec![None; blocks.len()];
    for (index, block) in blocks.iter().enumerate() {
        let Some(range) = block_annotation(block, src, sink) else {
            continue;
        };
        match target(blocks, index, src) {
            Ok(target) => {
                above[target] = Some(Marked {
                    annotations: parse(src.slice(&range), range.start, sink),
                    range,
                });
            }
            Err(Unattached::Nothing) => sink.report(
                Finding::new(
                    DiagnosticCode::AnnotationWithoutTarget,
                    range,
                    "an annotation on its own line has nothing after it to annotate",
                )
                .hint(last_hint),
            ),
            Err(Unattached::Because(finding)) => sink.report(finding(range)),
        }
    }
    above
}

/// Why a full-line annotation marks nothing: nothing follows it, or a located finding built on its range.
enum Unattached {
    Nothing,
    Because(fn(Range<usize>) -> Finding),
}

/// The index of the block the annotation block at `index` marks.
fn target(blocks: &[BlockNode], index: usize, src: &Source<'_>) -> Result<usize, Unattached> {
    use Unattached::Because;
    let block = &blocks[index];
    let next = blocks.get(index + 1).ok_or(Unattached::Nothing)?;
    let before = index.checked_sub(1).map(|i| &blocks[i]);
    if !directly_above(src, block.range().end, next.range().start) {
        return Err(Because(|r| {
            detached(r, "a blank line separates the annotation from the line below it")
        }));
    }
    match next {
        BlockNode::Html(html) => match comments(src.slice(&html.range)) {
            Some(found) if found.iter().any(|c| is_annotation(&src.slice(&html.range)[c.clone()])) => {
                Err(Because(|r| {
                    duplicate(r).hint("merge the two annotation lines into one comment")
                }))
            }
            Some(_) => Err(Because(|r| {
                detached(r, "another comment separates the annotation from the line it marks")
            })),
            None => Err(Because(|r| {
                misplaced(r, "the line below this annotation cannot carry one")
            })),
        },
        BlockNode::Unsupported(_) => Err(Because(|r| {
            misplaced(r, "the line below this annotation cannot carry one")
        })),
        BlockNode::Paragraph(_) if before.is_some_and(|b| continues_text(b, block, src)) => Err(Because(|r| {
            misplaced(
                r,
                "without this annotation line, the text below would join the line above it",
            )
            .hint("put a blank line above the annotation line")
        })),
        BlockNode::List(_) if before.is_some_and(|b| !same_column(b, block, next, src)) => Err(Because(|r| {
            misplaced(r, "this annotation line is not at the column of the bullets around it")
                .hint("start the comment where the bullet below it starts its `-`")
        })),
        _ => Ok(index + 1),
    }
}

/// For a list item: reports an annotation line that opens the item, which has nothing of its own to mark, and
/// returns how many blocks to skip before the full-line placement applies (0 or 1).
pub(crate) fn opening_line(blocks: &[BlockNode], src: &Source<'_>, sink: &mut Sink<'_>) -> usize {
    match blocks.first().and_then(|b| block_annotation(b, src, sink)) {
        Some(range) => {
            sink.report(misplaced(range, "a bullet that is only an annotation marks nothing"));
            1
        }
        None => 0,
    }
}

/// The range of the first annotation in a block that is only comments; each later one is reported as a duplicate.
fn block_annotation(block: &BlockNode, src: &Source<'_>, sink: &mut Sink<'_>) -> Option<Range<usize>> {
    let BlockNode::Html(html) = block else { return None };
    let text = src.slice(&html.range);
    let mut found = comments(text)?
        .into_iter()
        .filter(|c| is_annotation(&text[c.clone()]))
        .map(|c| html.range.start + c.start..html.range.start + c.end);
    let first = found.next();
    found.for_each(|extra| sink.report(duplicate(extra)));
    first
}

/// The ranges of the comments in `text`, or `None` unless `text` is only whole comments and whitespace.
///
/// The one comment splitter: `blocks::classify_html`, [`is_annotation`], and the full-line placement all use it.
pub(crate) fn comments(text: &str) -> Option<Vec<Range<usize>>> {
    let mut found = Vec::new();
    let mut at = 0;
    loop {
        let rest = &text[at..];
        let skipped = rest.len() - rest.trim_start().len();
        at += skipped;
        let rest = &text[at..];
        if rest.is_empty() {
            return Some(found);
        }
        let len = comment_len(rest)?;
        found.push(at..at + len);
        at += len;
    }
}

/// The length of the comment that opens `text`, or `None` when `text` does not open with one that closes.
pub(crate) fn comment_len(text: &str) -> Option<usize> {
    let after_open = text.strip_prefix("<!--")?;
    // `<!-->` and `<!--->` are complete, empty comments (CommonMark 0.31).
    Some(if after_open.starts_with('>') {
        5
    } else if after_open.starts_with("->") {
        6
    } else {
        4 + after_open.find("-->")? + 3
    })
}

/// True when the text between `end` and `start` is whitespace holding exactly one line ending.
fn directly_above(src: &Source<'_>, end: usize, start: usize) -> bool {
    let gap = src.text.get(end..start).unwrap_or("");
    let endings = gap.matches('\n').count()
        + gap
            .match_indices('\r')
            .filter(|(i, _)| !gap[i + 1..].starts_with('\n'))
            .count();
    gap.chars().all(char::is_whitespace) && endings == 1
}

/// For a comment between two blocks: true unless the block above is a list and the comment or the list below it
/// does not start at that list's column, which would turn a nested bullet into a sibling.
fn same_column(before: &BlockNode, comment: &BlockNode, after: &BlockNode, src: &Source<'_>) -> bool {
    if !matches!(before, BlockNode::List(_)) {
        return true;
    }
    let column = |at: usize| at - src.text[..at].rfind('\n').map_or(0, |i| i + 1);
    let list = column(before.range().start);
    column(comment.range().start) == list && column(after.range().start) == list
}

/// True when two lists split by a comment line would be one list without it: CommonMark ends a list at a
/// different bullet marker (`-` then `*`), so the walker must not join those.
pub(crate) fn same_list(before: &BlockNode, after: &BlockNode, src: &Source<'_>) -> bool {
    let marker = |block: &BlockNode| src.text[block.range().start..].chars().next();
    marker(before) == marker(after)
}

/// True when `comment` sits right under a paragraph or list with no blank line, so without the comment the
/// paragraph below it would continue that text (CommonMark lazy continuation) instead of starting its own.
fn continues_text(before: &BlockNode, comment: &BlockNode, src: &Source<'_>) -> bool {
    matches!(before, BlockNode::Paragraph(_) | BlockNode::List(_))
        && directly_above(src, before.range().end, comment.range().start)
}

fn detached(range: Range<usize>, message: &str) -> Finding {
    Finding::new(DiagnosticCode::AnnotationDetached, range, message)
        .hint("a comment alone on a line marks only the line directly below it; remove what is in between")
}

/// Reports every annotation-shaped comment in `nodes`, at any depth, as misplaced.
///
/// For runs that take no annotation (the name and contact lines) and for the part of a run before its end.
pub(crate) fn reject_in(nodes: &[InlineNode], sink: &mut Sink<'_>) {
    for node in nodes {
        match node {
            InlineNode::Html { text, range } if is_annotation(text) => sink.report(misplaced(
                range.clone(),
                "an annotation must be the last thing on the line it marks",
            )),
            InlineNode::Strong { children, .. }
            | InlineNode::Emph { children, .. }
            | InlineNode::Link { children, .. } => {
                reject_in(children, sink);
            }
            _ => {}
        }
    }
}

/// Reports an alternate where none is allowed (a section heading, role line, or era line) and drops it.
pub(crate) fn forbid_alternate(marked: &mut Option<Marked>, what: &str, sink: &mut Sink<'_>) {
    let Some(marked) = marked.as_mut() else { return };
    if marked.annotations.alternate.take().is_some() {
        sink.report(
            Finding::new(
                DiagnosticCode::AlternateNotAllowed,
                marked.range.clone(),
                format!("a {what} has one wording, so it takes no `=group.variant`"),
            )
            .hint("remove the `=group.variant`; keep the priority, status, and tags"),
        );
    }
}

fn is_space(node: &InlineNode) -> bool {
    matches!(node, InlineNode::Text(text) if text.text.trim().is_empty())
}

/// An annotation where none is allowed.
pub(crate) fn misplaced(range: Range<usize>, message: &str) -> Finding {
    Finding::new(DiagnosticCode::AnnotationMisplaced, range, message)
        .hint("put the annotation at the end of the line it marks, or alone on the line directly above it")
}

/// A second annotation for one item.
pub(crate) fn duplicate(range: Range<usize>) -> Finding {
    Finding::new(
        DiagnosticCode::AnnotationDuplicate,
        range,
        "a second annotation for the same item",
    )
    .hint("an item takes one annotation; merge them into one comment")
}

/// One item of a document in reading order, as the group checks see it.
///
/// The caller numbers its containers (a resume's section bodies, roles, and eras; a letter's sections) and lists
/// every item that could be a variant, including the lines that start a container, which never are.
#[derive(Debug, Clone)]
pub(crate) struct GroupItem<T> {
    pub alternate: Option<Alternate>,
    pub priority: Option<Priority>,
    pub container: usize,
    /// What kind of item it is, in the caller's terms; the variants of a group are all one kind.
    pub kind: ItemKind,
    /// The annotation comment, where every group finding points.
    pub range: Range<usize>,
    /// What the caller wants back for each variant of a valid group, such as its text or its index.
    pub payload: T,
}

/// The kinds of item a group can hold. A blurb and the paragraphs after it are one kind, because a blurb's other
/// wordings are written as the paragraphs that follow it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum ItemKind {
    Bullet,
    Paragraph,
    /// A line that is never a variant: a heading, a role or era line, or a page break.
    Boundary,
}

/// Checks the alternate groups of one document and returns each valid one as the payloads of its variants.
///
/// A group is a run of consecutive items in one container that share a group name. Its variants are one kind of
/// item, unique, agree on priority, and one of them is `default`. A group name is used by one group in the whole
/// document, because a position spec picks a wording by group name alone.
pub(crate) fn group<T: Clone>(items: &[GroupItem<T>], sink: &mut Sink<'_>) -> Vec<Vec<T>> {
    let mut valid = Vec::new();
    let mut seen: Vec<Run<'_, T>> = Vec::new();
    for run in runs(items) {
        if let Some(earlier) = seen.iter().find(|r| r.name == run.name) {
            report_scattered(earlier, &run, sink);
            continue;
        }
        if check_run(&run, sink) {
            valid.push(run.members.iter().map(|(item, _)| item.payload.clone()).collect());
        }
        seen.push(run);
    }
    valid
}

/// A run of consecutive variants that share a group name in one container.
struct Run<'a, T> {
    name: &'a GroupName,
    members: Vec<(&'a GroupItem<T>, &'a Alternate)>,
}

impl<T> Run<'_, T> {
    fn has_default(&self) -> bool {
        self.members.iter().any(|(_, a)| a.variant.is_default())
    }
}

/// Splits `items` into runs: a variant joins the run before it when it is the very next item, in the same
/// container, with the same group name.
fn runs<T>(items: &[GroupItem<T>]) -> Vec<Run<'_, T>> {
    let mut runs: Vec<Run<'_, T>> = Vec::new();
    let mut last: Option<usize> = None;
    for (index, item) in items.iter().enumerate() {
        let Some(alternate) = &item.alternate else { continue };
        let continues = last == index.checked_sub(1)
            && runs.last().is_some_and(|run| {
                run.name == &alternate.group
                    && run
                        .members
                        .last()
                        .is_some_and(|(prev, _)| prev.container == item.container)
            });
        match runs.last_mut() {
            Some(run) if continues => run.members.push((item, alternate)),
            _ => runs.push(Run {
                name: &alternate.group,
                members: vec![(item, alternate)],
            }),
        }
        last = Some(index);
    }
    runs
}

/// Reports a group name that comes back after other items: one group split apart, or a second group of that name.
fn report_scattered<T>(earlier: &Run<'_, T>, later: &Run<'_, T>, sink: &mut Sink<'_>) {
    let range = later.members[0].0.range.clone();
    let name = later.name;
    let finding = if earlier.has_default() && later.has_default() {
        Finding::new(
            DiagnosticCode::AlternateGroupReused,
            range,
            format!("a second group is named `{name}`"),
        )
        .hint("a position spec picks a wording by group name, so each group needs its own name; rename one")
    } else {
        Finding::new(
            DiagnosticCode::AlternateNotAdjacent,
            range,
            format!("this variant of `{name}` is apart from the others"),
        )
        .hint("put the variants of a group next to each other, in the same section, role, era, or parent bullet")
    };
    sink.report(finding);
}

/// Checks one run's variants against the first; true when the run is a valid group.
fn check_run<T>(run: &Run<'_, T>, sink: &mut Sink<'_>) -> bool {
    let (first, _) = run.members[0];
    let mut problems = Vec::new();
    for (index, &(item, alternate)) in run.members.iter().enumerate() {
        if item.kind != first.kind {
            problems.push(differing_kind(item, first, run.name));
        }
        if run.members[..index].iter().any(|(_, a)| a.variant == alternate.variant) {
            problems.push(
                Finding::new(
                    DiagnosticCode::AlternateVariantRepeated,
                    item.range.clone(),
                    format!("`{}` has two variants named `{}`", run.name, alternate.variant),
                )
                .hint("rename one of the variants"),
            );
        }
        if item.priority != first.priority {
            problems.push(differing_priority(item, first, run.name));
        }
    }
    if !run.has_default() {
        let name = run.name;
        problems.push(
            Finding::new(
                DiagnosticCode::AlternateMissingDefault,
                first.range.clone(),
                format!("the group `{name}` has no `default` variant"),
            )
            .hint(format!(
                "name one variant `{name}.default`; a build that prefers nothing renders it"
            )),
        );
    }
    let ok = problems.is_empty();
    problems.into_iter().for_each(|finding| sink.report(finding));
    ok
}

fn differing_kind<T>(item: &GroupItem<T>, first: &GroupItem<T>, name: &GroupName) -> Finding {
    Finding::new(
        DiagnosticCode::AlternateNotAdjacent,
        item.range.clone(),
        format!(
            "this variant of `{name}` is a {}, but the first is a {}",
            item.kind, first.kind
        ),
    )
    .hint("the variants of a group are one item worded several ways: make them all bullets or all paragraphs")
}

fn differing_priority<T>(item: &GroupItem<T>, first: &GroupItem<T>, name: &GroupName) -> Finding {
    let said = |p: Option<Priority>| p.map_or_else(|| "no priority".to_string(), |p| format!("`@{p}`"));
    Finding::new(
        DiagnosticCode::AlternatePriorityConflict,
        item.range.clone(),
        format!(
            "this variant of `{name}` has {}, but the first has {}",
            said(item.priority),
            said(first.priority)
        ),
    )
    .hint("the variants are one item, so they share one priority: state it on every variant, or on none")
}

/// The words of `text`, each with its byte range in `text`.
fn words(text: &str) -> impl Iterator<Item = (Range<usize>, &str)> {
    text.split_whitespace().map(move |word| {
        // `split_whitespace` yields subslices of `text`, so the pointer difference is the offset.
        let start = word.as_ptr() as usize - text.as_ptr() as usize;
        (start..start + word.len(), word)
    })
}

/// Reads the annotation comment `comment`, which starts at byte `at` of the source.
///
/// Reports every bad token with its own range and returns what the valid tokens say, so later checks still
/// run; a reported error stops the build. Call only when [`is_annotation`] holds.
pub(crate) fn parse(comment: &str, at: usize, sink: &mut Sink<'_>) -> Annotations {
    let mut annotations = Annotations::default();
    let Some((offset, inner)) = body(comment) else {
        return annotations;
    };
    let base = at + offset;
    for (range, word) in words(inner) {
        let range = base + range.start..base + range.end;
        if let Some(keyword) = word.strip_prefix('@') {
            keyword_token(keyword, range, &mut annotations, sink);
        } else if let Some(tag) = word.strip_prefix('#') {
            match Tag::try_new(tag) {
                // A repeated tag says nothing new, so it is not ambiguous.
                Ok(tag) => {
                    annotations.tags.insert(tag);
                }
                Err(_) => sink.report(malformed(range, "tag", tag)),
            }
        } else if let Some(alternate) = word.strip_prefix('=') {
            alternate_token(alternate, range, &mut annotations, sink);
        } else {
            sink.report(
                Finding::new(
                    DiagnosticCode::AnnotationMalformed,
                    range,
                    format!("`{word}` is not an annotation; every word must start with `@`, `#`, or `=`"),
                )
                .hint("move notes into a separate comment that starts with a word"),
            );
        }
    }
    annotations
}

fn keyword_token(keyword: &str, range: Range<usize>, annotations: &mut Annotations, sink: &mut Sink<'_>) {
    if let Ok(priority) = Priority::from_str(keyword) {
        set_once(&mut annotations.priority, priority, "priority", range, sink);
    } else if let Ok(status) = Status::from_str(keyword) {
        set_once(&mut annotations.status, status, "status", range, sink);
    } else {
        let mut finding = Finding::new(
            DiagnosticCode::AnnotationUnknownKeyword,
            range,
            format!("`@{keyword}` is not an annotation keyword"),
        );
        if let Some(close) = closest_keyword(keyword) {
            finding = finding.hint(format!("did you mean `@{close}`?"));
        }
        sink.report(finding.hint("the keywords are `@core`, `@extra`, `@draft`, and `@verify`; a topic is a `#tag`"));
    }
}

/// The keyword `word` is most likely a typo of, if any is close.
fn closest_keyword(word: &str) -> Option<&'static str> {
    use strum::IntoEnumIterator;
    let keywords = Priority::iter()
        .map(|p| -> &'static str { p.into() })
        .chain(Status::iter().map(|s| -> &'static str { s.into() }));
    let lower = word.to_lowercase();
    keywords
        .map(|k| (strsim::jaro_winkler(&lower, k), k))
        .filter(|(score, _)| *score >= 0.8)
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, k)| k)
}

fn alternate_token(text: &str, range: Range<usize>, annotations: &mut Annotations, sink: &mut Sink<'_>) {
    let parsed = text.split_once('.').and_then(|(group, variant)| {
        Some(Alternate {
            group: GroupName::try_new(group).ok()?,
            variant: VariantName::try_new(variant).ok()?,
        })
    });
    match parsed {
        Some(alternate) => set_once(&mut annotations.alternate, alternate, "alternate", range, sink),
        None => {
            sink.report(malformed(range, "alternate", text).hint("write `=group.variant`, such as `=gateway.short`"))
        }
    }
}

/// Sets `slot` unless it is set already, which is a conflict.
fn set_once<T>(slot: &mut Option<T>, value: T, what: &str, range: Range<usize>, sink: &mut Sink<'_>) {
    if slot.is_some() {
        sink.report(
            Finding::new(
                DiagnosticCode::AnnotationConflict,
                range,
                format!("a second {what} in one annotation"),
            )
            .hint(format!("an item has at most one {what}; keep the one you mean")),
        );
    } else {
        *slot = Some(value);
    }
}

fn malformed(range: Range<usize>, what: &str, name: &str) -> Finding {
    Finding::new(
        DiagnosticCode::AnnotationMalformed,
        range,
        format!("`{name}` is not a valid {what} name"),
    )
    .hint("names are lowercase letters and digits, starting with a letter, with single hyphens, at most 40 characters")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Diagnostic;
    use crate::markdown::testutil::codes;
    use claims::assert_ok;
    use pretty_assertions::assert_eq;
    use std::collections::BTreeSet;
    use std::path::Path;

    /// Parses `comment` placed at byte 10 of a source, so ranges prove they are offset.
    fn parsed(comment: &str) -> (Annotations, Vec<Diagnostic>) {
        let mut sink = Sink::new(Path::new("r.md"));
        let annotations = parse(comment, 10, &mut sink);
        (annotations, sink.finish())
    }

    fn tags(names: &[&str]) -> BTreeSet<Tag> {
        names.iter().map(|n| assert_ok!(Tag::try_new(*n))).collect()
    }

    #[test]
    fn a_comment_is_an_annotation_when_its_first_word_starts_with_a_sigil() {
        for yes in [
            "<!-- @core -->",
            "<!--#ai-->",
            "<!-- =g.v @extra -->",
            "<!--\n  @draft\n-->",
            "<!-- #AI -->",
            "<!-- @Draft -->",
        ] {
            assert!(is_annotation(yes), "{yes:?}");
        }
        for no in [
            "<!-- note -->",
            "<!-- note @core -->",
            "<!---->",
            "<!-->",
            "<!--->",
            "<!-- -->",
            "<!-- a --> <!-- @core -->",
            "<b>",
            "<!-- @core",
            "@core",
            "<!--\n## Hobbies\n- Chess\n-->",
            "<!-- ===== Old ===== -->",
            "<!-- #1 priority for Q3 -->",
            "<!-- # heading -->",
            "<!-- @ -->",
            "<!-- =1 -->",
        ] {
            assert!(!is_annotation(no), "{no:?}");
        }
    }

    #[test]
    fn every_kind_of_token_parses() {
        let (annotations, diagnostics) = parsed("<!-- =gateway.short @core @verify #ai #ai-platform -->");
        assert_eq!(codes(&diagnostics), vec![]);
        assert_eq!(
            annotations,
            Annotations {
                priority: Some(Priority::Core),
                status: Some(Status::Verify),
                tags: tags(&["ai", "ai-platform"]),
                alternate: Some(Alternate {
                    group: assert_ok!(GroupName::try_new("gateway")),
                    variant: assert_ok!(VariantName::try_new("short")),
                }),
            }
        );
    }

    #[test]
    fn zero_one_or_many_tags_and_a_repeated_tag_collapses() {
        assert_eq!(parsed("<!-- @extra -->").0.tags, tags(&[]));
        assert_eq!(parsed("<!-- #ai -->").0.tags, tags(&["ai"]));
        let (annotations, diagnostics) = parsed("<!-- #ai #data #ai -->");
        assert_eq!(annotations.tags, tags(&["ai", "data"]));
        assert_eq!(codes(&diagnostics), vec![]);
    }

    /// The code and the source text under the span of each finding, for a comment at byte 10.
    fn findings(comment: &str) -> Vec<(DiagnosticCode, String)> {
        let source = format!("{}{comment}", " ".repeat(10));
        parsed(comment)
            .1
            .iter()
            .map(|d| (d.code, source[span_of(d)].to_string()))
            .collect()
    }

    fn span_of(d: &Diagnostic) -> Range<usize> {
        claims::assert_some!(d.span.as_ref()).range.clone()
    }

    #[test]
    fn each_bad_token_is_its_own_located_error() {
        use DiagnosticCode::*;
        let cases: &[(&str, &[(DiagnosticCode, &str)])] = &[
            ("<!-- @cor -->", &[(AnnotationUnknownKeyword, "@cor")]),
            ("<!-- @core @Extra -->", &[(AnnotationUnknownKeyword, "@Extra")]),
            ("<!-- #AI #ok -->", &[(AnnotationMalformed, "#AI")]),
            ("<!-- #ai_ml -->", &[(AnnotationMalformed, "#ai_ml")]),
            ("<!-- #ai # -->", &[(AnnotationMalformed, "#")]),
            ("<!-- =gateway -->", &[(AnnotationMalformed, "=gateway")]),
            ("<!-- =gateway. -->", &[(AnnotationMalformed, "=gateway.")]),
            ("<!-- =.short -->", &[(AnnotationMalformed, "=.short")]),
            ("<!-- =a.b.c -->", &[(AnnotationMalformed, "=a.b.c")]),
            (
                "<!-- @core because it matters -->",
                &[
                    (AnnotationMalformed, "because"),
                    (AnnotationMalformed, "it"),
                    (AnnotationMalformed, "matters"),
                ],
            ),
            ("<!-- @core @extra -->", &[(AnnotationConflict, "@extra")]),
            ("<!-- @draft @verify -->", &[(AnnotationConflict, "@verify")]),
            ("<!-- @core @core -->", &[(AnnotationConflict, "@core")]),
            ("<!-- =a.default =a.short -->", &[(AnnotationConflict, "=a.short")]),
        ];
        for (comment, expected) in cases {
            let expected: Vec<(DiagnosticCode, String)> = expected.iter().map(|(c, s)| (*c, s.to_string())).collect();
            assert_eq!(findings(comment), expected, "{comment}");
        }
    }

    fn item(group: Option<(&str, &str)>, container: usize, kind: ItemKind, payload: usize) -> GroupItem<usize> {
        GroupItem {
            alternate: group.map(|(g, v)| Alternate {
                group: assert_ok!(GroupName::try_new(g)),
                variant: assert_ok!(VariantName::try_new(v)),
            }),
            priority: None,
            container,
            kind,
            range: payload..payload + 1,
            payload,
        }
    }

    fn grouped(items: &[GroupItem<usize>]) -> (Vec<Vec<usize>>, Vec<DiagnosticCode>) {
        let mut sink = Sink::new(Path::new("r.md"));
        let groups = group(items, &mut sink);
        (groups, codes(&sink.finish()))
    }

    #[test]
    fn group_works_on_a_flat_item_list_with_caller_numbered_containers() {
        use ItemKind::{Boundary, Paragraph};
        // One letter section's paragraphs, as a letter parser would list them.
        let letter = [
            item(Some(("evidence", "default")), 1, Paragraph, 10),
            item(Some(("evidence", "technical")), 1, Paragraph, 11),
            item(None, 1, Paragraph, 12),
        ];
        assert_eq!(grouped(&letter), (vec![vec![10, 11]], vec![]));
        // A new container splits a run even when the items are consecutive.
        let split = [
            item(Some(("g", "default")), 1, Paragraph, 0),
            item(Some(("g", "short")), 2, Paragraph, 1),
        ];
        assert_eq!(grouped(&split).1, vec![DiagnosticCode::AlternateNotAdjacent]);
        // So does an item without an alternate, or a boundary.
        for between in [item(None, 1, Paragraph, 1), item(None, 1, Boundary, 1)] {
            let apart = [
                item(Some(("g", "default")), 1, Paragraph, 0),
                between,
                item(Some(("g", "short")), 1, Paragraph, 2),
            ];
            assert_eq!(grouped(&apart).1, vec![DiagnosticCode::AlternateNotAdjacent]);
        }
        // A group of only `default` is valid; a third run of one name is reported each time.
        assert_eq!(
            grouped(&[item(Some(("g", "default")), 1, Paragraph, 0)]),
            (vec![vec![0]], vec![])
        );
        let three = [
            item(Some(("g", "default")), 1, Paragraph, 0),
            item(None, 1, Paragraph, 1),
            item(Some(("g", "short")), 1, Paragraph, 2),
            item(None, 1, Paragraph, 3),
            item(Some(("g", "long")), 1, Paragraph, 4),
        ];
        assert_eq!(grouped(&three).1, vec![DiagnosticCode::AlternateNotAdjacent; 2]);
    }

    #[test]
    fn a_misspelled_keyword_gets_a_did_you_mean_hint() {
        let (_, diagnostics) = parsed("<!-- @cor -->");
        assert!(
            diagnostics[0].hints.iter().any(|h| h == "did you mean `@core`?"),
            "{:?}",
            diagnostics[0].hints
        );
        let (_, diagnostics) = parsed("<!-- @Draft -->");
        assert!(
            diagnostics[0].hints.iter().any(|h| h == "did you mean `@draft`?"),
            "{:?}",
            diagnostics[0].hints
        );
        let (_, diagnostics) = parsed("<!-- @zzz -->");
        assert!(!diagnostics[0].hints.iter().any(|h| h.starts_with("did you mean")));
    }

    #[test]
    fn the_valid_tokens_still_count_when_another_is_bad() {
        let (annotations, diagnostics) = parsed("<!-- @core #AI #ai -->");
        assert_eq!(annotations.priority, Some(Priority::Core));
        assert_eq!(annotations.tags, tags(&["ai"]));
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::AnnotationMalformed]);
    }

    #[test]
    fn a_multi_line_comment_locates_tokens_on_every_line() {
        assert_eq!(
            findings("<!--\r\n  @core\r\n  #Bad\n-->"),
            vec![(DiagnosticCode::AnnotationMalformed, "#Bad".to_string())]
        );
    }
}
