use super::*;

fn text(seconds: u64) -> String {
    timestamp_from_unix(seconds).unwrap().to_string()
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
        assert_eq!(text(seconds), expected, "{seconds}");
    }
}

#[test]
fn consecutive_days_never_skip_or_repeat_across_four_years_of_leap_rules() {
    // Walking day by day for 1461 days (one full leap cycle) from 2024-01-01 must give
    // dates that only go forward, and land on the right day; a wrong month length or
    // leap rule shows up as a repeat or as a wrong last day.
    let start = 1_704_067_200_u64;
    let mut previous = text(start);
    assert_eq!(previous, "2024-01-01T00:00:00Z");
    for day in 1..=1461_u64 {
        let current = text(start + day * 86_400);
        assert!(current > previous, "{current} did not follow {previous}");
        previous = current;
    }
    // 366 + 365 + 365 + 365 days later it is the first day of 2028.
    assert_eq!(previous, "2028-01-01T00:00:00Z");
}

#[test]
fn an_instant_beyond_the_year_9999_is_refused() {
    assert!(timestamp_from_unix(253_402_300_800).is_err());
    assert!(timestamp_from_unix(u64::MAX).is_err());
    assert_eq!(text(253_402_300_799), "9999-12-31T23:59:59Z");
}
