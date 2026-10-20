//! The maths of the opportunity analyses, as pure functions. Prices are cents; the ratios and
//! statistics are `f64`, because they are estimates, not money.

use serde::Serialize;
use time::{Date, Duration, OffsetDateTime, PrimitiveDateTime, Weekday};

// ---- valid prices ----

/// How many decimal digits a positive number has.
fn digits(mut n: i64) -> u32 {
    let mut digits = 0;
    while n > 0 {
        digits += 1;
        n /= 10;
    }
    digits
}

/// A valid EVE price has at most four significant digits and at most two decimals; in cents that
/// means every digit after the fourth is zero. This is the size of that last allowed step.
fn price_step(cents: i64) -> i64 {
    10_i64.pow(digits(cents).saturating_sub(4))
}

/// The highest valid price that is not above `cents`.
pub fn round_down_valid(cents: i64) -> i64 {
    if cents <= 0 {
        return 0;
    }
    cents - cents % price_step(cents)
}

/// The highest valid price below `cents`: one tick below it.
pub fn tick_below(cents: i64) -> i64 {
    if cents <= 1 {
        return 0;
    }
    round_down_valid(cents - 1)
}

// ---- fees ----

/// The broker fee and sales tax of one character at the station, as fractions (0.0141 = 1.41%).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Fees {
    pub broker: f64,
    pub tax: f64,
}

/// What a character without known fees counts with.
pub const BASE_FEES: Fees = Fees {
    broker: 0.03,
    tax: 0.075,
};

/// Where the fees an analysis uses come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FeeSource {
    Character(i64),
    /// Nobody is selected.
    Base,
}

/// The margin a flip must earn to break even: (1 + b) / (1 − b − t) − 1.
pub fn min_margin(fees: Fees) -> f64 {
    (1.0 + fees.broker) / (1.0 - fees.broker - fees.tax) - 1.0
}

/// The fees of the preferred character when it is among the characters, otherwise those of the
/// character with the lowest minimum margin. A character whose fees are not known counts with the
/// base rates. The first character wins a tie.
pub fn choose_fees(
    characters: &[(i64, Option<Fees>)],
    preferred: Option<i64>,
) -> (Fees, FeeSource) {
    if let Some((id, fees)) = characters.iter().find(|(id, _)| Some(*id) == preferred) {
        return (fees.unwrap_or(BASE_FEES), FeeSource::Character(*id));
    }
    let mut best: Option<(Fees, i64)> = None;
    for (id, fees) in characters {
        let fees = fees.unwrap_or(BASE_FEES);
        if best.is_none_or(|(chosen, _)| min_margin(fees) < min_margin(chosen)) {
            best = Some((fees, *id));
        }
    }
    match best {
        Some((fees, id)) => (fees, FeeSource::Character(id)),
        None => (BASE_FEES, FeeSource::Base),
    }
}

// ---- cost, revenue, margin, order size ----

fn isk(cents: i64) -> f64 {
    cents as f64 / 100.0
}

/// Cost per unit: c = B × (1 + b), in ISK.
pub fn cost(best_buy_cents: i64, fees: Fees) -> f64 {
    isk(best_buy_cents) * (1.0 + fees.broker)
}

/// Revenue per unit: r = S × (1 − b − t), in ISK.
pub fn revenue(sell_cents: i64, fees: Fees) -> f64 {
    isk(sell_cents) * (1.0 - fees.broker - fees.tax)
}

/// Net margin per unit: r − c, in ISK.
pub fn net_per_unit(best_buy_cents: i64, sell_cents: i64, fees: Fees) -> f64 {
    revenue(sell_cents, fees) - cost(best_buy_cents, fees)
}

/// m = (r − c) / c.
pub fn margin(best_buy_cents: i64, sell_cents: i64, fees: Fees) -> f64 {
    net_per_unit(best_buy_cents, sell_cents, fees) / cost(best_buy_cents, fees)
}

/// Order size: U = ⌊capital per item / c⌋.
pub fn order_size(capital: f64, cost: f64) -> i64 {
    if cost <= 0.0 {
        return 0;
    }
    (capital / cost).floor() as i64
}

// ---- liquidity and holding time ----

/// Market share of one side: 1 / (n + 1), with n the orders within 1% of the side's best price.
pub fn market_share(near_orders: i64) -> f64 {
    1.0 / (near_orders.max(0) as f64 + 1.0)
}

/// ρ: of the units taken from either side, the fraction taken from buy orders. 0.5 when nothing
/// was taken.
pub fn buy_side_ratio(buy_taken: i64, sell_taken: i64) -> f64 {
    let total = buy_taken + sell_taken;
    if total <= 0 {
        return 0.5;
    }
    buy_taken as f64 / total as f64
}

/// Daily log returns needed for a volatility.
pub const MIN_RETURNS: usize = 10;

/// σ: the sample standard deviation of the daily log returns of the average prices (oldest
/// first). `None` with fewer than [`MIN_RETURNS`] returns.
pub fn volatility(averages: &[f64]) -> Option<f64> {
    let returns: Vec<f64> = averages
        .windows(2)
        .filter(|w| w[0] > 0.0 && w[1] > 0.0)
        .map(|w| (w[1] / w[0]).ln())
        .collect();
    if returns.len() < MIN_RETURNS {
        return None;
    }
    let n = returns.len() as f64;
    let mean = returns.iter().sum::<f64>() / n;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (n - 1.0);
    Some(variance.sqrt())
}

/// The longest an order may be held, in days, however calm the price.
pub const MAX_HOLDING_DAYS: f64 = 3.0;

/// What one row needs from the item and the account's settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Inputs {
    pub best_buy_cents: i64,
    /// S: the table's sell price.
    pub sell_cents: i64,
    pub fees: Fees,
    /// Capital per item, in ISK.
    pub capital: f64,
    /// V: the average daily traded volume of the last 7 days.
    pub volume: f64,
    pub rho: f64,
    pub buy_near_orders: i64,
    pub sell_near_orders: i64,
    /// `None` without enough history.
    pub sigma: Option<f64>,
}

/// The shared figures of a row.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Figures {
    pub cost: f64,
    pub revenue: f64,
    pub net_per_unit: f64,
    pub margin: f64,
    pub units: i64,
    pub q_buy: f64,
    pub q_sell: f64,
    pub t_fill: f64,
    pub t_max: f64,
    pub min_volume: f64,
    pub isk_per_day: f64,
}

/// Why an item is in no table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exclusion {
    NotProfitable,
    /// Not even one unit fits the capital.
    TooSmall,
    NotEnoughHistory,
    NoVolume,
    /// T_fill exceeds T_max: the volume is below the minimum daily volume.
    TooSlow,
}

/// The figures of a row for a volatility σ, whether or not the row is excluded. Figures that
/// cannot be reached (nobody takes from a side) are infinite.
pub fn figures(input: &Inputs, sigma: f64) -> Figures {
    let c = cost(input.best_buy_cents, input.fees);
    let r = revenue(input.sell_cents, input.fees);
    let m = (r - c) / c;
    let units = order_size(input.capital, c);
    let share_buy = market_share(input.buy_near_orders);
    let share_sell = market_share(input.sell_near_orders);
    let q_buy = share_buy * input.volume * input.rho;
    let q_sell = share_sell * input.volume * (1.0 - input.rho);
    let u = units as f64;
    let t_fill = u / q_buy + u / q_sell;
    let t_max = MAX_HOLDING_DAYS.min((m / (2.0 * sigma)).powi(2));
    // T_fill = U / V * (1 / (share_buy ρ) + 1 / (share_sell (1 − ρ))): the V where it equals T_max.
    let per_volume = 1.0 / (share_buy * input.rho) + 1.0 / (share_sell * (1.0 - input.rho));
    Figures {
        cost: c,
        revenue: r,
        net_per_unit: r - c,
        margin: m,
        units,
        q_buy,
        q_sell,
        t_fill,
        t_max,
        min_volume: u * per_volume / t_max,
        isk_per_day: (r - c) * u / t_fill,
    }
}

/// The figures of a row, or why the item is excluded from every table.
pub fn evaluate(input: &Inputs) -> Result<Figures, Exclusion> {
    if net_per_unit(input.best_buy_cents, input.sell_cents, input.fees) <= 0.0 {
        return Err(Exclusion::NotProfitable);
    }
    if order_size(input.capital, cost(input.best_buy_cents, input.fees)) < 1 {
        return Err(Exclusion::TooSmall);
    }
    let sigma = input.sigma.ok_or(Exclusion::NotEnoughHistory)?;
    if input.volume <= 0.0 {
        return Err(Exclusion::NoVolume);
    }
    let figures = figures(input, sigma);
    if figures.t_fill.is_nan() || figures.t_fill > figures.t_max {
        return Err(Exclusion::TooSlow);
    }
    Ok(figures)
}

// ---- indicators on the daily averages (oldest first) ----

/// The mean of the last `n` values, or `None` with fewer.
pub fn sma(values: &[f64], n: usize) -> Option<f64> {
    if n == 0 || values.len() < n {
        return None;
    }
    Some(values[values.len() - n..].iter().sum::<f64>() / n as f64)
}

/// Bollinger bands over the last 20 values, and where the latest value sits in them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Bollinger {
    pub sma: f64,
    pub lower: f64,
    pub upper: f64,
    /// %B: 0 on the lower band, 1 on the upper band, below 0 under the lower band.
    pub percent_b: f64,
}

const BOLLINGER_DAYS: usize = 20;

/// SMA 20 ± 2 standard deviations (of the population, as Bollinger defined them) of the last 20
/// values. A flat series has bands of zero width, and its value counts as in the middle.
pub fn bollinger(values: &[f64]) -> Option<Bollinger> {
    let mean = sma(values, BOLLINGER_DAYS)?;
    let window = &values[values.len() - BOLLINGER_DAYS..];
    let deviation =
        (window.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / BOLLINGER_DAYS as f64).sqrt();
    let (lower, upper) = (mean - 2.0 * deviation, mean + 2.0 * deviation);
    let latest = *values.last()?;
    let percent_b = if upper > lower {
        (latest - lower) / (upper - lower)
    } else {
        0.5
    };
    Some(Bollinger {
        sma: mean,
        lower,
        upper,
        percent_b,
    })
}

const RSI_DAYS: usize = 14;

/// Wilder's RSI 14: the first 14 changes seed the average gain and loss, every later change
/// smooths them with a weight of 1/14. `None` with fewer than 14 changes.
pub fn rsi14(values: &[f64]) -> Option<f64> {
    if values.len() <= RSI_DAYS {
        return None;
    }
    let n = RSI_DAYS as f64;
    let changes: Vec<f64> = values.windows(2).map(|w| w[1] - w[0]).collect();
    let gain = |c: f64| c.max(0.0);
    let loss = |c: f64| (-c).max(0.0);
    let mut avg_gain = changes[..RSI_DAYS].iter().map(|&c| gain(c)).sum::<f64>() / n;
    let mut avg_loss = changes[..RSI_DAYS].iter().map(|&c| loss(c)).sum::<f64>() / n;
    for &c in &changes[RSI_DAYS..] {
        avg_gain = (avg_gain * (n - 1.0) + gain(c)) / n;
        avg_loss = (avg_loss * (n - 1.0) + loss(c)) / n;
    }
    let total = avg_gain + avg_loss;
    Some(if total == 0.0 {
        50.0
    } else {
        100.0 * avg_gain / total
    })
}

/// How many days ago the SMA 5 crossed above the SMA 20, when it is above now and was not above
/// on one of the three days before today. `Some(0)` is today. `None` without a fresh crossing,
/// or with fewer than 23 values.
pub fn sma_crossing(values: &[f64]) -> Option<usize> {
    const LOOK_BACK: usize = 3;
    if values.len() < 20 + LOOK_BACK {
        return None;
    }
    let above = |end: usize| match (sma(&values[..end], 5), sma(&values[..end], 20)) {
        (Some(fast), Some(slow)) => fast > slow,
        _ => false,
    };
    if !above(values.len()) {
        return None;
    }
    (1..=LOOK_BACK)
        .find(|k| !above(values.len() - k))
        .map(|k| k - 1)
}

/// Whether the average daily volume of the last 5 days is above that of the last 20.
pub fn volume_rising(volumes: &[f64]) -> bool {
    match (sma(volumes, 5), sma(volumes, 20)) {
        (Some(recent), Some(longer)) => recent > longer,
        _ => false,
    }
}

// ---- recurring spread expansions ----

/// One hourly rollup of an item, with the figures the recurrence logic reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HourPoint {
    /// The start of the hour, UTC.
    pub start: OffsetDateTime,
    pub spread_avg: f64,
    pub spread_max: f64,
    /// The highest best sell of the hour, in cents.
    pub sell_max_cents: i64,
}

/// One daily figure of an item (own data or history, see `station_daily_figures`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DayPoint {
    pub day: Date,
    pub spread: f64,
    /// The day's high, in cents.
    pub high_cents: i64,
}

/// What recurs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum WindowKind {
    /// The same hour on most days.
    HourOfDay { hour: u8 },
    /// The same hour on the same weekday in most weeks.
    WeekdayHour {
        #[serde(serialize_with = "serialize_weekday")]
        weekday: Weekday,
        hour: u8,
    },
    /// The same weekday, from the daily figures alone.
    Weekday {
        #[serde(serialize_with = "serialize_weekday")]
        weekday: Weekday,
    },
}

fn serialize_weekday<S: serde::Serializer>(weekday: &Weekday, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&weekday.to_string())
}

impl WindowKind {
    /// The start of the next occurrence after `now`, UTC. A weekday window starts at midnight.
    pub fn next_start(self, now: OffsetDateTime) -> OffsetDateTime {
        match self {
            Self::HourOfDay { hour } => next_start(now, hour, |_| true),
            Self::WeekdayHour { weekday, hour } => {
                next_start(now, hour, |day| day.weekday() == weekday)
            }
            Self::Weekday { weekday } => next_start(now, 0, |day| day.weekday() == weekday),
        }
    }

    /// How the window is named on the page: `daily 18:00`, `weekly Tue 18:00`, `weekly Tue`.
    pub fn label(self) -> String {
        let short = |weekday: Weekday| weekday.to_string()[..3].to_owned();
        match self {
            Self::HourOfDay { hour } => format!("daily {hour:02}:00"),
            Self::WeekdayHour { weekday, hour } => {
                format!("weekly {} {hour:02}:00", short(weekday))
            }
            Self::Weekday { weekday } => format!("weekly {}", short(weekday)),
        }
    }
}

/// A recurring spread expansion of one item.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Window {
    pub kind: WindowKind,
    /// The highest best sell of each past occurrence in which the expansion happened, in cents,
    /// lowest first.
    pub peaks: Vec<i64>,
    /// The same peaks with the start of their occurrence, oldest first.
    pub occurrences: Vec<(OffsetDateTime, i64)>,
    /// S: one tick below the 25th percentile of the peaks, in cents.
    pub sell_cents: i64,
    /// The start of the next occurrence, UTC.
    pub next_start: OffsetDateTime,
}

/// Own snapshot days needed before hourly patterns are used.
pub const MIN_HOURLY_DAYS: usize = 7;

/// How many UTC days the hourly rollups cover.
pub fn snapshot_days(hours: &[HourPoint]) -> usize {
    let mut days: Vec<Date> = hours.iter().map(|h| h.start.date()).collect();
    days.sort_unstable();
    days.dedup();
    days.len()
}

/// A spread this many times the median counts as an expansion.
const EXPANSION: f64 = 1.5;
/// Days looked at for the hour-of-day pattern, and the share of them that must have expanded.
const HOUR_OF_DAY_DAYS: i64 = 14;
const HOUR_OF_DAY_SHARE: (usize, usize) = (3, 5);
const HOUR_OF_DAY_MIN_DAYS: usize = 7;
/// Weeks looked at for the weekday patterns, and how many of them must have expanded.
const WEEKS: i64 = 4;
const WEEKS_NEEDED: usize = 3;
/// Days whose spread makes the median.
const MEDIAN_DAYS: i64 = 30;

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    Some(if values.len() % 2 == 1 {
        values[mid]
    } else {
        (values[mid - 1] + values[mid]) / 2.0
    })
}

/// The 25th percentile with linear interpolation between ranks, of sorted values.
fn percentile_25(sorted: &[i64]) -> f64 {
    let rank = 0.25 * (sorted.len() - 1) as f64;
    let (low, high) = (rank.floor() as usize, rank.ceil() as usize);
    sorted[low] as f64 + (sorted[high] - sorted[low]) as f64 * (rank - low as f64)
}

/// S for a set of peaks (any order): one tick below their 25th percentile. `None` when no valid
/// price results.
fn sell_price(peaks: &mut [i64]) -> Option<i64> {
    peaks.sort_unstable();
    let sell = tick_below(percentile_25(peaks).ceil() as i64);
    (sell > 0).then_some(sell)
}

/// The first moment after `now` that is `hour:00` UTC on a day for which `day_ok` holds.
fn next_start(now: OffsetDateTime, hour: u8, day_ok: impl Fn(Date) -> bool) -> OffsetDateTime {
    (0..=7)
        .map(|ahead| now.date() + Duration::days(ahead))
        .filter(|day| day_ok(*day))
        .filter_map(|day| day.with_hms(hour, 0, 0).ok())
        .map(PrimitiveDateTime::assume_utc)
        .find(|start| *start > now)
        .expect("every weekday and hour comes up within eight days")
}

/// The most recent `WEEKS` dates of this weekday before today.
fn recent_weekdays(today: Date, weekday: Weekday) -> Vec<Date> {
    (1..=WEEKS * 7)
        .map(|back| today - Duration::days(back))
        .filter(|day| day.weekday() == weekday)
        .collect()
}

const WEEKDAYS: [Weekday; 7] = [
    Weekday::Monday,
    Weekday::Tuesday,
    Weekday::Wednesday,
    Weekday::Thursday,
    Weekday::Friday,
    Weekday::Saturday,
    Weekday::Sunday,
];

/// The recurring spread expansions of an item, the one with the highest S first.
///
/// With at least [`MIN_HOURLY_DAYS`] days of hourly rollups: an hour of day recurs when, on at
/// least 60% of the last 14 days that have data for it (and at least 7), its maximum spread was at
/// least 1.5 × the median hourly spread; a weekday and hour recur when that held in 3 of the last
/// 4 weeks. With fewer days, only weekdays are looked at, from the daily figures: 3 of the last 4
/// weeks with a daily spread of at least 1.5 × the median daily spread. Only complete UTC days
/// before today count.
pub fn recurring_windows(
    hours: &[HourPoint],
    days: &[DayPoint],
    now: OffsetDateTime,
) -> Vec<Window> {
    let mut windows = if snapshot_days(hours) >= MIN_HOURLY_DAYS {
        hourly_windows(hours, now)
    } else {
        weekday_windows(days, now)
    };
    windows.sort_by(|a, b| {
        b.sell_cents
            .cmp(&a.sell_cents)
            .then(a.next_start.cmp(&b.next_start))
    });
    windows
}

fn hourly_windows(hours: &[HourPoint], now: OffsetDateTime) -> Vec<Window> {
    let today = now.date();
    let in_days = |point: &HourPoint, back: i64| {
        let age = (today - point.start.date()).whole_days();
        (1..=back).contains(&age)
    };
    let Some(median) = median(
        hours
            .iter()
            .filter(|h| in_days(h, MEDIAN_DAYS))
            .map(|h| h.spread_avg)
            .collect(),
    ) else {
        return Vec::new();
    };
    let threshold = EXPANSION * median;
    let expanded = |h: &HourPoint| h.spread_max >= threshold;
    let mut windows = Vec::new();

    for hour in 0..24u8 {
        let points: Vec<&HourPoint> = hours
            .iter()
            .filter(|h| h.start.hour() == hour && in_days(h, HOUR_OF_DAY_DAYS))
            .collect();
        let held: Vec<&HourPoint> = points.iter().copied().filter(|h| expanded(h)).collect();
        if points.len() >= HOUR_OF_DAY_MIN_DAYS
            && !held.is_empty()
            && held.len() * HOUR_OF_DAY_SHARE.1 >= points.len() * HOUR_OF_DAY_SHARE.0
            && let Some(window) = window_of(
                WindowKind::HourOfDay { hour },
                held.iter().map(|h| (h.start, h.sell_max_cents)).collect(),
                next_start(now, hour, |_| true),
            )
        {
            windows.push(window);
        }

        for weekday in WEEKDAYS {
            let occurrences = recent_weekdays(today, weekday);
            let held: Vec<(OffsetDateTime, i64)> = occurrences
                .iter()
                .filter_map(|day| {
                    hours
                        .iter()
                        .find(|h| h.start.date() == *day && h.start.hour() == hour)
                })
                .filter(|h| expanded(h))
                .map(|h| (h.start, h.sell_max_cents))
                .collect();
            if held.len() >= WEEKS_NEEDED
                && let Some(window) = window_of(
                    WindowKind::WeekdayHour { weekday, hour },
                    held,
                    next_start(now, hour, |day| day.weekday() == weekday),
                )
            {
                windows.push(window);
            }
        }
    }
    windows
}

fn weekday_windows(days: &[DayPoint], now: OffsetDateTime) -> Vec<Window> {
    let today = now.date();
    let Some(median) = median(
        days.iter()
            .filter(|d| (1..=MEDIAN_DAYS).contains(&(today - d.day).whole_days()))
            .map(|d| d.spread)
            .collect(),
    ) else {
        return Vec::new();
    };
    let threshold = EXPANSION * median;
    WEEKDAYS
        .into_iter()
        .filter_map(|weekday| {
            let held: Vec<(OffsetDateTime, i64)> = recent_weekdays(today, weekday)
                .iter()
                .filter_map(|date| days.iter().find(|d| d.day == *date))
                .filter(|d| d.spread >= threshold)
                .map(|d| (d.day.midnight().assume_utc(), d.high_cents))
                .collect();
            if held.len() < WEEKS_NEEDED {
                return None;
            }
            window_of(
                WindowKind::Weekday { weekday },
                held,
                next_start(now, 0, |day| day.weekday() == weekday),
            )
        })
        .collect()
}

fn window_of(
    kind: WindowKind,
    mut occurrences: Vec<(OffsetDateTime, i64)>,
    next_start: OffsetDateTime,
) -> Option<Window> {
    occurrences.sort_by_key(|(at, _)| *at);
    let mut peaks: Vec<i64> = occurrences.iter().map(|(_, peak)| *peak).collect();
    let sell_cents = sell_price(&mut peaks)?;
    Some(Window {
        kind,
        peaks,
        occurrences,
        sell_cents,
        next_start,
    })
}
