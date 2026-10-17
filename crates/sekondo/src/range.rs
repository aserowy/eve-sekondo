//! The time ranges of the over-time graphs and the buckets they are drawn in, shared by the
//! dashboard and the wealth page.

use time::Duration;

/// The time range of the over-time graphs, as the `range` query parameter names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Range {
    Day,
    #[default]
    Week,
    Month,
    Quarter,
}

impl Range {
    pub const ALL: [Range; 4] = [Range::Day, Range::Week, Range::Month, Range::Quarter];

    /// Never fails: a missing or unknown value is the default.
    pub fn parse(raw: Option<&str>) -> Self {
        Self::ALL
            .into_iter()
            .find(|range| Some(range.name()) == raw)
            .unwrap_or_default()
    }

    pub fn name(self) -> &'static str {
        match self {
            Range::Day => "24h",
            Range::Week => "7d",
            Range::Month => "30d",
            Range::Quarter => "90d",
        }
    }

    pub fn span(self) -> Duration {
        match self {
            Range::Day => Duration::hours(24),
            Range::Week => Duration::days(7),
            Range::Month => Duration::days(30),
            Range::Quarter => Duration::days(90),
        }
    }

    /// The width of one point of the graphs: 48 to 120 points per range.
    pub fn bucket(self) -> Duration {
        match self {
            Range::Day => Duration::minutes(30),
            Range::Week => Duration::hours(2),
            Range::Month => Duration::hours(6),
            Range::Quarter => Duration::days(1),
        }
    }
}

impl Range {
    pub fn label(self) -> &'static str {
        match self {
            Range::Day => "24 hours",
            Range::Week => "7 days",
            Range::Month => "30 days",
            Range::Quarter => "90 days",
        }
    }
}

/// The common table expressions of every over-time query: `ends` is the bucket ends over the range
/// (the last one clamped to `now`) and `chars` the selected characters of the account.
///
/// Parameters: `$1` account, `$2` character IDs, `$3` now, `$4` bucket width and `$5` range span,
/// both in seconds.
pub const BUCKET_CTES: &str =
    "WITH width AS (SELECT make_interval(secs => $4) AS w, make_interval(secs => $5) AS span),
     ends AS (
         SELECT g AS at
         FROM width, generate_series(time_bucket(w, $3 - span) + w, $3, w) AS g
         WHERE g < $3
         UNION ALL
         SELECT $3
     ),
     chars AS (
         SELECT character_id FROM characters
         WHERE account_id = $1 AND character_id = ANY($2)
     )";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_range_value_selects_its_range() {
        assert_eq!(Range::parse(Some("24h")), Range::Day);
        assert_eq!(Range::parse(Some("7d")), Range::Week);
        assert_eq!(Range::parse(Some("30d")), Range::Month);
        assert_eq!(Range::parse(Some("90d")), Range::Quarter);
    }

    #[test]
    fn a_missing_or_unknown_range_falls_back_to_seven_days() {
        assert_eq!(Range::parse(None), Range::Week);
        assert_eq!(Range::parse(Some("1y")), Range::Week);
        assert_eq!(Range::parse(Some("")), Range::Week);
        assert_eq!(Range::parse(Some("7D")), Range::Week);
    }

    #[test]
    fn names_round_trip_through_parse() {
        for range in Range::ALL {
            assert_eq!(Range::parse(Some(range.name())), range);
        }
    }

    #[test]
    fn spans_and_bucket_widths_follow_the_design() {
        let table = [
            (Range::Day, Duration::hours(24), Duration::minutes(30)),
            (Range::Week, Duration::days(7), Duration::hours(2)),
            (Range::Month, Duration::days(30), Duration::hours(6)),
            (Range::Quarter, Duration::days(90), Duration::days(1)),
        ];
        for (range, span, bucket) in table {
            assert_eq!((range.span(), range.bucket()), (span, bucket), "{range:?}");
        }
    }
}
