//! The calendar arithmetic the manifest command needs, without a date library: the
//! UTC date and time of a Unix instant, written the way the manifest writes it.

use anaquel_update::Timestamp;

const SECONDS_PER_DAY: u64 = 86_400;

/// The manifest timestamp of `seconds` after 1970-01-01T00:00:00Z.
///
/// # Errors
/// Returns what is wrong when the instant is beyond the year 9999, which the manifest
/// format cannot write.
pub fn timestamp_from_unix(seconds: u64) -> Result<Timestamp, String> {
    let days = i64::try_from(seconds / SECONDS_PER_DAY)
        .map_err(|_| format!("{seconds} seconds is too far in the future"))?;
    let rest = seconds % SECONDS_PER_DAY;
    let (hour, minute, second) = (rest / 3600, rest % 3600 / 60, rest % 60);
    let (year, month, day) = civil_from_days(days);
    Timestamp::parse(&format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
    ))
}

/// Year, month and day of the day number `days` counted from 1970-01-01, in the
/// proleptic Gregorian calendar (the days-from-civil algorithm of Howard Hinnant).
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

#[cfg(test)]
mod tests;
