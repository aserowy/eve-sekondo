//! The data of the detail window's charts, in the shape `static/app.js` draws: category labels,
//! epoch milliseconds for time axes, and series of values with the exact text of each value for
//! the tooltip. ISK values are cents (the script shortens them for the axis), spreads are
//! percentages, and a value that does not exist (an average that needs 20 days) is `null`.

use serde::Serialize;
use time::{Date, OffsetDateTime, Weekday};

use super::analysis::{
    DayPoint, Direction, ExpandedHour, HourPoint, WindowKind, bollinger, rsi14, sma,
};
use crate::format;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Series {
    pub name: &'static str,
    pub values: Vec<Option<f64>>,
    /// The text of each value for the tooltip; empty where there is no value.
    pub exact: Vec<String>,
    /// `line` for a line drawn over bars.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Chart {
    pub labels: Vec<String>,
    /// Epoch milliseconds of each label, for charts over time; empty for the others.
    pub times: Vec<i64>,
    pub series: Vec<Series>,
}

/// One history day.
#[derive(Debug, Clone, Copy)]
pub struct Day {
    pub date: Date,
    /// ISK
    pub average: f64,
    pub volume: f64,
}

/// One hourly rollup of the prices.
#[derive(Debug, Clone, Copy)]
pub struct PricePoint {
    pub at: OffsetDateTime,
    /// ISK
    pub best_buy: f64,
    pub best_sell: f64,
    /// Fraction
    pub spread: f64,
}

/// The orders and units within 1% of each side's best price in the latest snapshot.
#[derive(Debug, Clone, Copy)]
pub struct Depth {
    pub buy_units: i64,
    pub buy_orders: i32,
    pub sell_units: i64,
    pub sell_orders: i32,
}

fn cents(isk: f64) -> f64 {
    (isk * 100.0).round()
}

fn isk_text(cents: f64) -> String {
    format::isk_short(cents as i64)
}

fn day_times(days: &[Day]) -> (Vec<String>, Vec<i64>) {
    (
        days.iter().map(|d| d.date.to_string()).collect(),
        days.iter()
            .map(|d| d.date.midnight().assume_utc().unix_timestamp() * 1000)
            .collect(),
    )
}

fn price_series(name: &'static str, values: Vec<Option<f64>>) -> Series {
    let exact = values
        .iter()
        .map(|v| v.map_or_else(String::new, isk_text))
        .collect();
    Series {
        name,
        values,
        exact,
        kind: None,
    }
}

fn averages(days: &[Day]) -> Vec<f64> {
    days.iter().map(|d| d.average).collect()
}

/// The values of a function of the averages up to each day.
fn rolling(days: &[Day], f: impl Fn(&[f64]) -> Option<f64>) -> Vec<Option<f64>> {
    let averages = averages(days);
    (0..averages.len())
        .map(|i| f(&averages[..=i]).map(cents))
        .collect()
}

/// The daily average with its SMA 5 and SMA 20.
pub fn trend_price(days: &[Day]) -> Chart {
    let (labels, times) = day_times(days);
    Chart {
        labels,
        times,
        series: vec![
            price_series(
                "Average",
                days.iter().map(|d| Some(cents(d.average))).collect(),
            ),
            price_series("SMA 5", rolling(days, |a| sma(a, 5))),
            price_series("SMA 20", rolling(days, |a| sma(a, 20))),
        ],
    }
}

/// The daily traded volume.
pub fn trend_volume(days: &[Day]) -> Chart {
    let (labels, times) = day_times(days);
    let values: Vec<Option<f64>> = days.iter().map(|d| Some(d.volume)).collect();
    let exact = days
        .iter()
        .map(|d| format::thousands(d.volume.round() as i64))
        .collect();
    Chart {
        labels,
        times,
        series: vec![Series {
            name: "Volume",
            values,
            exact,
            kind: None,
        }],
    }
}

/// The daily average with its SMA 20 and the Bollinger bands around it.
pub fn reversion_price(days: &[Day]) -> Chart {
    let (labels, times) = day_times(days);
    Chart {
        labels,
        times,
        series: vec![
            price_series(
                "Average",
                days.iter().map(|d| Some(cents(d.average))).collect(),
            ),
            price_series("SMA 20", rolling(days, |a| bollinger(a).map(|b| b.sma))),
            price_series(
                "Upper band",
                rolling(days, |a| bollinger(a).map(|b| b.upper)),
            ),
            price_series(
                "Lower band",
                rolling(days, |a| bollinger(a).map(|b| b.lower)),
            ),
        ],
    }
}

/// RSI 14 per day.
pub fn rsi(days: &[Day]) -> Chart {
    let (labels, times) = day_times(days);
    let averages = averages(days);
    let values: Vec<Option<f64>> = (0..averages.len())
        .map(|i| rsi14(&averages[..=i]))
        .collect();
    let exact = values
        .iter()
        .map(|v| v.map_or_else(String::new, |v| format!("{v:.1}")))
        .collect();
    Chart {
        labels,
        times,
        series: vec![Series {
            name: "RSI 14",
            values,
            exact,
            kind: None,
        }],
    }
}

fn price_times(points: &[PricePoint]) -> (Vec<String>, Vec<i64>) {
    (
        points.iter().map(|p| format::eve_time(p.at)).collect(),
        points
            .iter()
            .map(|p| p.at.unix_timestamp() * 1000)
            .collect(),
    )
}

/// The best buy and the best sell over time.
pub fn classic_prices(points: &[PricePoint]) -> Chart {
    let (labels, times) = price_times(points);
    Chart {
        labels,
        times,
        series: vec![
            price_series(
                "Best buy",
                points.iter().map(|p| Some(cents(p.best_buy))).collect(),
            ),
            price_series(
                "Best sell",
                points.iter().map(|p| Some(cents(p.best_sell))).collect(),
            ),
        ],
    }
}

fn percent_series(name: &'static str, values: Vec<Option<f64>>) -> Series {
    let exact = values
        .iter()
        .map(|v| v.map_or_else(String::new, |v| format::percent(v / 100.0)))
        .collect();
    Series {
        name,
        values,
        exact,
        kind: None,
    }
}

/// The spread over time, in percent.
pub fn classic_spread(points: &[PricePoint]) -> Chart {
    let (labels, times) = price_times(points);
    Chart {
        labels,
        times,
        series: vec![percent_series(
            "Spread",
            points.iter().map(|p| Some(p.spread * 100.0)).collect(),
        )],
    }
}

/// The units within 1% of the best price of each side, with their number of orders.
pub fn depth(depth: &Depth) -> Chart {
    let orders = |n: i32| format!("{n} order{}", if n == 1 { "" } else { "s" });
    let units = [depth.buy_units, depth.sell_units];
    Chart {
        labels: vec![
            format!("Buy ({})", orders(depth.buy_orders)),
            format!("Sell ({})", orders(depth.sell_orders)),
        ],
        times: Vec::new(),
        series: vec![Series {
            name: "Units within 1%",
            values: units.iter().map(|u| Some(*u as f64)).collect(),
            exact: units.iter().map(|u| format::thousands(*u)).collect(),
            kind: None,
        }],
    }
}

fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

/// The average maximum spread and the average spread of each hour of day, in percent.
pub fn spread_by_hour(hours: &[HourPoint]) -> Chart {
    let per_hour = |pick: fn(&HourPoint) -> f64| -> Vec<Option<f64>> {
        (0..24u8)
            .map(|hour| {
                let values: Vec<f64> = hours
                    .iter()
                    .filter(|h| h.start.hour() == hour)
                    .map(pick)
                    .collect();
                mean(&values).map(|m| m * 100.0)
            })
            .collect()
    };
    Chart {
        labels: (0..24).map(|hour| format!("{hour:02}:00")).collect(),
        times: Vec::new(),
        series: vec![
            percent_series("Average maximum spread", per_hour(|h| h.spread_max)),
            percent_series("Average spread", per_hour(|h| h.spread_avg)),
        ],
    }
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

/// The average daily spread of each weekday, in percent.
pub fn spread_by_weekday(days: &[DayPoint]) -> Chart {
    let values: Vec<Option<f64>> = WEEKDAYS
        .iter()
        .map(|weekday| {
            let spreads: Vec<f64> = days
                .iter()
                .filter(|d| d.day.weekday() == *weekday)
                .map(|d| d.spread)
                .collect();
            mean(&spreads).map(|m| m * 100.0)
        })
        .collect();
    Chart {
        labels: WEEKDAYS
            .iter()
            .map(|w| w.to_string()[..3].to_owned())
            .collect(),
        times: Vec::new(),
        series: vec![percent_series("Average daily spread", values)],
    }
}

/// The past occurrences of a window in ISK, from its expanded hours: the maximum best sell of
/// each with S drawn over them for a spike, the minimum best buy with B for a dip.
pub fn peaks(
    kind: WindowKind,
    direction: Direction,
    price_cents: i64,
    expanded: &[ExpandedHour],
) -> Chart {
    let held: Vec<&ExpandedHour> = expanded
        .iter()
        .filter(|h| h.direction == direction)
        .filter(|h| match kind {
            WindowKind::HourOfDay { hour } => h.start.hour() == hour,
            WindowKind::WeekdayHour { weekday, hour } => {
                h.start.hour() == hour && h.start.weekday() == weekday
            }
        })
        .collect();
    let (name, mark) = match direction {
        Direction::Spike => ("Peak", "S"),
        Direction::Dip => ("Trough", "B"),
    };
    let extremes: Vec<Option<f64>> = held.iter().map(|h| Some(h.extreme_cents as f64)).collect();
    let marked = vec![Some(price_cents as f64); extremes.len()];
    let mut line = price_series(mark, marked);
    line.kind = Some("line");
    Chart {
        labels: held.iter().map(|h| format::eve_time(h.start)).collect(),
        times: Vec::new(),
        series: vec![price_series(name, extremes), line],
    }
}

/// The starts of the next `count` occurrences of a window, in EVE time.
pub fn next_occurrences(kind: WindowKind, now: OffsetDateTime, count: usize) -> Vec<String> {
    let mut at = now;
    (0..count)
        .map(|_| {
            at = kind.next_start(at);
            format::eve_time(at)
        })
        .collect()
}
