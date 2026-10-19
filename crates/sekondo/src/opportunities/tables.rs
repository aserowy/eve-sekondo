//! The four opportunity tables. Everything that does not depend on the account is in
//! `station_item_stats`; a request adds what does: the fees, the capital, the filters and the
//! account's own orders. The maths are [`super::analysis`]'s; this module selects, sorts and
//! shapes rows.

use std::{cmp::Ordering, collections::HashSet};

use sqlx::PgPool;
use time::{Date, OffsetDateTime, Weekday};
use uuid::Uuid;

use super::settings::Settings;
use super::{
    REGION_ID, STATION_ID,
    analysis::{
        Exclusion, FeeSource, Fees, Figures, Inputs, WindowKind, choose_fees, evaluate, figures,
        round_down_valid, tick_below,
    },
};
use crate::orders::Dir;

/// The four tables, in the order the page shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Table {
    Expansion,
    Classic,
    Reversion,
    Trend,
}

impl Table {
    pub const ALL: [Table; 4] = [Self::Expansion, Self::Classic, Self::Reversion, Self::Trend];

    /// The part of the section id and of the detail URL.
    pub fn id(self) -> &'static str {
        match self {
            Self::Expansion => "expansion",
            Self::Classic => "classic",
            Self::Reversion => "reversion",
            Self::Trend => "trend",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.id() == id)
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Expansion => "Recurring spread expansion",
            Self::Classic => "Classic spread trade",
            Self::Reversion => "Mean reversion",
            Self::Trend => "Trend",
        }
    }

    /// The query keys of this table's sorting.
    pub fn sort_params(self) -> (&'static str, &'static str) {
        match self {
            Self::Expansion => ("xs", "xd"),
            Self::Classic => ("cs", "cd"),
            Self::Reversion => ("ms", "md"),
            Self::Trend => ("ts", "td"),
        }
    }

    /// The columns only this table has.
    fn extra_keys(self) -> &'static [SortKey] {
        match self {
            Self::Expansion => &[SortKey::Window, SortKey::NextStart],
            Self::Classic => &[],
            Self::Reversion => &[SortKey::PercentB, SortKey::Rsi],
            Self::Trend => &[SortKey::Sma5, SortKey::Sma20, SortKey::Cross],
        }
    }

    /// The columns of the table, in order.
    pub fn keys(self) -> Vec<SortKey> {
        let mut keys = vec![
            SortKey::Item,
            SortKey::Buy,
            SortKey::Sell,
            SortKey::Net,
            SortKey::Margin,
            SortKey::IskPerDay,
        ];
        keys.extend(self.extra_keys());
        keys.extend([
            SortKey::V7,
            SortKey::V30,
            SortKey::Sigma,
            SortKey::TMax,
            SortKey::TFill,
            SortKey::MinVolume,
            SortKey::Near,
        ]);
        keys
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Item,
    Buy,
    Sell,
    Net,
    Margin,
    IskPerDay,
    V7,
    V30,
    Sigma,
    TMax,
    TFill,
    MinVolume,
    Near,
    Window,
    NextStart,
    PercentB,
    Rsi,
    Sma5,
    Sma20,
    Cross,
}

impl SortKey {
    /// The name used in the sort parameters.
    pub fn name(self) -> &'static str {
        match self {
            Self::Item => "item",
            Self::Buy => "buy",
            Self::Sell => "sell",
            Self::Net => "net",
            Self::Margin => "margin",
            Self::IskPerDay => "isk-day",
            Self::V7 => "v7",
            Self::V30 => "v30",
            Self::Sigma => "sigma",
            Self::TMax => "t-max",
            Self::TFill => "t-fill",
            Self::MinVolume => "min-volume",
            Self::Near => "near",
            Self::Window => "window",
            Self::NextStart => "next",
            Self::PercentB => "pb",
            Self::Rsi => "rsi",
            Self::Sma5 => "sma5",
            Self::Sma20 => "sma20",
            Self::Cross => "cross",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Item => "Item",
            Self::Buy => "Current best buy",
            Self::Sell => "Sell at (S)",
            Self::Net => "Net / unit",
            Self::Margin => "Margin (m)",
            Self::IskPerDay => "ISK/day",
            Self::V7 => "Volume 7 d",
            Self::V30 => "Volume 30 d",
            Self::Sigma => "\u{03C3}",
            Self::TMax => "T max",
            Self::TFill => "Fill time",
            Self::MinVolume => "Min. volume",
            Self::Near => "Orders within 1% (buy / sell)",
            Self::Window => "Window",
            Self::NextStart => "Next start (EVE time)",
            Self::PercentB => "%B",
            Self::Rsi => "RSI 14",
            Self::Sma5 => "SMA 5",
            Self::Sma20 => "SMA 20",
            Self::Cross => "Crossed on",
        }
    }

    /// Text columns are left-aligned; the others are numbers.
    pub fn is_numeric(self) -> bool {
        !matches!(
            self,
            Self::Item | Self::Window | Self::NextStart | Self::Cross
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub key: SortKey,
    pub dir: Dir,
}

impl Default for Sort {
    /// ISK/day, highest first.
    fn default() -> Self {
        Self {
            key: SortKey::IskPerDay,
            dir: Dir::Desc,
        }
    }
}

impl Sort {
    /// A key that is not one of the table's columns, or no key, gives the default sorting; an
    /// unknown direction gives the key's natural one (text up, numbers down).
    pub fn parse(table: Table, key: Option<&str>, dir: Option<&str>) -> Self {
        let Some(key) = key.and_then(|k| table.keys().into_iter().find(|c| c.name() == k)) else {
            return Self::default();
        };
        let natural = if key.is_numeric() {
            Dir::Desc
        } else {
            Dir::Asc
        };
        let dir = match dir {
            Some("asc") => Dir::Asc,
            Some("desc") => Dir::Desc,
            _ => natural,
        };
        Self { key, dir }
    }
}

/// An item as `station_item_stats` holds it, with its name.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Stats {
    pub type_id: i64,
    pub name: Option<String>,
    pub best_buy_cents: i64,
    pub best_sell_cents: i64,
    pub buy_near_orders: i32,
    pub sell_near_orders: i32,
    pub rho: Option<f64>,
    pub volume_7d: Option<f64>,
    pub volume_30d: Option<f64>,
    pub sigma: Option<f64>,
    pub sma5: Option<f64>,
    pub sma20: Option<f64>,
    pub cross_day: Option<Date>,
    pub volume_rising: Option<bool>,
    pub percent_b: Option<f64>,
    pub rsi14: Option<f64>,
    pub expansion_kind: Option<String>,
    pub expansion_weekday: Option<i16>,
    pub expansion_hour: Option<i16>,
    pub expansion_sell_cents: Option<i64>,
}

impl Stats {
    pub fn display_name(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| format!("Type {}", self.type_id))
    }

    /// The recurring window the expansion fields describe.
    pub fn window(&self) -> Option<WindowKind> {
        let hour = || u8::try_from(self.expansion_hour?).ok();
        let weekday = || weekday_from_monday(self.expansion_weekday?);
        match self.expansion_kind.as_deref()? {
            "hour" => Some(WindowKind::HourOfDay { hour: hour()? }),
            "weekday_hour" => Some(WindowKind::WeekdayHour {
                weekday: weekday()?,
                hour: hour()?,
            }),
            "weekday" => Some(WindowKind::Weekday {
                weekday: weekday()?,
            }),
            _ => None,
        }
    }
}

fn weekday_from_monday(n: i16) -> Option<Weekday> {
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

/// Every item with book fields, with its name.
pub async fn load_stats(pool: &PgPool) -> Result<Vec<Stats>, sqlx::Error> {
    sqlx::query_as(
        "SELECT s.type_id, n.name,
                (s.best_buy * 100)::bigint AS best_buy_cents,
                (s.best_sell * 100)::bigint AS best_sell_cents,
                s.buy_near_orders, s.sell_near_orders, s.rho, s.volume_7d, s.volume_30d, s.sigma,
                s.sma5, s.sma20, s.cross_day, s.volume_rising, s.percent_b, s.rsi14,
                s.expansion_kind, s.expansion_weekday, s.expansion_hour,
                (s.expansion_sell * 100)::bigint AS expansion_sell_cents
         FROM station_item_stats s
         LEFT JOIN names n ON n.id = s.type_id
         WHERE s.region_id = $1 AND s.station_id = $2
           AND s.best_buy IS NOT NULL AND s.best_sell IS NOT NULL
           AND s.buy_near_orders IS NOT NULL AND s.sell_near_orders IS NOT NULL",
    )
    .bind(REGION_ID)
    .bind(STATION_ID)
    .fetch_all(pool)
    .await
}

/// The items on which the selected characters have an order at the station.
#[derive(Debug, Default)]
pub struct OwnOrders {
    pub buy: HashSet<i64>,
    pub sell: HashSet<i64>,
}

pub async fn load_own_orders(
    pool: &PgPool,
    account_id: Uuid,
    character_ids: &[i64],
) -> Result<OwnOrders, sqlx::Error> {
    let rows: Vec<(i64, bool)> = sqlx::query_as(
        "SELECT DISTINCT o.type_id, o.is_buy_order FROM current_orders o
         JOIN characters c ON c.character_id = o.character_id
         WHERE c.account_id = $1 AND o.character_id = ANY($2) AND o.location_id = $3",
    )
    .bind(account_id)
    .bind(character_ids)
    .bind(STATION_ID)
    .fetch_all(pool)
    .await?;
    let mut own = OwnOrders::default();
    for (type_id, is_buy) in rows {
        if is_buy {
            own.buy.insert(type_id);
        } else {
            own.sell.insert(type_id);
        }
    }
    Ok(own)
}

/// The fees the analyses use, and whose they are.
#[derive(Debug, Clone)]
pub struct FeeChoice {
    pub fees: Fees,
    pub source: FeeSource,
    /// The base rates are used, because nobody is selected or the chosen character's fees are
    /// not known yet.
    pub base_rates: bool,
}

/// The fees at the station of the selected characters: the lowest minimum margin wins, and a
/// character whose fees are not known yet counts with the base rates.
pub async fn choose_station_fees(
    pool: &PgPool,
    account_id: Uuid,
    character_ids: &[i64],
) -> Result<FeeChoice, sqlx::Error> {
    let rows = crate::fees::at_location(pool, account_id, character_ids, STATION_ID).await?;
    let percent = |text: &Option<String>| text.as_deref().and_then(|t| t.parse::<f64>().ok());
    let characters: Vec<(i64, Option<Fees>)> = rows
        .iter()
        .map(|row| {
            let fees = match (percent(&row.broker_fee), percent(&row.sales_tax)) {
                (Some(broker), Some(tax)) => Some(Fees {
                    broker: broker / 100.0,
                    tax: tax / 100.0,
                }),
                _ => None,
            };
            (row.character_id, fees)
        })
        .collect();
    let (fees, source) = choose_fees(&characters);
    let base_rates = match source {
        FeeSource::Base => true,
        FeeSource::Character(id) => characters
            .iter()
            .find(|(character, _)| *character == id)
            .is_none_or(|(_, known)| known.is_none()),
    };
    Ok(FeeChoice {
        fees,
        source,
        base_rates,
    })
}

/// What a table shows beyond the shared figures.
#[derive(Debug, Clone)]
pub enum Extra {
    None,
    Expansion {
        window: WindowKind,
        next_start: OffsetDateTime,
    },
    Reversion {
        percent_b: f64,
        rsi: f64,
    },
    Trend {
        sma5: f64,
        sma20: f64,
        cross_day: Date,
    },
}

/// One row of one table.
#[derive(Debug, Clone)]
pub struct Row {
    pub type_id: i64,
    pub name: String,
    pub best_buy_cents: i64,
    pub sell_cents: i64,
    pub figures: Figures,
    pub v7: f64,
    pub v30: f64,
    pub sigma: f64,
    pub near_buy: i32,
    pub near_sell: i32,
    pub own_buy: bool,
    pub own_sell: bool,
    pub extra: Extra,
}

impl Row {
    fn cmp_by(&self, other: &Row, key: SortKey) -> Ordering {
        let float = |a: f64, b: f64| a.total_cmp(&b);
        let f = &self.figures;
        let g = &other.figures;
        match key {
            SortKey::Item => self.name.to_lowercase().cmp(&other.name.to_lowercase()),
            SortKey::Buy => self.best_buy_cents.cmp(&other.best_buy_cents),
            SortKey::Sell => self.sell_cents.cmp(&other.sell_cents),
            SortKey::Net => float(f.net_per_unit, g.net_per_unit),
            SortKey::Margin => float(f.margin, g.margin),
            SortKey::IskPerDay => float(f.isk_per_day, g.isk_per_day),
            SortKey::V7 => float(self.v7, other.v7),
            SortKey::V30 => float(self.v30, other.v30),
            SortKey::Sigma => float(self.sigma, other.sigma),
            SortKey::TMax => float(f.t_max, g.t_max),
            SortKey::TFill => float(f.t_fill, g.t_fill),
            SortKey::MinVolume => float(f.min_volume, g.min_volume),
            SortKey::Near => {
                (self.near_buy + self.near_sell).cmp(&(other.near_buy + other.near_sell))
            }
            SortKey::Window => self.window_label().cmp(&other.window_label()),
            SortKey::NextStart => self.next_start().cmp(&other.next_start()),
            SortKey::PercentB => float(self.percent_b(), other.percent_b()),
            SortKey::Rsi => float(self.rsi(), other.rsi()),
            SortKey::Sma5 => float(self.sma5(), other.sma5()),
            SortKey::Sma20 => float(self.sma20(), other.sma20()),
            SortKey::Cross => self.cross_day().cmp(&other.cross_day()),
        }
    }

    pub fn window_label(&self) -> String {
        match &self.extra {
            Extra::Expansion { window, .. } => window.label(),
            _ => String::new(),
        }
    }

    fn next_start(&self) -> Option<OffsetDateTime> {
        match &self.extra {
            Extra::Expansion { next_start, .. } => Some(*next_start),
            _ => None,
        }
    }

    fn percent_b(&self) -> f64 {
        match &self.extra {
            Extra::Reversion { percent_b, .. } => *percent_b,
            _ => f64::NAN,
        }
    }

    fn rsi(&self) -> f64 {
        match &self.extra {
            Extra::Reversion { rsi, .. } => *rsi,
            _ => f64::NAN,
        }
    }

    fn sma5(&self) -> f64 {
        match &self.extra {
            Extra::Trend { sma5, .. } => *sma5,
            _ => f64::NAN,
        }
    }

    fn sma20(&self) -> f64 {
        match &self.extra {
            Extra::Trend { sma20, .. } => *sma20,
            _ => f64::NAN,
        }
    }

    fn cross_day(&self) -> Option<Date> {
        match &self.extra {
            Extra::Trend { cross_day, .. } => Some(*cross_day),
            _ => None,
        }
    }
}

/// What a table needs to know about the request.
pub struct Context<'a> {
    pub settings: &'a Settings,
    pub fees: Fees,
    pub own: &'a OwnOrders,
    pub now: OffsetDateTime,
}

/// The sell price S and the extra columns of an item in a table, or `None` when the table does
/// not list it.
pub fn candidate(table: Table, stats: &Stats, now: OffsetDateTime) -> Option<(i64, Extra)> {
    match table {
        Table::Classic => Some((tick_below(stats.best_sell_cents), Extra::None)),
        Table::Trend => {
            let cross_day = stats.cross_day?;
            if stats.volume_rising != Some(true) {
                return None;
            }
            Some((
                tick_below(stats.best_sell_cents),
                Extra::Trend {
                    sma5: stats.sma5?,
                    sma20: stats.sma20?,
                    cross_day,
                },
            ))
        }
        Table::Reversion => {
            let sma20 = stats.sma20?;
            let oversold = stats.percent_b.is_some_and(|b| b < 0.0)
                || stats.rsi14.is_some_and(|rsi| rsi < 30.0);
            if !oversold {
                return None;
            }
            let sell = round_down_valid((sma20 * 100.0).round() as i64);
            // S must be above the current best buy, or there is nothing to earn.
            (sell > stats.best_buy_cents).then(|| {
                (
                    sell,
                    Extra::Reversion {
                        percent_b: stats.percent_b.unwrap_or(f64::NAN),
                        rsi: stats.rsi14.unwrap_or(f64::NAN),
                    },
                )
            })
        }
        Table::Expansion => {
            let window = stats.window()?;
            Some((
                stats.expansion_sell_cents?,
                Extra::Expansion {
                    window,
                    next_start: window.next_start(now),
                },
            ))
        }
    }
}

/// The shared-rule inputs of an item at a sell price.
pub fn inputs(stats: &Stats, sell_cents: i64, context: &Context<'_>) -> Inputs {
    Inputs {
        best_buy_cents: stats.best_buy_cents,
        sell_cents,
        fees: context.fees,
        capital: context.settings.capital,
        volume: stats.volume_7d.unwrap_or(0.0),
        rho: stats.rho.unwrap_or(0.5),
        buy_near_orders: i64::from(stats.buy_near_orders),
        sell_near_orders: i64::from(stats.sell_near_orders),
        sigma: stats.sigma,
    }
}

/// Why an excluded item is in no table, as a sentence.
pub fn exclusion_text(exclusion: Exclusion) -> &'static str {
    match exclusion {
        Exclusion::NotProfitable => "Its net margin is not positive.",
        Exclusion::TooSmall => "Not even one unit fits the capital per item.",
        Exclusion::NotEnoughHistory => "There are fewer than 10 daily returns for the volatility.",
        Exclusion::NoVolume => "Its daily volume is 0.",
        Exclusion::TooSlow => "Its fill time exceeds the time it may be held.",
    }
}

/// One item in one table, for the detail window: whether the table lists it right now, why not,
/// and its figures whether or not it is listed.
pub struct Detail<'a> {
    pub stats: &'a Stats,
    pub listed: bool,
    pub reason: Option<String>,
    pub sell_cents: Option<i64>,
    pub extra: Option<Extra>,
    pub figures: Option<Figures>,
}

pub fn detail<'a>(
    table: Table,
    type_id: i64,
    all: &'a [Stats],
    context: &Context<'_>,
    sort: Sort,
) -> Option<Detail<'a>> {
    let stats = all.iter().find(|s| s.type_id == type_id)?;
    let listed = rows(table, all, context, sort)
        .iter()
        .any(|row| row.type_id == type_id);
    let candidate = candidate(table, stats, context.now);
    let (sell_cents, extra) = match candidate {
        Some((sell, extra)) => (Some(sell), Some(extra)),
        None => (None, None),
    };
    let figures = sell_cents.and_then(|sell| {
        let input = inputs(stats, sell, context);
        input.sigma.map(|sigma| figures(&input, sigma))
    });
    let reason = (!listed).then(|| match sell_cents {
        None => "It no longer meets this table's rule.".to_owned(),
        Some(sell) => match evaluate(&inputs(stats, sell, context)) {
            Err(exclusion) => exclusion_text(exclusion).to_owned(),
            Ok(f) if f.margin < context.settings.min_margin => {
                "Its net margin is below your minimum margin.".to_owned()
            }
            Ok(f) if f.isk_per_day < context.settings.min_isk_per_day => {
                "Its ISK/day is below your minimum.".to_owned()
            }
            Ok(_) => {
                let search = context.settings.search.trim().to_lowercase();
                if !search.is_empty() && !stats.display_name().to_lowercase().contains(&search) {
                    "Your name search excludes it.".to_owned()
                } else {
                    "It is not listed.".to_owned()
                }
            }
        },
    });
    Some(Detail {
        stats,
        listed,
        reason,
        sell_cents,
        extra,
        figures,
    })
}

/// The rows of one table: every item it lists that passes the shared rules and the filters, sorted.
/// The page shows a slice of them.
pub fn rows(table: Table, all: &[Stats], context: &Context<'_>, sort: Sort) -> Vec<Row> {
    let settings = context.settings;
    let search = settings.search.trim().to_lowercase();
    let mut rows: Vec<Row> = all
        .iter()
        .filter_map(|stats| {
            let (sell_cents, extra) = candidate(table, stats, context.now)?;
            let name = stats.display_name();
            if !search.is_empty() && !name.to_lowercase().contains(&search) {
                return None;
            }
            let v7 = stats.volume_7d.unwrap_or(0.0);
            let figures = evaluate(&inputs(stats, sell_cents, context)).ok()?;
            if figures.margin < settings.min_margin
                || figures.isk_per_day < settings.min_isk_per_day
            {
                return None;
            }
            Some(Row {
                type_id: stats.type_id,
                name,
                best_buy_cents: stats.best_buy_cents,
                sell_cents,
                figures,
                v7,
                v30: stats.volume_30d.unwrap_or(0.0),
                sigma: stats.sigma.unwrap_or(0.0),
                near_buy: stats.buy_near_orders,
                near_sell: stats.sell_near_orders,
                own_buy: context.own.buy.contains(&stats.type_id),
                own_sell: context.own.sell.contains(&stats.type_id),
                extra,
            })
        })
        .collect();
    rows.sort_by(|a, b| {
        let ordering = a.cmp_by(b, sort.key);
        let ordering = match sort.dir {
            Dir::Asc => ordering,
            Dir::Desc => ordering.reverse(),
        };
        ordering.then(a.type_id.cmp(&b.type_id))
    });
    rows
}
