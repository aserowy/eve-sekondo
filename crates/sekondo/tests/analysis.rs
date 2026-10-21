//! The pure maths of the station opportunity analyses.

use sekondo::opportunities::analysis::{
    BASE_FEES, Direction, Exclusion, ExpandedHour, FeeSource, Fees, FlaggedDay, HourFigures,
    Inputs, Medians, Window, WindowKind, bollinger, buy_side_ratio, choose_fees, cost, evaluate,
    figures, flag_day, margin, market_share, min_margin, net_per_unit, order_size, percentile,
    recurring_windows, revenue, round_down_valid, rsi14, sma, sma_crossing, tick_above, tick_below,
    volatility, volume_rising,
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
fn one_tick_above_a_round_price_rises_to_the_next_valid_price() {
    // 1,234,500.00 -> 1,235,000.00
    assert_eq!(tick_above(123_450_000), 123_500_000);
    // 820.00 -> 820.10
    assert_eq!(tick_above(82_000), 82_010);
    // 9.99 -> 10.00
    assert_eq!(tick_above(999), 1_000);
}

#[test]
fn a_tick_above_a_price_that_is_not_valid_goes_to_the_valid_price_above() {
    // 150.37 -> 150.40
    assert_eq!(tick_above(15_037), 15_040);
    // 1,234,567.00 -> 1,235,000.00
    assert_eq!(tick_above(123_456_700), 123_500_000);
}

#[test]
fn percentiles_interpolate_linearly_between_ranks() {
    let sorted = [100, 200, 300, 400, 500];
    close(percentile(&sorted, 0.25), 200.0, 1e-9);
    close(percentile(&sorted, 0.75), 400.0, 1e-9);
    let even = [100, 200, 300, 400];
    close(percentile(&even, 0.25), 175.0, 1e-9);
    close(percentile(&even, 0.75), 325.0, 1e-9);
    close(percentile(&[700], 0.25), 700.0, 1e-9);
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

/// The 7-day medians the windows are priced with: best buy 1,000.00 and best sell 10,000.00.
const MEDIANS: Medians = Medians {
    buy_cents: 100_000.0,
    sell_cents: 1_000_000.0,
};

/// What one item's days look like to the recurrence: covered days and their expanded hours.
#[derive(Default)]
struct Item {
    flagged: Vec<FlaggedDay>,
    expanded: Vec<ExpandedHour>,
}

fn midnight(back: i64) -> OffsetDateTime {
    now().replace_time(time::Time::MIDNIGHT) - Duration::days(back)
}

impl Item {
    /// A covered day `back` days ago (1 = yesterday) with 24 hours and no expansion.
    fn day(&mut self, back: i64) {
        self.flagged.push(FlaggedDay {
            day: midnight(back).date(),
            hours: 24,
            expanded: 0,
        });
    }

    /// An expanded hour on a day that `day` made, taking `taken` units from the moving side. The
    /// extreme price is the ratio of a median price of 8,000.00 (sell) or 1,200.00 (buy), a
    /// different level than `MEDIANS`.
    fn expand(&mut self, back: i64, hour: u8, direction: Direction, ratio: f64, taken: i64) {
        let day = midnight(back).date();
        self.flagged
            .iter_mut()
            .find(|f| f.day == day)
            .expect("the day was made")
            .expanded += 1;
        let level = match direction {
            Direction::Spike => 800_000.0,
            Direction::Dip => 120_000.0,
        };
        self.expanded.push(ExpandedHour {
            start: midnight(back) + Duration::hours(i64::from(hour)),
            direction,
            ratio,
            extreme_cents: (ratio * level).round() as i64,
            taken,
        });
    }

    fn windows(&self) -> Vec<Window> {
        recurring_windows(&self.flagged, &self.expanded, MEDIANS, now())
    }
}

/// Covered days 1..=days back.
fn covered(days: i64) -> Item {
    let mut item = Item::default();
    for back in 1..=days {
        item.day(back);
    }
    item
}

#[test]
fn a_daily_evening_spike_is_a_recurring_window_priced_from_its_peak_ratios() {
    // 20 of the last 28 covered days at 18:00; ten of the peaks are these, the rest are higher.
    let mut item = covered(28);
    let peaks = [1.20, 1.22, 1.25, 1.26, 1.28, 1.30, 1.31, 1.33, 1.35, 1.40];
    for back in 1..=20 {
        let ratio = peaks.get(back as usize - 1).copied().unwrap_or(1.5);
        item.expand(back, 18, Direction::Spike, ratio, 100);
    }

    let windows = item.windows();

    // Most weekdays expanded in 3 of the last 4 weeks as well, so their 18:00 are weekly windows
    // too.
    let window = windows
        .iter()
        .find(|w| w.kind == WindowKind::HourOfDay { hour: 18 })
        .expect("the daily window");
    assert_eq!(window.direction, Direction::Spike);
    assert_eq!(window.occurrences.len(), 20);
    assert_eq!(window.covered, 28);
    // p25 of the 20 peaks is 1.295 (between 1.28 and 1.30), x 10,000.00 = 12,950.00; one tick
    // below it is 12,940.00.
    assert_eq!(window.price_cents, 1_294_000);
    assert_eq!(window.next_start, utc(10, 17, 18));
}

#[test]
fn the_next_start_is_tomorrow_once_todays_window_has_begun() {
    let mut item = covered(14);
    for back in 1..=10 {
        item.expand(back, 11, Direction::Spike, 1.3, 1);
    }
    let windows = recurring_windows(&item.flagged, &item.expanded, MEDIANS, now());
    assert_eq!(windows[0].next_start, utc(10, 18, 11));
}

#[test]
fn a_weekly_tuesday_dip_is_priced_one_tick_above_the_p75_of_its_troughs() {
    // 20 weeks whose Tuesday is covered (and no other covered day); the Tuesday 11:00 hour
    // dipped in 14 of them, including 3 of the last 4.
    let mut item = Item::default();
    let tuesdays: Vec<i64> = (4..).step_by(7).take(20).collect(); // Tuesday 13 Oct is 4 days back
    for back in &tuesdays {
        item.day(*back);
    }
    let troughs = [
        0.70, 0.72, 0.74, 0.76, 0.78, 0.79, 0.80, 0.81, 0.815, 0.82, 0.82, 0.83, 0.84, 0.85,
    ];
    let weeks = [0, 1, 2, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];
    for (week, trough) in weeks.iter().zip(troughs) {
        item.expand(tuesdays[*week], 11, Direction::Dip, trough, 40);
    }

    let windows = item.windows();

    assert_eq!(windows.len(), 1, "{windows:?}");
    let window = &windows[0];
    assert_eq!(
        window.kind,
        WindowKind::WeekdayHour {
            weekday: Weekday::Tuesday,
            hour: 11
        }
    );
    assert_eq!(window.direction, Direction::Dip);
    assert_eq!(window.occurrences.len(), 14);
    assert_eq!(window.covered, 20);
    // p75 of the troughs is 0.82, x 1,000.00 = 820.00; one tick above it is 820.10.
    assert_eq!(window.price_cents, 82_010);
    assert_eq!(window.next_start, utc(10, 20, 11));
}

#[test]
fn a_constant_spread_has_no_window() {
    assert_eq!(covered(30).windows(), []);
}

#[test]
fn an_item_that_is_jumpy_all_day_has_no_window() {
    // 4 of 24 hours (16.7%) expanded every day, 18:00 on 20 of the last 28 days.
    let mut item = covered(30);
    for back in 1..=30 {
        for hour in [2, 7, 13, 22] {
            if back > 20 && hour == 22 {
                continue;
            }
            item.expand(back, hour, Direction::Spike, 1.4, 5);
        }
    }
    for back in 1..=20 {
        item.expand(back, 18, Direction::Spike, 1.4, 5);
    }
    assert_eq!(item.windows(), []);
}

#[test]
fn ten_percent_of_the_hours_still_leaves_the_windows() {
    // 18:00 on 20 of 28 days plus 2 other hours on 10 days: 40 of 720 hours is 5.6%.
    let mut item = covered(30);
    for back in 1..=20 {
        item.expand(back, 18, Direction::Spike, 1.4, 5);
    }
    for back in 1..=10 {
        item.expand(back, 4, Direction::Spike, 1.4, 5);
        item.expand(back, 9, Direction::Spike, 1.4, 5);
    }
    assert!(
        item.windows()
            .iter()
            .any(|w| w.kind == WindowKind::HourOfDay { hour: 18 })
    );
}

#[test]
fn an_hour_that_expanded_on_ten_of_28_days_is_too_irregular() {
    let mut item = covered(28);
    for back in 1..=10 {
        item.expand(back, 18, Direction::Spike, 1.4, 5);
    }
    assert_eq!(item.windows(), []);
}

#[test]
fn sixty_percent_of_the_covered_days_is_enough_and_less_is_not() {
    let build = |days_expanded: i64| {
        let mut item = covered(15);
        for back in 1..=days_expanded {
            item.expand(back, 18, Direction::Spike, 1.4, 5);
        }
        item.windows().len()
    };
    assert_eq!(build(9), 1, "9 of 15 is 60%");
    assert_eq!(build(8), 0, "8 of 15 is at least 7 days but under 60%");
}

#[test]
fn an_hour_needs_seven_days_even_when_all_of_them_expanded() {
    let mut item = covered(6);
    for back in 1..=6 {
        item.expand(back, 18, Direction::Spike, 1.4, 5);
    }
    assert_eq!(item.windows(), [], "6 of 6 is 100% but under 7 days");
}

#[test]
fn a_weekday_hour_that_died_out_is_not_recurring() {
    // 12 of the 20 covered Tuesdays dipped, but none of the last 4.
    let mut item = Item::default();
    let tuesdays: Vec<i64> = (4..).step_by(7).take(20).collect();
    for back in &tuesdays {
        item.day(*back);
    }
    for back in tuesdays.iter().skip(4).take(12) {
        item.expand(*back, 11, Direction::Dip, 0.8, 5);
    }
    assert_eq!(item.windows(), []);
}

#[test]
fn two_of_the_last_four_weeks_are_enough_and_one_is_not() {
    let windows = |weeks: &[usize]| {
        let mut item = Item::default();
        let tuesdays: Vec<i64> = (4..).step_by(7).take(8).collect();
        for back in &tuesdays {
            item.day(*back);
        }
        for week in weeks {
            item.expand(tuesdays[*week], 11, Direction::Dip, 0.8, 5);
        }
        item.windows().len()
    };
    // 5 of 8 weeks is above 60%; two of them are among the last four, or only one
    assert_eq!(windows(&[0, 1, 4, 5, 6]), 1);
    assert_eq!(windows(&[0, 4, 5, 6, 7]), 0);
}

#[test]
fn directions_are_separate() {
    let mut item = covered(28);
    for back in 1..=10 {
        item.expand(back, 18, Direction::Dip, 0.8, 5);
    }
    for back in 11..=20 {
        item.expand(back, 18, Direction::Spike, 1.3, 5);
    }
    assert_eq!(item.windows(), []);
}

#[test]
fn a_dip_and_a_spike_window_at_different_hours_are_both_listed_by_next_start() {
    let mut item = covered(14);
    for back in 1..=10 {
        item.expand(back, 18, Direction::Spike, 1.3, 5);
        item.expand(back, 11, Direction::Dip, 0.8, 5);
    }
    let windows = item.windows();
    assert_eq!(windows.len(), 2, "{windows:?}");
    assert_eq!(
        (windows[0].kind, windows[0].direction),
        (WindowKind::HourOfDay { hour: 18 }, Direction::Spike),
        "18:00 starts today, 11:00 tomorrow"
    );
    assert_eq!(
        (windows[1].kind, windows[1].direction),
        (WindowKind::HourOfDay { hour: 11 }, Direction::Dip)
    );
}

#[test]
fn old_peaks_follow_todays_price() {
    // The peaks were recorded at a median of 8,000.00 (extreme 10,000.00 for 1.25); the 7-day
    // median is 10,000.00 now.
    let mut item = Item::default();
    let tuesdays: Vec<i64> = (4..).step_by(7).take(8).collect();
    for back in &tuesdays {
        item.day(*back);
    }
    for week in 0..6 {
        item.expand(tuesdays[week], 18, Direction::Spike, 1.25, 5);
    }
    let windows = item.windows();
    assert_eq!(windows.len(), 1, "{windows:?}");
    assert_eq!(
        windows[0].occurrences[0].1, 1_000_000,
        "the ISK peak of back then"
    );
    // 1.25 x 10,000.00 = 12,500.00; one tick below it is 12,490.00
    assert_eq!(windows[0].price_cents, 1_249_000);
}

#[test]
fn the_explainers_peaks_give_their_example_sell_price() {
    // The peaks of "daily 18:00" on 10 of 14 covered days; their 25th percentile is 1.2525.
    let mut item = covered(14);
    let peaks = [1.20, 1.22, 1.25, 1.26, 1.28, 1.30, 1.31, 1.33, 1.35, 1.40];
    for (back, peak) in (1..=10).zip(peaks) {
        item.expand(back, 18, Direction::Spike, peak, 5);
    }
    let windows = item.windows();
    assert_eq!(windows.len(), 1, "{windows:?}");
    assert_eq!(windows[0].kind.label(), "daily 18:00");
    // 1.2525 x 10,000.00 = 12,525.00; one tick below is 12,520.00
    assert_eq!(windows[0].price_cents, 1_252_000);
}

#[test]
fn the_window_volume_is_the_average_taken_per_occurrence() {
    let mut item = covered(14);
    for back in 1..=10 {
        item.expand(
            back,
            18,
            Direction::Spike,
            1.3,
            if back <= 5 { 100 } else { 300 },
        );
    }
    close(item.windows()[0].v_win, 200.0, 1e-9);
}

#[test]
fn the_first_days_after_the_release_have_no_hour_window() {
    let mut item = covered(5);
    for back in 1..=5 {
        item.expand(back, 18, Direction::Spike, 1.3, 5);
    }
    assert_eq!(item.windows(), []);
}

#[test]
fn without_data_there_are_no_windows() {
    assert_eq!(recurring_windows(&[], &[], MEDIANS, now()), []);
}

#[test]
fn today_does_not_count() {
    // an expansion earlier today on a (partly) covered day is ignored
    let mut item = Item::default();
    item.flagged.push(FlaggedDay {
        day: now().date(),
        hours: 24,
        expanded: 1,
    });
    item.expanded.push(ExpandedHour {
        start: utc(10, 17, 8),
        direction: Direction::Spike,
        ratio: 1.3,
        extreme_cents: 1,
        taken: 1,
    });
    assert_eq!(item.windows(), []);
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
}

// ---- the explainers' worked examples ----

/// The inputs every explainer's worked example shares: b = 1.41%, t = 3.375%, 100,000,000
/// capital per item, V = 80,000, ρ = 0.5, three buy and one sell order within 1%.
fn explainer_example(best_buy_cents: i64, sell_cents: i64, sigma: f64) -> Inputs {
    Inputs {
        best_buy_cents,
        sell_cents,
        fees: Fees {
            broker: 0.0141,
            tax: 0.03375,
        },
        capital: 100_000_000.0,
        volume: 80_000.0,
        rho: 0.5,
        buy_near_orders: 3,
        sell_near_orders: 1,
        sigma: Some(sigma),
    }
}

/// What a worked example prints, in ISK, as fractions and in days.
struct Printed {
    cost: f64,
    revenue: f64,
    net: f64,
    margin: f64,
    units: i64,
    t_fill: f64,
    /// T_max before the cap of 3 days.
    t_max_uncapped: f64,
    t_max: f64,
    min_volume: f64,
    isk_per_day: f64,
    outbid_cents: i64,
    outbid_cost: f64,
    outbid_margin: f64,
}

/// Runs a worked example through the figures and checks every number its card prints, that the
/// row is listed and that it clears the default filters (m ≥ 5%, ISK/d ≥ 5M).
fn check_example(input: Inputs, sigma: f64, printed: Printed) {
    let f = evaluate(&input).expect("the worked example is listed");
    assert_eq!(f, figures(&input, sigma));
    close(f.cost, printed.cost, 0.005);
    close(f.revenue, printed.revenue, 0.005);
    close(f.net_per_unit, printed.net, 0.005);
    close(f.margin, printed.margin, 0.00005);
    assert_eq!(f.units, printed.units);
    close(f.q_buy, 10_000.0, 1e-9);
    close(f.q_sell, 20_000.0, 1e-9);
    close(f.t_fill, printed.t_fill, 0.005);
    close(
        (f.margin / (2.0 * sigma)).powi(2),
        printed.t_max_uncapped,
        0.005,
    );
    close(f.t_max, printed.t_max, 0.005);
    close(f.min_volume, printed.min_volume, 0.5);
    close(f.isk_per_day, printed.isk_per_day, 5_000.0);
    assert!(f.t_fill <= f.t_max);
    assert!(f.margin >= 0.05);
    assert!(f.isk_per_day >= 5_000_000.0);

    // Outbidding: the next valid price above B, then c and m at that price.
    let outbid = printed.outbid_cents;
    assert_eq!(
        round_down_valid(outbid),
        outbid,
        "the outbid price is valid"
    );
    assert_eq!(
        tick_below(outbid),
        input.best_buy_cents,
        "it is one tick above B"
    );
    let outbid_input = Inputs {
        best_buy_cents: outbid,
        ..input
    };
    let o = figures(&outbid_input, sigma);
    close(o.cost, printed.outbid_cost, 0.005);
    close(o.margin, printed.outbid_margin, 0.00005);
}

#[test]
fn the_expansion_example_reads_17_55_percent_and_11_87m_a_day() {
    check_example(
        explainer_example(1_000_000, 1_252_000, 0.02),
        0.02,
        Printed {
            cost: 10_141.00,
            revenue: 11_920.92,
            net: 1_779.92,
            margin: 0.1755,
            units: 9_860,
            t_fill: 1.48,
            t_max_uncapped: 19.25,
            t_max: 3.0,
            min_volume: 39_440.0,
            isk_per_day: 11_870_000.0,
            outbid_cents: 1_001_000,
            outbid_cost: 10_151.14,
            outbid_margin: 0.1743,
        },
    );
}

#[test]
fn the_classic_example_reads_7_88_percent_and_5_33m_a_day() {
    check_example(
        explainer_example(1_000_000, 1_149_000, 0.02),
        0.02,
        Printed {
            cost: 10_141.00,
            revenue: 10_940.20,
            net: 799.20,
            margin: 0.0788,
            units: 9_860,
            t_fill: 1.48,
            t_max_uncapped: 3.88,
            t_max: 3.0,
            min_volume: 39_440.0,
            isk_per_day: 5_330_000.0,
            outbid_cents: 1_001_000,
            outbid_cost: 10_151.14,
            outbid_margin: 0.0777,
        },
    );
}

#[test]
fn the_reversion_example_has_a_t_max_below_the_cap() {
    check_example(
        explainer_example(900_000, 1_053_000, 0.03),
        0.03,
        Printed {
            cost: 9_126.90,
            revenue: 10_026.14,
            net: 899.24,
            margin: 0.0985,
            units: 10_956,
            t_fill: 1.64,
            t_max_uncapped: 2.70,
            t_max: 2.70,
            min_volume: 48_756.0,
            isk_per_day: 5_990_000.0,
            outbid_cents: 900_100,
            outbid_cost: 9_127.91,
            outbid_margin: 0.0984,
        },
    );
}

#[test]
fn the_trend_example_reads_10_70_percent_and_7_23m_a_day() {
    check_example(
        explainer_example(1_000_000, 1_179_000, 0.02),
        0.02,
        Printed {
            cost: 10_141.00,
            revenue: 11_225.85,
            net: 1_084.85,
            margin: 0.1070,
            units: 9_860,
            t_fill: 1.48,
            t_max_uncapped: 7.15,
            t_max: 3.0,
            min_volume: 39_440.0,
            isk_per_day: 7_230_000.0,
            outbid_cents: 1_001_000,
            outbid_cost: 10_151.14,
            outbid_margin: 0.1059,
        },
    );
}

#[test]
fn the_explainers_signals_give_their_examples_sell_prices() {
    // Classic and trend: one tick below the best sell.
    assert_eq!(tick_below(1_150_000), 1_149_000);
    assert_eq!(tick_below(1_180_000), 1_179_000);
    // Mean reversion: the SMA 20 rounded down to a valid price.
    assert_eq!(round_down_valid(1_053_740), 1_053_000);
    // Expansion: see the_explainers_peaks_give_their_example_sell_price.
}

// ---- flagging a day ----

fn day_hours(count: u8) -> Vec<HourFigures> {
    (0..count)
        .map(|hour| HourFigures {
            start: utc(10, 5, hour),
            spread_max: 0.10,
            best_buy_avg: 10_000.0,
            best_sell_avg: 11_000.0,
            best_buy_min_cents: 10_000,
            best_sell_max_cents: 11_000,
            buy_taken: 5,
            sell_taken: 7,
        })
        .collect()
}

#[test]
fn a_constant_spread_has_no_expanded_hour() {
    let flags = flag_day(&day_hours(24));
    assert!(flags.covered);
    assert_eq!(flags.hours, 24);
    assert_eq!(flags.expanded, []);
}

#[test]
fn one_sell_jump_at_18_is_a_spike_with_its_peak_ratio() {
    let mut hours = day_hours(24);
    hours[18].spread_max = 0.25;
    hours[18].best_sell_max_cents = 13_750;
    hours[18].sell_taken = 900;
    let flags = flag_day(&hours);
    assert_eq!(flags.expanded.len(), 1);
    let hour = &flags.expanded[0];
    assert_eq!(hour.start, utc(10, 5, 18));
    assert_eq!(hour.direction, Direction::Spike);
    close(hour.ratio, 1.25, 1e-9);
    assert_eq!(hour.extreme_cents, 13_750);
    assert_eq!(hour.taken, 900);
}

#[test]
fn a_buy_drop_is_a_dip_with_its_trough_ratio() {
    let mut hours = day_hours(24);
    hours[11].spread_max = 0.30;
    hours[11].best_buy_min_cents = 8_200;
    hours[11].buy_taken = 400;
    let flags = flag_day(&hours);
    assert_eq!(flags.expanded.len(), 1);
    let hour = &flags.expanded[0];
    assert_eq!(hour.direction, Direction::Dip);
    close(hour.ratio, 0.82, 1e-9);
    assert_eq!(hour.extreme_cents, 8_200);
    assert_eq!(hour.taken, 400);
}

#[test]
fn equal_moves_are_a_spike() {
    let mut hours = day_hours(24);
    hours[3].spread_max = 0.30;
    hours[3].best_buy_min_cents = 9_000; // -10%
    hours[3].best_sell_max_cents = 12_100; // +10%
    let flags = flag_day(&hours);
    assert_eq!(flags.expanded.len(), 1);
    assert_eq!(flags.expanded[0].direction, Direction::Spike);
}

#[test]
fn an_even_hour_count_uses_the_mean_of_the_middle_two() {
    // 20 hours: ten at 0.125, nine at 0.25 and one at 0.375, so the median is 0.1875 and 0.375 is
    // exactly 2x (all exact in binary)
    let mut hours = day_hours(20);
    for hour in hours.iter_mut() {
        hour.spread_max = 0.125;
    }
    for hour in hours.iter_mut().skip(10) {
        hour.spread_max = 0.25;
    }
    hours[19].spread_max = 0.375;
    hours[19].best_sell_max_cents = 12_000;
    let flags = flag_day(&hours);
    assert_eq!(flags.expanded.len(), 1, "0.375 >= 2 x 0.1875");
    assert_eq!(flags.expanded[0].start, utc(10, 5, 19));
}

#[test]
fn a_day_with_19_hours_is_not_covered() {
    let mut hours = day_hours(19);
    hours[5].spread_max = 0.50;
    hours[5].best_sell_max_cents = 15_000;
    let flags = flag_day(&hours);
    assert!(!flags.covered);
    assert_eq!(flags.hours, 19);
    assert_eq!(flags.expanded, []);
}
