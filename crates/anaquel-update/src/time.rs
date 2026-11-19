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

    /// The text of the timestamp.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
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

    #[test]
    fn text_order_is_time_order() {
        let early = Timestamp::parse("2026-11-19T09:00:00Z").unwrap();
        let late = Timestamp::parse("2026-11-19T10:00:00Z").unwrap();
        let next_year = Timestamp::parse("2027-01-01T00:00:00Z").unwrap();
        assert!(early < late && late < next_year);
    }
}
