//! ABOUTME: Resume date and period types: validated years, seasons, partial dates, and periods.
//! ABOUTME: Dates never carry a day and "Present" is a variant, so output stays deterministic.

use nutype::nutype;
use serde::{Serialize, Serializer};
use snafu::prelude::*;
use strum::{AsRefStr, Display, EnumIter, EnumString};

/// A calendar year, 1950 to 2100 inclusive.
#[nutype(
    validate(greater_or_equal = 1950, less_or_equal = 2100),
    derive(
        Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Display, Serialize, TryFrom, Into
    )
)]
pub struct Year(u16);

/// A season used as a date precision; `Autumn` parses as `Fall`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Display, EnumString, EnumIter, AsRefStr)]
#[strum(ascii_case_insensitive)]
#[serde(rename_all = "kebab-case")]
pub enum Season {
    Spring,
    Summer,
    #[strum(serialize = "Fall", serialize = "Autumn")]
    Fall,
    Winter,
}

impl Season {
    /// The first month of the season, used for chronological ordering.
    const fn first_month(self) -> u8 {
        match self {
            Self::Spring => 3,
            Self::Summer => 6,
            Self::Fall => 9,
            Self::Winter => 12,
        }
    }

    /// The last month of the season within its calendar year; Winter stops at December.
    const fn last_month(self) -> u8 {
        match self {
            Self::Spring => 5,
            Self::Summer => 8,
            Self::Fall => 11,
            Self::Winter => 12,
        }
    }
}

fn month_number<S: Serializer>(month: &time::Month, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_u8(u8::from(*month))
}

/// A date with year, month, or season precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "precision", rename_all = "kebab-case")]
pub enum PartialDate {
    Year {
        year: Year,
    },
    Month {
        year: Year,
        #[serde(serialize_with = "month_number")]
        month: time::Month,
    },
    Season {
        year: Year,
        season: Season,
    },
}

impl PartialDate {
    /// Chronological sort key; a season maps to its first month (Spring=3, Summer=6, Fall=9, Winter=12).
    ///
    /// A bare year sorts before any month of that year.
    #[must_use]
    pub fn sort_key(&self) -> (Year, u8) {
        match *self {
            Self::Year { year } => (year, 0),
            Self::Month { year, month } => (year, u8::from(month)),
            Self::Season { year, season } => (year, season.first_month()),
        }
    }

    /// The latest month this date can denote; a bare year is December, a season its last month.
    ///
    /// Use this for the end of a range, so `Mar 2017 – 2017` is not "ending before it starts".
    #[must_use]
    pub fn latest_key(&self) -> (Year, u8) {
        match *self {
            Self::Year { year } => (year, 12),
            Self::Month { year, month } => (year, u8::from(month)),
            Self::Season { year, season } => (year, season.last_month()),
        }
    }
}

/// The end of a period: still ongoing, or a date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "kind", content = "date", rename_all = "kebab-case")]
pub enum PeriodEnd {
    Present,
    Date(PartialDate),
}

/// Error constructing a [`Period`].
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum PeriodError {
    /// The end date falls before the start date.
    #[snafu(display("period ends ({end:?}) before it starts ({start:?})"))]
    EndBeforeStart { start: PartialDate, end: PartialDate },
}

/// A span of time as written on the resume, with its parsed bounds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Period {
    /// The normalized text, e.g. `Aug 2017 – Present`; styles display this.
    pub text: String,
    pub start: PartialDate,
    pub end: PeriodEnd,
}

impl Period {
    /// Builds a period, enforcing that the end is not before the start.
    ///
    /// # Errors
    /// - [`PeriodError::EndBeforeStart`] - When `end` is a date earlier than `start`.
    pub fn try_new(text: impl Into<String>, start: PartialDate, end: PeriodEnd) -> Result<Self, PeriodError> {
        if let PeriodEnd::Date(end_date) = end {
            ensure!(
                end_date.latest_key() >= start.sort_key(),
                EndBeforeStartSnafu { start, end: end_date }
            );
        }
        Ok(Self {
            text: text.into(),
            start,
            end,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claims::{assert_err, assert_ok};
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn year(y: u16) -> Year {
        assert_ok!(Year::try_new(y))
    }

    fn month(y: u16, m: time::Month) -> PartialDate {
        PartialDate::Month {
            year: year(y),
            month: m,
        }
    }

    #[test]
    fn year_rejects_out_of_range() {
        assert_err!(Year::try_new(1949));
        assert_err!(Year::try_new(2101));
        assert_ok!(Year::try_new(1950));
        assert_ok!(Year::try_new(2100));
    }

    #[test]
    fn season_parses_fall_and_autumn() {
        assert_eq!(assert_ok!("Fall".parse::<Season>()), Season::Fall);
        assert_eq!(assert_ok!("autumn".parse::<Season>()), Season::Fall);
        assert_eq!(assert_ok!("SPRING".parse::<Season>()), Season::Spring);
        assert_err!("monsoon".parse::<Season>());
    }

    #[test]
    fn partial_date_sort_key_orders_season_and_month() {
        let y = year(2020);
        let year_only = PartialDate::Year { year: y };
        let spring = PartialDate::Season {
            year: y,
            season: Season::Spring,
        };
        let may = month(2020, time::Month::May);
        let fall = PartialDate::Season {
            year: y,
            season: Season::Fall,
        };
        let sept = month(2020, time::Month::September);
        let winter = PartialDate::Season {
            year: y,
            season: Season::Winter,
        };
        let next = month(2021, time::Month::January);

        let keys = [year_only, spring, may, fall, winter, next].map(|d| d.sort_key());
        assert!(keys.windows(2).all(|w| w[0] < w[1]), "keys must ascend: {keys:?}");
        assert_eq!(fall.sort_key(), sept.sort_key());
    }

    #[test]
    fn period_rejects_end_before_start() {
        let start = month(2020, time::Month::June);
        let end = month(2019, time::Month::June);
        assert_err!(Period::try_new("x", start, PeriodEnd::Date(end)));
        assert_ok!(Period::try_new("x", start, PeriodEnd::Date(start)));
        assert_ok!(Period::try_new("x", start, PeriodEnd::Present));
    }

    #[test]
    fn period_accepts_year_end_in_start_month_year() {
        let start = month(2017, time::Month::March);
        let end = PartialDate::Year { year: year(2017) };
        assert_ok!(Period::try_new("Mar 2017 – 2017", start, PeriodEnd::Date(end)));
        assert_err!(Period::try_new(
            "Mar 2017 – 2016",
            start,
            PeriodEnd::Date(PartialDate::Year { year: year(2016) })
        ));
    }

    #[test]
    fn latest_key_is_the_last_month_a_date_can_denote() {
        let y = year(2020);
        assert_eq!(PartialDate::Year { year: y }.latest_key(), (y, 12));
        assert_eq!(month(2020, time::Month::May).latest_key(), (y, 5));
        let spring = PartialDate::Season {
            year: y,
            season: Season::Spring,
        };
        assert_eq!(spring.latest_key(), (y, 5));
        let fall = PartialDate::Season {
            year: y,
            season: Season::Fall,
        };
        assert_eq!(fall.latest_key(), (y, 11));
    }

    #[test]
    fn period_end_present_serializes_tagged() {
        assert_eq!(
            assert_ok!(serde_json::to_value(PeriodEnd::Present)),
            json!({"kind": "present"})
        );
        let date = PartialDate::Season {
            year: year(2018),
            season: Season::Fall,
        };
        assert_eq!(
            assert_ok!(serde_json::to_value(PeriodEnd::Date(date))),
            json!({"kind": "date", "date": {"precision": "season", "year": 2018, "season": "fall"}})
        );
    }

    #[test]
    fn month_serializes_as_number() {
        assert_eq!(
            assert_ok!(serde_json::to_value(month(2017, time::Month::August))),
            json!({"precision": "month", "year": 2017, "month": 8})
        );
    }
}
