//! ABOUTME: Period grammar for role and era lines: month, season, or year starts; en dash; Present.
//! ABOUTME: Pure parsing plus located findings (separator and Present-spelling Edits, range and order errors).

use std::ops::Range;
use std::str::FromStr;

use time::Month;

use super::tree::TextNode;
use super::{Finding, Sink, Source};
use crate::diagnostics::DiagnosticCode;
use crate::model::{PartialDate, Period, PeriodEnd, Season, Year};

/// Longest period text (in bytes) the grammar will examine.
const MAX_PERIOD_BYTES: usize = 64;
/// The canonical separator between the two dates of a period.
const CANONICAL_SEPARATOR: &str = " – ";
const PRESENT: &str = "Present";

/// Month names and abbreviations, matched ASCII-case-insensitively after one trailing `.` is dropped.
const MONTHS: [(&str, Month); 25] = [
    ("january", Month::January),
    ("february", Month::February),
    ("march", Month::March),
    ("april", Month::April),
    ("may", Month::May),
    ("june", Month::June),
    ("july", Month::July),
    ("august", Month::August),
    ("september", Month::September),
    ("october", Month::October),
    ("november", Month::November),
    ("december", Month::December),
    ("jan", Month::January),
    ("feb", Month::February),
    ("mar", Month::March),
    ("apr", Month::April),
    ("jun", Month::June),
    ("jul", Month::July),
    ("aug", Month::August),
    ("sep", Month::September),
    ("sept", Month::September),
    ("oct", Month::October),
    ("nov", Month::November),
    ("dec", Month::December),
    ("may", Month::May),
];

/// A parsed period with the text ranges of any non-canonical tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PeriodSyntax {
    pub period: Period,
    /// Whitespace and dashes between the two dates, as written; `None` for a single date.
    pub separator: Option<Range<usize>>,
    /// The end token when it is `Current`, `Now`, or `Present` in another case.
    pub present_alias: Option<Range<usize>>,
}

/// Why a period could not be read; ranges index the input text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PeriodProblem {
    NoYear,
    TooLong,
    Token {
        range: Range<usize>,
        message: String,
        hint: String,
    },
    YearOutOfRange {
        range: Range<usize>,
        year: u16,
    },
    EndBeforeStart,
}

/// True when `text` contains a standalone ASCII four-digit number (a year).
pub(crate) fn contains_year(text: &str) -> bool {
    let mut run = 0;
    for byte in text.bytes().chain(std::iter::once(b' ')) {
        if byte.is_ascii_digit() {
            run += 1;
        } else {
            if run == 4 {
                return true;
            }
            run = 0;
        }
    }
    false
}

/// Parses period text such as `Aug 2017 – Present`; pure, and the target of the round-trip proptest.
pub(crate) fn parse_text(text: &str) -> Result<PeriodSyntax, PeriodProblem> {
    if text.len() > MAX_PERIOD_BYTES {
        return Err(PeriodProblem::TooLong);
    }
    if !contains_year(text) {
        return Err(PeriodProblem::NoYear);
    }
    let (start_range, separator, end_range) = split_dates(text)?;
    let start_tokens = tokens(text, start_range);
    let start = parse_date(&start_tokens, text)?;
    let written_end = end_range.map(|range| parse_end(text, range)).transpose()?;
    let mut normalized = join_tokens(&start_tokens);
    if let Some(end) = &written_end {
        normalized.push_str(CANONICAL_SEPARATOR);
        normalized.push_str(&end.text);
    }
    let end = written_end.as_ref().map_or(PeriodEnd::Date(start), |e| e.end);
    let present_alias = written_end.and_then(|e| e.alias);
    let period = Period::try_new(normalized, start, end).map_err(|_| PeriodProblem::EndBeforeStart)?;
    Ok(PeriodSyntax {
        period,
        separator,
        present_alias,
    })
}

type Split = (Range<usize>, Option<Range<usize>>, Option<Range<usize>>);

/// Splits at the single run of dash characters into the start, the separator, and the end.
fn split_dates(text: &str) -> Result<Split, PeriodProblem> {
    let is_dash = |c: char| matches!(c, '-' | '–' | '—');
    let trimmed = trim_range(text, 0..text.len());
    let Some(run_start) = text[trimmed.clone()].find(is_dash).map(|i| i + trimmed.start) else {
        return Ok((trimmed, None, None));
    };
    let run_end = run_start
        + text[run_start..]
            .find(|c| !is_dash(c))
            .unwrap_or(text.len() - run_start);
    if let Some(second) = text[run_end..trimmed.end].find(is_dash) {
        let at = run_end + second;
        return Err(token_problem(
            at..at + 1,
            "-",
            "a period has one separator; write `Aug 2017 – 2020`",
        ));
    }
    let start = trim_range(text, trimmed.start..run_start);
    let end = trim_range(text, run_end..trimmed.end);
    if start.is_empty() || end.is_empty() {
        let range = run_start..run_end;
        let hint = "give both dates, or write `Present` for an ongoing role";
        return Err(token_problem(range, &text[run_start..run_end], hint));
    }
    Ok((start.clone(), Some(start.end..end.start), Some(end)))
}

/// The end of a period as written: its value, a non-canonical Present token, and its normalized text.
struct EndSyntax {
    end: PeriodEnd,
    alias: Option<Range<usize>>,
    text: String,
}

fn parse_end(text: &str, range: Range<usize>) -> Result<EndSyntax, PeriodProblem> {
    let end_tokens = tokens(text, range);
    if let [(token, token_range)] = end_tokens.as_slice()
        && let Some(canonical) = present_spelling(token)
    {
        let alias = (!canonical).then(|| token_range.clone());
        return Ok(EndSyntax {
            end: PeriodEnd::Present,
            alias,
            text: PRESENT.to_string(),
        });
    }
    let date = parse_date(&end_tokens, text)?;
    Ok(EndSyntax {
        end: PeriodEnd::Date(date),
        alias: None,
        text: join_tokens(&end_tokens),
    })
}

type Token<'a> = (&'a str, Range<usize>);

fn parse_date(tokens: &[Token<'_>], text: &str) -> Result<PartialDate, PeriodProblem> {
    match tokens {
        [(year, range)] => parse_year(year, range).map(|year| PartialDate::Year { year }),
        [(word, word_range), (year, year_range)] => {
            let year = parse_year(year, year_range)?;
            if let Some(month) = month(word) {
                Ok(PartialDate::Month { year, month })
            } else if let Ok(season) = Season::from_str(word) {
                Ok(PartialDate::Season { year, season })
            } else {
                Err(unknown_word(word, word_range))
            }
        }
        _ => {
            if let Some((word, range)) = tokens.iter().find(|(word, _)| is_range_word(word)) {
                return Err(unknown_word(word, range));
            }
            let range = tokens
                .first()
                .map_or(0..text.len(), |t| t.1.start..tokens.last().map_or(t.1.end, |l| l.1.end));
            Err(token_problem(
                range.clone(),
                &text[range],
                "a date is `Aug 2017`, `Fall 2019`, or `2017`",
            ))
        }
    }
}

fn parse_year(token: &str, range: &Range<usize>) -> Result<Year, PeriodProblem> {
    if token.len() == 4 && token.bytes().all(|b| b.is_ascii_digit()) {
        let value: u16 = token.parse().map_err(|_| unknown_word(token, range))?;
        return Year::try_new(value).map_err(|_| PeriodProblem::YearOutOfRange {
            range: range.clone(),
            year: value,
        });
    }
    Err(unknown_word(token, range))
}

/// Builds the problem for a token that is neither a month, a season, nor a year.
fn unknown_word(token: &str, range: &Range<usize>) -> PeriodProblem {
    let lower = token.to_ascii_lowercase();
    let hint = if is_range_word(&lower) {
        "separate the dates with an en dash: `Aug 2017 – 2020`"
    } else if token.contains('/') || token.bytes().filter(u8::is_ascii_digit).count() == token.len() && token.len() < 4
    {
        "numeric months aren't supported; write the month name, like `Aug 2017`"
    } else if token.ends_with(',') {
        "remove the comma: `Aug 2017`"
    } else if token.bytes().any(|b| b.is_ascii_digit()) {
        "a year has four digits, like `2017`"
    } else {
        "expected a month (`Aug`), a season (`Fall`), or a year (`2017`)"
    };
    token_problem(range.clone(), token, hint)
}

/// Words people write between two dates instead of a dash.
fn is_range_word(word: &str) -> bool {
    ["to", "until", "through", "thru"]
        .iter()
        .any(|w| w.eq_ignore_ascii_case(word))
}

fn token_problem(range: Range<usize>, token: &str, hint: &str) -> PeriodProblem {
    PeriodProblem::Token {
        range,
        message: format!("can't read `{token}` as part of a date"),
        hint: hint.to_string(),
    }
}

fn month(token: &str) -> Option<Month> {
    let word = token.strip_suffix('.').unwrap_or(token);
    MONTHS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(word))
        .map(|(_, month)| *month)
}

/// `Some(true)` for the canonical `Present`, `Some(false)` for an alias, `None` for anything else.
fn present_spelling(token: &str) -> Option<bool> {
    if token == PRESENT {
        Some(true)
    } else if ["present", "current", "now"]
        .iter()
        .any(|alias| alias.eq_ignore_ascii_case(token))
    {
        Some(false)
    } else {
        None
    }
}

fn join_tokens(tokens: &[Token<'_>]) -> String {
    tokens.iter().map(|(token, _)| *token).collect::<Vec<_>>().join(" ")
}

/// Whitespace-separated tokens of `text[range]`, with their ranges in `text`.
fn tokens(text: &str, range: Range<usize>) -> Vec<Token<'_>> {
    let mut found = Vec::new();
    let mut start = None;
    for (offset, c) in text[range.clone()].char_indices() {
        let at = range.start + offset;
        match (c.is_whitespace(), start) {
            (true, Some(from)) => {
                found.push((&text[from..at], from..at));
                start = None;
            }
            (false, None) => start = Some(at),
            _ => {}
        }
    }
    if let Some(from) = start {
        found.push((&text[from..range.end], from..range.end));
    }
    found
}

fn trim_range(text: &str, range: Range<usize>) -> Range<usize> {
    let slice = &text[range.clone()];
    let start = range.start + (slice.len() - slice.trim_start().len());
    let end = range.end - (slice.len() - slice.trim_end().len());
    start..end.max(start)
}

/// Parses the period part of a role or era line and reports located findings.
///
/// `part` is a range into `node.text`. Warnings (a non-canonical separator or Present spelling) carry
/// an Edit and still yield a period; problems yield a finding and `None`.
pub(crate) fn parse_located(
    node: &TextNode,
    part: Range<usize>,
    src: &Source<'_>,
    sink: &mut Sink<'_>,
) -> Option<Period> {
    let offset = part.start;
    let to_source = |range: Range<usize>| node.source_range(range.start + offset..range.end + offset);
    let whole = to_source(0..part.len());
    match parse_text(&node.text[part.clone()]) {
        Ok(syntax) => {
            report_separator(&syntax, &to_source, src, sink);
            report_present_alias(&syntax, &to_source, sink);
            Some(syntax.period)
        }
        Err(problem) => {
            sink.report(problem_finding(&problem, &node.text[part], whole, &to_source));
            None
        }
    }
}

fn report_separator(
    syntax: &PeriodSyntax,
    to_source: &impl Fn(Range<usize>) -> Range<usize>,
    src: &Source<'_>,
    sink: &mut Sink<'_>,
) {
    let Some(range) = syntax.separator.clone().map(to_source) else {
        return;
    };
    let written = src.slice(&range);
    if written == CANONICAL_SEPARATOR {
        return;
    }
    let message = format!(
        "dates are separated by `{}`; use an en dash with a space on each side",
        written.trim()
    );
    let finding = Finding::new(DiagnosticCode::PeriodHyphen, range.clone(), message)
        .hint(format!("write `{}`", syntax.period.text))
        .fix(range, CANONICAL_SEPARATOR);
    sink.report(finding);
}

fn report_present_alias(syntax: &PeriodSyntax, to_source: &impl Fn(Range<usize>) -> Range<usize>, sink: &mut Sink<'_>) {
    let Some(range) = syntax.present_alias.clone().map(to_source) else {
        return;
    };
    let finding = Finding::new(
        DiagnosticCode::PeriodPresentSpelling,
        range.clone(),
        "write `Present` for an ongoing role",
    )
    .hint(format!("write `{}`", syntax.period.text))
    .fix(range, PRESENT);
    sink.report(finding);
}

fn problem_finding(
    problem: &PeriodProblem,
    text: &str,
    whole: Range<usize>,
    to_source: &impl Fn(Range<usize>) -> Range<usize>,
) -> Finding {
    const FORMS: &str = "dates look like `Aug 2017 – Present`, `2015 – 2017`, or `Fall 2019 – Spring 2021`";
    match problem {
        PeriodProblem::NoYear => Finding::new(
            DiagnosticCode::PeriodUnparseable,
            whole,
            format!("no four-digit year in `{}`", text.trim()),
        )
        .hint(FORMS),
        PeriodProblem::TooLong => Finding::new(
            DiagnosticCode::PeriodUnparseable,
            whole,
            "the period is too long to be dates",
        )
        .hint(FORMS),
        PeriodProblem::Token { range, message, hint } => Finding::new(
            DiagnosticCode::PeriodUnparseable,
            to_source(range.clone()),
            message.clone(),
        )
        .hint(hint.clone()),
        PeriodProblem::YearOutOfRange { range, year } => Finding::new(
            DiagnosticCode::PeriodUnparseable,
            to_source(range.clone()),
            format!("year {year} is outside 1950–2100"),
        )
        .hint("years must be between 1950 and 2100"),
        PeriodProblem::EndBeforeStart => Finding::new(
            DiagnosticCode::PeriodEndBeforeStart,
            whole,
            "the period ends before it starts",
        )
        .hint("swap the dates, or check the years"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::{Diagnostic, Severity};
    use claims::{assert_err, assert_ok, assert_some};
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use std::path::Path;

    fn year(y: u16) -> Year {
        assert_ok!(Year::try_new(y))
    }

    fn month_date(y: u16, m: Month) -> PartialDate {
        PartialDate::Month {
            year: year(y),
            month: m,
        }
    }

    /// Parses `text` as if it were all of a verbatim source line.
    fn located(text: &str) -> (Option<Period>, Vec<Diagnostic>) {
        let mut node = TextNode::default();
        node.push(text, 0..text.len(), true);
        let mut sink = Sink::new(Path::new("r.md"));
        let src = Source { text };
        let period = parse_located(&node, 0..text.len(), &src, &mut sink);
        (period, sink.finish())
    }

    #[test]
    fn parses_every_accepted_form() {
        let cases = [
            (
                "Aug 2017 – Present",
                month_date(2017, Month::August),
                PeriodEnd::Present,
            ),
            (
                "August 2017 – Present",
                month_date(2017, Month::August),
                PeriodEnd::Present,
            ),
            (
                "Sept 2017 – 2018",
                month_date(2017, Month::September),
                PeriodEnd::Date(PartialDate::Year { year: year(2018) }),
            ),
            (
                "Sep. 2017 – Dec. 2018",
                month_date(2017, Month::September),
                PeriodEnd::Date(month_date(2018, Month::December)),
            ),
            (
                "2015 – 2017",
                PartialDate::Year { year: year(2015) },
                PeriodEnd::Date(PartialDate::Year { year: year(2017) }),
            ),
        ];
        for (text, start, end) in cases {
            let syntax = assert_ok!(parse_text(text), "{text}");
            assert_eq!((syntax.period.start, syntax.period.end), (start, end), "{text}");
            assert_eq!(syntax.period.text, text, "{text} is canonical");
            assert_eq!(syntax.separator.is_some(), true);
        }
    }

    #[test]
    fn parses_seasons_case_insensitively() {
        let syntax = assert_ok!(parse_text("SPRING 2020 – autumn 2021"));
        assert_eq!(
            syntax.period.start,
            PartialDate::Season {
                year: year(2020),
                season: Season::Spring
            }
        );
        assert_eq!(
            syntax.period.end,
            PeriodEnd::Date(PartialDate::Season {
                year: year(2021),
                season: Season::Fall
            })
        );
        assert_eq!(
            syntax.period.text, "SPRING 2020 – autumn 2021",
            "the author's spelling is kept"
        );
    }

    #[test]
    fn single_date_period_has_equal_start_and_end() {
        let syntax = assert_ok!(parse_text("2019"));
        assert_eq!(syntax.period.end, PeriodEnd::Date(syntax.period.start));
        assert_eq!(syntax.period.text, "2019");
        assert_eq!(syntax.separator, None);
    }

    #[test]
    fn canonical_period_has_no_findings() {
        let (period, diagnostics) = located("Aug 2017 – Present");
        assert_some!(period);
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn hyphen_range_warns_with_edit() {
        let (period, diagnostics) = located("2015 - 2017");
        assert_eq!(assert_some!(period).text, "2015 – 2017");
        let d = &diagnostics[0];
        assert_eq!((d.code, d.severity), (DiagnosticCode::PeriodHyphen, Severity::Warning));
        assert_eq!(assert_some!(d.span.as_ref()).range, 4..7);
        let fix = assert_some!(d.fix.as_ref());
        assert_eq!((fix.range.clone(), fix.replacement.as_str()), (4..7, " – "));
        assert_eq!(d.hints, vec!["write `2015 – 2017`"]);
    }

    #[test]
    fn unspaced_and_em_dash_separators_warn_with_edit() {
        for text in ["2015-2017", "2015—2017", "2015 — 2017", "2015–2017", "2015  -  2017"] {
            let (period, diagnostics) = located(text);
            assert_eq!(assert_some!(period).text, "2015 – 2017", "{text}");
            assert_eq!(diagnostics.len(), 1, "{text}");
            let fix = assert_some!(diagnostics[0].fix.as_ref());
            let mut fixed = text.to_string();
            fixed.replace_range(fix.range.clone(), &fix.replacement);
            assert_eq!(fixed, "2015 – 2017", "{text}");
        }
    }

    #[test]
    fn double_hyphen_warns_with_edit() {
        // Smart punctuation turned the source `--` into an en dash; the edit must cover the source bytes.
        let source = "2015 -- 2017";
        let mut node = TextNode::default();
        node.push("2015 ", 0..5, true);
        node.push("–", 5..7, false);
        node.push(" 2017", 7..12, true);
        let mut sink = Sink::new(Path::new("r.md"));
        let src = Source { text: source };
        let period = parse_located(&node, 0..node.text.len(), &src, &mut sink);
        assert_some!(period);
        let diagnostics = sink.finish();
        let fix = assert_some!(diagnostics[0].fix.as_ref());
        assert_eq!((fix.range.clone(), fix.replacement.as_str()), (4..8, " – "));
    }

    #[test]
    fn current_end_warns_with_edit() {
        for (text, token) in [
            ("2020 – Current", "Current"),
            ("2020 – now", "now"),
            ("2020 – PRESENT", "PRESENT"),
        ] {
            let (period, diagnostics) = located(text);
            let period = assert_some!(period);
            assert_eq!(period.end, PeriodEnd::Present, "{text}");
            assert_eq!(period.text, "2020 – Present");
            assert_eq!(diagnostics.len(), 1, "{text}");
            let d = &diagnostics[0];
            assert_eq!(
                (d.code, d.severity),
                (DiagnosticCode::PeriodPresentSpelling, Severity::Warning)
            );
            let fix = assert_some!(d.fix.as_ref());
            assert_eq!(&text[fix.range.clone()], token);
            assert_eq!(fix.replacement, "Present");
        }
    }

    #[test]
    fn end_year_before_start_is_error() {
        let (period, diagnostics) = located("2020 – 2018");
        assert!(period.is_none());
        assert_eq!(diagnostics[0].code, DiagnosticCode::PeriodEndBeforeStart);
        assert_eq!(assert_some!(diagnostics[0].span.as_ref()).range, 0..13);
        assert!(!diagnostics[0].hints.is_empty());
    }

    #[test]
    fn month_start_and_year_end_in_same_year_is_valid() {
        let (period, diagnostics) = located("Mar 2017 – 2017");
        assert_some!(period);
        assert_eq!(diagnostics, vec![]);
    }

    #[test]
    fn year_out_of_range_is_period_unparseable() {
        let (period, diagnostics) = located("1949 – 2017");
        assert!(period.is_none());
        let d = &diagnostics[0];
        assert_eq!(d.code, DiagnosticCode::PeriodUnparseable);
        assert_eq!(d.message, "year 1949 is outside 1950–2100");
        assert_eq!(assert_some!(d.span.as_ref()).range, 0..4);
        let (period, _) = located("2017 – 2101");
        assert!(period.is_none());
    }

    #[test]
    fn non_ascii_digits_are_not_years() {
        assert!(!contains_year("２０２０ – ２０２１"));
        assert!(!contains_year("123"));
        assert!(!contains_year("12345"));
        assert!(contains_year("Aug 2017 – Present"));
        assert_err!(parse_text("２０２０ – ２０２１"));
    }

    #[test]
    fn word_separators_are_unparseable_with_hint() {
        for text in ["2015 to 2017", "Aug 2017 until 2018", "2015 through 2017"] {
            let (period, diagnostics) = located(text);
            assert!(period.is_none(), "{text}");
            assert_eq!(diagnostics[0].code, DiagnosticCode::PeriodUnparseable, "{text}");
            assert!(
                diagnostics[0].hints[0].contains("en dash"),
                "{text}: {:?}",
                diagnostics[0].hints
            );
        }
    }

    #[test]
    fn numeric_month_is_unparseable() {
        for text in ["08/2017 – 2018", "2017-08"] {
            let (period, diagnostics) = located(text);
            assert!(period.is_none(), "{text}");
            assert_eq!(diagnostics[0].code, DiagnosticCode::PeriodUnparseable, "{text}");
        }
        let (_, diagnostics) = located("08/2017 – 2018");
        assert!(diagnostics[0].hints[0].contains("numeric months"));
    }

    #[test]
    fn comma_after_month_is_unparseable_with_hint() {
        let (_, diagnostics) = located("Aug, 2017 – 2018");
        assert_eq!(diagnostics[0].hints, vec!["remove the comma: `Aug 2017`"]);
    }

    #[test]
    fn periods_without_a_year_or_too_long_are_unparseable() {
        let (_, diagnostics) = located("Aug – Present");
        assert_eq!(diagnostics[0].message, "no four-digit year in `Aug – Present`");
        assert_eq!(
            assert_err!(parse_text(&format!("2017 – {}", "x".repeat(80)))),
            PeriodProblem::TooLong
        );
    }

    #[test]
    fn open_ended_ranges_are_unparseable() {
        for text in ["2015 –", "– 2017", "2015 – 2016 – 2017"] {
            let (period, diagnostics) = located(text);
            assert!(period.is_none(), "{text}");
            assert_eq!(diagnostics[0].code, DiagnosticCode::PeriodUnparseable, "{text}");
        }
    }

    #[test]
    fn present_cannot_start_a_period() {
        let (period, _) = located("Present – 2017");
        assert!(period.is_none());
    }

    fn month_word() -> impl Strategy<Value = String> {
        (0..MONTHS.len(), 0..3usize, any::<bool>()).prop_map(|(i, case, dot)| {
            let name = MONTHS[i].0;
            let word = match case {
                0 => name.to_string(),
                1 => name.to_uppercase(),
                _ => name[..1].to_uppercase() + &name[1..],
            };
            if dot { format!("{word}.") } else { word }
        })
    }

    fn date_text() -> impl Strategy<Value = String> {
        let season = prop::sample::select(vec!["Spring", "summer", "FALL", "Autumn", "Winter"]);
        prop_oneof![
            (1950u16..=2100).prop_map(|y| y.to_string()),
            (month_word(), 1950u16..=2100).prop_map(|(m, y)| format!("{m} {y}")),
            (season, 1950u16..=2100).prop_map(|(s, y)| format!("{s} {y}")),
        ]
    }

    fn period_text() -> impl Strategy<Value = String> {
        let separator = prop::sample::select(vec![" – ", " - ", "-", "–", " — ", "  -  ", " -- "]);
        let end = prop_oneof![
            date_text(),
            prop::sample::select(vec!["Present", "present", "Current", "now"]).prop_map(str::to_string),
        ];
        (date_text(), proptest::option::of((separator, end))).prop_map(|(start, rest)| match rest {
            Some((sep, end)) => format!("{start}{sep}{end}"),
            None => start,
        })
    }

    proptest! {
        #[test]
        fn period_text_reparses_identically(text in period_text()) {
            let Ok(first) = parse_text(&text) else { return Ok(()) };
            let second = parse_text(&first.period.text);
            prop_assert!(second.is_ok(), "{} -> {}", text, first.period.text);
            let second = second.unwrap_or_else(|_| unreachable!());
            prop_assert_eq!(second.period.start, first.period.start);
            prop_assert_eq!(second.period.end, first.period.end);
            prop_assert_eq!(&second.period.text, &first.period.text);
            prop_assert_eq!(second.present_alias, None);
            if let Some(range) = second.separator {
                prop_assert_eq!(&second.period.text[range], CANONICAL_SEPARATOR);
            }
        }
    }
}
