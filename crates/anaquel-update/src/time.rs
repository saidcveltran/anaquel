use std::fmt;

use serde::{Deserialize, Deserializer};

/// A UTC instant written `YYYY-MM-DDTHH:MM:SSZ`.
///
/// The fixed width makes the text order the same as the time order, so two
/// timestamps compare without a date library.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(String);

impl Timestamp {
    /// Reads a timestamp, checking every field including the length of the month.
    ///
    /// # Errors
    /// Returns what is wrong when the text is not a valid `YYYY-MM-DDTHH:MM:SSZ` instant.
    pub fn parse(text: &str) -> Result<Self, String> {
        let bytes = text.as_bytes();
        let shape_ok = bytes.len() == 20
            && bytes[4] == b'-'
            && bytes[7] == b'-'
            && bytes[10] == b'T'
            && bytes[13] == b':'
            && bytes[16] == b':'
            && bytes[19] == b'Z'
            && [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18]
                .iter()
                .all(|&i| bytes[i].is_ascii_digit());
        if !shape_ok {
            return Err(format!("`{text}` is not written YYYY-MM-DDTHH:MM:SSZ"));
        }
        let field = |from: usize, to: usize| text[from..to].parse::<u32>().unwrap_or(u32::MAX);
        let (year, month, day) = (field(0, 4), field(5, 7), field(8, 10));
        let (hour, minute, second) = (field(11, 13), field(14, 16), field(17, 19));
        let valid = (1..=12).contains(&month)
            && (1..=days_in_month(year, month)).contains(&day)
            && hour < 24
            && minute < 60
            && second < 60;
        if valid {
            Ok(Self(text.to_owned()))
        } else {
            Err(format!("`{text}` is not a real date and time"))
        }
    }

    /// The instant `seconds` after 1970-01-01T00:00:00Z, written the way a manifest writes it.
    /// This is pure calendar arithmetic with no date library; the crate still reads no clock,
    /// so the caller says what "now" is.
    ///
    /// # Errors
    /// Returns what is wrong when the instant is beyond the year 9999, which the format cannot
    /// write.
    pub fn from_unix_seconds(seconds: u64) -> Result<Self, String> {
        let days = i64::try_from(seconds / SECONDS_PER_DAY)
            .map_err(|_| format!("{seconds} seconds is too far in the future"))?;
        let rest = seconds % SECONDS_PER_DAY;
        let (hour, minute, second) = (rest / 3600, rest % 3600 / 60, rest % 60);
        let (year, month, day) = civil_from_days(days);
        Self::parse(&format!(
            "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
        ))
    }

    /// The text of the timestamp.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

const SECONDS_PER_DAY: u64 = 86_400;

/// Year, month and day of the day number `days` counted from 1970-01-01, in the proleptic
/// Gregorian calendar (the days-from-civil algorithm of Howard Hinnant).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_real_instants_including_a_leap_day() {
        for ok in [
            "2026-11-19T00:00:00Z",
            "2024-02-29T23:59:59Z",
            "2000-02-29T12:00:00Z",
            "2026-12-31T00:00:00Z",
        ] {
            assert_eq!(Timestamp::parse(ok).unwrap().as_str(), ok);
        }
    }

    #[test]
    fn rejects_what_is_not_an_instant() {
        for bad in [
            "",
            "2026-11-19",
            "2026-11-19 00:00:00Z",
            "2026-11-19T00:00:00",
            "2026-11-19T00:00:00+00:00",
            "2026-13-01T00:00:00Z",
            "2026-00-10T00:00:00Z",
            "2026-02-29T00:00:00Z",
            "1900-02-29T00:00:00Z",
            "2026-04-31T00:00:00Z",
            "2026-11-00T00:00:00Z",
            "2026-11-19T24:00:00Z",
            "2026-11-19T00:60:00Z",
            "2026-11-19T00:00:60Z",
            "2026-1-19T00:00:00Z",
            "２０２６-11-19T00:00:00Z",
        ] {
            assert!(Timestamp::parse(bad).is_err(), "{bad:?} must be refused");
        }
    }

    fn from_unix(seconds: u64) -> String {
        Timestamp::from_unix_seconds(seconds).unwrap().to_string()
    }

    #[test]
    fn known_instants_are_written_as_the_manifest_writes_them() {
        // Each pair was worked out from a source other than this code: the Unix epoch,
        // the billion-second mark, a leap day, the end of a century year that is not a
        // leap year, and the first second of 2026.
        for (seconds, expected) in [
            (0, "1970-01-01T00:00:00Z"),
            (86_399, "1970-01-01T23:59:59Z"),
            (86_400, "1970-01-02T00:00:00Z"),
            (951_782_400, "2000-02-29T00:00:00Z"),
            (1_000_000_000, "2001-09-09T01:46:40Z"),
            (1_709_164_800, "2024-02-29T00:00:00Z"),
            (1_767_225_600, "2026-01-01T00:00:00Z"),
            (4_107_542_399, "2100-02-28T23:59:59Z"),
            (4_107_542_400, "2100-03-01T00:00:00Z"),
        ] {
            assert_eq!(from_unix(seconds), expected, "{seconds}");
        }
    }

    #[test]
    fn consecutive_days_never_skip_or_repeat_across_four_years_of_leap_rules() {
        // Walking day by day for 1461 days (one full leap cycle) from 2024-01-01 must give
        // dates that only go forward, and land on the right day; a wrong month length or
        // leap rule shows up as a repeat or as a wrong last day.
        let start = 1_704_067_200_u64;
        let mut previous = from_unix(start);
        assert_eq!(previous, "2024-01-01T00:00:00Z");
        for day in 1..=1461_u64 {
            let current = from_unix(start + day * 86_400);
            assert!(current > previous, "{current} did not follow {previous}");
            previous = current;
        }
        // 366 + 365 + 365 + 365 days later it is the first day of 2028.
        assert_eq!(previous, "2028-01-01T00:00:00Z");
    }

    #[test]
    fn an_instant_beyond_the_year_9999_is_refused() {
        assert!(Timestamp::from_unix_seconds(253_402_300_800).is_err());
        assert!(Timestamp::from_unix_seconds(u64::MAX).is_err());
        assert_eq!(from_unix(253_402_300_799), "9999-12-31T23:59:59Z");
    }

    #[test]
    fn text_order_is_time_order() {
        let early = Timestamp::parse("2026-11-19T09:00:00Z").unwrap();
        let late = Timestamp::parse("2026-11-19T10:00:00Z").unwrap();
        let next_year = Timestamp::parse("2027-01-01T00:00:00Z").unwrap();
        assert!(early < late && late < next_year);
    }
}
