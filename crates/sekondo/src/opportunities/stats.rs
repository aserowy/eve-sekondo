//! `station_item_stats`: everything about a tradable item that does not depend on an account, kept
//! up to date at three points. The maths are the pure functions of [`super::analysis`]; SQL only
//! collects and stores.

use std::collections::HashMap;

use time::{Date, Duration, OffsetDateTime};

use crate::store::{BookFields, ExpansionColumns, IndicatorColumns, StationStore, StoreError};

use super::{
    REGION_ID, STATION_ID,
    analysis::{
        self, DayPoint, HourPoint, WindowKind, bollinger, buy_side_ratio, recurring_windows, rsi14,
        sma, sma_crossing, volatility, volume_rising,
    },
};

/// How far back the buy side ratio looks, in days.
const RHO_DAYS: i64 = 30;
/// Days of history the indicators read.
const HISTORY_DAYS: i64 = 30;
/// Days of rollups the recurrence reads.
const ROLLUP_DAYS: i64 = 30;

/// After a book refresh: the item rows follow the tradable items, and their book fields (the best
/// prices and orders within 1% from the latest snapshot, and ρ over 30 days) are replaced.
pub async fn refresh_book_fields(
    store: &dyn StationStore,
    now: OffsetDateTime,
) -> Result<(), StoreError> {
    store.prune_item_stats(REGION_ID, STATION_ID).await?;
    let latest = store
        .latest_snapshots(REGION_ID, STATION_ID, now - Duration::days(1))
        .await?;

    // Units taken over 30 days: complete days from the daily rollup, the rest from the hourly one.
    let taken = store
        .units_taken(REGION_ID, STATION_ID, now - Duration::days(RHO_DAYS))
        .await?;
    let taken: HashMap<i64, (i64, i64)> = taken.into_iter().map(|(t, b, s)| (t, (b, s))).collect();

    let rows: Vec<BookFields> = latest
        .into_iter()
        .map(
            |(type_id, best_buy, best_sell, buy_near_orders, sell_near_orders)| {
                let (buy, sell) = taken.get(&type_id).copied().unwrap_or((0, 0));
                BookFields {
                    type_id,
                    best_buy,
                    best_sell,
                    buy_near_orders,
                    sell_near_orders,
                    rho: buy_side_ratio(buy, sell),
                }
            },
        )
        .collect();
    store
        .upsert_book_fields(REGION_ID, STATION_ID, now, &rows)
        .await
}

/// One item's history, oldest day first.
#[derive(Default)]
struct Series {
    days: Vec<Date>,
    averages: Vec<f64>,
    volumes: Vec<f64>,
}

/// After a history pass: the indicator fields of every item row, from its last 30 days of
/// history. A figure without enough history is left empty.
pub async fn refresh_indicators(
    store: &dyn StationStore,
    now: OffsetDateTime,
) -> Result<(), StoreError> {
    let today = now.date();
    let rows = store
        .history_rows(REGION_ID, STATION_ID, today - Duration::days(HISTORY_DAYS))
        .await?;
    let mut series: HashMap<i64, Series> = HashMap::new();
    for (type_id, day, average, volume) in rows {
        let s = series.entry(type_id).or_default();
        s.days.push(day);
        s.averages.push(average);
        s.volumes.push(volume as f64);
    }

    let item_ids = store.item_stat_ids(REGION_ID, STATION_ID).await?;

    let mut columns = IndicatorColumns::default();
    for type_id in &item_ids {
        let empty = Series::default();
        let s = series.get(type_id).unwrap_or(&empty);
        push_indicators(&mut columns, *type_id, today, s);
    }
    store
        .update_indicators(REGION_ID, STATION_ID, now, &columns)
        .await
}

fn push_indicators(columns: &mut IndicatorColumns, type_id: i64, today: Date, s: &Series) {
    let has_history = !s.averages.is_empty();
    // A day without a row had no trades, so the averages divide by the days, not the rows.
    let volume_over = |days: i64| {
        let from = today - Duration::days(days);
        let sum: f64 = s
            .days
            .iter()
            .zip(&s.volumes)
            .filter(|(day, _)| **day >= from)
            .map(|(_, volume)| volume)
            .sum();
        sum / days as f64
    };
    columns.type_id.push(type_id);
    columns.volume_7d.push(has_history.then(|| volume_over(7)));
    columns
        .volume_30d
        .push(has_history.then(|| volume_over(30)));
    columns.sigma.push(volatility(&s.averages));
    columns
        .returns
        .push(has_history.then(|| (s.averages.len() - 1) as i32));
    columns.sma5.push(sma(&s.averages, 5));
    columns.sma20.push(sma(&s.averages, 20));
    columns
        .cross_day
        .push(sma_crossing(&s.averages).map(|days_ago| s.days[s.days.len() - 1 - days_ago]));
    columns
        .volume_rising
        .push((s.volumes.len() >= 20).then(|| volume_rising(&s.volumes)));
    columns
        .percent_b
        .push(bollinger(&s.averages).map(|b| b.percent_b));
    columns.rsi14.push(rsi14(&s.averages));
}

/// When the hourly rollup advanced (its newest hour differs from the one the item rows were last
/// computed for): the expansion fields of every item row, from the recurring window with the
/// highest sell price. Returns whether it recomputed.
pub async fn refresh_expansions_if_advanced(
    store: &dyn StationStore,
    now: OffsetDateTime,
) -> Result<bool, StoreError> {
    let Some(latest) = store.latest_hour(REGION_ID, STATION_ID).await? else {
        return Ok(false);
    };
    if !store
        .expansions_stale(REGION_ID, STATION_ID, latest)
        .await?
    {
        return Ok(false);
    }

    let from = now - Duration::days(ROLLUP_DAYS);
    // Hourly patterns need 7 days of own snapshots; until then only the daily figures are read.
    let own_days = store.own_hourly_days(REGION_ID, STATION_ID, from).await?;
    let mut hours: HashMap<i64, Vec<HourPoint>> = HashMap::new();
    if own_days >= analysis::MIN_HOURLY_DAYS as i64 {
        let rows = store.hourly_points(REGION_ID, STATION_ID, from).await?;
        for (type_id, start, spread_avg, spread_max, sell_max_cents) in rows {
            hours.entry(type_id).or_default().push(HourPoint {
                start,
                spread_avg,
                spread_max,
                sell_max_cents,
            });
        }
    }
    let rows = store.daily_points(REGION_ID, from.date()).await?;
    let mut days: HashMap<i64, Vec<DayPoint>> = HashMap::new();
    for (type_id, day, spread, high_cents) in rows {
        days.entry(type_id).or_default().push(DayPoint {
            day,
            spread,
            high_cents,
        });
    }

    let item_ids = store.item_stat_ids(REGION_ID, STATION_ID).await?;
    let mut columns = ExpansionColumns {
        type_id: item_ids.clone(),
        ..ExpansionColumns::default()
    };
    for type_id in &item_ids {
        let windows = recurring_windows(
            hours.get(type_id).map_or(&[], Vec::as_slice),
            days.get(type_id).map_or(&[], Vec::as_slice),
            now,
        );
        let best = windows.first();
        let (k, w, h) = match best.map(|w| w.kind) {
            Some(WindowKind::HourOfDay { hour }) => (Some("hour"), None, Some(i16::from(hour))),
            Some(WindowKind::WeekdayHour { weekday, hour }) => (
                Some("weekday_hour"),
                Some(i16::from(weekday.number_days_from_monday())),
                Some(i16::from(hour)),
            ),
            Some(WindowKind::Weekday { weekday }) => (
                Some("weekday"),
                Some(i16::from(weekday.number_days_from_monday())),
                None,
            ),
            None => (None, None, None),
        };
        columns.kind.push(k.map(str::to_owned));
        columns.weekday.push(w);
        columns.hour.push(h);
        columns.sell.push(best.map(|w| w.sell_cents));
    }
    store
        .update_expansions(REGION_ID, STATION_ID, latest, &columns)
        .await?;
    Ok(true)
}
