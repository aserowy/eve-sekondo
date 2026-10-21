//! `station_item_stats`: everything about a tradable item that does not depend on an account, kept
//! up to date at three points. The maths are the pure functions of [`super::analysis`]; SQL only
//! collects and stores.

use std::collections::HashMap;

use time::{Date, Duration, OffsetDateTime};

use crate::store::{BookFields, IndicatorColumns, StationStore, StoreError, WindowRow};

use super::{
    REGION_ID, STATION_ID,
    analysis::{
        ExpandedHour, FlaggedDay, Medians, bollinger, buy_side_ratio, recurring_windows, rsi14,
        sma, sma_crossing, volatility, volume_rising,
    },
};

/// How far back the buy side ratio looks, in days.
const RHO_DAYS: i64 = 30;
/// Days of history the indicators read.
const HISTORY_DAYS: i64 = 30;
/// Days of flagged days the recurrence reads: 52 weeks.
const RECURRENCE_DAYS: i64 = 52 * 7;
/// Days whose hourly averages make the medians the windows are priced with.
const MEDIAN_DAYS: i64 = 7;

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

/// How long flagged days and expanded hours are kept, in days.
const FLAG_DAYS: i64 = 365;

/// Flags every completed UTC day of the hourly rollup that is not flagged yet: a day is complete
/// once the newest rollup hour is at least an hour past its end. Then drops what is older than a
/// year. Returns how many days it flagged.
pub async fn flag_completed_days(
    store: &dyn StationStore,
    now: OffsetDateTime,
) -> Result<usize, StoreError> {
    let Some(latest) = store.latest_hour(REGION_ID, STATION_ID).await? else {
        return Ok(0);
    };
    let days = store.unflagged_days(REGION_ID, STATION_ID, latest).await?;
    if !days.is_empty() {
        store.flag_days(REGION_ID, STATION_ID, &days).await?;
    }
    store
        .prune_flags(
            REGION_ID,
            STATION_ID,
            now.date() - Duration::days(FLAG_DAYS),
        )
        .await?;
    Ok(days.len())
}

/// When the hourly rollup advanced (its newest hour differs from the one the item rows were last
/// computed for): flags the days that completed, then stores the recurring windows of every item
/// from the flagged days and the 7-day medians. Returns whether it recomputed.
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
    flag_completed_days(store, now).await?;

    let from = now.date() - Duration::days(RECURRENCE_DAYS);
    let mut flagged: HashMap<i64, Vec<FlaggedDay>> = HashMap::new();
    for row in store.flagged_days(REGION_ID, STATION_ID, from).await? {
        flagged.entry(row.type_id).or_default().push(FlaggedDay {
            day: row.day,
            hours: usize::try_from(row.hours).unwrap_or_default(),
            expanded: usize::try_from(row.expanded_hours).unwrap_or_default(),
        });
    }
    let mut expanded: HashMap<i64, Vec<ExpandedHour>> = HashMap::new();
    for row in store
        .expanded_hours(REGION_ID, STATION_ID, from.midnight().assume_utc())
        .await?
    {
        expanded.entry(row.type_id).or_default().push(row.hour);
    }
    let medians = store
        .medians(REGION_ID, STATION_ID, now - Duration::days(MEDIAN_DAYS))
        .await?;

    let mut rows = Vec::new();
    for median in medians {
        let Some(days) = flagged.get(&median.type_id) else {
            continue;
        };
        let hours = expanded.get(&median.type_id).map_or(&[][..], Vec::as_slice);
        let medians = Medians {
            buy_cents: median.buy_cents,
            sell_cents: median.sell_cents,
        };
        rows.extend(
            recurring_windows(days, hours, medians, now)
                .into_iter()
                .map(|w| WindowRow {
                    type_id: median.type_id,
                    kind: w.kind,
                    direction: w.direction,
                    price_cents: w.price_cents,
                    occurrences: w.occurrences.len() as i32,
                    covered: w.covered as i32,
                    v_win: w.v_win,
                }),
        );
    }
    store
        .replace_windows(REGION_ID, STATION_ID, latest, &rows)
        .await?;
    Ok(true)
}
