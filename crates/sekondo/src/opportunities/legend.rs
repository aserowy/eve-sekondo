//! The legend: the one wording of every short label the opportunity tables and detail pages show.
//! The headers' hover text, the table legends and the detail legend all read it from here.

use super::tables::Table;

/// A short label of a table column or a detail figure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Term {
    Item,
    BrokerFee,
    SalesTax,
    Buy,
    BestBuy,
    BestSell,
    Sell,
    Cost,
    Revenue,
    Net,
    Margin,
    Units,
    V7,
    V30,
    Rho,
    QBuy,
    QSell,
    Sigma,
    TFill,
    TMax,
    MinVolume,
    IskPerDay,
    Near,
    Window,
    Dir,
    VWin,
    NextStart,
    PercentB,
    Rsi,
    Sma5,
    Sma20,
    Cross,
}

impl Term {
    /// The label of the header or the card.
    pub fn short(self) -> &'static str {
        match self {
            Self::Item => "Item",
            Self::BrokerFee => "b",
            Self::SalesTax => "t",
            Self::Buy => "B",
            Self::BestBuy => "Best buy",
            Self::BestSell => "Best sell",
            Self::Sell => "S",
            Self::Cost => "c",
            Self::Revenue => "r",
            Self::Net => "Net",
            Self::Margin => "m",
            Self::Units => "U",
            Self::V7 => "V7",
            Self::V30 => "V30",
            Self::Rho => "\u{03C1}",
            Self::QBuy => "q_buy",
            Self::QSell => "q_sell",
            Self::Sigma => "\u{03C3}",
            Self::TFill => "T_fill",
            Self::TMax => "T_max",
            Self::MinVolume => "V_min",
            Self::IskPerDay => "ISK/d",
            Self::Near => "n b/s",
            Self::Window => "Window",
            Self::Dir => "Dir",
            Self::VWin => "V_win",
            Self::NextStart => "Next (UTC)",
            Self::PercentB => "%B",
            Self::Rsi => "RSI",
            Self::Sma5 => "SMA5",
            Self::Sma20 => "SMA20",
            Self::Cross => "Cross",
        }
    }

    /// The full name, shown on hover.
    pub fn name(self) -> &'static str {
        match self {
            Self::Item => "Item",
            Self::BrokerFee => "Broker fee",
            Self::SalesTax => "Sales tax",
            Self::Buy => {
                "Current best buy, not a suggested price, except in a dip row of the recurring \
                 spread expansion table, where it is the predicted buy price"
            }
            Self::BestBuy => "Current best buy",
            Self::BestSell => "Current best sell",
            Self::Sell => "Sell price",
            Self::Cost => "Cost per unit",
            Self::Revenue => "Revenue per unit",
            Self::Net => "Net margin per unit",
            Self::Margin => "Net margin",
            Self::Units => "Order size",
            Self::V7 => "Daily volume, 7 days",
            Self::V30 => "Daily volume, 30 days",
            Self::Rho => "Buy side ratio",
            Self::QBuy => "Daily capture, buy side",
            Self::QSell => "Daily capture, sell side",
            Self::Sigma => "Volatility",
            Self::TFill => "Fill time",
            Self::TMax => "Allowed holding time",
            Self::MinVolume => "Minimum daily volume",
            Self::IskPerDay => "ISK per day",
            Self::Near => "Orders within 1%, buy / sell",
            Self::Window => "Recurring window",
            Self::Dir => "Direction of the window",
            Self::VWin => "Window volume",
            Self::NextStart => "Next start in EVE time",
            Self::PercentB => "Percent B",
            Self::Rsi => "Relative strength index, 14 days",
            Self::Sma5 => "Simple moving average, 5 days",
            Self::Sma20 => "Simple moving average, 20 days",
            Self::Cross => "Day of the crossing",
        }
    }

    /// What the figure tells.
    pub fn meaning(self) -> &'static str {
        match self {
            Self::Item => {
                "The item traded. \u{201C}own buy\u{201D} or \u{201C}own sell\u{201D} marks an item \
                 on which a selected character has an order of that side at the station."
            }
            Self::BrokerFee => {
                "The fee character's broker fee at the station, paid on placing an order."
            }
            Self::SalesTax => "The fee character's sales tax, paid on every sale.",
            Self::Buy => {
                "The highest buy order in the station book now. It is where buying starts, not a \
                 price to place: to buy, outbid it by one tick. In a dip row of the recurring \
                 spread expansion table it is the predicted buy price instead, which you place \
                 as it is."
            }
            Self::BestBuy => {
                "The highest buy order in the station book now, shown when B is the predicted buy \
                 price of a dip row instead."
            }
            Self::BestSell => "The lowest sell order in the station book now.",
            Self::Sell => {
                "The price the table expects to sell at. Each table derives it from its own signal."
            }
            Self::Cost => "What one unit costs, broker fee included.",
            Self::Revenue => {
                "What selling one unit at S brings in, after broker fee and sales tax."
            }
            Self::Net => "What one unit earns.",
            Self::Margin => "What one unit earns, as a share of its cost.",
            Self::Units => "How many units the capital per item buys.",
            Self::V7 => {
                "The traded volume per day, averaged over the last 7 days of history: the V of the \
                 formulas."
            }
            Self::V30 => "The traded volume per day, averaged over the last 30 days of history.",
            Self::Rho => {
                "The share of the units taken from buy orders over 30 days of snapshots; 0.5 when \
                 none were taken."
            }
            Self::QBuy => {
                "How many units a day your buy order may expect to fill, with n the other buy \
                 orders within 1% of B."
            }
            Self::QSell => {
                "How many units a day your sell order may expect to fill, with n the other sell \
                 orders within 1% of the best sell."
            }
            Self::Sigma => {
                "The sample standard deviation of the daily log returns of the average price over \
                 the last 30 days."
            }
            Self::TFill => "How long it takes to fill U units on both sides.",
            Self::TMax => {
                "How long an order may be held before the price is likely to move by more than \
                 the margin, at most 3 days."
            }
            Self::MinVolume => {
                "The daily volume an item needs to fill within T_max. Items below it are excluded."
            }
            Self::IskPerDay => "What the trade earns per day of fill time.",
            Self::Near => {
                "How many other orders are within 1% of the best price on each side: the n of the \
                 daily capture."
            }
            Self::Window => {
                "When the spread recurs in an expanded hour, whose maximum spread is at least twice \
                 the median of its UTC day: daily at an hour (on 60% of the last 28 days, at least \
                 7) or weekly at a weekday and hour (in 60% of the last 52 weeks, at least 3, and \
                 2 of the last 4)."
            }
            Self::Dir => {
                "Dip: the best buy fell further below the day's median than the best sell rose \
                 above its own, so you can buy far below the normal price. Spike: the best sell \
                 rose, so you can sell far above it. Dips and spikes recur apart."
            }
            Self::VWin => {
                "The units taken in the window's hour from the side that moved, averaged over its \
                 occurrences: from buy orders for a dip, from sell orders for a spike."
            }
            Self::NextStart => "When the window next starts, in EVE time (UTC).",
            Self::PercentB => {
                "Where the latest daily average sits in the Bollinger bands (SMA 20 \u{00B1} 2 \
                 standard deviations of the last 20 daily averages): 0 on the lower band, 1 on the \
                 upper band, below 0 under the lower band."
            }
            Self::Rsi => {
                "Wilder's RSI over 14 daily changes, from 0 to 100. Below 30, the item counts as \
                 oversold."
            }
            Self::Sma5 => "The mean of the last 5 daily averages.",
            Self::Sma20 => "The mean of the last 20 daily averages.",
            Self::Cross => "The day SMA5 crossed above SMA20, within the last 3 days.",
        }
    }

    /// How a computed figure follows from the others.
    pub fn formula(self) -> Option<&'static str> {
        Some(match self {
            Self::Cost => "B \u{00D7} (1 + b)",
            Self::Revenue => "S \u{00D7} (1 \u{2212} b \u{2212} t)",
            Self::Net => "r \u{2212} c",
            Self::Margin => "(r \u{2212} c) / c",
            Self::Units => "\u{230A}capital per item / c\u{230B}",
            Self::QBuy => "1/(n + 1) \u{00D7} V \u{00D7} \u{03C1}",
            Self::QSell => "1/(n + 1) \u{00D7} V \u{00D7} (1 \u{2212} \u{03C1})",
            Self::TFill => "U / q_buy + U / q_sell",
            Self::TMax => "min(3, (m / 2\u{03C3})\u{00B2}) days",
            Self::MinVolume => "the V at which T_fill equals T_max",
            Self::IskPerDay => "Net \u{00D7} U / T_fill",
            Self::PercentB => "(average \u{2212} lower band) / (upper band \u{2212} lower band)",
            _ => return None,
        })
    }
}

/// One entry of a legend, as the templates show it.
pub struct LegendEntry {
    pub short: &'static str,
    pub name: &'static str,
    pub meaning: &'static str,
    pub formula: Option<&'static str>,
}

impl From<Term> for LegendEntry {
    fn from(term: Term) -> Self {
        Self {
            short: term.short(),
            name: term.name(),
            meaning: term.meaning(),
            formula: term.formula(),
        }
    }
}

/// The terms of a table's columns, in column order.
pub fn table_terms(table: Table) -> Vec<Term> {
    table.keys().into_iter().map(|key| key.term()).collect()
}

/// The terms of the detail page's figures, in the order of its groups.
pub fn detail_terms() -> Vec<Term> {
    vec![
        Term::BrokerFee,
        Term::SalesTax,
        Term::Buy,
        Term::BestBuy,
        Term::BestSell,
        Term::Sell,
        Term::Cost,
        Term::Revenue,
        Term::Net,
        Term::Margin,
        Term::Units,
        Term::V7,
        Term::V30,
        Term::Rho,
        Term::QBuy,
        Term::QSell,
        Term::Sigma,
        Term::TFill,
        Term::TMax,
        Term::MinVolume,
        Term::IskPerDay,
    ]
}

/// A legend of the given terms.
pub fn entries(terms: &[Term]) -> Vec<LegendEntry> {
    terms.iter().map(|&term| term.into()).collect()
}
