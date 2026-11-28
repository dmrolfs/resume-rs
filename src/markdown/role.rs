//! ABOUTME: Recognizes role lines (`### Company · Title · Period`) and era lines (`**Name · Period**`).
//! ABOUTME: Splits on the canonical separator, recovers look-alike separators with safe Edits, maps nutype errors.

use std::ops::Range;

use super::inline;
use super::period::{self, PeriodProblem, contains_year};
use super::tree::{HeadingNode, InlineNode, ParagraphNode, TextNode};
use super::{Finding, MAX_ROLE_LINE_BYTES, Sink, Source, trim_range};
use crate::diagnostics::DiagnosticCode;
use crate::model::text::{CompanyError, EraNameError, TitleError};
use crate::model::{Company, EraName, Period, Title};

/// The canonical separator: U+00B7 with one space on each side.
const SEPARATOR: &str = " · ";
/// Most separator candidates examined in one line; bounds recovery cost on pathological input.
const MAX_CANDIDATES: usize = 16;
/// Look-alike separators (a middle dot in any spacing, a pipe, bullet-like dots) that are never part of
/// a company, title, or era name. One list serves both recovery and the stray-separator check.
const LOOKALIKES: [char; 11] = ['·', '|', '｜', '•', '∙', '⋅', '・', '‧', '\u{387}', '⸱', '･'];

/// A parsed `###` role line.
#[derive(Debug)]
pub(crate) struct RoleHead {
    pub company: Company,
    pub title: Option<Title>,
    pub period: Period,
    /// Source range of the role line's text, for diagnostics about the whole role.
    pub range: Range<usize>,
}

/// A parsed era line.
#[derive(Debug)]
pub(crate) struct EraHead {
    pub name: EraName,
    pub period: Period,
}

/// How a paragraph inside a role relates to the era convention.
#[derive(Debug)]
pub(crate) enum EraCheck {
    /// An ordinary paragraph.
    NotEra,
    Era(EraHead),
    /// Looks like an era but is malformed; the finding is already reported and the paragraph is kept.
    Malformed,
}

/// Source range of a heading's text, without its `#` markers.
pub(crate) fn content_range(heading: &HeadingNode) -> Range<usize> {
    match (heading.content.first(), heading.content.last()) {
        (Some(first), Some(last)) => first.range().start..last.range().end,
        _ => heading.range.clone(),
    }
}

/// Reads a `###` heading as a role line, recovering look-alike separators with an Edit when it is safe.
pub(crate) fn role_line(heading: &HeadingNode, src: &Source<'_>, sink: &mut Sink<'_>) -> Option<RoleHead> {
    let range = content_range(heading);
    let Some(text) = inline::plain_text(&heading.content) else {
        let finding = Finding::new(DiagnosticCode::RoleLineShape, range, "role lines must be plain text")
            .hint("write `### Company · Title · Period` without bold, italics, links, or code; styles format it");
        sink.report(finding);
        return None;
    };
    if text.text.trim().is_empty() {
        let finding = Finding::new(DiagnosticCode::EmptyHeading, range, "the role line is empty")
            .hint("write `### Company · Title · 2019 – 2023`");
        sink.report(finding);
        return None;
    }
    if text.text.len() > MAX_ROLE_LINE_BYTES {
        let message = format!("the role line is longer than {MAX_ROLE_LINE_BYTES} bytes");
        sink.report(Finding::new(DiagnosticCode::TextTooLong, range, message).hint("shorten the company and title"));
        return None;
    }
    let split = split_canonical(&text.text);
    let shaped =
        matches!(split.parts.len(), 2 | 3) && split.parts.last().is_some_and(|p| contains_year(&text.text[p.clone()]));
    if shaped && canonical_is_clean(&text, &split) {
        return canonical(&text, &split, range, src, sink);
    }
    if let Some(recovery) = recover(&text.text) {
        return recovered(&text, &recovery, range, src, sink);
    }
    if shaped {
        return canonical(&text, &split, range, src, sink);
    }
    sink.report(shape_finding(&text.text, &split, range));
    None
}

/// Classifies a paragraph inside a role: an era line, a malformed era, or an ordinary paragraph.
pub(crate) fn era_line(paragraph: &ParagraphNode, src: &Source<'_>, sink: &mut Sink<'_>) -> EraCheck {
    let mut visible = paragraph.content.iter().filter(|n| !inline::is_blank(n));
    let Some(InlineNode::Strong { range, children }) = visible.next() else {
        return EraCheck::NotEra;
    };
    let has_trailing = visible.next().is_some();
    let Some(text) = inline::plain_text(children) else {
        return marked_up_era(children, range, sink);
    };
    let split = split_canonical(&text.text);
    let era_shaped = split.parts.len() == 2 && split.parts.last().is_some_and(|p| contains_year(&text.text[p.clone()]));
    if era_shaped {
        if has_trailing {
            let finding = Finding::new(
                DiagnosticCode::EraShape,
                paragraph.range.clone(),
                "text follows the era line",
            )
            .hint("an era line is only the bold text; move the rest into its own paragraph");
            sink.report(finding);
            return EraCheck::Malformed;
        }
        return build_era(&text, &split, range, src, sink);
    }
    // Bold text followed by prose (`**Python · Rust** are my favourite languages`) is not an era attempt.
    if split.parts.len() >= 2 && !has_trailing {
        sink.report(era_shape_finding(&text.text, &split, range.clone()));
        return EraCheck::Malformed;
    }
    match recover(&text.text) {
        Some(recovery) => recovered_era(&text, &recovery, range, src, sink),
        None if lookalike_before_period(&text.text) => {
            let finding = Finding::new(
                DiagnosticCode::EraShape,
                range.clone(),
                "era parts must be separated by ` · `",
            )
            .hint("write `**Era name · 2020 – 2023**`; the name contains a year, so no automatic fix is offered");
            sink.report(finding);
            EraCheck::Malformed
        }
        None => EraCheck::NotEra,
    }
}

/// A bold span with markup, or wrapped over lines, that has the shape of an era line is an attempted
/// era: say so instead of letting it become the role's description.
fn marked_up_era(children: &[InlineNode], bold: &Range<usize>, sink: &mut Sink<'_>) -> EraCheck {
    let flat = inline::flatten_text(children);
    let split = split_canonical(&flat);
    let era_shaped = split.parts.len() == 2 && split.parts.last().is_some_and(|p| contains_year(&flat[p.clone()]));
    if !era_shaped {
        return EraCheck::NotEra;
    }
    let finding = Finding::new(DiagnosticCode::EraShape, bold.clone(), "era lines must be plain text")
        .hint("write it without italics, links, or code: `**Era name · 2020 – 2023**`");
    sink.report(finding);
    EraCheck::Malformed
}

/// Parts and separators of a line split on the canonical ` · `; ranges index the text.
struct Split {
    parts: Vec<Range<usize>>,
    seps: Vec<Range<usize>>,
}

fn split_canonical(text: &str) -> Split {
    let mut parts = Vec::new();
    let mut seps = Vec::new();
    let mut from = 0;
    while let Some(found) = text[from..].find(SEPARATOR) {
        let at = from + found;
        parts.push(trim_range(text, from..at));
        seps.push(at..at + SEPARATOR.len());
        from = at + SEPARATOR.len();
    }
    parts.push(trim_range(text, from..text.len()));
    Split { parts, seps }
}

/// True when no company or title part contains a look-alike separator and the dates parse.
fn canonical_is_clean(text: &TextNode, split: &Split) -> bool {
    let (period, names) = split
        .parts
        .split_last()
        .map_or((&(0..0), &[][..]), |(p, rest)| (p, rest));
    period::parse_text(&text.text[period.clone()]).is_ok()
        && names.iter().all(|part| !has_stray_separator(&text.text[part.clone()]))
}

fn has_stray_separator(part: &str) -> bool {
    part.chars().any(|c| LOOKALIKES.contains(&c))
}

fn canonical(
    text: &TextNode,
    split: &Split,
    line: Range<usize>,
    src: &Source<'_>,
    sink: &mut Sink<'_>,
) -> Option<RoleHead> {
    let (period_part, names) = split.parts.split_last()?;
    let period = period::parse_located(text, period_part.clone(), src, sink)?;
    for part in names.iter().filter(|p| has_stray_separator(&text.text[(*p).clone()])) {
        let finding = Finding::new(
            DiagnosticCode::RoleSeparator,
            text.source_range(part.clone()),
            "a separator-like character is inside the company or title",
        )
        .hint("separate the parts with ` · `: U+00B7 with a space on each side");
        sink.report(finding);
    }
    let (company, title) = name_parts(text, names, &split.seps, &line, sink)?;
    Some(RoleHead {
        company,
        title,
        period,
        range: line,
    })
}

/// Validates the company and optional title parts, mapping nutype errors to located diagnostics.
fn name_parts(
    text: &TextNode,
    names: &[Range<usize>],
    seps: &[Range<usize>],
    line: &Range<usize>,
    sink: &mut Sink<'_>,
) -> Option<(Company, Option<Title>)> {
    let company_part = names.first()?;
    let company = match Company::try_new(&text.text[company_part.clone()]) {
        Ok(company) => Some(company),
        Err(CompanyError::NotEmptyViolated) => {
            report_empty(text, "company", seps.first(), line, sink);
            None
        }
        Err(CompanyError::LenCharMaxViolated) => {
            report_too_long(text, "company", company_part, 120, sink);
            None
        }
    };
    let title = match names.get(1) {
        None => Some(None),
        Some(part) => match Title::try_new(&text.text[part.clone()]) {
            Ok(title) => Some(Some(title)),
            Err(TitleError::NotEmptyViolated) => {
                report_empty(text, "title", seps.get(1).or(seps.first()), line, sink);
                None
            }
            Err(TitleError::LenCharMaxViolated) => {
                report_too_long(text, "title", part, 120, sink);
                None
            }
        },
    };
    Some((company?, title?))
}

fn report_empty(text: &TextNode, what: &str, sep: Option<&Range<usize>>, line: &Range<usize>, sink: &mut Sink<'_>) {
    let range = sep.map_or_else(|| line.clone(), |s| text.source_range(s.clone()));
    let finding = Finding::new(DiagnosticCode::EmptyRolePart, range, format!("the {what} is empty"))
        .hint("a role line is `Company · Title · Period` or `Company · Period`; every part needs text");
    sink.report(finding);
}

fn report_too_long(text: &TextNode, what: &str, part: &Range<usize>, max: usize, sink: &mut Sink<'_>) {
    let message = format!("the {what} is longer than {max} characters");
    let finding = Finding::new(DiagnosticCode::TextTooLong, text.source_range(part.clone()), message)
        .hint("shorten it; move detail into the bullets");
    sink.report(finding);
}

/// The finding for a `###` heading that is not a role line in any recoverable way.
fn shape_finding(text: &str, split: &Split, range: Range<usize>) -> Finding {
    const SHAPES: &str = "a role line is `### Company · Title · 2019 – 2023` or `### Company · 2019 – 2023`";
    if split.parts.len() > 3 {
        return Finding::new(
            DiagnosticCode::RoleLineShape,
            range,
            "the role line has too many ` · ` parts",
        )
        .hint(format!("{SHAPES}; use a comma inside a title"));
    }
    if !contains_year(text) {
        return Finding::new(DiagnosticCode::RoleLineShape, range, "this `###` heading is not a role line").hint(format!(
            "level-3 headings are reserved for roles: {SHAPES}; use `## ` for a section or `**Name · Period**` for an era"
        ));
    }
    let hint = if split.parts.len() >= 2 {
        format!("put the dates last: {SHAPES}")
    } else {
        format!("separate the parts with ` · ` (Option+Shift+9 on a Mac): {SHAPES}")
    };
    Finding::new(
        DiagnosticCode::RoleLineShape,
        range,
        "the role line is missing its ` · ` separators",
    )
    .hint(hint)
}

fn era_shape_finding(text: &str, split: &Split, range: Range<usize>) -> Finding {
    const SHAPE: &str = "an era line is a whole bold paragraph: `**Era name · 2020 – 2023**`";
    let message = if split.parts.len() > 2 {
        "the era line has too many ` · ` parts"
    } else if contains_year(text) {
        "the era line has the wrong shape"
    } else {
        "the era line has no dates"
    };
    Finding::new(DiagnosticCode::EraShape, range, message).hint(SHAPE)
}

fn build_era(text: &TextNode, split: &Split, bold: &Range<usize>, src: &Source<'_>, sink: &mut Sink<'_>) -> EraCheck {
    let (Some(name_part), Some(period_part)) = (split.parts.first(), split.parts.get(1)) else {
        return EraCheck::NotEra;
    };
    let name = match EraName::try_new(&text.text[name_part.clone()]) {
        Ok(name) => name,
        Err(EraNameError::NotEmptyViolated) => {
            let finding = Finding::new(DiagnosticCode::EraShape, bold.clone(), "the era name is empty")
                .hint("write `**Era name · 2020 – 2023**`");
            sink.report(finding);
            return EraCheck::Malformed;
        }
        Err(EraNameError::LenCharMaxViolated) => {
            report_too_long(text, "era name", name_part, 80, sink);
            return EraCheck::Malformed;
        }
    };
    match period::parse_located(text, period_part.clone(), src, sink) {
        Some(period) => EraCheck::Era(EraHead { name, period }),
        None => EraCheck::Malformed,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// ` · ` exactly.
    Canonical,
    /// A spaced `-`, `–`, or `—`, which could also be part of a name.
    Hyphenish,
    /// A pipe, a bullet-like dot, or a middle dot with the wrong spacing.
    Other,
}

#[derive(Debug, Clone)]
struct Candidate {
    /// The separator and the whitespace around it, as text offsets.
    range: Range<usize>,
    kind: Kind,
}

/// A reconstruction of a line whose separators are look-alikes.
#[derive(Debug)]
struct Recovery {
    /// Name parts before the period (text offsets).
    parts: Vec<Range<usize>>,
    /// The separators before each following part, and the one before the period.
    seps: Vec<Candidate>,
    period: Range<usize>,
}

/// Finds look-alike separators, working period first: the earliest separator whose suffix is a period.
///
/// A split is rejected when the text before it already contains a year: that is a period with a bad
/// start (`08/2017 – 2018`), whose own dash must not be read as a role separator.
fn recover(text: &str) -> Option<Recovery> {
    let candidates = candidates(text);
    let at = (0..candidates.len()).find(|&i| is_split_point(text, &candidates, i))?;
    if candidates[..=at].iter().all(|c| c.kind == Kind::Canonical) {
        return None;
    }
    let seps = candidates[..=at].to_vec();
    let mut parts = Vec::with_capacity(seps.len());
    let mut from = 0;
    for sep in &seps {
        parts.push(trim_range(text, from..sep.range.start));
        from = sep.range.end;
    }
    Some(Recovery {
        parts,
        seps,
        period: trim_range(text, from..text.len()),
    })
}

/// True when candidate `i` is where the dates start: the text after it is a period, and the text
/// before it is not itself part of a period (a year anywhere before it, or a lone `Present`, `Aug`,
/// or `Spring` just before it, means that candidate is the period's own dash).
fn is_split_point(text: &str, candidates: &[Candidate], i: usize) -> bool {
    let candidate = &candidates[i];
    let previous_end = i.checked_sub(1).map_or(0, |p| candidates[p].range.end);
    let part_before = text[previous_end..candidate.range.start].trim();
    is_period_suffix(&text[candidate.range.end..])
        && !contains_year(&text[..candidate.range.start])
        && !period::is_date_word(part_before)
}

/// True when a look-alike separator (not a hyphen) directly precedes a period, whatever the name holds.
fn lookalike_before_period(text: &str) -> bool {
    candidates(text)
        .iter()
        .any(|c| c.kind == Kind::Other && is_period_suffix(&text[c.range.end..]))
}

/// True when `suffix` is a period, including one that is well-formed but invalid (end before start,
/// year out of range): that is still where the dates start, so scanning must not continue past it.
fn is_period_suffix(suffix: &str) -> bool {
    if !contains_year(suffix) {
        return false;
    }
    match period::parse_text(suffix) {
        Ok(_) | Err(PeriodProblem::EndBeforeStart | PeriodProblem::YearOutOfRange { .. }) => true,
        Err(_) => false,
    }
}

fn candidates(text: &str) -> Vec<Candidate> {
    let mut found: Vec<Candidate> = Vec::new();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (i, &(at, c)) in chars.iter().enumerate() {
        if found.len() >= MAX_CANDIDATES {
            break;
        }
        // `\·` is not a CommonMark escape, so the backslash stays in the text; it marks a literal.
        if i > 0 && chars[i - 1].1 == '\\' {
            continue;
        }
        let spaced = i > 0 && chars[i - 1].1.is_whitespace() && chars.get(i + 1).is_some_and(|n| n.1.is_whitespace());
        let kind = match c {
            '-' | '–' | '—' if spaced => Kind::Hyphenish,
            c if LOOKALIKES.contains(&c) => Kind::Other,
            _ => continue,
        };
        let range = expand_whitespace(text, at..at + c.len_utf8());
        if found.last().is_some_and(|last| last.range.end > range.start) {
            continue;
        }
        let kind = if c == '·' && &text[range.clone()] == SEPARATOR {
            Kind::Canonical
        } else {
            kind
        };
        found.push(Candidate { range, kind });
    }
    found
}

fn expand_whitespace(text: &str, range: Range<usize>) -> Range<usize> {
    let start = text[..range.start].trim_end().len();
    let end = range.end + (text[range.end..].len() - text[range.end..].trim_start().len());
    start..end
}

fn recovered(
    text: &TextNode,
    recovery: &Recovery,
    line: Range<usize>,
    src: &Source<'_>,
    sink: &mut Sink<'_>,
) -> Option<RoleHead> {
    let seps: Vec<Range<usize>> = recovery.seps.iter().map(|c| c.range.clone()).collect();
    if recovery.parts.iter().any(Range::is_empty) {
        // A part is missing, so there is nothing to reconstruct: report the missing part only.
        name_parts(text, &recovery.parts, &seps, &line, sink);
        return None;
    }
    let reconstructed = reconstruct(text, recovery, src);
    let too_many = recovery.parts.len() > 2;
    let finding = separator_finding(text, recovery, line.clone(), &reconstructed, too_many);
    let finding = if too_many || ambiguous(recovery) {
        finding
    } else {
        attach_edit(finding, text, recovery, src)
    };
    sink.report(finding);
    if too_many {
        return None;
    }
    let period = period::parse_located(text, recovery.period.clone(), src, sink)?;
    let (company, title) = name_parts(text, &recovery.parts, &seps, &line, sink)?;
    Some(RoleHead {
        company,
        title,
        period,
        range: line,
    })
}

fn recovered_era(
    text: &TextNode,
    recovery: &Recovery,
    bold: &Range<usize>,
    src: &Source<'_>,
    sink: &mut Sink<'_>,
) -> EraCheck {
    if recovery.parts.iter().any(Range::is_empty) {
        // Nothing to reconstruct: report the missing name only.
        let finding = Finding::new(DiagnosticCode::EraShape, bold.clone(), "the era name is empty")
            .hint("write `**Era name · 2020 – 2023**`");
        sink.report(finding);
        return EraCheck::Malformed;
    }
    let single_name = recovery.parts.len() == 1;
    let reconstructed = reconstruct(text, recovery, src);
    let message = format!(
        "era parts must be separated by ` · `, not {}",
        found_tokens(text, recovery)
    );
    let finding = Finding::new(DiagnosticCode::EraShape, bold.clone(), message);
    let finding = if !single_name {
        finding.hint("an era is `**Era name · 2020 – 2023**`; the name can't contain separators")
    } else if era_edit_is_safe(text, recovery) {
        attach_edit(
            finding.hint(format!("did you mean `**{reconstructed}**`?")),
            text,
            recovery,
            src,
        )
    } else {
        finding.hint(format!(
            "if this is an era, write `**{reconstructed}**`; a hyphen can also be ordinary bold text, so no automatic fix is offered"
        ))
    };
    sink.report(finding);
    if !single_name {
        return EraCheck::Malformed;
    }
    let name_part = &recovery.parts[0];
    let split = Split {
        parts: vec![name_part.clone(), recovery.period.clone()],
        seps: Vec::new(),
    };
    match build_era(text, &split, bold, src, sink) {
        EraCheck::Era(head) => EraCheck::Era(head),
        _ => EraCheck::Malformed,
    }
}

/// A hyphen before a single date (`**Award - 2019**`) may be ordinary bold text, so only a range
/// (`**AI - 2016 – 2017**`) or a non-hyphen separator is clearly an era worth rewriting.
fn era_edit_is_safe(text: &TextNode, recovery: &Recovery) -> bool {
    let hyphen = recovery.seps.first().is_some_and(|s| s.kind == Kind::Hyphenish);
    let is_range = period::parse_text(&text.text[recovery.period.clone()]).is_ok_and(|p| p.separator.is_some());
    !hyphen || is_range
}

/// True when the only wrong separator before the title is a hyphen, which might belong to the company.
fn ambiguous(recovery: &Recovery) -> bool {
    recovery.parts.len() == 2 && recovery.seps.first().is_some_and(|s| s.kind == Kind::Hyphenish)
}

fn separator_finding(
    text: &TextNode,
    recovery: &Recovery,
    line: Range<usize>,
    reconstructed: &str,
    too_many: bool,
) -> Finding {
    let message = format!(
        "role parts must be separated by ` · `, not {}",
        found_tokens(text, recovery)
    );
    let finding = Finding::new(DiagnosticCode::RoleSeparator, line, message);
    if too_many {
        finding.hint("can't tell where the company ends; write `### Company · Title · 2019 – 2023`")
    } else if ambiguous(recovery) {
        finding.hint(format!(
            "if this is a company and a title, write `### {reconstructed}`; a hyphen could also belong to the company name"
        ))
    } else {
        finding.hint(format!("did you mean `### {reconstructed}`?"))
    }
}

/// Backticked list of the wrong separators as written, e.g. `` `-` `` or `` `|` ``.
fn found_tokens(text: &TextNode, recovery: &Recovery) -> String {
    let mut tokens: Vec<String> = Vec::new();
    for sep in recovery.seps.iter().filter(|s| s.kind != Kind::Canonical) {
        let token = format!("`{}`", text.text[sep.range.clone()].trim());
        if !tokens.contains(&token) {
            tokens.push(token);
        }
    }
    tokens.join(", ")
}

/// The line as it should be written, from the original source of each part.
fn reconstruct(text: &TextNode, recovery: &Recovery, src: &Source<'_>) -> String {
    let parts = recovery.parts.iter().chain(std::iter::once(&recovery.period));
    parts
        .map(|p| src.slice(&text.source_range(p.clone())).trim().to_string())
        .collect::<Vec<_>>()
        .join(SEPARATOR)
}

/// Attaches the single Edit that rewrites every wrong separator; the caller decides it is safe to offer.
fn attach_edit(finding: Finding, text: &TextNode, recovery: &Recovery, src: &Source<'_>) -> Finding {
    let wrong: Vec<usize> = recovery
        .seps
        .iter()
        .enumerate()
        .filter(|(_, s)| s.kind != Kind::Canonical)
        .map(|(i, _)| i)
        .collect();
    let (Some(&first), Some(&last)) = (wrong.first(), wrong.last()) else {
        return finding;
    };
    let start = text.source_range(recovery.seps[first].range.clone()).start;
    let end = text.source_range(recovery.seps[last].range.clone()).end;
    let mut replacement = String::new();
    for index in first..=last {
        replacement.push_str(SEPARATOR);
        if index < last {
            let part = &recovery.parts[index + 1];
            replacement.push_str(src.slice(&text.source_range(part.clone())).trim());
        }
    }
    finding.fix(start..end, replacement)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Diagnostic;
    use crate::markdown::testutil::{apply_fixes, codes};
    use crate::markdown::tree::{self, BlockNode};
    use claims::{assert_none, assert_some};
    use pretty_assertions::assert_eq;
    use std::path::Path;

    fn lower(markdown: &str) -> (tree::Document, Sink<'static>) {
        let mut sink = Sink::new(Path::new("r.md"));
        let doc = tree::lower(markdown, 0, &mut sink);
        (doc, sink)
    }

    fn role(markdown: &str) -> (Option<RoleHead>, Vec<Diagnostic>) {
        let (doc, mut sink) = lower(markdown);
        let Some(BlockNode::Heading(heading)) = doc.blocks.first() else {
            panic!("expected a heading")
        };
        let src = Source::new(markdown);
        let head = role_line(heading, &src, &mut sink);
        (head, sink.finish())
    }

    fn era(markdown: &str) -> (EraCheck, Vec<Diagnostic>) {
        let (doc, mut sink) = lower(markdown);
        let Some(BlockNode::Paragraph(paragraph)) = doc.blocks.first() else {
            panic!("expected a paragraph")
        };
        let src = Source::new(markdown);
        let check = era_line(paragraph, &src, &mut sink);
        (check, sink.finish())
    }

    #[test]
    fn parses_three_part_role_line() {
        let (head, diagnostics) = role("### HERE Technologies · Senior Principal Architect · Aug 2017 – Present");
        let head = assert_some!(head);
        assert_eq!(head.company.as_ref(), "HERE Technologies");
        assert_eq!(assert_some!(head.title.as_ref()).as_ref(), "Senior Principal Architect");
        assert_eq!(head.period.text, "Aug 2017 – Present");
        assert_eq!(head.range, 4..75);
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn parses_two_part_role_line() {
        let (head, diagnostics) = role("### Consulting and Early-Stage Companies · 2008 – 2013");
        let head = assert_some!(head);
        assert_eq!(head.company.as_ref(), "Consulting and Early-Stage Companies");
        assert_none!(head.title);
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn level_three_heading_without_role_shape_is_error() {
        let (head, diagnostics) = role("### Projects");
        assert_none!(head);
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::RoleLineShape]);
        assert_eq!(assert_some!(diagnostics[0].span.as_ref()).range, 4..12);
        assert!(diagnostics[0].hints[0].contains("## "), "{:?}", diagnostics[0].hints);
    }

    #[test]
    fn role_line_with_dates_in_the_wrong_place_is_error() {
        let (head, diagnostics) = role("### 2020 – 2021 · Acme");
        assert_none!(head);
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::RoleLineShape]);
        assert!(
            diagnostics[0].hints[0].contains("dates last"),
            "{:?}",
            diagnostics[0].hints
        );
    }

    #[test]
    fn role_line_with_too_many_parts_is_error() {
        let (head, diagnostics) = role("### A · B · C · 2020 – 2021");
        assert_none!(head);
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::RoleLineShape]);
        assert_eq!(diagnostics[0].message, "the role line has too many ` · ` parts");
    }

    #[test]
    fn role_line_with_markup_is_error() {
        for line in [
            "### **Acme** · VP · 2020 – 2021",
            "### [Acme](https://acme.co) · VP · 2020 – 2021",
            "### `Acme` · 2020",
        ] {
            let (head, diagnostics) = role(line);
            assert_none!(head, "{line}");
            assert_eq!(codes(&diagnostics), vec![DiagnosticCode::RoleLineShape], "{line}");
            assert_eq!(diagnostics[0].message, "role lines must be plain text");
        }
    }

    #[test]
    fn role_with_wrong_separator_suggests_middle_dot() {
        let line = "### Acme | Engineer | 2015 – 2017";
        let (head, diagnostics) = role(line);
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::RoleSeparator]);
        let d = &diagnostics[0];
        assert_eq!(d.message, "role parts must be separated by ` · `, not `|`");
        assert_eq!(d.hints, vec!["did you mean `### Acme · Engineer · 2015 – 2017`?"]);
        assert_eq!(assert_some!(d.span.as_ref()).range, 4..line.len());
        assert_eq!(apply_fixes(line, &diagnostics), "### Acme · Engineer · 2015 – 2017");
        let head = assert_some!(head, "the best-effort model keeps the reconstructed role");
        assert_eq!(head.company.as_ref(), "Acme");
    }

    #[test]
    fn role_separator_lookalikes_are_detected() {
        let lookalikes = ["|", "｜", "•", "∙", "⋅", "・", "‧", "⸱", "·"];
        for dot in lookalikes {
            let line = format!("### Acme{dot}Engineer{dot}2015 – 2017");
            let (_, diagnostics) = role(&line);
            assert_eq!(codes(&diagnostics), vec![DiagnosticCode::RoleSeparator], "{dot}");
            assert_eq!(
                apply_fixes(&line, &diagnostics),
                "### Acme · Engineer · 2015 – 2017",
                "{dot}"
            );
        }
        let (_, diagnostics) = role("### Acme · Engineer · 2015 – 2017");
        assert_eq!(diagnostics, vec![], "the canonical separator is accepted");
    }

    #[test]
    fn stray_separator_inside_a_part_is_reported_without_edit() {
        // `Acme | Co` could be one company or two parts, so no Edit is offered.
        let (head, diagnostics) = role("### Acme | Co · Engineer · 2015 – 2017");
        assert_none!(head);
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::RoleSeparator]);
        assert_eq!(diagnostics[0].fix, None);
    }

    #[test]
    fn stray_separator_in_a_two_part_line_is_recovered() {
        let line = "### Acme | Engineer · 2015 – 2017";
        let (head, diagnostics) = role(line);
        assert_some!(head);
        assert_eq!(apply_fixes(line, &diagnostics), "### Acme · Engineer · 2015 – 2017");
    }

    #[test]
    fn mixed_separators_are_recovered() {
        let line = "### Acme · Engineer - 2015 – 2017";
        let (head, diagnostics) = role(line);
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::RoleSeparator]);
        assert_eq!(apply_fixes(line, &diagnostics), "### Acme · Engineer · 2015 – 2017");
        assert_some!(head);
    }

    #[test]
    fn hyphenated_company_is_not_rewritten() {
        let (head, diagnostics) = role("### Rolls-Royce · Engineer · 2015 – 2017");
        assert_eq!(assert_some!(head).company.as_ref(), "Rolls-Royce");
        assert_eq!(diagnostics, vec![]);
        let line = "### Gusto-Webtab - 2015 – 2017";
        let (head, diagnostics) = role(line);
        assert_eq!(assert_some!(head).company.as_ref(), "Gusto-Webtab");
        assert_eq!(apply_fixes(line, &diagnostics), "### Gusto-Webtab · 2015 – 2017");
    }

    #[test]
    fn ambiguous_hyphen_separators_have_no_edit() {
        let (head, diagnostics) = role("### Acme - Engineer - 2015 – 2017");
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::RoleSeparator]);
        assert_eq!(diagnostics[0].fix, None);
        assert!(diagnostics[0].hints[0].contains("could also belong to the company name"));
        assert_some!(head);
        let (head, diagnostics) = role("### Rolls - Royce - Engineer - 2015 – 2017");
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::RoleSeparator]);
        assert_eq!(diagnostics[0].fix, None);
        assert_none!(head);
    }

    #[test]
    fn role_separator_and_period_hyphen_edits_do_not_overlap() {
        let line = "### Acme | Engineer | 2015 - 2017";
        let (_, diagnostics) = role(line);
        assert_eq!(
            codes(&diagnostics),
            vec![DiagnosticCode::RoleSeparator, DiagnosticCode::PeriodHyphen]
        );
        let first = assert_some!(diagnostics[0].fix.as_ref());
        let second = assert_some!(diagnostics[1].fix.as_ref());
        assert!(first.range.end <= second.range.start, "{first:?} overlaps {second:?}");
        assert_eq!(apply_fixes(line, &diagnostics), "### Acme · Engineer · 2015 – 2017");
    }

    #[test]
    fn empty_company_maps_nutype_error_to_located_diagnostic() {
        let (head, diagnostics) = role("### · Title · 2020 – 2022");
        assert_none!(head);
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::EmptyRolePart]);
        assert_eq!(diagnostics[0].message, "the company is empty");
        assert_eq!(assert_some!(diagnostics[0].span.as_ref()).range, 4..7);
    }

    #[test]
    fn empty_title_is_empty_role_part() {
        let (head, diagnostics) = role("### Acme ·  · 2020 – 2022");
        assert_none!(head);
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::EmptyRolePart]);
        assert_eq!(diagnostics[0].message, "the title is empty");
    }

    #[test]
    fn overlong_company_is_text_too_long() {
        let line = format!("### {} · 2020 – 2022", "A".repeat(121));
        let (head, diagnostics) = role(&line);
        assert_none!(head);
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::TextTooLong]);
        assert_eq!(assert_some!(diagnostics[0].span.as_ref()).range, 4..125);
    }

    #[test]
    fn overlong_role_line_is_text_too_long_and_not_recovered() {
        let line = format!("### {}", "a - ".repeat(2000));
        let (head, diagnostics) = role(&line);
        assert_none!(head);
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::TextTooLong]);
    }

    #[test]
    fn unparseable_period_in_a_well_shaped_line_is_period_unparseable() {
        let (head, diagnostics) = role("### Acme · Engineer · Aug 2017 – Prsent");
        assert_none!(head);
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::PeriodUnparseable]);
    }

    #[test]
    fn end_before_start_in_a_role_line_is_reported() {
        let (head, diagnostics) = role("### Acme · 2020 – 2018");
        assert_none!(head);
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::PeriodEndBeforeStart]);
    }

    #[test]
    fn a_period_with_a_bad_start_is_not_mistaken_for_a_role_separator() {
        // The period's own ` – ` must not be read as a wrong role separator.
        for line in [
            "### D · 08/2017 – 2018",
            "### E · Aug, 2017 – 2018",
            "### F · Engineer · 08/2017 – 2018",
        ] {
            let (head, diagnostics) = role(line);
            assert_none!(head, "{line}");
            assert_eq!(codes(&diagnostics), vec![DiagnosticCode::PeriodUnparseable], "{line}");
            assert_eq!(diagnostics[0].fix, None, "{line}");
        }
    }

    #[test]
    fn a_company_with_a_year_in_its_name_is_never_rewritten() {
        // Safe direction: an error with a hint, never an Edit that could split the name.
        let (head, diagnostics) = role("### Windows 2000 | Engineer | 2015 – 2017");
        assert_none!(head);
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::RoleLineShape]);
        assert_eq!(diagnostics[0].fix, None);
    }

    #[test]
    fn period_warnings_do_not_drop_the_role() {
        let (head, diagnostics) = role("### Acme · Engineer · 2015 - Current");
        assert_some!(head);
        assert_eq!(
            codes(&diagnostics),
            vec![DiagnosticCode::PeriodHyphen, DiagnosticCode::PeriodPresentSpelling]
        );
    }

    #[test]
    fn crlf_role_line_has_the_same_ranges() {
        let (head, _) = role("### Acme · Engineer · 2015 – 2017\r\n");
        assert_eq!(assert_some!(head).range, 4..37);
    }

    #[test]
    fn parses_an_era_line() {
        let (check, diagnostics) = era("**AI and data platforms · 2026 – Present**");
        let EraCheck::Era(head) = check else {
            panic!("expected an era, got {check:?}")
        };
        assert_eq!(head.name.as_ref(), "AI and data platforms");
        assert_eq!(head.period.text, "2026 – Present");
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn ordinary_paragraphs_are_not_eras() {
        for text in [
            "Plain paragraph.",
            "**Just bold**",
            "**Q3 2020 results**",
            "*italic · 2020*",
        ] {
            let (check, diagnostics) = era(text);
            assert!(matches!(check, EraCheck::NotEra), "{text}: {check:?}");
            assert_eq!(diagnostics, vec![], "{text}");
        }
    }

    #[test]
    fn era_without_year_is_error() {
        let text = "**AI platforms · recent work**";
        let (check, diagnostics) = era(text);
        assert!(matches!(check, EraCheck::Malformed));
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::EraShape]);
        assert_eq!(diagnostics[0].message, "the era line has no dates");
        assert_eq!(assert_some!(diagnostics[0].span.as_ref()).range, 0..31);
    }

    #[test]
    fn era_with_too_many_parts_is_error() {
        let (check, diagnostics) = era("**A · B · 2020 – 2021**");
        assert!(matches!(check, EraCheck::Malformed));
        assert_eq!(diagnostics[0].message, "the era line has too many ` · ` parts");
    }

    #[test]
    fn era_with_trailing_text_is_error() {
        let (check, diagnostics) = era("**AI platforms · 2026 – Present** and more words");
        assert!(matches!(check, EraCheck::Malformed));
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::EraShape]);
        assert_eq!(diagnostics[0].message, "text follows the era line");
    }

    #[test]
    fn era_with_wrong_separator_has_edit() {
        let text = "**AI platforms - 2026 – Present**";
        let (check, diagnostics) = era(text);
        assert!(matches!(check, EraCheck::Era(_)), "{check:?}");
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::EraShape]);
        assert_eq!(apply_fixes(text, &diagnostics), "**AI platforms · 2026 – Present**");
    }

    #[test]
    fn era_with_ambiguous_name_has_no_edit() {
        let (check, diagnostics) = era("**Gusto - Webtab - 2015 – 2017**");
        assert!(matches!(check, EraCheck::Malformed));
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::EraShape]);
        assert_eq!(diagnostics[0].fix, None);
    }

    #[test]
    fn era_period_problems_are_reported_once() {
        let (check, diagnostics) = era("**Phase · 2020 – 2018**");
        assert!(matches!(check, EraCheck::Malformed));
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::PeriodEndBeforeStart]);
    }

    #[test]
    fn era_period_warnings_keep_the_era() {
        let text = "**Phase · 2018 - 2020**";
        let (check, diagnostics) = era(text);
        assert!(matches!(check, EraCheck::Era(_)));
        assert_eq!(apply_fixes(text, &diagnostics), "**Phase · 2018 – 2020**");
    }

    #[test]
    fn candidates_are_capped() {
        let line = "a | ".repeat(100);
        assert_eq!(candidates(&line).len(), MAX_CANDIDATES);
    }

    #[test]
    fn escaped_middle_dot_is_not_a_separator() {
        // `\·` is not a CommonMark escape: the backslash stays in the text.
        let (head, diagnostics) = role("### Acme \\· Eng · 2019 – 2021");
        assert_some!(head);
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::RoleSeparator]);
        assert_eq!(diagnostics[0].fix, None);
    }

    #[test]
    fn a_period_cannot_start_with_a_month_word_or_present() {
        for line in [
            "### Acme · Eng · Present – 2019",
            "### Acme · Aug – 2019",
            "### Acme · Spring – 2019",
        ] {
            let (head, diagnostics) = role(line);
            assert_none!(head, "{line}");
            assert_eq!(codes(&diagnostics), vec![DiagnosticCode::PeriodUnparseable], "{line}");
        }
    }

    #[test]
    fn fullwidth_dot_inside_a_company_is_a_stray_separator() {
        let (_, diagnostics) = role("### Acme ･ Labs · Eng · 2019 – 2021");
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::RoleSeparator]);
    }

    #[test]
    fn era_with_markup_is_error() {
        for text in [
            "**Platform *x* · 2016**",
            "**`Platform` · 2016 – 2017**",
            "**[Platform](https://x.y) · 2016 – 2017**",
        ] {
            let (check, diagnostics) = era(text);
            assert!(matches!(check, EraCheck::Malformed), "{text}: {check:?}");
            assert_eq!(codes(&diagnostics), vec![DiagnosticCode::EraShape], "{text}");
            assert_eq!(diagnostics[0].message, "era lines must be plain text", "{text}");
            assert_eq!(diagnostics[0].fix, None);
        }
    }

    #[test]
    fn soft_wrapped_era_is_an_era() {
        let (check, diagnostics) = era("**Platform ·\n2016 – 2017**");
        let EraCheck::Era(head) = check else {
            panic!("expected an era, got {check:?}")
        };
        assert_eq!(head.name.as_ref(), "Platform");
        assert_eq!(head.period.text, "2016 – 2017");
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn look_alike_era_with_a_year_in_the_name_is_reported_without_an_edit() {
        let (check, diagnostics) = era("**Class of 2010 | 2016 – 2017**");
        assert!(matches!(check, EraCheck::Malformed), "{check:?}");
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::EraShape]);
        assert_eq!(diagnostics[0].fix, None);
    }

    #[test]
    fn bold_prose_with_a_middle_dot_is_not_an_era() {
        let (check, diagnostics) = era("**Python · Rust** are my favourite languages.");
        assert!(matches!(check, EraCheck::NotEra), "{check:?}");
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn a_single_date_after_a_hyphen_is_reported_without_an_edit() {
        // `**Award - 2019**` could be a plain bold paragraph; only a range is clearly an era.
        let (check, diagnostics) = era("**Award - 2019**");
        assert!(matches!(check, EraCheck::Era(_) | EraCheck::Malformed), "{check:?}");
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::EraShape]);
        assert_eq!(diagnostics[0].fix, None);
    }

    #[test]
    fn era_with_an_empty_name_is_reported_once_without_an_edit() {
        let (check, diagnostics) = era("**| 2020 – 2021**");
        assert!(matches!(check, EraCheck::Malformed), "{check:?}");
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::EraShape]);
        assert_eq!(diagnostics[0].fix, None);
    }

    #[test]
    fn overlong_title_is_text_too_long() {
        let line = format!("### Acme · {} · 2020 – 2022", "T".repeat(121));
        let (head, diagnostics) = role(&line);
        assert_none!(head);
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::TextTooLong]);
        assert_eq!(diagnostics[0].message, "the title is longer than 120 characters");
    }

    #[test]
    fn overlong_era_name_is_text_too_long() {
        let (check, diagnostics) = era(&format!("**{} · 2015 – 2016**", "A".repeat(81)));
        assert!(matches!(check, EraCheck::Malformed), "{check:?}");
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::TextTooLong]);
        assert_eq!(diagnostics[0].message, "the era name is longer than 80 characters");
    }

    #[test]
    fn canonical_era_with_an_empty_name_is_an_era_shape_error() {
        // `&#32;` decodes to a space, so the bold text is "  · 2020 – 2021" and the name part is empty.
        let (check, diagnostics) = era("**&#32; · 2020 – 2021**");
        assert!(matches!(check, EraCheck::Malformed), "{check:?}");
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::EraShape]);
        assert_eq!(diagnostics[0].message, "the era name is empty");
    }

    #[test]
    fn era_with_dates_in_the_wrong_part_has_the_wrong_shape() {
        let (check, diagnostics) = era("**2020 · Acme**");
        assert!(matches!(check, EraCheck::Malformed), "{check:?}");
        assert_eq!(diagnostics[0].message, "the era line has the wrong shape");
    }

    #[test]
    fn era_followed_by_a_comment_is_still_an_era() {
        let (check, diagnostics) = era("**AI · 2016 – 2017** <!-- note -->");
        assert!(matches!(check, EraCheck::Era(_)), "{check:?}");
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn role_line_with_inline_html_is_error() {
        let (head, diagnostics) = role("### Acme <b>x</b> · Eng · 2020 – 2021");
        assert_none!(head);
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::RoleLineShape]);
        assert_eq!(diagnostics[0].message, "role lines must be plain text");
    }

    #[test]
    fn a_spaced_hyphen_inside_a_canonical_company_is_not_a_separator() {
        // `canonical_is_clean` protects this: recovery alone would split the company at the hyphen.
        let (head, diagnostics) = role("### Acme - Labs · Engineer · 2015 – 2017");
        let head = assert_some!(head);
        assert_eq!(head.company.as_ref(), "Acme - Labs");
        assert_eq!(assert_some!(head.title.as_ref()).as_ref(), "Engineer");
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn the_role_line_length_limit_is_exact() {
        let tail = " · 2015 – 2017";
        let at_limit = format!("### {}{tail}", "A".repeat(MAX_ROLE_LINE_BYTES - tail.len()));
        assert_eq!(at_limit.len() - 4, MAX_ROLE_LINE_BYTES);
        let (_, diagnostics) = role(&at_limit);
        assert_eq!(
            diagnostics[0].message, "the company is longer than 120 characters",
            "examined, not rejected for length"
        );
        let (_, diagnostics) = role(&format!("{at_limit}x"));
        assert_eq!(
            diagnostics[0].message,
            format!("the role line is longer than {MAX_ROLE_LINE_BYTES} bytes")
        );
    }

    #[test]
    fn dates_in_the_middle_of_a_three_part_line_get_the_order_hint() {
        let (head, diagnostics) = role("### Acme · 2020 – 2021 · Engineer");
        assert_none!(head);
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::RoleLineShape]);
        assert!(
            diagnostics[0].hints[0].contains("put the dates last"),
            "{:?}",
            diagnostics[0].hints
        );
        assert_eq!(diagnostics[0].message, "the role line is missing its ` · ` separators");
    }

    #[test]
    fn four_parts_are_too_many_even_without_dates_last() {
        let (_, diagnostics) = role("### A · B · 2020 · C");
        assert_eq!(diagnostics[0].message, "the role line has too many ` · ` parts");
    }

    #[test]
    fn empty_title_error_points_at_the_separator() {
        let (_, diagnostics) = role("### Acme ·  · 2020 – 2022");
        let range = assert_some!(diagnostics[0].span.as_ref()).range.clone();
        assert_eq!(&"### Acme ·  · 2020 – 2022"[range], " · ");
    }

    #[test]
    fn markup_in_bold_that_is_not_era_shaped_is_ordinary_text() {
        for text in ["**Platform *x* · notes**", "**Platform *x* 2016**", "**`a` · `b`**"] {
            let (check, diagnostics) = era(text);
            assert!(matches!(check, EraCheck::NotEra), "{text}: {check:?}");
            assert_eq!(diagnostics, vec![], "{text}");
        }
    }

    #[test]
    fn a_look_alike_without_dates_after_it_is_not_an_era_attempt() {
        for text in ["**Python | Rust**", "**A • B**", "**Class of 2010 - 2016 – 2017**"] {
            let (check, diagnostics) = era(text);
            assert!(matches!(check, EraCheck::NotEra), "{text}: {check:?}");
            assert_eq!(diagnostics, vec![], "{text}");
        }
    }

    #[test]
    fn candidates_need_whitespace_on_both_sides_of_a_hyphen_and_never_overlap() {
        let found = candidates("a-b - c -d e- f");
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(&"a-b - c -d e- f"[found[0].range.clone()], " - ");
        assert_eq!(candidates("a | | b").len(), 1, "adjacent separators are one candidate");
        let kinds: Vec<_> = candidates("a · b · c").iter().map(|c| c.kind).collect();
        assert_eq!(kinds, vec![Kind::Canonical, Kind::Canonical]);
        assert_eq!(
            candidates("a·b")[0].kind,
            Kind::Other,
            "a middle dot without spaces is a look-alike"
        );
    }

    #[test]
    fn separators_that_touch_are_two_candidates_but_overlapping_ones_are_one() {
        // `·|` has no whitespace between the two, so neither expands over the other.
        assert_eq!(candidates("a·|b").len(), 2);
        assert_eq!(candidates("a | | b").len(), 1, "their whitespace overlaps");
    }
}
