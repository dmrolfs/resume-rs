//! ABOUTME: Recognizes role lines (`### Company · Title · Period`) and era lines (`**Name · Period**`).
//! ABOUTME: Splits on the canonical separator, recovers look-alike separators with safe Edits, maps nutype errors.

use std::ops::Range;

use super::inline;
use super::period::{self, PeriodProblem, contains_year};
use super::tree::{HeadingNode, InlineNode, ParagraphNode, TextNode};
use super::{Finding, MAX_ROLE_LINE_BYTES, Sink, Source};
use crate::diagnostics::DiagnosticCode;
use crate::model::text::{CompanyError, EraNameError, TitleError};
use crate::model::{Company, EraName, Period, Title};

/// The canonical separator: U+00B7 with one space on each side.
const SEPARATOR: &str = " · ";
/// Most separator candidates examined in one line; bounds recovery cost on pathological input.
const MAX_CANDIDATES: usize = 16;
/// Look-alike separators that are never part of a company, title, or era name.
const STRAY: [char; 9] = ['|', '｜', '•', '∙', '⋅', '・', '‧', '\u{387}', '⸱'];

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
        return EraCheck::NotEra;
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
    if split.parts.len() >= 2 {
        sink.report(era_shape_finding(&text.text, &split, range.clone()));
        return EraCheck::Malformed;
    }
    match recover(&text.text) {
        Some(recovery) => recovered_era(&text, &recovery, range, src, sink),
        None => EraCheck::NotEra,
    }
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

fn trim_range(text: &str, range: Range<usize>) -> Range<usize> {
    let slice = &text[range.clone()];
    let start = range.start + (slice.len() - slice.trim_start().len());
    let end = range.end - (slice.len() - slice.trim_end().len());
    start..end.max(start)
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
    part.chars().any(|c| STRAY.contains(&c) || c == '·')
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
    let at = candidates
        .iter()
        .position(|c| is_period_suffix(&text[c.range.end..]) && !contains_year(&text[..c.range.start]))?;
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

/// True when `suffix` is a period, including one that is well-formed but invalid (end before start,
/// year out of range): that is still where the dates start, so scanning must not continue past it.
fn is_period_suffix(suffix: &str) -> bool {
    if suffix.is_empty() || !contains_year(suffix) {
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
        let spaced = i > 0 && chars[i - 1].1.is_whitespace() && chars.get(i + 1).is_some_and(|n| n.1.is_whitespace());
        let kind = match c {
            '-' | '–' | '—' if spaced => Kind::Hyphenish,
            '·' | '|' | '｜' | '•' | '∙' | '⋅' | '・' | '‧' | '\u{387}' | '⸱' | '･' => Kind::Other,
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
    sink.report(attach_edit(
        finding,
        text,
        recovery,
        src,
        !too_many && !ambiguous(recovery),
    ));
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
    let single_name = recovery.parts.len() == 1;
    let reconstructed = reconstruct(text, recovery, src);
    let message = format!(
        "era parts must be separated by ` · `, not {}",
        found_tokens(text, recovery)
    );
    let mut finding = Finding::new(DiagnosticCode::EraShape, bold.clone(), message);
    finding = if single_name {
        finding.hint(format!("did you mean `**{reconstructed}**`?"))
    } else {
        finding.hint("an era is `**Era name · 2020 – 2023**`; the name can't contain separators")
    };
    sink.report(attach_edit(finding, text, recovery, src, single_name));
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

/// Attaches the single Edit that rewrites every wrong separator, when it is safe to offer.
fn attach_edit(finding: Finding, text: &TextNode, recovery: &Recovery, src: &Source<'_>, safe: bool) -> Finding {
    if !safe {
        return finding;
    }
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
    use crate::diagnostics::{Diagnostic, Edit};
    use crate::markdown::tree::{self, BlockNode};
    use claims::assert_some;
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
        let src = Source { text: markdown };
        let head = role_line(heading, &src, &mut sink);
        (head, sink.finish())
    }

    fn era(markdown: &str) -> (EraCheck, Vec<Diagnostic>) {
        let (doc, mut sink) = lower(markdown);
        let Some(BlockNode::Paragraph(paragraph)) = doc.blocks.first() else {
            panic!("expected a paragraph")
        };
        let src = Source { text: markdown };
        let check = era_line(paragraph, &src, &mut sink);
        (check, sink.finish())
    }

    fn codes(diagnostics: &[Diagnostic]) -> Vec<DiagnosticCode> {
        diagnostics.iter().map(|d| d.code).collect()
    }

    fn apply(text: &str, edits: &[&Edit]) -> String {
        let mut sorted: Vec<_> = edits.to_vec();
        sorted.sort_by_key(|e| std::cmp::Reverse(e.range.start));
        let mut out = text.to_string();
        for edit in sorted {
            out.replace_range(edit.range.clone(), &edit.replacement);
        }
        out
    }

    fn fixed(text: &str, diagnostics: &[Diagnostic]) -> String {
        let edits: Vec<&Edit> = diagnostics.iter().filter_map(|d| d.fix.as_ref()).collect();
        apply(text, &edits)
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
        assert!(head.title.is_none());
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn level_three_heading_without_role_shape_is_error() {
        let (head, diagnostics) = role("### Projects");
        assert!(head.is_none());
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::RoleLineShape]);
        assert_eq!(assert_some!(diagnostics[0].span.as_ref()).range, 4..12);
        assert!(diagnostics[0].hints[0].contains("## "), "{:?}", diagnostics[0].hints);
    }

    #[test]
    fn role_line_with_dates_in_the_wrong_place_is_error() {
        let (head, diagnostics) = role("### 2020 – 2021 · Acme");
        assert!(head.is_none());
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
        assert!(head.is_none());
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
            assert!(head.is_none(), "{line}");
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
        assert_eq!(fixed(line, &diagnostics), "### Acme · Engineer · 2015 – 2017");
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
            assert_eq!(fixed(&line, &diagnostics), "### Acme · Engineer · 2015 – 2017", "{dot}");
        }
        let (_, diagnostics) = role("### Acme · Engineer · 2015 – 2017");
        assert_eq!(diagnostics, vec![], "the canonical separator is accepted");
    }

    #[test]
    fn stray_separator_inside_a_part_is_reported_without_edit() {
        // `Acme | Co` could be one company or two parts, so no Edit is offered.
        let (head, diagnostics) = role("### Acme | Co · Engineer · 2015 – 2017");
        assert!(head.is_none());
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::RoleSeparator]);
        assert_eq!(diagnostics[0].fix, None);
    }

    #[test]
    fn stray_separator_in_a_two_part_line_is_recovered() {
        let line = "### Acme | Engineer · 2015 – 2017";
        let (head, diagnostics) = role(line);
        assert_some!(head);
        assert_eq!(fixed(line, &diagnostics), "### Acme · Engineer · 2015 – 2017");
    }

    #[test]
    fn mixed_separators_are_recovered() {
        let line = "### Acme · Engineer - 2015 – 2017";
        let (head, diagnostics) = role(line);
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::RoleSeparator]);
        assert_eq!(fixed(line, &diagnostics), "### Acme · Engineer · 2015 – 2017");
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
        assert_eq!(fixed(line, &diagnostics), "### Gusto-Webtab · 2015 – 2017");
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
        assert!(head.is_none());
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
        assert_eq!(fixed(line, &diagnostics), "### Acme · Engineer · 2015 – 2017");
    }

    #[test]
    fn empty_company_maps_nutype_error_to_located_diagnostic() {
        let (head, diagnostics) = role("### · Title · 2020 – 2022");
        assert!(head.is_none());
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::EmptyRolePart]);
        assert_eq!(diagnostics[0].message, "the company is empty");
        assert_eq!(assert_some!(diagnostics[0].span.as_ref()).range, 4..7);
    }

    #[test]
    fn empty_title_is_empty_role_part() {
        let (head, diagnostics) = role("### Acme ·  · 2020 – 2022");
        assert!(head.is_none());
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::EmptyRolePart]);
        assert_eq!(diagnostics[0].message, "the title is empty");
    }

    #[test]
    fn overlong_company_is_text_too_long() {
        let line = format!("### {} · 2020 – 2022", "A".repeat(121));
        let (head, diagnostics) = role(&line);
        assert!(head.is_none());
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::TextTooLong]);
        assert_eq!(assert_some!(diagnostics[0].span.as_ref()).range, 4..125);
    }

    #[test]
    fn overlong_role_line_is_text_too_long_and_not_recovered() {
        let line = format!("### {}", "a - ".repeat(2000));
        let (head, diagnostics) = role(&line);
        assert!(head.is_none());
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::TextTooLong]);
    }

    #[test]
    fn unparseable_period_in_a_well_shaped_line_is_period_unparseable() {
        let (head, diagnostics) = role("### Acme · Engineer · Aug 2017 – Prsent");
        assert!(head.is_none());
        assert_eq!(codes(&diagnostics), vec![DiagnosticCode::PeriodUnparseable]);
    }

    #[test]
    fn end_before_start_in_a_role_line_is_reported() {
        let (head, diagnostics) = role("### Acme · 2020 – 2018");
        assert!(head.is_none());
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
            assert!(head.is_none(), "{line}");
            assert_eq!(codes(&diagnostics), vec![DiagnosticCode::PeriodUnparseable], "{line}");
            assert_eq!(diagnostics[0].fix, None, "{line}");
        }
    }

    #[test]
    fn a_company_with_a_year_in_its_name_is_never_rewritten() {
        // Safe direction: an error with a hint, never an Edit that could split the name.
        let (head, diagnostics) = role("### Windows 2000 | Engineer | 2015 – 2017");
        assert!(head.is_none());
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
            "**[link](https://x.y) · 2020**",
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
        assert_eq!(fixed(text, &diagnostics), "**AI platforms · 2026 – Present**");
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
        assert_eq!(fixed(text, &diagnostics), "**Phase · 2018 – 2020**");
    }

    #[test]
    fn candidates_are_capped() {
        let line = "a | ".repeat(100);
        assert_eq!(candidates(&line).len(), MAX_CANDIDATES);
    }
}
