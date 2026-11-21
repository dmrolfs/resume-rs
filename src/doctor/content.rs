//! ABOUTME: The eight content lints: the book's advice on bullets, summaries, and the order of sections.
//! ABOUTME: Pure functions over the parser's located outline; no clock, no environment, so a run is repeatable.

use std::ops::Range;
use std::path::Path;

use crate::diagnostics::{Diagnostic, DiagnosticCode, SourceKind, Span};
use crate::markdown::{BulletOutline, Located, Outline, Parsed, RoleOutline, SectionOutline};
use crate::model::{PartialDate, Period, PeriodEnd};

/// A bullet longer than this many characters runs to about three lines.
pub const LONG_BULLET_CHARS: usize = 320;
/// A summary of more words than this is too long.
pub const LONG_SUMMARY_WORDS: usize = 120;
/// A role that ended this many years before the latest year in the resume is old.
pub const OLD_ROLE_YEARS: u16 = 15;
/// An old role needs at least this many bullets to be worth a warning.
const OLD_ROLE_MIN_BULLETS: usize = 2;

/// Runs every content lint over `outline`, in document order within each lint.
///
/// Reads nothing but the outline: no clock and no environment, so the same file gives the same findings in any
/// year. Findings are in a fixed order, by the position of their span.
#[must_use]
pub fn lint_parsed(parsed: &Parsed, file: &Path) -> Vec<Diagnostic> {
    parsed.outline().map_or_else(Vec::new, |outline| lint(outline, file))
}

fn lint(outline: &Outline, file: &Path) -> Vec<Diagnostic> {
    let mut findings = Vec::new();
    for section in &outline.sections {
        let class = SectionClass::of(&section.title);
        findings.extend(section_findings(section, class, file));
    }
    findings.extend(education_not_last(outline, file));
    findings.extend(old_role_detail(outline, file));
    findings.extend(
        outline
            .groups
            .iter()
            .filter_map(|group| alternate_facts_differ(group, file)),
    );
    findings.sort_by_key(|d| d.span.as_ref().map_or(usize::MAX, |s| s.range.start));
    findings
}

// ---------------------------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------------------------

/// What a section is for, judged from its title, case-insensitively and by whole word.
///
/// The user's own words, so this is a heuristic that errs toward not linting: an unclassified section gets
/// fewer lints, never more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SectionClass {
    Education,
    Summary,
    /// Certifications, awards, publications, and the like, which may follow Education.
    Trailer,
    Ordinary,
}

impl SectionClass {
    fn of(title: &str) -> Self {
        let words: Vec<String> = title
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(str::to_lowercase)
            .collect();
        let has = |names: &[&str]| words.iter().any(|w| names.contains(&w.as_str()));
        if has(&["education"]) {
            Self::Education
        } else if has(&["summary", "profile"]) || words == ["about"] || words == ["about", "me"] {
            Self::Summary
        } else if has(&[
            "certification",
            "certifications",
            "license",
            "licenses",
            "award",
            "awards",
            "honor",
            "honors",
            "honour",
            "honours",
            "publication",
            "publications",
            "patent",
            "patents",
            "interest",
            "interests",
            "reference",
            "references",
        ]) {
            Self::Trailer
        } else {
            Self::Ordinary
        }
    }
}

fn section_findings(section: &SectionOutline, class: SectionClass, file: &Path) -> Vec<Diagnostic> {
    let mut findings = Vec::new();
    let education = class == SectionClass::Education;
    // Bullets directly under a section: Summary highlights, Skills lists.
    for bullet in &section.bullets {
        findings.extend(long_bullet(bullet, file));
        if !education {
            findings.extend(first_person(&bullet.content, file));
            if bullet.label.is_none() {
                findings.extend(weak_opener(&bullet.content, file));
            }
        }
        findings.extend(sloppy_phrase(&bullet.content, file));
    }
    if class == SectionClass::Summary {
        findings.extend(long_summary(section, file));
        for prose in &section.prose {
            findings.extend(first_person(prose, file));
        }
    }
    for prose in &section.prose {
        findings.extend(sloppy_phrase(prose, file));
    }
    for role in &section.roles {
        findings.extend(role_findings(role, education, file));
    }
    findings
}

fn role_findings(role: &RoleOutline, education: bool, file: &Path) -> Vec<Diagnostic> {
    let mut findings = Vec::new();
    for bullet in &role.bullets {
        findings.extend(long_bullet(bullet, file));
        if !education {
            if bullet.label.is_none() {
                findings.extend(unquantified_bullet(bullet, file));
            }
            findings.extend(weak_opener(&bullet.content, file));
            findings.extend(first_person(&bullet.content, file));
        }
        findings.extend(sloppy_phrase(&bullet.content, file));
    }
    // The blurb describes the company, so it is in the company's voice: sloppy phrases only.
    for text in role.blurb.iter().chain(&role.paragraphs) {
        findings.extend(sloppy_phrase(text, file));
    }
    findings
}

// ---------------------------------------------------------------------------------------------
// Findings
// ---------------------------------------------------------------------------------------------

fn finding(code: DiagnosticCode, file: &Path, range: Range<usize>, message: String, hint: &str) -> Diagnostic {
    Diagnostic {
        severity: code.default_severity(),
        code,
        message,
        span: Some(Span {
            file: file.to_path_buf(),
            kind: SourceKind::Markdown,
            range,
        }),
        hints: vec![hint.to_string()],
        fix: None,
    }
}

// ---------------------------------------------------------------------------------------------
// Words
// ---------------------------------------------------------------------------------------------

/// A word of `text`: a maximal run of letters and digits, with an apostrophe (`'` or `’`) kept only between two
/// letters or digits, so `etc3` and `S3` are whole words and never read as `etc` or `S`.
fn words(text: &str) -> Vec<(Range<usize>, &str)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let is_apostrophe = |c: char| c == '\'' || c == '\u{2019}';
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].1.is_alphanumeric() {
            i += 1;
            continue;
        }
        let start = chars[i].0;
        let mut j = i;
        while j < chars.len() {
            let c = chars[j].1;
            let joins = is_apostrophe(c) && j > i && chars.get(j + 1).is_some_and(|(_, next)| next.is_alphanumeric());
            if c.is_alphanumeric() || joins {
                j += 1;
            } else {
                break;
            }
        }
        let end = chars.get(j).map_or(text.len(), |(at, _)| *at);
        out.push((start..end, &text[start..end]));
        i = j;
    }
    out
}

// ---------------------------------------------------------------------------------------------
// long-bullet
// ---------------------------------------------------------------------------------------------

/// A bullet whose visible text, label included, is longer than [`LONG_BULLET_CHARS`] characters.
fn long_bullet(bullet: &BulletOutline, file: &Path) -> Option<Diagnostic> {
    let label = bullet.label.as_ref().map(Located::trimmed);
    let content = bullet.content.trimmed();
    let joined = match label {
        Some(label) => format!("{label} {content}"),
        None => content.to_string(),
    };
    let length = collapse_whitespace(&joined).chars().count();
    (length > LONG_BULLET_CHARS).then(|| {
        finding(
            DiagnosticCode::LongBullet,
            file,
            bullet.item.clone(),
            format!("bullet is {length} characters, about {} lines", length.div_ceil(110)),
            "split it into two bullets, or cut it to its result; the book advises two lines or fewer",
        )
    })
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------------------------
// unquantified-bullet
// ---------------------------------------------------------------------------------------------

/// Words that state a quantity. `one` is left out: "one of the" and "one-off" are not quantities.
const QUANTITY_WORDS: &[&str] = &[
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "twenty",
    "thirty",
    "forty",
    "fifty",
    "hundred",
    "hundreds",
    "thousand",
    "thousands",
    "million",
    "millions",
    "billion",
    "billions",
    "dozen",
    "dozens",
    "half",
    "double",
    "doubled",
    "triple",
    "tripled",
    "quadruple",
    "quadrupled",
    "halve",
    "halved",
    "tenfold",
    "percent",
];

// ---------------------------------------------------------------------------------------------
// alternate-facts-differ
// ---------------------------------------------------------------------------------------------

/// The numbers of `text`, in order: each run of digits with the `.`, `,`, `%`, `+`, and unit letters around it.
///
/// `5,000+` and `5,000` are different numbers here, which is the point: one wording was edited and the other
/// was not.
fn numbers(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let continues = !current.is_empty()
            && (matches!(c, '%' | '+' | '×')
                || (matches!(c, '.' | ',') && chars.peek().is_some_and(char::is_ascii_digit)));
        if c.is_ascii_digit() || continues {
            current.push(c);
        } else if !current.is_empty() {
            found.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        found.push(current);
    }
    found.sort();
    found
}

/// A group whose variants do not all state the same numbers.
fn alternate_facts_differ(group: &[Located], file: &Path) -> Option<Diagnostic> {
    let (first, rest) = group.split_first()?;
    let expected = numbers(&first.matched());
    let differs = rest.iter().find(|variant| numbers(&variant.matched()) != expected)?;
    let range = differs.source_range(0..differs.matched().len());
    Some(finding(
        DiagnosticCode::AlternateFactsDiffer,
        file,
        range,
        format!(
            "this wording states {} but the group's first states {}",
            listed(&numbers(&differs.matched())),
            listed(&expected)
        ),
        "make the numbers agree in every wording of the group",
    ))
}

fn listed(numbers: &[String]) -> String {
    if numbers.is_empty() {
        "no numbers".to_string()
    } else {
        numbers.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")
    }
}

/// A bullet with no number: no digit that starts a quantity, and no quantity word.
fn unquantified_bullet(bullet: &BulletOutline, file: &Path) -> Option<Diagnostic> {
    let matched = bullet.content.matched();
    if has_quantity(&matched) {
        return None;
    }
    Some(finding(
        DiagnosticCode::UnquantifiedBullet,
        file,
        bullet.item.clone(),
        "bullet has no number".to_string(),
        "add a measurable result: team size, %, $, users, or time saved",
    ))
}

fn has_quantity(text: &str) -> bool {
    has_number(text)
        || words(text)
            .iter()
            .any(|(_, word)| QUANTITY_WORDS.contains(&word.to_lowercase().as_str()))
}

/// A digit that starts a number token and is not a bare year.
///
/// A digit counts only when what precedes it is not a letter, a digit, `/`, or `.`, so `EC2`, `Q4`, `K8s`, `v2`
/// and `HTTP/2` do not count while `3rd`, `40%`, `$39K`, and `~5` do. A token of exactly four digits from 1950 to
/// 2100 is a year, not a quantity, unless a currency sign precedes it or `%`, `+`, `x`, `k`, `m`, or `b` follows.
fn has_number(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let before = i.checked_sub(1).map(|k| chars[k]);
        let starts_token = before.is_none_or(|c| !(c.is_alphabetic() || c.is_ascii_digit() || c == '/' || c == '.'));
        let mut j = i;
        while j < chars.len()
            && (chars[j].is_ascii_digit()
                || ((chars[j] == '.' || chars[j] == ',') && chars.get(j + 1).is_some_and(char::is_ascii_digit)))
        {
            j += 1;
        }
        if starts_token {
            let token: String = chars[i..j].iter().collect();
            let currency = before.is_some_and(|c| "$£€¥".contains(c));
            let suffix = chars.get(j).is_some_and(|c| "%+x×kKmMbB".contains(*c));
            if !is_bare_year(&token) || currency || suffix {
                return true;
            }
        }
        i = j.max(i + 1);
    }
    false
}

fn is_bare_year(token: &str) -> bool {
    token.len() == 4 && token.parse::<u16>().is_ok_and(|year| (1950..=2100).contains(&year))
}

// ---------------------------------------------------------------------------------------------
// weak-opener
// ---------------------------------------------------------------------------------------------

/// Openings that hide what you did; the longer phrase of a pair comes first so it is the one reported.
const WEAK_OPENERS: &[&[&str]] = &[
    &["responsible", "for"],
    &["worked", "on"],
    &["participated", "in"],
    &["involved", "in"],
    &["helped"],
    &["assisted"],
];

/// A bullet that opens with a weak phrase; only the opening counts.
fn weak_opener(content: &Located, file: &Path) -> Option<Diagnostic> {
    let matched = content.matched();
    let found = words(&matched);
    let (_phrase, len) = WEAK_OPENERS.iter().find_map(|phrase| {
        let lead = found.get(..phrase.len())?;
        let same = lead
            .iter()
            .zip(*phrase)
            .all(|((_, word), want)| word.to_lowercase() == *want);
        // The words must be separated by whitespace only: "Responsible. For years" is not the phrase.
        let joined = lead
            .windows(2)
            .all(|pair| matched[pair[0].0.end..pair[1].0.start].chars().all(char::is_whitespace));
        // Only punctuation may come before the first word, so a number or a word in front rules it out.
        let before = &matched[..lead.first()?.0.start];
        let tight = before.chars().all(|c| !c.is_alphanumeric());
        (same && joined && tight).then_some((*phrase, lead.last()?.0.end))
    })?;
    let start = found.first()?.0.start;
    let range = content.source_range(start..len);
    Some(finding(
        DiagnosticCode::WeakOpener,
        file,
        range,
        format!("bullet opens with \"{}\"", &matched[start..len]),
        "start with what you did: Led, Built, Cut, Shipped",
    ))
}

// ---------------------------------------------------------------------------------------------
// first-person
// ---------------------------------------------------------------------------------------------

const FIRST_PERSON: &[&str] = &[
    "me",
    "my",
    "myself",
    "mine",
    "we",
    "we're",
    "we've",
    "we'd",
    "we'll",
    "us",
    "our",
    "ours",
    "ourselves",
];

/// A bullet or summary that speaks as "I" or "we".
fn first_person(text: &Located, file: &Path) -> Option<Diagnostic> {
    let matched = text.matched();
    let found = words(&matched);
    let mut hits = found
        .iter()
        .filter(|(range, word)| is_first_person(&matched, range, word));
    let (range, word) = hits.next()?;
    let more = hits.count();
    let mut message = format!("first person: \"{word}\"");
    if more > 0 {
        message.push_str(&format!(" and {more} more"));
    }
    Some(finding(
        DiagnosticCode::FirstPerson,
        file,
        text.source_range(range.clone()),
        message,
        "drop \"I\" and \"my\", and say what you did instead of what we did",
    ))
}

/// Words after which a bare `I` is a numeral, not the pronoun: "Phase I", "Tier I support".
const NUMERAL_LABELS: &[&str] = &[
    "phase", "tier", "level", "type", "series", "grade", "class", "stage", "part", "step", "chapter", "volume",
    "section", "category", "group", "round",
];

fn is_first_person(text: &str, range: &Range<usize>, word: &str) -> bool {
    let normalized = word.replace('\u{2019}', "'");
    let lower = normalized.to_lowercase();
    if normalized == "I" || ["I'm", "I've", "I'd", "I'll"].contains(&normalized.as_str()) {
        // `I/O` and `I-9` are not the pronoun.
        let next = text[range.end..].chars().next();
        if matches!(next, Some('/' | '-')) {
            return false;
        }
        // "Phase I" is a numeral: only the bare `I` can be one, and only right after a label word.
        return !(normalized == "I" && follows_numeral_label(&text[..range.start]));
    }
    // `us` and `me` count only in lower case, so `US` and `ME` are left alone.
    if lower == "us" || lower == "me" {
        return normalized == lower;
    }
    FIRST_PERSON.contains(&lower.as_str())
}

/// True when the last word of `before` is a label such as `phase` and only spaces follow it.
fn follows_numeral_label(before: &str) -> bool {
    let trimmed = before.trim_end_matches(' ');
    if trimmed.len() == before.len() {
        return false;
    }
    words(trimmed).last().is_some_and(|(range, word)| {
        range.end == trimmed.len() && NUMERAL_LABELS.contains(&word.to_lowercase().as_str())
    })
}

// ---------------------------------------------------------------------------------------------
// sloppy-phrase
// ---------------------------------------------------------------------------------------------

/// Phrases that stand where a specific item should be.
const SLOPPY_PHRASES: &[&[&str]] = &[
    &["etc"],
    &["and", "so", "on"],
    &["and", "so", "forth"],
    &["and", "the", "like"],
    &["various"],
    &["stuff"],
];

/// The first sloppy phrase in `text`.
fn sloppy_phrase(text: &Located, file: &Path) -> Option<Diagnostic> {
    let matched = text.matched();
    let found = words(&matched);
    let (start, end, phrase) = (0..found.len()).find_map(|at| {
        SLOPPY_PHRASES.iter().find_map(|phrase| {
            let run = found.get(at..at + phrase.len())?;
            let same = run
                .iter()
                .zip(*phrase)
                .all(|((_, word), want)| word.to_lowercase() == *want);
            let single_spaced = run
                .windows(2)
                .all(|pair| matched[pair[0].0.end..pair[1].0.start].chars().all(char::is_whitespace));
            (same && single_spaced).then(|| (run[0].0.start, run[run.len() - 1].0.end, phrase.join(" ")))
        })
    })?;
    // `etc.` takes its period with it.
    let end = if phrase == "etc" && matched[end..].starts_with('.') {
        end + 1
    } else {
        end
    };
    Some(finding(
        DiagnosticCode::SloppyPhrase,
        file,
        text.source_range(start..end),
        format!("\"{phrase}\" says nothing"),
        "name the items, or cut the phrase",
    ))
}

// ---------------------------------------------------------------------------------------------
// long-summary
// ---------------------------------------------------------------------------------------------

/// A Summary section of more than [`LONG_SUMMARY_WORDS`] words, prose and bullets together.
fn long_summary(section: &SectionOutline, file: &Path) -> Option<Diagnostic> {
    let texts = section.prose.iter().map(Located::trimmed).chain(
        section
            .bullets
            .iter()
            .flat_map(|b| b.label.iter().map(Located::trimmed).chain([b.content.trimmed()])),
    );
    let count: usize = texts.map(count_words).sum();
    (count > LONG_SUMMARY_WORDS).then(|| {
        finding(
            DiagnosticCode::LongSummary,
            file,
            section.heading.clone(),
            format!("the summary is {count} words"),
            "keep it to two or three sentences, about 50 to 120 words",
        )
    })
}

/// Whitespace-separated tokens with at least one letter or digit, so a lone `–` or `·` is not a word.
fn count_words(text: &str) -> usize {
    text.split_whitespace()
        .filter(|token| token.chars().any(char::is_alphanumeric))
        .count()
}

// ---------------------------------------------------------------------------------------------
// education-not-last
// ---------------------------------------------------------------------------------------------

/// An Education section followed by an ordinary section; certifications, awards, and the like may follow it.
fn education_not_last(outline: &Outline, file: &Path) -> Vec<Diagnostic> {
    let classes: Vec<SectionClass> = outline.sections.iter().map(|s| SectionClass::of(&s.title)).collect();
    outline
        .sections
        .iter()
        .enumerate()
        .filter(|(at, _)| classes[*at] == SectionClass::Education)
        .filter_map(|(at, section)| {
            let later = outline.sections[at + 1..]
                .iter()
                .zip(&classes[at + 1..])
                .find(|(_, class)| **class == SectionClass::Ordinary)
                .map(|(later, _)| later)?;
            Some(finding(
                DiagnosticCode::EducationNotLast,
                file,
                section.heading.clone(),
                format!("`{}` comes before `{}`", section.title, later.title),
                &format!("move `{}` after `{}`", section.title, later.title),
            ))
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// old-role-detail
// ---------------------------------------------------------------------------------------------

/// The latest year the resume mentions: the largest start year, era year, or dated end year of any role.
///
/// Taken from the document and never from the clock. A role that is still `Present` contributes only its dates,
/// so this can be earlier than today, which makes the lint warn less, never more.
fn reference_year(roles: &[&RoleOutline]) -> Option<u16> {
    roles
        .iter()
        .flat_map(|role| std::iter::once(&role.period).chain(&role.eras))
        .flat_map(period_years)
        .max()
}

fn period_years(period: &Period) -> impl Iterator<Item = u16> {
    let end = match period.end {
        PeriodEnd::Present => None,
        PeriodEnd::Date(date) => Some(year_of(date)),
    };
    [Some(year_of(period.start)), end].into_iter().flatten()
}

fn year_of(date: PartialDate) -> u16 {
    date.sort_key().0.into()
}

/// The end of a period as an ordering key: `Present` after every date.
fn end_key(period: &Period) -> (bool, u16, u8) {
    match period.end {
        PeriodEnd::Present => (true, 0, 0),
        PeriodEnd::Date(date) => {
            let (year, month) = date.latest_key();
            (false, year.into(), month)
        }
    }
}

/// A role that ended 15 or more years before the latest year, with at least two bullets and more of them than
/// the most recent role has.
fn old_role_detail(outline: &Outline, file: &Path) -> Vec<Diagnostic> {
    let roles: Vec<&RoleOutline> = outline
        .sections
        .iter()
        .filter(|s| SectionClass::of(&s.title) != SectionClass::Education)
        .flat_map(|s| &s.roles)
        .collect();
    let (Some(reference), Some(recent)) = (
        reference_year(&roles),
        // The latest end wins; on a tie the earlier role in the document.
        roles.iter().copied().reduce(|best, role| {
            if end_key(&role.period) > end_key(&best.period) {
                role
            } else {
                best
            }
        }),
    ) else {
        return Vec::new();
    };
    roles
        .iter()
        .filter(|role| !std::ptr::eq(**role, recent))
        .filter_map(|role| {
            let PeriodEnd::Date(end) = role.period.end else {
                return None;
            };
            let ended = year_of(end);
            let age = reference.saturating_sub(ended);
            let bullets = role.bullets.len();
            (age >= OLD_ROLE_YEARS && bullets >= OLD_ROLE_MIN_BULLETS && bullets > recent.bullets.len()).then(|| {
                finding(
                    DiagnosticCode::OldRoleDetail,
                    file,
                    role.line.clone(),
                    format!(
                        "`{}` ended in {ended}, {age} years before {reference}, and has {bullets} bullets",
                        role.company
                    ),
                    &format!(
                        "older roles take less space: `{}` has {}; cut this one to one or two lines",
                        recent.company,
                        recent.bullets.len()
                    ),
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::parse;
    use claims::assert_some;
    use pretty_assertions::assert_eq;

    /// Lints `body` as the experience of a resume; `body` is everything after the contact line.
    fn run(body: &str) -> Vec<Diagnostic> {
        let text = format!("# Ada\n\nLondon\n\n{body}");
        let parsed = parse("r.md", &text);
        let outline = assert_some!(parsed.outline());
        lint(outline, Path::new("r.md"))
    }

    fn role(bullets: &str) -> String {
        format!("## Experience\n\n### Acme · Engineer · 2019 – 2020\n\n{bullets}\n")
    }

    fn codes(findings: &[Diagnostic]) -> Vec<DiagnosticCode> {
        findings.iter().map(|d| d.code).collect()
    }

    fn has(body: &str, code: DiagnosticCode) -> bool {
        run(body).iter().any(|d| d.code == code)
    }

    fn only(body: &str, code: DiagnosticCode) -> Vec<Diagnostic> {
        run(body).into_iter().filter(|d| d.code == code).collect()
    }

    // ============================================================================
    // alternate-facts-differ
    // ============================================================================

    #[test]
    fn the_numbers_of_a_text_keep_their_signs_and_ignore_their_order() {
        assert_eq!(
            numbers("Served 5,000+ users in 3.5 years, 40% faster"),
            ["3.5", "40%", "5,000+"]
        );
        assert_eq!(
            numbers("in 3.5 years, 40% faster, served 5,000+"),
            numbers("Served 5,000+ users in 3.5 years, 40% faster")
        );
        assert_eq!(numbers("No numbers."), Vec::<String>::new());
        assert_eq!(numbers("Ended 2019."), ["2019"]);
    }

    #[test]
    fn variants_whose_numbers_differ_are_flagged_on_the_differing_variant() {
        let body = role("- Served 5,000+ users. <!-- =reach.default -->\n- Reached 5,000 users. <!-- =reach.short -->");
        let found = only(&body, DiagnosticCode::AlternateFactsDiffer);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].severity, crate::diagnostics::Severity::Warning);
        assert!(
            found[0].message.contains("`5,000`") && found[0].message.contains("`5,000+`"),
            "{}",
            found[0].message
        );
        let text = format!("# Ada\n\nLondon\n\n{body}");
        let range = assert_some!(found[0].span.as_ref()).range.clone();
        assert_eq!(text[range].trim(), "Reached 5,000 users.");
    }

    #[test]
    fn variants_with_the_same_numbers_in_other_words_are_not_flagged() {
        let body = role(
            "- Served 5,000+ users in 3 regions. <!-- =reach.default -->\n- 3 regions, 5,000+ users. <!-- =reach.short -->\n- Led the team. <!-- =team.default -->\n- Ran the team. <!-- =team.short -->",
        );
        assert!(!has(&body, DiagnosticCode::AlternateFactsDiffer), "{:?}", run(&body));
    }

    #[test]
    fn a_number_only_one_variant_states_is_flagged() {
        let body = role("- Cut costs 40%. <!-- =cost.default -->\n- Cut costs. <!-- =cost.short -->");
        let found = only(&body, DiagnosticCode::AlternateFactsDiffer);
        assert_eq!(found.len(), 1);
        assert!(found[0].message.contains("no numbers"), "{}", found[0].message);
    }

    // ============================================================================
    // long-bullet
    // ============================================================================

    fn long(n: usize) -> String {
        format!("- {}\n", "a".repeat(n))
    }

    #[test]
    fn long_bullet_over_320_chars_is_flagged_and_320_is_not() {
        assert!(has(&role(&long(321)), DiagnosticCode::LongBullet));
        assert!(!has(&role(&long(320)), DiagnosticCode::LongBullet));
    }

    #[test]
    fn long_bullet_ignores_link_urls_and_counts_the_label_and_characters_not_bytes() {
        let url = format!(
            "- {} [site](https://example.com/{})\n",
            "a".repeat(200),
            "b".repeat(200)
        );
        assert!(!has(&role(&url), DiagnosticCode::LongBullet));
        let labelled = format!("- **{}:** {}\n", "L".repeat(30), "a".repeat(295));
        assert!(has(&role(&labelled), DiagnosticCode::LongBullet));
        let dashes = format!("- {}\n", "\u{2013}".repeat(300));
        assert!(!has(&role(&dashes), DiagnosticCode::LongBullet));
    }

    #[test]
    fn long_bullet_applies_outside_experience_too() {
        let body = format!("## Skills\n\n{}", long(321));
        assert!(has(&body, DiagnosticCode::LongBullet));
    }

    // ============================================================================
    // unquantified-bullet
    // ============================================================================

    fn unquantified(bullet: &str) -> bool {
        has(&role(&format!("- {bullet}\n")), DiagnosticCode::UnquantifiedBullet)
    }

    #[test]
    fn a_bullet_without_a_number_is_flagged_and_one_with_a_percent_is_not() {
        assert!(unquantified("Designed the first loop construct."));
        assert!(!unquantified("Cut latency 40%."));
    }

    #[test]
    fn every_quantity_form_counts() {
        for text in [
            "Led 12 people.",
            "Saved $39K.",
            "Cut costs ~5 points.",
            "Finished 3rd of 40.",
            "Ran 2x faster.",
            "Kept it up 24/7.",
            "Reached 1.5M users.",
            "Served 10,000 requests.",
            "Doubled throughput.",
            "Led two teams.",
            "Saved $2000 a month.",
        ] {
            assert!(!unquantified(text), "{text} is quantified");
        }
    }

    #[test]
    fn identifier_digits_a_bare_year_and_one_do_not_count() {
        for text in [
            "Moved services to EC2 and S3 in Q4.",
            "Adopted Rust in 2019.",
            "Ported to K8s and OAuth2.",
            "Spoke HTTP/2 and IPv6.",
            "One of the first to adopt Rust.",
            "Upgraded to v2.",
        ] {
            assert!(unquantified(text), "{text} has no quantity");
        }
    }

    #[test]
    fn digits_in_code_do_not_count() {
        assert!(unquantified("Tuned `p99` latency."));
    }

    #[test]
    fn only_unlabelled_experience_bullets_are_checked() {
        assert!(!has(
            "## Skills\n\n- **Languages:** Rust\n\n- Rust, Java\n",
            DiagnosticCode::UnquantifiedBullet
        ));
        assert!(!has(
            &role("- **Stack:** Rust, Kafka\n"),
            DiagnosticCode::UnquantifiedBullet
        ));
        let education = "## Education\n\n### University · 1990 – 1994\n\n- Thesis on engines\n";
        assert!(!has(education, DiagnosticCode::UnquantifiedBullet));
    }

    #[test]
    fn era_bullets_are_checked() {
        let body = "## Experience\n\n### Acme · 2019 – 2020\n\n**Platform · 2019**\n\n- Designed a thing.\n";
        assert!(has(body, DiagnosticCode::UnquantifiedBullet));
    }

    // ============================================================================
    // weak-opener
    // ============================================================================

    fn weak(bullet: &str) -> Vec<Diagnostic> {
        only(&role(&format!("- {bullet}\n")), DiagnosticCode::WeakOpener)
    }

    #[test]
    fn a_weak_opener_is_flagged_where_it_is_written() {
        let found = weak("Responsible for the build system, 3 of them.");
        assert_eq!(found.len(), 1);
        let span = assert_some!(found[0].span.as_ref());
        let text = format!(
            "# Ada\n\nLondon\n\n{}",
            role("- Responsible for the build system, 3 of them.\n")
        );
        assert_eq!(&text[span.range.clone()], "Responsible for");
    }

    #[test]
    fn a_weak_opener_is_case_insensitive_and_follows_a_label_or_emphasis() {
        assert_eq!(weak("HELPED migrate 3 services.").len(), 1);
        assert_eq!(weak("**Platform:** Worked on the scheduler, 3 times.").len(), 1);
        assert_eq!(weak("*Helped* ship v2 to 40 users.").len(), 1);
    }

    #[test]
    fn a_weak_phrase_is_a_whole_word_and_only_an_opening() {
        assert!(weak("Helpdesk tooling rewrite cut tickets 30%.").is_empty());
        assert!(weak("Led the migration and helped hire 4 engineers.").is_empty());
        assert!(weak("Led a team of 12.").is_empty());
    }

    #[test]
    fn a_labelled_non_role_bullet_is_not_checked_for_openers() {
        assert!(!has(
            "## Skills\n\n- **Tools:** Helped is not a verb here\n",
            DiagnosticCode::WeakOpener
        ));
        assert!(has("## Skills\n\n- Helped with tooling\n", DiagnosticCode::WeakOpener));
    }

    // ============================================================================
    // first-person
    // ============================================================================

    fn first(bullet: &str) -> Vec<Diagnostic> {
        only(&role(&format!("- {bullet}\n")), DiagnosticCode::FirstPerson)
    }

    #[test]
    fn first_person_is_flagged_in_each_form() {
        assert_eq!(first("I led the migration of 3 services.").len(), 1);
        assert_eq!(
            first("I'm the owner of 3 services.").len(),
            1,
            "curly apostrophe after smart punctuation"
        );
        assert_eq!(first("Grew our team to 9.").len(), 1);
        assert_eq!(first("We shipped v2 in 6 weeks.").len(), 1);
    }

    #[test]
    fn first_person_leaves_io_us_code_and_longer_words_alone() {
        assert!(first("Cut I/O wait 30% for US customers.").is_empty());
        assert!(first("Removed the `my_var` global in 3 files.").is_empty());
        assert!(first("Cut weekly usage 20% to determine demand.").is_empty());
    }

    #[test]
    fn first_person_is_reported_once_per_bullet_with_a_count() {
        let found = first("I led it and I shipped it, 3 times.");
        assert_eq!(found.len(), 1);
        assert!(found[0].message.contains("1 more"), "{}", found[0].message);
    }

    #[test]
    fn first_person_checks_summary_prose_but_not_a_role_blurb() {
        assert!(has(
            "## Summary\n\nI build distributed systems.\n",
            DiagnosticCode::FirstPerson
        ));
        let blurb = "## Experience\n\n### Acme · 2019 – 2020\n\nWe build payment software.\n\n- Cut costs 40%.\n";
        assert!(!has(blurb, DiagnosticCode::FirstPerson));
    }

    // ============================================================================
    // sloppy-phrase
    // ============================================================================

    #[test]
    fn a_sloppy_phrase_is_flagged_anywhere_it_prints() {
        assert!(has(
            "## Skills\n\n- **Languages:** Rust, Java, etc.\n",
            DiagnosticCode::SloppyPhrase
        ));
        assert!(has(
            &role("- Wrote tools and so\non, 3 of them.\n"),
            DiagnosticCode::SloppyPhrase
        ));
        assert!(has(
            "## Summary\n\nBuilt various systems.\n",
            DiagnosticCode::SloppyPhrase
        ));
        let blurb = "## Experience\n\n### Acme · 2019 – 2020\n\nMakes various tools.\n\n- Cut costs 40%.\n";
        assert!(has(blurb, DiagnosticCode::SloppyPhrase));
    }

    #[test]
    fn a_sloppy_phrase_is_a_whole_word_and_never_in_code() {
        assert!(!has(
            &role("- Built etcd clusters, 3 of them.\n"),
            DiagnosticCode::SloppyPhrase
        ));
        assert!(!has(
            &role("- Cut stuffing errors 10%.\n"),
            DiagnosticCode::SloppyPhrase
        ));
        assert!(!has(
            &role("- Edited `/etc/hosts` in 3 places.\n"),
            DiagnosticCode::SloppyPhrase
        ));
    }

    #[test]
    fn etc_takes_its_period_in_the_span() {
        let body = role("- Used Rust, Java, etc. in 3 projects.\n");
        let found = only(&body, DiagnosticCode::SloppyPhrase);
        let text = format!("# Ada\n\nLondon\n\n{body}");
        let span = assert_some!(found[0].span.as_ref());
        assert_eq!(&text[span.range.clone()], "etc.");
    }

    // ============================================================================
    // long-summary
    // ============================================================================

    fn summary(words: usize) -> String {
        format!("## Summary\n\n{}\n", vec!["word"; words].join(" "))
    }

    #[test]
    fn a_summary_over_120_words_is_flagged_and_120_is_not() {
        assert!(has(&summary(121), DiagnosticCode::LongSummary));
        assert!(!has(&summary(120), DiagnosticCode::LongSummary));
    }

    #[test]
    fn a_summary_counts_its_bullets_and_skips_lone_punctuation() {
        let with_bullets = format!(
            "## Summary\n\n{}\n\n- {}\n- {}\n",
            vec!["w"; 100].join(" "),
            vec!["x"; 15].join(" "),
            vec!["y"; 15].join(" ")
        );
        assert!(has(&with_bullets, DiagnosticCode::LongSummary));
        let dashes = format!("## Summary\n\n{} \u{2013} \u{b7}\n", vec!["w"; 118].join(" "));
        assert!(!has(&dashes, DiagnosticCode::LongSummary));
    }

    #[test]
    fn only_a_summary_section_is_counted() {
        let body = format!("## Experience\n\n{}\n", vec!["word"; 200].join(" "));
        assert!(!has(&body, DiagnosticCode::LongSummary));
        let profile = format!("## Profile\n\n{}\n", vec!["word"; 121].join(" "));
        assert!(has(&profile, DiagnosticCode::LongSummary));
    }

    // ============================================================================
    // education-not-last
    // ============================================================================

    #[test]
    fn education_before_an_ordinary_section_is_flagged() {
        assert!(has(
            "## Education\n\n- BSc\n\n## Experience\n\n- x\n",
            DiagnosticCode::EducationNotLast
        ));
        assert!(has(
            "## Education\n\n- BSc\n\n## Skills\n\n- Rust\n",
            DiagnosticCode::EducationNotLast
        ));
    }

    #[test]
    fn education_last_or_followed_only_by_trailers_is_fine() {
        assert!(!has(
            "## Experience\n\n- x\n\n## Education\n\n- BSc\n",
            DiagnosticCode::EducationNotLast
        ));
        assert!(!has(
            "## Education\n\n- BSc\n\n## Certifications\n\n- AWS\n",
            DiagnosticCode::EducationNotLast
        ));
        assert!(!has("## Experience\n\n- x\n", DiagnosticCode::EducationNotLast));
    }

    #[test]
    fn education_is_matched_by_whole_word_case_insensitively() {
        assert!(has(
            "## EDUCATION & TRAINING\n\n- BSc\n\n## Experience\n\n- x\n",
            DiagnosticCode::EducationNotLast
        ));
        assert!(!has(
            "## Educational Outreach\n\n- Talks\n\n## Experience\n\n- x\n",
            DiagnosticCode::EducationNotLast
        ));
    }

    // ============================================================================
    // old-role-detail
    // ============================================================================

    fn career(recent_bullets: usize, old_end: u16, old_bullets: usize) -> String {
        let bullets = |n: usize| {
            (0..n)
                .map(|i| format!("- Did thing {i} for 3 teams.\n"))
                .collect::<String>()
        };
        format!(
            "## Experience\n\n### Now Inc · 2020 – Present\n\n{}\n### Then Ltd · 2000 – {old_end}\n\n{}",
            bullets(recent_bullets),
            bullets(old_bullets)
        )
    }

    #[test]
    fn an_old_role_with_more_bullets_is_flagged_at_15_years_and_not_at_14() {
        assert!(has(&career(2, 2005, 4), DiagnosticCode::OldRoleDetail));
        assert!(!has(&career(2, 2006, 4), DiagnosticCode::OldRoleDetail));
    }

    #[test]
    fn an_old_role_needs_more_bullets_than_the_latest_and_at_least_two() {
        assert!(!has(&career(2, 2005, 2), DiagnosticCode::OldRoleDetail));
        assert!(!has(&career(0, 2005, 1), DiagnosticCode::OldRoleDetail));
        assert!(has(&career(0, 2005, 2), DiagnosticCode::OldRoleDetail));
    }

    #[test]
    fn the_reference_year_comes_from_an_era_under_a_present_role() {
        let body = "## Experience\n\n### Now Inc · 2017 – Present\n\n**Platform · 2026 – Present**\n\n- Led 3 teams.\n\n### Then Ltd · 2004 – 2011\n\n- A 3.\n- B 3.\n- C 3.\n";
        let found = only(body, DiagnosticCode::OldRoleDetail);
        assert_eq!(found.len(), 1, "2026 - 2011 = 15");
        assert!(found[0].message.contains("2026"), "{}", found[0].message);
    }

    #[test]
    fn education_roles_and_the_latest_role_are_never_flagged() {
        let education = "## Experience\n\n### Now · 2020 – Present\n\n- A 3.\n\n## Education\n\n### University · 1990 – 1994\n\n- A 3.\n- B 3.\n- C 3.\n";
        assert!(!has(education, DiagnosticCode::OldRoleDetail));
        assert!(!has(
            "## Experience\n\n### Only · 1990 – 1994\n\n- A 3.\n- B 3.\n",
            DiagnosticCode::OldRoleDetail
        ));
    }

    #[test]
    fn roles_in_different_sections_are_compared() {
        let body = "## Experience\n\n### Now · 2020 – Present\n\n- A 3.\n\n## Earlier Experience\n\n### Then · 1995 – 2003\n\n- A 3.\n- B 3.\n";
        assert!(has(body, DiagnosticCode::OldRoleDetail));
    }

    // ============================================================================
    // Whole-run properties
    // ============================================================================

    #[test]
    fn findings_are_located_in_the_markdown_in_document_order() {
        let body = format!(
            "{}{}",
            role("- Helped fix things, etc.\n- Led 12 people.\n"),
            "## Summary\n\nI build.\n"
        );
        let found = run(&body);
        assert!(found.len() >= 3, "{:?}", codes(&found));
        let starts: Vec<usize> = found
            .iter()
            .map(|d| assert_some!(d.span.as_ref()).range.start)
            .collect();
        assert!(starts.windows(2).all(|pair| pair[0] <= pair[1]), "{starts:?}");
        assert!(
            found
                .iter()
                .all(|d| d.span.as_ref().is_some_and(|s| s.kind == SourceKind::Markdown))
        );
    }

    #[test]
    fn content_findings_never_carry_an_edit_and_are_never_errors() {
        let found = run(&role("- Helped with various things.\n"));
        assert!(!found.is_empty());
        assert!(
            found
                .iter()
                .all(|d| d.fix.is_none() && d.severity != crate::diagnostics::Severity::Error)
        );
    }

    #[test]
    fn the_same_input_gives_the_same_findings() {
        let body = career(2, 2005, 4);
        assert_eq!(run(&body), run(&body));
    }

    #[test]
    fn the_full_fixture_has_content_advice_while_its_parse_has_none() {
        let text = include_str!("../../tests/fixtures/full.md");
        let parsed = parse("full.md", text);
        assert!(parsed.diagnostics().is_empty(), "{:?}", parsed.diagnostics());
        let outline = assert_some!(parsed.outline());
        assert!(!lint(outline, Path::new("full.md")).is_empty());
    }
    // ============================================================================
    // Boundaries the mutation run is meant to find
    // ============================================================================

    #[test]
    fn a_summary_word_filter_is_exercised_on_the_wrong_side_of_the_boundary() {
        // 119 words and two marks: 121 tokens unfiltered, 119 filtered.
        let body = format!("## Summary\n\n{} \u{2013} \u{b7}\n", vec!["w"; 119].join(" "));
        assert!(!has(&body, DiagnosticCode::LongSummary));
        let one_more = format!("## Summary\n\n{} \u{2013} \u{b7}\n", vec!["w"; 121].join(" "));
        assert!(has(&one_more, DiagnosticCode::LongSummary));
    }

    #[test]
    fn a_summary_counts_the_label_words_of_its_bullets() {
        // 118 prose words, plus a bullet whose label is 2 words and content 1: 121 with the label, 119 without.
        let body = format!(
            "## Summary\n\n{}\n\n- **Core skills:** Rust\n",
            vec!["w"; 118].join(" ")
        );
        assert!(has(&body, DiagnosticCode::LongSummary));
    }

    #[test]
    fn a_year_with_a_quantity_suffix_or_a_currency_sign_is_a_quantity() {
        for text in [
            "Grew to 2000+ users.",
            "Cut 2000x.",
            "Saved 2000k.",
            "Held 2000m.",
            "Won 2000b.",
            "Saved \u{a3}2000.",
        ] {
            assert!(!unquantified(text), "{text}");
        }
        assert!(unquantified("Shipped in 2000."));
    }

    #[test]
    fn the_year_range_is_1950_to_2100_inclusive() {
        assert!(unquantified("Shipped in 1950."));
        assert!(unquantified("Shipped in 2100."));
        assert!(!unquantified("Shipped in 1949."));
        assert!(!unquantified("Shipped in 2101."));
    }

    #[test]
    fn a_bullet_that_starts_with_a_digit_is_quantified() {
        assert!(!unquantified("40% faster builds."));
        assert!(!unquantified("3 teams merged."));
    }

    #[test]
    fn a_digit_after_a_dot_or_a_slash_or_a_letter_is_part_of_a_name_not_a_number() {
        assert!(unquantified("Edited the .7z handler."));
        assert!(unquantified("Served /2 routes."));
        assert!(unquantified("Moved to S3."));
    }

    #[test]
    fn education_bullets_are_not_checked_for_openers_or_first_person_even_directly_under_the_section() {
        let direct = "## Education\n\n- I helped teach a course.\n- Helped plan it.\n";
        assert!(!has(direct, DiagnosticCode::FirstPerson));
        assert!(!has(direct, DiagnosticCode::WeakOpener));
        let in_role = "## Education\n\n### University · 1990 – 1994\n\n- I helped teach a course.\n";
        assert!(!has(in_role, DiagnosticCode::FirstPerson));
        assert!(!has(in_role, DiagnosticCode::WeakOpener));
        assert!(has(
            "## Skills\n\n- I helped teach a course.\n",
            DiagnosticCode::FirstPerson
        ));
    }

    #[test]
    fn every_first_person_word_is_found_and_the_look_alikes_are_not() {
        for word in ["me", "my", "myself", "mine", "we", "us", "our", "ours", "ourselves"] {
            let found = first(&format!("Gave {word} the keys to 3 rooms."));
            assert_eq!(found.len(), 1, "{word}");
        }
        for text in [
            "I've shipped 3 things.",
            "I'd cut costs 3 times.",
            "I'll own 3 teams.",
            "I\u{2019}m on 3 teams.",
        ] {
            assert_eq!(first(text).len(), 1, "{text}");
        }
        for text in [
            "Filed I-9 forms for 30 hires.",
            "Shipped to ME and US sites, 3 of them.",
            "Kept ME data for 3 years.",
        ] {
            assert!(first(text).is_empty(), "{text}");
        }
    }

    #[test]
    fn a_roman_numeral_after_a_label_word_is_not_the_pronoun() {
        for text in [
            "Ran Tier I support for 30 clients.",
            "Led Phase I of 3 trials.",
            "Passed Level I of 4.",
        ] {
            assert!(first(text).is_empty(), "{text}");
        }
        assert_eq!(
            first("I ran Tier I support for 30 clients.").len(),
            1,
            "the first I is the pronoun"
        );
        assert_eq!(first("Then I led it, 3 times.").len(), 1);
    }

    #[test]
    fn first_person_says_nothing_about_a_count_for_one_hit_and_counts_the_rest_for_more() {
        assert_eq!(first("I led 3 teams.")[0].message, "first person: \"I\"");
        assert_eq!(
            first("I led it and I shipped it, 3 times.")[0].message,
            "first person: \"I\" and 1 more"
        );
        assert_eq!(
            first("I led it, I shipped it, and we won, 3 times.")[0].message,
            "first person: \"I\" and 2 more"
        );
    }

    #[test]
    fn a_weak_opener_needs_whitespace_between_its_words_and_nothing_alphanumeric_before_it() {
        assert!(weak("Responsible, for the build, 3 of them.").is_empty());
        assert!(weak("Worked; on call 3 nights.").is_empty());
        assert!(weak("2019: Helped migrate 3 services.").is_empty());
        assert_eq!(weak("\u{201c}Helped migrate 3 services.\u{201d}").len(), 1);
        assert_eq!(weak("Responsible   for the build, 3 of them.").len(), 1);
    }

    #[test]
    fn every_weak_opener_is_found() {
        for opener in [
            "Responsible for",
            "Worked on",
            "Participated in",
            "Involved in",
            "Helped",
            "Assisted",
        ] {
            let found = weak(&format!("{opener} the 3 teams."));
            assert_eq!(found.len(), 1, "{opener}");
            assert_eq!(found[0].message, format!("bullet opens with \"{opener}\""));
        }
    }

    #[test]
    fn every_sloppy_phrase_is_found_and_a_comma_breaks_it() {
        for phrase in ["etc.", "and so on", "and so forth", "and the like", "various", "stuff"] {
            assert!(
                has(
                    &role(&format!("- Built tools {phrase} for 3 teams.\n")),
                    DiagnosticCode::SloppyPhrase
                ),
                "{phrase}"
            );
        }
        assert!(!has(
            &role("- Built tools and, so on, for 3 teams.\n"),
            DiagnosticCode::SloppyPhrase
        ));
        assert!(!has(&role("- Built etc3 for 3 teams.\n"), DiagnosticCode::SloppyPhrase));
    }

    #[test]
    fn a_sloppy_phrase_span_is_exactly_the_phrase() {
        let body = role("- Wrote tools and so on. Cut 3 costs.\n");
        let text = format!("# Ada\n\nLondon\n\n{body}");
        let found = only(&body, DiagnosticCode::SloppyPhrase);
        let span = assert_some!(found[0].span.as_ref());
        assert_eq!(&text[span.range.clone()], "and so on");
    }

    #[test]
    fn the_latest_end_decides_the_recent_role_and_the_first_wins_a_tie() {
        // Now ends 2024 with 1 bullet; Then ends 2009 with 2: 2024 - 2009 = 15.
        let body = "## Experience\n\n### Now \u{b7} 2010 \u{2013} 2024\n\n- A 3.\n\n### Then \u{b7} 2000 \u{2013} 2009\n\n- A 3.\n- B 3.\n";
        assert!(
            has(body, DiagnosticCode::OldRoleDetail),
            "end years are in the reference year"
        );
        // A and B tie on end year: the earlier in the document is the recent one, so C is compared with A (1 bullet).
        let tie = "## Experience\n\n### A \u{b7} 2018 \u{2013} 2020\n\n- A 3.\n\n### B \u{b7} 2019 \u{2013} 2020\n\n- A 3.\n- B 3.\n- C 3.\n\n### C \u{b7} 2000 \u{2013} 2005\n\n- A 3.\n- B 3.\n";
        let found = only(tie, DiagnosticCode::OldRoleDetail);
        assert!(found.iter().any(|d| d.message.contains("`C`")), "{found:?}");
    }

    #[test]
    fn a_soft_break_in_a_long_bullet_counts_as_one_space() {
        let wrapped = format!("- {}\n  {}\n", "a".repeat(200), "b".repeat(119));
        assert!(!has(&role(&wrapped), DiagnosticCode::LongBullet), "200 + 1 + 119 = 320");
        let longer = format!("- {}\n  {}\n", "a".repeat(200), "b".repeat(120));
        assert!(has(&role(&longer), DiagnosticCode::LongBullet));
    }

    #[test]
    fn the_long_bullet_message_gives_the_exact_length() {
        let found = only(&role(&long(330)), DiagnosticCode::LongBullet);
        assert_eq!(found[0].message, "bullet is 330 characters, about 3 lines");
    }

    // ============================================================================
    // Spans, exactly, on every line ending
    // ============================================================================

    /// `(body, code, the source text the span must cover)`.
    fn span_cases() -> Vec<(String, DiagnosticCode, &'static str)> {
        vec![
            (
                role("- I led [the site](https://example.com) with 3 teams.\n"),
                DiagnosticCode::FirstPerson,
                "I",
            ),
            (
                role("- Led [the site](https://example.com) and we won, 3 times.\n"),
                DiagnosticCode::FirstPerson,
                "we",
            ),
            (
                role("- Tuned `p99` and various things, 3 of them.\n"),
                DiagnosticCode::SloppyPhrase,
                "various",
            ),
            (role("- Helped fix 3 things.\n"), DiagnosticCode::WeakOpener, "Helped"),
            (
                role("- Used [Rust](https://rust-lang.org) etc. in 3 jobs.\n"),
                DiagnosticCode::SloppyPhrase,
                "etc.",
            ),
            (
                format!("## Summary\n\n{}\n", vec!["word"; 121].join(" ")),
                DiagnosticCode::LongSummary,
                "Summary",
            ),
            (
                "## Education\n\n- BSc\n\n## Experience\n\n- x\n".to_string(),
                DiagnosticCode::EducationNotLast,
                "Education",
            ),
            (role(&long(321)), DiagnosticCode::LongBullet, ""),
        ]
    }

    #[test]
    fn spans_cover_exactly_the_words_that_caused_the_finding() {
        for (body, code, expected) in span_cases() {
            let text = format!("# Ada\n\nLondon\n\n{body}");
            let found = only(&body, code);
            assert_eq!(found.len(), 1, "{code}: {found:?}");
            let span = assert_some!(found[0].span.as_ref());
            let covered = &text[span.range.clone()];
            if expected.is_empty() {
                assert!(
                    covered.starts_with("- aaa") || covered.starts_with("aaa"),
                    "{code}: {covered:?}"
                );
            } else {
                assert_eq!(covered, expected, "{code}");
            }
        }
    }

    #[test]
    fn the_same_findings_and_spans_come_out_on_crlf_text() {
        for (body, code, expected) in span_cases() {
            let lf_text = format!("# Ada\n\nLondon\n\n{body}");
            let crlf_body = body.replace('\n', "\r\n");
            let crlf_text = format!("# Ada\r\n\r\nLondon\r\n\r\n{crlf_body}");
            let parsed = parse("r.md", &crlf_text);
            let outline = assert_some!(parsed.outline());
            let found: Vec<_> = lint(outline, Path::new("r.md"))
                .into_iter()
                .filter(|d| d.code == code)
                .collect();
            assert_eq!(found.len(), 1, "{code} on CRLF: {found:?}");
            let span = assert_some!(found[0].span.as_ref());
            assert!(crlf_text.is_char_boundary(span.range.start) && crlf_text.is_char_boundary(span.range.end));
            if !expected.is_empty() {
                assert_eq!(
                    &crlf_text[span.range.clone()],
                    expected,
                    "{code} on CRLF (lf was {})",
                    lf_text.len()
                );
            }
        }
    }

    #[test]
    fn a_long_bullet_that_wraps_over_crlf_is_counted_without_the_carriage_return() {
        let wrapped = format!("- {}\r\n  {}\r\n", "a".repeat(200), "b".repeat(119));
        let text = format!(
            "# Ada\r\n\r\nLondon\r\n\r\n## Experience\r\n\r\n### Acme \u{b7} 2019 \u{2013} 2020\r\n\r\n{wrapped}"
        );
        let parsed = parse("r.md", &text);
        let found: Vec<_> = lint(assert_some!(parsed.outline()), Path::new("r.md"))
            .into_iter()
            .filter(|d| d.code == DiagnosticCode::LongBullet)
            .collect();
        assert!(found.is_empty(), "a CR in the text pushed it over 320: {found:?}");
    }
    #[test]
    fn about_and_about_me_are_summary_sections_and_a_longer_title_is_not() {
        let long = vec!["word"; 121].join(" ");
        assert!(has(&format!("## About\n\n{long}\n"), DiagnosticCode::LongSummary));
        assert!(has(&format!("## About me\n\n{long}\n"), DiagnosticCode::LongSummary));
        assert!(!has(
            &format!("## About the company\n\n{long}\n"),
            DiagnosticCode::LongSummary
        ));
        assert!(!has(&format!("## Me\n\n{long}\n"), DiagnosticCode::LongSummary));
    }

    #[test]
    fn lint_parsed_finds_what_lint_finds_and_nothing_without_a_name() {
        let parsed = parse(
            "r.md",
            &format!("# Ada\n\nLondon\n\n{}", role("- Helped fix 3 things.\n")),
        );
        assert_eq!(lint_parsed(&parsed, Path::new("r.md")).len(), 1);
        let nameless = parse("r.md", "## Experience\n\n- Helped fix 3 things.\n");
        assert!(lint_parsed(&nameless, Path::new("r.md")).is_empty());
    }

    #[test]
    fn words_split_on_everything_but_letters_digits_and_a_joining_apostrophe() {
        let found: Vec<&str> = words("Led S3, EC2 & it's we\u{2019}re ' x'y z' 3d")
            .into_iter()
            .map(|(_, w)| w)
            .collect();
        assert_eq!(found, ["Led", "S3", "EC2", "it's", "we\u{2019}re", "x'y", "z", "3d"]);
        assert!(words("").is_empty());
        assert!(words(" .,; ").is_empty());
        let spans: Vec<Range<usize>> = words("ab cd").into_iter().map(|(r, _)| r).collect();
        assert_eq!(spans, [0..2, 3..5]);
    }
    #[test]
    fn a_number_token_runs_over_a_decimal_point_or_a_thousands_comma_but_not_a_sentence_end() {
        // `2019.5` and `2,019` are numbers; cut at the `.` or `,` they would read as the year 2019.
        assert!(!unquantified("Scored 2019.5 points."));
        assert!(!unquantified("Scored 2019,5 points."));
        assert!(!unquantified("Served 2,019 users."));
        // A full stop or a comma after the year ends the token: it is still a bare year.
        assert!(unquantified("Adopted Rust in 2019."));
        assert!(unquantified("Adopted Rust in 2019, then Go."));
    }

    #[test]
    fn a_bullet_that_ends_on_a_digit_is_read_without_running_off_its_end() {
        assert!(unquantified("Adopted Rust in 2019"));
        assert!(!unquantified("Led 12"));
        assert!(unquantified("Moved to S3"));
    }
}
