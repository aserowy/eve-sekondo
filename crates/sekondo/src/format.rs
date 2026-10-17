//! Presentation helpers: ISK amounts, time left and EVE time. Pure functions, so every rule is
//! unit-tested here and the templates only call them.

use time::{Duration, OffsetDateTime};

/// (divisor in cents, suffix). Amounts below 1,000 ISK are shown in full.
const UNITS: [(i128, &str); 4] = [
    (100_000, "k"),
    (100_000_000, "m"),
    (100_000_000_000, "b"),
    (100_000_000_000_000, "t"),
];

/// ISK shortened to at most four significant digits (`1.235b`, `15m`, `950.50`).
pub fn isk_short(cents: i64) -> String {
    let negative = cents < 0;
    let value = i128::from(cents).abs();
    let text = match UNITS.iter().rposition(|(divisor, _)| value >= *divisor) {
        None => format!("{}.{:02}", value / 100, value % 100),
        Some(start) => shorten(value, start),
    };
    if negative { format!("-{text}") } else { text }
}

fn shorten(value: i128, start: usize) -> String {
    for (index, (divisor, suffix)) in UNITS.iter().enumerate().skip(start) {
        let integer_digits = ((value / divisor).max(1)).to_string().len();
        let decimals = 4usize.saturating_sub(integer_digits);
        let scale = 10i128.pow(decimals as u32);
        // Half up, in integers: no floating point near money.
        let rounded = (value * scale * 2 + divisor) / (divisor * 2);
        let overflows = rounded / scale >= 1000;
        if overflows && index + 1 < UNITS.len() {
            continue;
        }
        let whole = rounded / scale;
        let fraction = rounded % scale;
        let mut text = whole.to_string();
        if decimals > 0 && fraction > 0 {
            let digits = format!("{fraction:0decimals$}");
            text.push('.');
            text.push_str(digits.trim_end_matches('0'));
        }
        text.push_str(suffix);
        return text;
    }
    unreachable!("the last unit always returns")
}

/// The exact amount with thousands separators and two decimals: `1,234,567,890.12 ISK`.
pub fn isk_exact(cents: i64) -> String {
    let negative = cents < 0;
    let value = i128::from(cents).abs();
    let grouped = group_thousands(&(value / 100).to_string());
    format!(
        "{}{grouped}.{:02} ISK",
        if negative { "-" } else { "" },
        value % 100
    )
}

/// A whole number with thousands separators: `1,250`.
pub fn thousands(number: i64) -> String {
    let grouped = group_thousands(&i128::from(number).abs().to_string());
    if number < 0 {
        format!("-{grouped}")
    } else {
        grouped
    }
}

/// Puts a comma between every three digits of a string of digits, counted from the right.
fn group_thousands(digits: &str) -> String {
    let mut grouped = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// Whole days and hours until `expires_at`, rounded down: `9 days 22 hours`.
pub fn time_left(expires_at: OffsetDateTime, now: OffsetDateTime) -> String {
    let left = expires_at - now;
    if left <= Duration::ZERO {
        return "expired".to_owned();
    }
    let days = left.whole_days();
    let hours = (left - Duration::days(days)).whole_hours();
    let unit = |n: i64, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    match (days, hours) {
        (0, 0) => "less than 1 hour".to_owned(),
        (0, h) => unit(h, "hour", "hours"),
        (d, 0) => unit(d, "day", "days"),
        (d, h) => format!("{} {}", unit(d, "day", "days"), unit(h, "hour", "hours")),
    }
}

/// `part` as a percentage of `whole` (both in cents), rounded half up to one decimal, without
/// floating point: `12,500` of `1,000,000` ISK is `1.3`.
pub fn percent_of(part_cents: i64, whole_cents: i64) -> String {
    if whole_cents <= 0 {
        return "0.0".to_owned();
    }
    let part = i128::from(part_cents);
    let whole = i128::from(whole_cents);
    // Tenths of a percent: part / whole * 1000, half up.
    let tenths = (part * 1000 * 2 + whole) / (whole * 2);
    format!("{}.{}", tenths / 10, tenths % 10)
}

/// A fraction as a percentage with two decimals: `0.1287` is `12.87%`. Not finite shows a dash.
pub fn percent(fraction: f64) -> String {
    if !fraction.is_finite() {
        return "\u{2014}".to_owned();
    }
    format!("{:.2}%", fraction * 100.0)
}

/// A number of days with one or two decimals, whichever is shorter and still exact enough:
/// `3` days is `3.0 d`, `0.0388` is `0.04 d`. Not finite shows a dash.
pub fn days(days: f64) -> String {
    if !days.is_finite() {
        return "\u{2014}".to_owned();
    }
    if days >= 10.0 {
        format!("{days:.0} d")
    } else if days >= 1.0 {
        format!("{days:.1} d")
    } else {
        format!("{days:.2} d")
    }
}

/// A moment in EVE time (UTC): `2026-10-01 12:00`.
pub fn eve_time(moment: OffsetDateTime) -> String {
    let moment = moment.to_offset(time::UtcOffset::UTC);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        moment.year(),
        u8::from(moment.month()),
        moment.day(),
        moment.hour(),
        moment.minute()
    )
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use super::*;

    #[test]
    fn a_fraction_is_a_percentage_with_two_decimals() {
        assert_eq!(percent(0.1287), "12.87%");
        assert_eq!(percent(0.05), "5.00%");
        assert_eq!(percent(-0.0049), "-0.49%");
        assert_eq!(percent(f64::INFINITY), "\u{2014}");
        assert_eq!(percent(f64::NAN), "\u{2014}");
    }

    #[test]
    fn days_get_fewer_decimals_the_longer_they_are() {
        assert_eq!(days(0.0388), "0.04 d");
        assert_eq!(days(3.0), "3.0 d");
        assert_eq!(days(5.916), "5.9 d");
        assert_eq!(days(12.4), "12 d");
        assert_eq!(days(f64::INFINITY), "\u{2014}");
    }

    /// ISK to cents, for readable cases.
    fn isk(whole: i64, cents: i64) -> i64 {
        whole * 100 + cents
    }

    #[test]
    fn amounts_below_a_thousand_are_shown_in_full() {
        assert_eq!(isk_short(isk(950, 50)), "950.50");
        assert_eq!(isk_short(0), "0.00");
        assert_eq!(isk_short(isk(0, 5)), "0.05");
        assert_eq!(isk_short(isk(999, 99)), "999.99");
    }

    #[test]
    fn larger_amounts_keep_four_significant_digits() {
        assert_eq!(isk_short(isk(1_234_567_890, 12)), "1.235b");
        assert_eq!(isk_short(isk(1_234, 0)), "1.234k");
        assert_eq!(isk_short(isk(12_345, 0)), "12.35k");
        assert_eq!(isk_short(isk(123_456, 0)), "123.5k");
        assert_eq!(isk_short(isk(2_500_000, 0)), "2.5m");
        assert_eq!(isk_short(isk(98_765_432, 0)), "98.77m");
    }

    #[test]
    fn trailing_zeros_are_dropped() {
        assert_eq!(isk_short(isk(1_000, 0)), "1k");
        assert_eq!(isk_short(isk(15_000_000, 0)), "15m");
        assert_eq!(isk_short(isk(1_500_000, 0)), "1.5m");
        assert_eq!(isk_short(isk(1_000_000_000_000, 0)), "1t");
    }

    #[test]
    fn rounding_up_to_the_next_unit_carries() {
        assert_eq!(isk_short(isk(999_999, 99)), "1m");
        assert_eq!(isk_short(isk(999_950, 0)), "1m");
        assert_eq!(isk_short(isk(999_949, 0)), "999.9k");
        assert_eq!(isk_short(isk(999_999_999, 0)), "1b");
    }

    #[test]
    fn the_thresholds_between_units() {
        assert_eq!(isk_short(isk(1_000, 0)), "1k");
        assert_eq!(isk_short(isk(1_000_000, 0)), "1m");
        assert_eq!(isk_short(isk(1_000_000_000, 0)), "1b");
    }

    #[test]
    fn trillions_are_the_largest_unit() {
        assert_eq!(isk_short(isk(1_234_567_890_123, 0)), "1.235t");
        assert_eq!(isk_short(isk(12_345_000_000_000, 0)), "12.35t");
        assert_eq!(isk_short(isk(5_000_000_000_000_000, 0)), "5000t");
    }

    #[test]
    fn exact_amounts_have_separators_and_two_decimals() {
        assert_eq!(isk_exact(isk(1_234_567_890, 12)), "1,234,567,890.12 ISK");
        assert_eq!(isk_exact(isk(950, 50)), "950.50 ISK");
        assert_eq!(isk_exact(isk(1_000, 0)), "1,000.00 ISK");
        assert_eq!(isk_exact(0), "0.00 ISK");
        assert_eq!(isk_exact(isk(15_000_000, 0)), "15,000,000.00 ISK");
        assert_eq!(isk_exact(5), "0.05 ISK");
    }

    #[test]
    fn a_gap_is_a_percentage_of_the_best_price_rounded_half_up_to_one_decimal() {
        assert_eq!(percent_of(isk(12_500, 0), isk(1_000_000, 0)), "1.3");
        assert_eq!(percent_of(isk(5, 0), isk(100, 0)), "5.0");
        // Exactly on a half: 1.25 % is 1.3, 1.24 % is 1.2.
        assert_eq!(percent_of(125, 10_000), "1.3");
        assert_eq!(percent_of(124, 10_000), "1.2");
        assert_eq!(percent_of(isk(100, 0), isk(100, 0)), "100.0");
        assert_eq!(percent_of(isk(250, 0), isk(100, 0)), "250.0");
    }

    #[test]
    fn a_gap_percentage_survives_tiny_and_huge_amounts() {
        // One cent behind a best price of 100 trillion ISK is far below a tenth of a percent.
        assert_eq!(percent_of(1, i64::MAX), "0.0");
        assert_eq!(percent_of(1, 1), "100.0");
        assert_eq!(percent_of(i64::MAX, 1), "922337203685477580700.0");
        assert_eq!(percent_of(i64::MAX / 2, i64::MAX), "50.0");
        // A best price of nothing cannot be a base for a percentage.
        assert_eq!(percent_of(5, 0), "0.0");
    }

    #[test]
    fn whole_numbers_have_thousands_separators() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_250), "1,250");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(-1_234), "-1,234");
        assert_eq!(thousands(i64::MIN), "-9,223,372,036,854,775,808");
    }

    #[test]
    fn time_left_is_whole_days_and_hours_rounded_down() {
        let now = datetime!(2026-10-16 12:00 UTC);
        // Issued 80 days and 2 hours ago with a 90-day duration.
        let issued = now - Duration::days(80) - Duration::hours(2);
        let expires = issued + Duration::days(90);
        assert_eq!(time_left(expires, now), "9 days 22 hours");

        assert_eq!(
            time_left(now + Duration::hours(1) + Duration::minutes(59), now),
            "1 hour"
        );
        assert_eq!(time_left(now + Duration::days(1), now), "1 day");
        assert_eq!(
            time_left(now + Duration::days(2) + Duration::hours(1), now),
            "2 days 1 hour"
        );
        assert_eq!(
            time_left(now + Duration::minutes(59), now),
            "less than 1 hour"
        );
        assert_eq!(time_left(now, now), "expired");
        assert_eq!(time_left(now - Duration::hours(3), now), "expired");
    }

    #[test]
    fn dates_are_shown_in_utc() {
        assert_eq!(
            eve_time(datetime!(2026-10-01 12:05:59 UTC)),
            "2026-10-01 12:05"
        );
        assert_eq!(eve_time(datetime!(2026-10-01 01:30 +2)), "2026-09-30 23:30");
    }
}
