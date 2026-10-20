//! The pure maths of the station opportunity analyses.

use sekondo::opportunities::analysis::{
    BASE_FEES, DayPoint, Exclusion, FeeSource, Fees, HourPoint, Inputs, WindowKind, bollinger,
    buy_side_ratio, choose_fees, cost, evaluate, figures, margin, market_share, min_margin,
    net_per_unit, order_size, recurring_windows, revenue, round_down_valid, rsi14, sma,
    sma_crossing, snapshot_days, tick_below, volatility, volume_rising,
};
use time::{Date, Duration, Month, OffsetDateTime, Weekday};

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} is not within {tolerance} of {expected}"
    );
}

// ---- valid prices ----

#[test]
fn one_tick_below_a_round_million_drops_to_the_next_valid_price() {
    // 1,234,000.00 in cents -> 1,233,000.00
    assert_eq!(tick_below(123_400_000), 123_300_000);
}

#[test]
fn small_prices_tick_by_a_cent() {
    assert_eq!(tick_below(999), 998);
    assert_eq!(tick_below(12_000), 11_990, "120.00 -> 119.90");
}

#[test]
fn a_tick_crosses_a_digit_boundary_downwards() {
    // 100.00 -> 99.99 (four significant digits, two decimals); 10,000.00 -> 9,999.00
    assert_eq!(tick_below(10_000), 9_999);
    assert_eq!(tick_below(1_000_000), 999_900);
    // 1,000,000.00 -> 999,900.00 (four digits kept)
    assert_eq!(tick_below(100_000_000), 99_990_000);
}

#[test]
fn a_tick_below_a_price_that_is_not_valid_goes_to_the_valid_price_below() {
    // 150.37 is not a valid price (five significant digits); one tick below it is 150.30
    assert_eq!(tick_below(15_037), 15_030);
    // an invalid price in the millions: 1,234,567.00 -> 1,234,000.00
    assert_eq!(tick_below(123_456_700), 123_400_000);
}

#[test]
fn nothing_is_below_the_smallest_price() {
    assert_eq!(tick_below(1), 0);
    assert_eq!(tick_below(0), 0);
}

#[test]
fn rounding_down_keeps_valid_prices_and_trims_the_others() {
    assert_eq!(round_down_valid(15_037), 15_030, "150.37 -> 150.30");
    assert_eq!(round_down_valid(15_030), 15_030);
    assert_eq!(round_down_valid(999), 999);
    assert_eq!(round_down_valid(123_456_700), 123_400_000);
    assert_eq!(round_down_valid(0), 0);
}

// ---- fees ----

#[test]
fn the_minimum_margin_is_what_a_flip_must_earn_to_break_even() {
    let fees = Fees {
        broker: 0.0141,
        tax: 0.03375,
    };
    close(min_margin(fees), 0.06506, 0.0001);
    close(
        min_margin(BASE_FEES),
        1.03 / (1.0 - 0.03 - 0.075) - 1.0,
        1e-12,
    );
}

#[test]
fn the_character_with_the_lowest_minimum_margin_gives_the_fees() {
    let cheap = Fees {
        broker: 0.0141,
        tax: 0.03375,
    };
    let dear = Fees {
        broker: 0.0225,
        tax: 0.055,
    };
    assert!(min_margin(cheap) < min_margin(dear));

    let (fees, source) = choose_fees(&[(2, Some(dear)), (1, Some(cheap))], None);

    assert_eq!(fees, cheap);
    assert_eq!(source, FeeSource::Character(1));
}

#[test]
fn a_character_without_known_fees_counts_with_base_rates() {
    // Known fees that are worse than the base rates lose against a character that has none yet.
    let awful = Fees {
        broker: 0.05,
        tax: 0.075,
    };
    let (fees, source) = choose_fees(&[(1, Some(awful)), (2, None)], None);
    assert_eq!(fees, BASE_FEES);
    assert_eq!(source, FeeSource::Character(2));
}

#[test]
fn nobody_selected_or_nobody_known_means_base_rates() {
    assert_eq!(choose_fees(&[], None), (BASE_FEES, FeeSource::Base));
    assert_eq!(
        choose_fees(&[(1, None)], None),
        (BASE_FEES, FeeSource::Character(1))
    );
}

#[test]
fn the_preferred_character_wins_over_a_better_one() {
    let cheap = Fees {
        broker: 0.0141,
        tax: 0.03375,
    };
    let dear = Fees {
        broker: 0.03,
        tax: 0.075,
    };

    let (fees, source) = choose_fees(&[(1, Some(cheap)), (2, Some(dear))], Some(2));

    assert_eq!(fees, dear);
    assert_eq!(source, FeeSource::Character(2));
}

#[test]
fn a_preferred_character_with_unknown_fees_gets_base_rates() {
    let cheap = Fees {
        broker: 0.0141,
        tax: 0.03375,
    };

    let (fees, source) = choose_fees(&[(1, Some(cheap)), (2, None)], Some(2));

    assert_eq!(fees, BASE_FEES);
    assert_eq!(source, FeeSource::Character(2));
}

#[test]
fn a_preferred_character_that_is_not_selected_falls_back_to_the_best_one() {
    let cheap = Fees {
        broker: 0.0141,
        tax: 0.03375,
    };
    let dear = Fees {
        broker: 0.03,
        tax: 0.075,
    };

    let (fees, source) = choose_fees(&[(2, Some(dear)), (1, Some(cheap))], Some(9));

    assert_eq!(fees, cheap);
    assert_eq!(source, FeeSource::Character(1));
}

#[test]
fn a_preferred_character_with_nobody_selected_means_base_rates() {
    assert_eq!(choose_fees(&[], Some(1)), (BASE_FEES, FeeSource::Base));
}

// ---- cost, revenue, margin, order size ----

#[test]
fn the_shared_figures_follow_the_worked_example() {
    let fees = Fees {
        broker: 0.0141,
        tax: 0.03375,
    };
    let c = cost(10_000, fees);
    let r = revenue(11_500, fees);
    close(c, 101.41, 1e-9);
    close(r, 109.49725, 1e-9);
    close(net_per_unit(10_000, 11_500, fees), 8.08725, 1e-9);
    close(margin(10_000, 11_500, fees), 0.07975, 0.0001);
    assert_eq!(order_size(1_000_000.0, c), 9_860);
}

#[test]
fn an_order_size_is_zero_when_one_unit_costs_more_than_the_capital() {
    assert_eq!(order_size(50.0, 101.41), 0);
}

// ---- liquidity and holding time ----

/// The worked example of the spec: b = 1.41%, t = 3.375%, B = 100.00, S = 115.00, 1,000,000
/// capital per item, V = 20,000, ρ = 0.5, three buy and one sell order within 1%, σ = 2%.
fn example() -> Inputs {
    Inputs {
        best_buy_cents: 10_000,
        sell_cents: 11_500,
        fees: Fees {
            broker: 0.0141,
            tax: 0.03375,
        },
        capital: 1_000_000.0,
        volume: 20_000.0,
        rho: 0.5,
        buy_near_orders: 3,
        sell_near_orders: 1,
        sigma: Some(0.02),
    }
}

#[test]
fn the_market_share_of_a_side_is_one_over_its_competitors_plus_one() {
    assert_eq!(market_share(3), 0.25);
    assert_eq!(market_share(1), 0.5);
    assert_eq!(market_share(0), 1.0);
}

#[test]
fn the_buy_side_ratio_is_the_share_of_units_taken_from_buy_orders() {
    close(buy_side_ratio(30, 70), 0.3, 1e-12);
    close(buy_side_ratio(5, 0), 1.0, 1e-12);
}

#[test]
fn the_buy_side_ratio_is_half_when_nothing_was_taken() {
    assert_eq!(buy_side_ratio(0, 0), 0.5);
}

#[test]
fn the_worked_example_takes_almost_six_days_to_fill_but_may_only_wait_three() {
    let f = figures(&example(), 0.02);

    assert_eq!(f.units, 9_860);
    close(f.cost, 101.41, 1e-9);
    close(f.revenue, 109.49725, 1e-9);
    close(f.margin, 0.07975, 0.0001);
    close(f.q_buy, 2_500.0, 1e-9);
    close(f.q_sell, 5_000.0, 1e-9);
    close(f.t_fill, 5.916, 1e-9);
    assert_eq!(f.t_max, 3.0, "(0.0797 / 0.04)^2 is 3.97, capped at 3");
    close(f.min_volume, 39_440.0, 1e-6);
    close(f.isk_per_day, 13_478.75, 0.05);
}

#[test]
fn the_worked_example_is_excluded_because_it_fills_too_slowly() {
    assert_eq!(evaluate(&example()), Err(Exclusion::TooSlow));
}

#[test]
fn an_item_is_listed_once_its_volume_reaches_the_minimum_daily_volume() {
    let mut input = example();
    input.volume = 39_440.0 * 1.01;
    let listed = evaluate(&input).unwrap();
    assert!(listed.t_fill <= listed.t_max);

    input.volume = 39_440.0 * 0.99;
    assert_eq!(evaluate(&input), Err(Exclusion::TooSlow));
}

#[test]
fn a_volatile_and_thin_item_is_excluded_despite_its_margin() {
    // 8% margin, filled in about 4 days, but a volatility that only allows about 1.2 days.
    let mut input = example();
    input.volume = 29_580.0;
    input.sigma = Some(0.0365);
    let f = figures(&input, 0.0365);
    close(f.t_fill, 4.0, 0.001);
    close(f.t_max, 1.19, 0.01);

    assert_eq!(evaluate(&input), Err(Exclusion::TooSlow));
}

#[test]
fn an_item_without_enough_history_for_the_volatility_is_excluded() {
    let mut input = example();
    input.sigma = None;
    assert_eq!(evaluate(&input), Err(Exclusion::NotEnoughHistory));
}

#[test]
fn volatility_needs_ten_daily_returns() {
    let prices = |n: usize| -> Vec<f64> {
        (0..n)
            .map(|i| if i % 2 == 0 { 100.0 } else { 110.0 })
            .collect()
    };
    assert_eq!(volatility(&prices(10)), None, "nine returns");
    assert!(volatility(&prices(11)).is_some(), "ten returns");
    assert_eq!(volatility(&prices(6)), None);
}

#[test]
fn volatility_is_the_sample_deviation_of_the_log_returns() {
    // Alternating 100 and 110: ten returns of +-ln(1.1), mean 0, so s = ln(1.1) * sqrt(10 / 9).
    let prices: Vec<f64> = (0..11)
        .map(|i| if i % 2 == 0 { 100.0 } else { 110.0 })
        .collect();
    close(volatility(&prices).unwrap(), 0.100466, 1e-5);

    // A steady 1% a day has no deviation at all.
    let steady: Vec<f64> = (0..11).map(|i| 100.0 * 1.01_f64.powi(i)).collect();
    close(volatility(&steady).unwrap(), 0.0, 1e-12);
}

#[test]
fn other_exclusions_apply_before_the_fill_time() {
    let mut input = example();
    input.sell_cents = 10_000;
    assert_eq!(evaluate(&input), Err(Exclusion::NotProfitable), "no margin");

    let mut input = example();
    input.capital = 50.0;
    assert_eq!(evaluate(&input), Err(Exclusion::TooSmall), "not one unit");

    let mut input = example();
    input.volume = 0.0;
    assert_eq!(evaluate(&input), Err(Exclusion::NoVolume));
}

#[test]
fn a_steady_price_may_be_held_for_three_days() {
    let mut input = example();
    input.volume = 1_000_000.0;
    input.sigma = Some(0.0);
    let listed = evaluate(&input).unwrap();
    assert_eq!(listed.t_max, 3.0);
}

#[test]
fn a_side_nobody_takes_from_means_it_never_fills() {
    let mut input = example();
    input.rho = 1.0;
    assert_eq!(evaluate(&input), Err(Exclusion::TooSlow));
}

// ---- indicators ----

fn run(first: f64, len: usize) -> Vec<f64> {
    vec![first; len]
}

#[test]
fn a_simple_moving_average_is_the_mean_of_the_last_n_values() {
    let values: Vec<f64> = (1..=20).map(f64::from).collect();
    assert_eq!(sma(&values, 5), Some(18.0));
    assert_eq!(sma(&values, 20), Some(10.5));
    assert_eq!(sma(&values, 21), None);
}

#[test]
fn the_bollinger_bands_are_two_standard_deviations_around_the_sma_20() {
    // 1..=20: mean 10.5, population variance (20^2 - 1) / 12 = 33.25.
    let values: Vec<f64> = (1..=20).map(f64::from).collect();
    let bands = bollinger(&values).unwrap();
    close(bands.sma, 10.5, 1e-12);
    close(bands.lower, 10.5 - 2.0 * 33.25_f64.sqrt(), 1e-9);
    close(bands.upper, 10.5 + 2.0 * 33.25_f64.sqrt(), 1e-9);
    close(bands.percent_b, 0.9119, 0.0001);
    assert_eq!(bollinger(&values[..19]), None, "needs 20 days");
}

#[test]
fn an_average_below_the_lower_band_has_a_negative_percent_b() {
    // 19 days at 100 and a last day at 90: mean 99.5, variance (19 * 0.25 + 90.25) / 20 = 4.75.
    let mut values = run(100.0, 19);
    values.push(90.0);
    let bands = bollinger(&values).unwrap();
    close(bands.lower, 99.5 - 2.0 * 4.75_f64.sqrt(), 1e-9);
    assert!(90.0 < bands.lower);
    close(bands.percent_b, -0.5897, 0.0001);
}

#[test]
fn an_average_inside_the_band_has_a_percent_b_between_zero_and_one() {
    // Ten days at 99 then ten at 101: mean 100, deviation 1, bands 98 and 102, last value 101.
    let mut values = run(99.0, 10);
    values.extend(run(101.0, 10));
    close(bollinger(&values).unwrap().percent_b, 0.75, 1e-9);
}

#[test]
fn a_flat_series_sits_in_the_middle_of_its_zero_width_band() {
    assert_eq!(bollinger(&run(100.0, 20)).unwrap().percent_b, 0.5);
}

#[test]
fn rsi_is_100_for_a_steady_rise_and_0_for_a_steady_fall() {
    let up: Vec<f64> = (1..=15).map(f64::from).collect();
    let down: Vec<f64> = (1..=15).rev().map(f64::from).collect();
    assert_eq!(rsi14(&up), Some(100.0));
    assert_eq!(rsi14(&down), Some(0.0));
    assert_eq!(rsi14(&up[..14]), None, "needs 14 changes");
}

#[test]
fn rsi_of_equal_gains_and_losses_is_50() {
    let mut values = vec![100.0];
    for _ in 0..7 {
        values.extend([101.0, 100.0]);
    }
    assert_eq!(values.len(), 15);
    close(rsi14(&values).unwrap(), 50.0, 1e-9);
}

#[test]
fn rsi_smooths_later_changes_the_way_wilder_does() {
    // 14 alternating changes (average gain and loss 0.5), then one more rise of 1:
    // gain = (0.5 * 13 + 1) / 14, loss = (0.5 * 13) / 14, RSI = gain / (gain + loss) * 100.
    let mut values = vec![100.0];
    for _ in 0..7 {
        values.extend([101.0, 100.0]);
    }
    values.push(101.0);
    close(rsi14(&values).unwrap(), 53.5714, 0.0001);
}

fn rising_after(flat_days: usize, risen_days: usize) -> Vec<f64> {
    let mut values = run(100.0, flat_days);
    values.extend(run(110.0, risen_days));
    values
}

#[test]
fn a_crossing_two_days_ago_is_found() {
    // 23 flat days, then three days at 110: the SMA 5 is above the SMA 20 on the last three days
    // and was not above on the day before them.
    assert_eq!(sma_crossing(&rising_after(23, 3)), Some(2));
}

#[test]
fn a_crossing_yesterday_and_today_are_found() {
    assert_eq!(sma_crossing(&rising_after(24, 2)), Some(1));
    assert_eq!(sma_crossing(&rising_after(25, 1)), Some(0));
}

#[test]
fn a_crossing_older_than_three_days_is_not_fresh() {
    assert_eq!(sma_crossing(&rising_after(21, 5)), None);
}

#[test]
fn no_crossing_while_the_sma_5_is_not_above_the_sma_20() {
    assert_eq!(sma_crossing(&run(100.0, 26)), None, "equal is not above");
    let mut falling = run(110.0, 23);
    falling.extend(run(100.0, 3));
    assert_eq!(sma_crossing(&falling), None);
}

#[test]
fn a_crossing_needs_enough_days_to_look_back_three() {
    assert_eq!(sma_crossing(&rising_after(21, 1)), None, "22 days");
}

#[test]
fn volume_is_rising_when_the_last_five_days_beat_the_last_twenty() {
    // 15 days at 100 and 5 at 200: 200 against (1500 + 1000) / 20 = 125.
    let mut volumes = run(100.0, 15);
    volumes.extend(run(200.0, 5));
    assert!(volume_rising(&volumes));

    let mut falling = run(200.0, 15);
    falling.extend(run(100.0, 5));
    assert!(!volume_rising(&falling));
    assert!(!volume_rising(&run(100.0, 20)), "equal is not rising");
    assert!(!volume_rising(&volumes[..19]), "needs 20 days");
}

// ---- recurring windows ----

fn utc(month: u8, day: u8, hour: u8) -> OffsetDateTime {
    Date::from_calendar_date(2026, Month::try_from(month).unwrap(), day)
        .unwrap()
        .with_hms(hour, 0, 0)
        .unwrap()
        .assume_utc()
}

/// Saturday 17 October 2026, 12:00 UTC.
fn now() -> OffsetDateTime {
    utc(10, 17, 12)
}

/// The best sell of every ordinary hour, in cents.
const FLAT_SELL: i64 = 10_000;

/// Hourly rollups for the `days_back` days before today (1 = yesterday), every hour of each, with
/// a spread of 5%. `expand(days_back, hour)` returns the peak best sell in cents of an hour whose
/// spread jumps to 10%.
fn hours_of(
    days_back: std::ops::RangeInclusive<i64>,
    expand: impl Fn(i64, u8) -> Option<i64>,
) -> Vec<HourPoint> {
    let mut hours = Vec::new();
    for back in days_back {
        for hour in 0..24u8 {
            let start = now().replace_time(time::Time::MIDNIGHT) - Duration::days(back)
                + Duration::hours(i64::from(hour));
            let peak = expand(back, hour);
            hours.push(HourPoint {
                start,
                spread_avg: 0.05,
                spread_max: if peak.is_some() { 0.10 } else { 0.05 },
                sell_max_cents: peak.unwrap_or(FLAT_SELL),
            });
        }
    }
    hours
}

#[test]
fn a_daily_evening_expansion_is_a_recurring_window_with_the_p25_of_its_peaks() {
    // 10 of the last 14 days, at 18:00, with these best sell peaks in ISK.
    let peaks = [120, 122, 125, 126, 128, 130, 131, 133, 135, 140];
    let hours = hours_of(1..=14, |back, hour| {
        (hour == 18 && back <= 10).then(|| peaks[back as usize - 1] * 100)
    });

    let windows = recurring_windows(&hours, &[], now());

    assert_eq!(windows.len(), 1, "{windows:?}");
    let window = &windows[0];
    assert_eq!(window.kind, WindowKind::HourOfDay { hour: 18 });
    assert_eq!(window.peaks.len(), 10);
    // P25 of the peaks is 125.25 (linear interpolation between 125 and 126); one tick below it
    // is 125.20.
    assert_eq!(window.sell_cents, 12_520);
    assert_eq!(window.next_start, utc(10, 17, 18));
}

#[test]
fn the_next_start_is_tomorrow_once_todays_window_has_begun() {
    let hours = hours_of(1..=14, |back, hour| {
        (hour == 18 && back <= 10).then_some(13_000)
    });

    let windows = recurring_windows(&hours, &[], utc(10, 17, 18));

    assert_eq!(windows[0].next_start, utc(10, 18, 18));
}

#[test]
fn an_expansion_on_too_few_days_is_not_recurring() {
    let hours = hours_of(1..=14, |back, hour| {
        (hour == 18 && back <= 5).then_some(13_000)
    });
    assert_eq!(recurring_windows(&hours, &[], now()), []);
}

#[test]
fn sixty_percent_of_the_days_with_data_is_enough_and_less_is_not() {
    let six_of_ten = hours_of(1..=10, |back, hour| {
        (hour == 18 && back <= 6).then_some(13_000)
    });
    assert_eq!(recurring_windows(&six_of_ten, &[], now()).len(), 1);

    let five_of_ten = hours_of(1..=10, |back, hour| {
        (hour == 18 && back <= 5).then_some(13_000)
    });
    assert_eq!(recurring_windows(&five_of_ten, &[], now()), []);
}

#[test]
fn an_hour_needs_data_on_at_least_seven_days() {
    // Eight days of data, but hour 18 is only there on six of them, and always expanded.
    let mut hours = hours_of(1..=8, |back, hour| {
        (hour == 18 && back <= 6).then_some(13_000)
    });
    hours.retain(|h| h.start.hour() != 18 || (now().date() - h.start.date()).whole_days() <= 6);
    assert_eq!(recurring_windows(&hours, &[], now()), []);
}

#[test]
fn a_weekday_and_hour_that_held_three_of_the_last_four_weeks_is_recurring() {
    // Tuesdays before Saturday 17 October: 13 and 6 October, 29 and 22 September, which are 4, 11,
    // 18 and 25 days back. Expanded at 18:00 on three of them.
    let hours = hours_of(1..=28, |back, hour| {
        (hour == 18 && [4, 11, 25].contains(&back)).then_some(13_000 + back * 100)
    });

    let windows = recurring_windows(&hours, &[], now());

    assert_eq!(windows.len(), 1, "{windows:?}");
    assert_eq!(
        windows[0].kind,
        WindowKind::WeekdayHour {
            weekday: Weekday::Tuesday,
            hour: 18
        }
    );
    assert_eq!(windows[0].peaks, [13_400, 14_100, 15_500]);
    assert_eq!(windows[0].next_start, utc(10, 20, 18));
}

#[test]
fn two_of_the_last_four_weeks_are_not_enough() {
    let hours = hours_of(1..=28, |back, hour| {
        (hour == 18 && [4, 11].contains(&back)).then_some(13_000)
    });
    assert_eq!(recurring_windows(&hours, &[], now()), []);
}

fn daily(
    days_back: std::ops::RangeInclusive<i64>,
    expand: impl Fn(i64) -> Option<i64>,
) -> Vec<DayPoint> {
    days_back
        .map(|back| {
            let high = expand(back);
            DayPoint {
                day: now().date() - Duration::days(back),
                spread: if high.is_some() { 0.10 } else { 0.05 },
                high_cents: high.unwrap_or(FLAT_SELL),
            }
        })
        .collect()
}

#[test]
fn with_three_days_of_snapshots_only_weekday_windows_are_listed() {
    // Hour 18 expands on all three snapshot days, but that is no pattern yet.
    let hours = hours_of(1..=3, |_, hour| (hour == 18).then_some(13_000));
    assert_eq!(snapshot_days(&hours), 3);
    // From the daily figures: three of the last four Tuesdays were at least 1.5 times the median.
    let days = daily(1..=28, |back| {
        [4, 18, 25].contains(&back).then_some(12_000 + back * 10)
    });

    let windows = recurring_windows(&hours, &days, now());

    assert_eq!(windows.len(), 1, "{windows:?}");
    assert_eq!(
        windows[0].kind,
        WindowKind::Weekday {
            weekday: Weekday::Tuesday
        }
    );
    assert_eq!(windows[0].peaks, [12_040, 12_180, 12_250]);
    // P25 of 120.40, 121.80, 122.50 is 121.10; one tick below it is 121.00.
    assert_eq!(windows[0].sell_cents, 12_100);
    assert_eq!(windows[0].next_start, utc(10, 20, 0));
}

#[test]
fn the_window_with_the_highest_sell_price_comes_first() {
    let hours = hours_of(1..=14, |back, hour| match hour {
        7 if back <= 10 => Some(20_000),
        18 if back <= 10 => Some(13_000),
        _ => None,
    });

    let windows = recurring_windows(&hours, &[], now());

    assert_eq!(
        windows.iter().map(|w| w.kind).collect::<Vec<_>>(),
        [
            WindowKind::HourOfDay { hour: 7 },
            WindowKind::HourOfDay { hour: 18 }
        ]
    );
    assert!(windows[0].sell_cents > windows[1].sell_cents);
}

#[test]
fn without_data_there_are_no_windows() {
    assert_eq!(recurring_windows(&[], &[], now()), []);
}

// ---- window names and next starts ----

#[test]
fn a_window_is_named_by_its_recurrence() {
    assert_eq!(WindowKind::HourOfDay { hour: 18 }.label(), "daily 18:00");
    assert_eq!(WindowKind::HourOfDay { hour: 7 }.label(), "daily 07:00");
    assert_eq!(
        WindowKind::WeekdayHour {
            weekday: Weekday::Tuesday,
            hour: 18
        }
        .label(),
        "weekly Tue 18:00"
    );
    assert_eq!(
        WindowKind::Weekday {
            weekday: Weekday::Sunday
        }
        .label(),
        "weekly Sun"
    );
}

#[test]
fn the_next_start_of_a_window_is_the_first_occurrence_after_now() {
    // Saturday 17 October 2026, 12:00 UTC.
    assert_eq!(
        WindowKind::HourOfDay { hour: 18 }.next_start(now()),
        utc(10, 17, 18)
    );
    assert_eq!(
        WindowKind::HourOfDay { hour: 12 }.next_start(now()),
        utc(10, 18, 12)
    );
    assert_eq!(
        WindowKind::WeekdayHour {
            weekday: Weekday::Tuesday,
            hour: 18
        }
        .next_start(now()),
        utc(10, 20, 18)
    );
    assert_eq!(
        WindowKind::Weekday {
            weekday: Weekday::Saturday
        }
        .next_start(now()),
        utc(10, 24, 0),
        "today's midnight has passed"
    );
}
