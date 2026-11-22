//! ABOUTME: Validated literal field values, explicit dates, and the resolved letter JSON model.
//! ABOUTME: No unresolved placeholder or annotation node can enter the serialized contract.

use super::Placeholder;
use crate::model::{Inline, Name};
use nutype::nutype;
use serde::{Serialize, Serializer};
use snafu::{ResultExt, Snafu, ensure};
use std::{fmt, str::FromStr};

/// A trimmed, nonempty, single-line literal value of at most 120 Unicode scalars.
#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 120, predicate = |text: &str| {
        !text.chars().any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
    }),
    derive(Debug, Clone, PartialEq, Eq, Hash, Display, AsRef, Serialize, TryFrom)
)]
pub struct FieldValue(String);

/// An explicit four-digit-year ISO calendar date; never supplied by the system clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LetterDate(time::Date);

/// Distinguishes invalid date syntax from invalid calendar components.
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
pub enum LetterDateError {
    #[snafu(display("`{input}` is not a date written YYYY-MM-DD"))]
    Shape { input: String },
    #[snafu(display("`{input}` is not a real calendar date: {source}"))]
    OutOfRange {
        input: String,
        source: time::error::ComponentRange,
    },
}

impl LetterDate {
    /// Returns the explicitly supplied calendar date.
    #[must_use]
    pub const fn date(self) -> time::Date {
        self.0
    }
}

impl FromStr for LetterDate {
    type Err = LetterDateError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let bytes = input.as_bytes();
        ensure!(
            bytes.len() == 10
                && bytes.iter().enumerate().all(|(i, b)| {
                    if i == 4 || i == 7 {
                        *b == b'-'
                    } else {
                        b.is_ascii_digit()
                    }
                }),
            ShapeSnafu { input }
        );
        // Shape validation makes these small decimal components infallible without extra parse errors.
        let decimal = |part: &[u8]| {
            part.iter()
                .fold(0_u16, |value, digit| value * 10 + u16::from(digit - b'0'))
        };
        let year = i32::from(decimal(&bytes[..4]));
        let month = time::Month::try_from(decimal(&bytes[5..7]) as u8).context(OutOfRangeSnafu { input })?;
        let day = decimal(&bytes[8..]) as u8;
        time::Date::from_calendar_date(year, month, day)
            .map(Self)
            .context(OutOfRangeSnafu { input })
    }
}

impl TryFrom<time::Date> for LetterDate {
    type Error = LetterDateError;

    /// A date from a place that already holds a calendar date, such as a spec file; the year must have four digits.
    fn try_from(date: time::Date) -> Result<Self, Self::Error> {
        let text = date.to_string();
        // `time` prints a year outside 0..=9999 with a sign or extra digits, which the letter never accepts.
        text.parse()
    }
}

impl fmt::Display for LetterDate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Serialize for LetterDate {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// Optional facts supplied by the application; missing facts fail only when used by selected text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LetterFields {
    pub company: Option<FieldValue>,
    pub title: Option<FieldValue>,
    pub recipient: Option<FieldValue>,
    pub date: Option<LetterDate>,
}

impl LetterFields {
    pub(super) fn get(&self, key: Placeholder) -> Option<&FieldValue> {
        match key {
            Placeholder::Company => self.company.as_ref(),
            Placeholder::Title => self.title.as_ref(),
            Placeholder::Recipient => self.recipient.as_ref(),
        }
    }
}

/// Version of the separate `/letter.json` contract.
pub const LETTER_SCHEMA_VERSION: u32 = 1;

/// Discriminator for the letter style contract, independent of resume schema 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, strum::Display)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum LetterKind {
    Letter,
}

/// A resolved letter. The producer stamps the schema and emits only existing model inline nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Letter {
    pub schema: u32,
    pub kind: LetterKind,
    pub name: Name,
    pub contact: Vec<Inline>,
    pub date: Option<LetterDate>,
    pub recipient: Option<FieldValue>,
    pub company: Option<FieldValue>,
    pub greeting: Vec<Inline>,
    pub body: Vec<Vec<Inline>>,
    pub closing: Vec<Inline>,
}
