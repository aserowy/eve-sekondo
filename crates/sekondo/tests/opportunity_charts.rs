//! The data of the detail window's charts, as the browser draws it.

use sekondo::opportunities::{
    analysis::{DayPoint, HourPoint, Window, WindowKind},
    charts::{self, Chart, Day, Depth, PricePoint},
};
use time::{Date, Duration, Month, OffsetDateTime, Weekday};

fn date(day: u8) -> Date {
    Date::from_calendar_date(2026, Month::September, day).unwrap()
}

fn midnight(date: Date) -> i64 {
    date.midnight().assume_utc().unix_timestamp() * 1000
}

/// 30 days from 1 September, 100 or 110 ISK alternately, the volume 1000 + the day number.
fn alternating() -> Vec<Day> {
    (0..30)
        .map(|i| Day {
            date: date(1) + Duration::days(i),
            average: if i % 2 == 0 { 100.0 } else { 110.0 },
            volume: 1000.0 + i as f64,
        })
        .collect()
}

fn values(chart: &Chart, series: usize) -> Vec<Option<f64>> {
    chart.series[series].values.clone()
}

fn close(actual: Option<f64>, expected: f64, tolerance: f64) {
    let actual = actual.expect("a value");
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} is not within {tolerance} of {expected}"
    );
}

#[test]
fn the_trend_chart_draws_the_average_and_both_moving_averages_in_cents() {
    let chart = charts::trend_price(&alternating());

    assert_eq!(
        chart.series.iter().map(|s| s.name).collect::<Vec<_>>(),
        ["Average", "SMA 5", "SMA 20"]
    );
    assert_eq!(chart.times.len(), 30);
    assert_eq!(chart.times[0], midnight(date(1)));
    assert_eq!(chart.labels[0], "2026-09-01");
    assert_eq!(values(&chart, 0)[1], Some(11_000.0));
    // The SMA 5 starts on the fifth day, the SMA 20 on the twentieth.
    assert_eq!(
        values(&chart, 1).iter().take_while(|v| v.is_none()).count(),
        4
    );
    assert_eq!(
        values(&chart, 2).iter().take_while(|v| v.is_none()).count(),
        19
    );
    // The last five (110, 100, 110, 100, 110) average 106; the last twenty average 105.
    assert_eq!(values(&chart, 1)[29], Some(10_600.0));
    assert_eq!(values(&chart, 2)[29], Some(10_500.0));
    assert_eq!(chart.series[0].exact[1], "110.00");
}

#[test]
fn the_volume_chart_has_one_bar_per_day() {
    let chart = charts::trend_volume(&alternating());

    assert_eq!(chart.series.len(), 1);
    assert_eq!(chart.series[0].name, "Volume");
    assert_eq!(values(&chart, 0)[0], Some(1000.0));
    assert_eq!(values(&chart, 0)[29], Some(1029.0));
    assert_eq!(chart.series[0].exact[29], "1,029");
    assert_eq!(chart.labels.len(), 30);
}

#[test]
fn the_reversion_chart_adds_the_bollinger_bands_from_the_twentieth_day() {
    // 19 days at 152 and a last day at 119.40: the SMA 20 is 150.37 and the last day is below the lower band.
    let days: Vec<Day> = (0..20)
        .map(|i| Day {
            date: date(1) + Duration::days(i),
            average: if i == 19 { 119.4 } else { 152.0 },
            volume: 1.0,
        })
        .collect();

    let chart = charts::reversion_price(&days);

    assert_eq!(
        chart.series.iter().map(|s| s.name).collect::<Vec<_>>(),
        ["Average", "SMA 20", "Upper band", "Lower band"]
    );
    assert_eq!(values(&chart, 1).iter().filter(|v| v.is_some()).count(), 1);
    close(values(&chart, 1)[19], 15_037.0, 1e-6);
    // Population deviation sqrt(50.48) = 7.1049: bands at 150.37 -+ 14.2098 ISK.
    close(values(&chart, 2)[19], 16_457.98, 0.5);
    close(values(&chart, 3)[19], 13_616.02, 0.5);
    assert!(
        values(&chart, 0)[19].unwrap() < values(&chart, 3)[19].unwrap(),
        "below the lower band"
    );
}

#[test]
fn the_rsi_chart_starts_after_fourteen_changes_and_follows_the_rise() {
    let rising: Vec<Day> = (0..20)
        .map(|i| Day {
            date: date(1) + Duration::days(i),
            average: 100.0 + i as f64,
            volume: 1.0,
        })
        .collect();

    let chart = charts::rsi(&rising);

    assert_eq!(chart.series.len(), 1);
    assert_eq!(chart.series[0].name, "RSI 14");
    assert_eq!(
        values(&chart, 0).iter().take_while(|v| v.is_none()).count(),
        14
    );
    assert_eq!(values(&chart, 0)[14], Some(100.0));
    assert_eq!(values(&chart, 0)[19], Some(100.0));
    assert_eq!(chart.series[0].exact[19], "100.0");
}

#[test]
fn the_classic_charts_draw_the_best_prices_and_the_spread_over_time() {
    let at = |hour: i64| OffsetDateTime::UNIX_EPOCH + Duration::hours(hour);
    let points = [
        PricePoint {
            at: at(0),
            best_buy: 100.0,
            best_sell: 110.0,
            spread: 0.10,
        },
        PricePoint {
            at: at(1),
            best_buy: 100.0,
            best_sell: 120.0,
            spread: 0.20,
        },
    ];

    let prices = charts::classic_prices(&points);
    let spread = charts::classic_spread(&points);

    assert_eq!(
        prices.series.iter().map(|s| s.name).collect::<Vec<_>>(),
        ["Best buy", "Best sell"]
    );
    assert_eq!(values(&prices, 1), [Some(11_000.0), Some(12_000.0)]);
    assert_eq!(prices.times, [0, 3_600_000]);
    assert_eq!(prices.labels[1], "1970-01-01 01:00");
    assert_eq!(spread.series[0].name, "Spread");
    assert_eq!(values(&spread, 0), [Some(10.0), Some(20.0)]);
    assert_eq!(spread.series[0].exact[1], "20.00%");
}

#[test]
fn the_depth_chart_shows_the_units_within_one_percent_of_each_side_with_their_orders() {
    let chart = charts::depth(&Depth {
        buy_units: 800,
        buy_orders: 2,
        sell_units: 200,
        sell_orders: 1,
    });

    assert_eq!(chart.labels, ["Buy (2 orders)", "Sell (1 order)"]);
    assert_eq!(chart.series[0].name, "Units within 1%");
    assert_eq!(values(&chart, 0), [Some(800.0), Some(200.0)]);
    assert_eq!(chart.series[0].exact, ["800", "200"]);
}

#[test]
fn the_spread_by_hour_averages_the_maximum_and_the_average_spread_of_each_hour() {
    let hours: Vec<HourPoint> = (1..=3)
        .flat_map(|day| {
            [
                (0, 0.05, 0.05),
                (18, 0.05, if day == 3 { 0.10 } else { 0.40 }),
            ]
            .into_iter()
            .map(move |(hour, avg, max)| HourPoint {
                start: date(day as u8).with_hms(hour, 0, 0).unwrap().assume_utc(),
                spread_avg: avg,
                spread_max: max,
                sell_max_cents: 10_000,
            })
        })
        .collect();

    let chart = charts::spread_by_hour(&hours);

    assert_eq!(chart.labels.len(), 24);
    assert_eq!(chart.labels[18], "18:00");
    assert_eq!(
        chart.series.iter().map(|s| s.name).collect::<Vec<_>>(),
        ["Average maximum spread", "Average spread"]
    );
    // Hour 18: maxima 40%, 40% and 10% average 30%; the average spread is 5%.
    close(values(&chart, 0)[18], 30.0, 1e-9);
    close(values(&chart, 1)[18], 5.0, 1e-9);
    close(values(&chart, 0)[0], 5.0, 1e-9);
    assert_eq!(values(&chart, 0)[5], None, "no data for that hour");
    assert_eq!(chart.series[0].exact[18], "30.00%");
}

#[test]
fn the_spread_by_weekday_averages_the_daily_spread_of_each_weekday() {
    // 1 September 2026 is a Tuesday.
    let days: Vec<DayPoint> = (0..14)
        .map(|i| DayPoint {
            day: date(1) + Duration::days(i),
            spread: if i % 7 == 0 { 0.20 } else { 0.05 },
            high_cents: 10_000,
        })
        .collect();

    let chart = charts::spread_by_weekday(&days);

    assert_eq!(
        chart.labels,
        ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
    );
    close(values(&chart, 0)[1], 20.0, 1e-9);
    close(values(&chart, 0)[0], 5.0, 1e-9);
}

#[test]
fn the_peaks_chart_marks_the_sell_price_over_every_past_occurrence() {
    let at = |day: u8| date(day).with_hms(18, 0, 0).unwrap().assume_utc();
    let window = Window {
        kind: WindowKind::HourOfDay { hour: 18 },
        peaks: vec![12_000, 13_000, 14_000],
        occurrences: vec![(at(1), 14_000), (at(2), 12_000), (at(3), 13_000)],
        sell_cents: 12_520,
        next_start: at(4),
    };

    let chart = charts::peaks(&window);

    assert_eq!(
        chart.labels,
        ["2026-09-01 18:00", "2026-09-02 18:00", "2026-09-03 18:00"]
    );
    assert_eq!(chart.series[0].name, "Peak");
    assert_eq!(
        values(&chart, 0),
        [Some(14_000.0), Some(12_000.0), Some(13_000.0)]
    );
    assert_eq!(chart.series[1].name, "S");
    assert_eq!(chart.series[1].kind, Some("line"));
    assert_eq!(values(&chart, 1), [Some(12_520.0); 3]);
    assert_eq!(chart.series[1].exact[0], "125.20");
}

#[test]
fn the_next_occurrences_of_a_window_follow_its_recurrence() {
    use sekondo::opportunities::analysis::WindowKind;
    let now = date(1).with_hms(12, 0, 0).unwrap().assume_utc();

    let daily = charts::next_occurrences(WindowKind::HourOfDay { hour: 18 }, now, 3);
    let weekly = charts::next_occurrences(
        WindowKind::WeekdayHour {
            weekday: Weekday::Friday,
            hour: 7,
        },
        now,
        2,
    );

    assert_eq!(
        daily,
        ["2026-09-01 18:00", "2026-09-02 18:00", "2026-09-03 18:00"]
    );
    // 1 September is a Tuesday: Fridays are the 4th and the 11th.
    assert_eq!(weekly, ["2026-09-04 07:00", "2026-09-11 07:00"]);
}

#[test]
fn a_chart_serializes_to_the_json_the_browser_reads() {
    let chart = charts::trend_volume(&alternating());

    let json: serde_json::Value = serde_json::to_value(&chart).unwrap();

    assert_eq!(json["labels"].as_array().unwrap().len(), 30);
    assert_eq!(json["times"].as_array().unwrap().len(), 30);
    assert_eq!(json["series"][0]["name"], "Volume");
    assert_eq!(json["series"][0]["values"][0], 1000.0);
    assert!(json["series"][0].get("kind").is_none() || json["series"][0]["kind"].is_null());
}
