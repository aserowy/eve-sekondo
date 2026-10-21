//! The maths of the opportunity analyses, as pure functions. Prices are cents; the ratios and
//! statistics are `f64`, because they are estimates, not money.

use std::collections::HashMap;

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

/// The lowest valid price above `cents`: one tick above it.
pub fn tick_above(cents: i64) -> i64 {
    if cents < 0 {
        return 1;
    }
    let step = price_step(cents);
    cents - cents % step + step
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

/// One hourly rollup of an item, with the figures the day flagging reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HourFigures {
    /// The start of the hour, UTC.
    pub start: OffsetDateTime,
    pub spread_max: f64,
    pub best_buy_avg: f64,
    pub best_sell_avg: f64,
    /// The lowest best buy of the hour, in cents.
    pub best_buy_min_cents: i64,
    /// The highest best sell of the hour, in cents.
    pub best_sell_max_cents: i64,
    pub buy_taken: i64,
    pub sell_taken: i64,
}

/// Which side of the book an expanded hour moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub enum Direction {
    /// The best buy fell: a chance to buy far below the normal price.
    Dip,
    /// The best sell rose: a chance to sell far above the normal price.
    Spike,
}

impl Direction {
    /// How the direction is stored.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dip => "dip",
            Self::Spike => "spike",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "dip" => Some(Self::Dip),
            "spike" => Some(Self::Spike),
            _ => None,
        }
    }

    /// How the direction is shown: "Dip" or "Spike".
    pub fn label(self) -> &'static str {
        match self {
            Self::Dip => "Dip",
            Self::Spike => "Spike",
        }
    }
}

/// An hour whose maximum spread was at least twice the median of its day.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExpandedHour {
    pub start: OffsetDateTime,
    pub direction: Direction,
    /// The trough (minimum best buy) or peak (maximum best sell) over the day's median average.
    pub ratio: f64,
    /// The minimum best buy of a dip or the maximum best sell of a spike, in cents.
    pub extreme_cents: i64,
    /// The units taken from the side that moved.
    pub taken: i64,
}

/// What one UTC day of one item looks like.
#[derive(Debug, Clone, PartialEq)]
pub struct DayFlags {
    /// The number of hours with a rollup.
    pub hours: usize,
    /// Rollups in at least [`COVERED_HOURS`] hours.
    pub covered: bool,
    /// Empty for a day that is not covered.
    pub expanded: Vec<ExpandedHour>,
}

/// Hours with a rollup a day needs to count.
pub const COVERED_HOURS: usize = 20;
/// An hour's maximum spread this many times its day's median counts as an expansion.
pub const EXPANSION_FACTOR: f64 = 2.0;

/// Flags the expanded hours of one UTC day.
pub fn flag_day(hours: &[HourFigures]) -> DayFlags {
    let covered = hours.len() >= COVERED_HOURS;
    let mut flags = DayFlags {
        hours: hours.len(),
        covered,
        expanded: Vec::new(),
    };
    if !covered {
        return flags;
    }
    let med = |pick: fn(&HourFigures) -> f64| median(hours.iter().map(pick).collect());
    let (Some(spread), Some(buy), Some(sell)) = (
        med(|h| h.spread_max),
        med(|h| h.best_buy_avg),
        med(|h| h.best_sell_avg),
    ) else {
        return flags;
    };
    for hour in hours
        .iter()
        .filter(|h| h.spread_max >= EXPANSION_FACTOR * spread)
    {
        let trough = hour.best_buy_min_cents as f64 / buy;
        let peak = hour.best_sell_max_cents as f64 / sell;
        // relative moves: the drop below the median buy against the rise above the median sell
        flags.expanded.push(if 1.0 - trough > peak - 1.0 {
            ExpandedHour {
                start: hour.start,
                direction: Direction::Dip,
                ratio: trough,
                extreme_cents: hour.best_buy_min_cents,
                taken: hour.buy_taken,
            }
        } else {
            ExpandedHour {
                start: hour.start,
                direction: Direction::Spike,
                ratio: peak,
                extreme_cents: hour.best_sell_max_cents,
                taken: hour.sell_taken,
            }
        });
    }
    flags
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
}

/// The weekday with this number of days from Monday.
pub fn weekday_from_monday(n: i16) -> Option<Weekday> {
    [
        Weekday::Monday,
        Weekday::Tuesday,
        Weekday::Wednesday,
        Weekday::Thursday,
        Weekday::Friday,
        Weekday::Saturday,
        Weekday::Sunday,
    ]
    .get(usize::try_from(n).ok()?)
    .copied()
}

fn serialize_weekday<S: serde::Serializer>(weekday: &Weekday, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&weekday.to_string())
}

impl WindowKind {
    /// The start of the next occurrence after `now`, UTC.
    pub fn next_start(self, now: OffsetDateTime) -> OffsetDateTime {
        match self {
            Self::HourOfDay { hour } => next_start(now, hour, |_| true),
            Self::WeekdayHour { weekday, hour } => {
                next_start(now, hour, |day| day.weekday() == weekday)
            }
        }
    }

    /// How the window is named on the page: `daily 18:00`, `weekly Tue 18:00`.
    pub fn label(self) -> String {
        let short = |weekday: Weekday| weekday.to_string()[..3].to_owned();
        match self {
            Self::HourOfDay { hour } => format!("daily {hour:02}:00"),
            Self::WeekdayHour { weekday, hour } => {
                format!("weekly {} {hour:02}:00", short(weekday))
            }
        }
    }
}

/// A recurring spread expansion of one item.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Window {
    pub kind: WindowKind,
    pub direction: Direction,
    /// S for a spike (one tick below the 25th percentile of the peak ratios x the 7-day median
    /// best sell), B for a dip (one tick above the 75th percentile of the trough ratios x the
    /// 7-day median best buy), in cents.
    pub price_cents: i64,
    /// Every past occurrence with the extreme price of its hour in cents, oldest first.
    pub occurrences: Vec<(OffsetDateTime, i64)>,
    /// The days (hour of day) or weeks (weekday x hour) with data the occurrences are counted
    /// against.
    pub covered: usize,
    /// The units taken from the side that moved, averaged over the occurrences.
    pub v_win: f64,
    /// The start of the next occurrence, UTC.
    pub next_start: OffsetDateTime,
}

/// One UTC day of an item as flagged: its hours with a rollup and how many of them expanded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlaggedDay {
    pub day: Date,
    pub hours: usize,
    pub expanded: usize,
}

/// The medians of the hourly average best buy and best sell over the last 7 days, in cents.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Medians {
    pub buy_cents: f64,
    pub sell_cents: f64,
}

/// How many UTC days the hourly rollups cover.
pub fn snapshot_days(hours: &[HourPoint]) -> usize {
    let mut days: Vec<Date> = hours.iter().map(|h| h.start.date()).collect();
    days.sort_unstable();
    days.dedup();
    days.len()
}

/// Days the quiet rule looks at, and the share of their hours that may have expanded.
const QUIET_DAYS: i64 = 30;
const QUIET_SHARE: (usize, usize) = (1, 10);
/// Days looked at for the hour-of-day pattern, and the least days it needs.
const HOUR_OF_DAY_DAYS: i64 = 28;
const HOUR_OF_DAY_MIN: usize = 7;
/// Days looked at for the weekday pattern (52 weeks), and the least weeks it needs.
const WEEKDAY_DAYS: i64 = 52 * 7;
const WEEKDAY_MIN: usize = 3;
/// Of the last four weeks, so many must have expanded.
const RECENT_DAYS: i64 = 28;
const RECENT_NEEDED: usize = 2;
/// The share of the covered days (or weeks) that must have expanded.
const SHARE: (usize, usize) = (3, 5);

/// The median of the values, the mean of the middle two for an even count.
pub fn median(mut values: Vec<f64>) -> Option<f64> {
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

/// The `q` quantile (0.25 is the 25th percentile) with linear interpolation between ranks, of
/// sorted values.
pub fn percentile(sorted: &[i64], q: f64) -> f64 {
    let rank = q * (sorted.len() - 1) as f64;
    let (low, high) = (rank.floor() as usize, rank.ceil() as usize);
    sorted[low] as f64 + (sorted[high] - sorted[low]) as f64 * (rank - low as f64)
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

/// The recurring windows of an item, the one that starts next first.
///
/// `flagged` holds its flagged days and `expanded` its expanded hours (any order, any age).
/// Only complete UTC days before today count. An item with more than 10% of its hours expanded
/// over the covered days of the last 30 days has none. Dips and spikes are counted apart.
pub fn recurring_windows(
    flagged: &[FlaggedDay],
    expanded: &[ExpandedHour],
    medians: Medians,
    now: OffsetDateTime,
) -> Vec<Window> {
    let today = now.date();
    let age = |day: Date| (today - day).whole_days();
    let covered: Vec<Date> = flagged
        .iter()
        .filter(|f| f.hours >= COVERED_HOURS && (1..=WEEKDAY_DAYS).contains(&age(f.day)))
        .map(|f| f.day)
        .collect();
    let (hours, jumped) = flagged
        .iter()
        .filter(|f| f.hours >= COVERED_HOURS && (1..=QUIET_DAYS).contains(&age(f.day)))
        .fold((0, 0), |(h, e), f| (h + f.hours, e + f.expanded));
    if hours == 0 || jumped * QUIET_SHARE.1 > hours * QUIET_SHARE.0 {
        return Vec::new();
    }

    let days_28 = covered
        .iter()
        .filter(|day| age(**day) <= HOUR_OF_DAY_DAYS)
        .count();
    let mut by_hour: HashMap<(Direction, u8), Vec<&ExpandedHour>> = HashMap::new();
    let mut by_weekday: HashMap<(Direction, Weekday, u8), Vec<&ExpandedHour>> = HashMap::new();
    for hour in expanded
        .iter()
        .filter(|h| covered.contains(&h.start.date()))
    {
        let (day, at) = (hour.start.date(), hour.start.hour());
        if age(day) <= HOUR_OF_DAY_DAYS {
            by_hour.entry((hour.direction, at)).or_default().push(hour);
        }
        by_weekday
            .entry((hour.direction, day.weekday(), at))
            .or_default()
            .push(hour);
    }

    let mut windows = Vec::new();
    for ((direction, hour), held) in &by_hour {
        if held.len() >= HOUR_OF_DAY_MIN && held.len() * SHARE.1 >= days_28 * SHARE.0 {
            windows.extend(window_of(
                WindowKind::HourOfDay { hour: *hour },
                *direction,
                held,
                days_28,
                medians,
                now,
            ));
        }
    }
    for ((direction, weekday, hour), held) in &by_weekday {
        let weeks = covered
            .iter()
            .filter(|day| day.weekday() == *weekday)
            .count();
        let recent = held
            .iter()
            .filter(|h| age(h.start.date()) <= RECENT_DAYS)
            .count();
        if held.len() >= WEEKDAY_MIN
            && held.len() * SHARE.1 >= weeks * SHARE.0
            && recent >= RECENT_NEEDED
        {
            windows.extend(window_of(
                WindowKind::WeekdayHour {
                    weekday: *weekday,
                    hour: *hour,
                },
                *direction,
                held,
                weeks,
                medians,
                now,
            ));
        }
    }
    windows.sort_by_key(|w| (w.next_start, w.direction));
    windows
}

fn window_of(
    kind: WindowKind,
    direction: Direction,
    held: &[&ExpandedHour],
    covered: usize,
    medians: Medians,
    now: OffsetDateTime,
) -> Option<Window> {
    let median = match direction {
        Direction::Spike => medians.sell_cents,
        Direction::Dip => medians.buy_cents,
    };
    if median <= 0.0 {
        return None;
    }
    // Each ratio is turned into today's price first; the percentile of the prices is the price
    // of the percentile of the ratios.
    let mut prices: Vec<i64> = held
        .iter()
        .map(|h| (h.ratio * median).round() as i64)
        .collect();
    prices.sort_unstable();
    let price_cents = match direction {
        Direction::Spike => tick_below(percentile(&prices, 0.25).ceil() as i64),
        Direction::Dip => tick_above(percentile(&prices, 0.75).floor() as i64),
    };
    if price_cents <= 0 {
        return None;
    }
    let mut occurrences: Vec<(OffsetDateTime, i64)> =
        held.iter().map(|h| (h.start, h.extreme_cents)).collect();
    occurrences.sort_by_key(|(at, _)| *at);
    let v_win = held.iter().map(|h| h.taken as f64).sum::<f64>() / held.len() as f64;
    Some(Window {
        kind,
        direction,
        price_cents,
        occurrences,
        covered,
        v_win,
        next_start: kind.next_start(now),
    })
}
