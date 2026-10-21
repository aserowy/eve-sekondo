use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use time::{Date, Duration, OffsetDateTime, Time};

use super::{
    Shared, backoff, micros,
    numeric::Numeric,
    tables::{Bucket, HistoryRow, ItemStatsRow, SnapshotRow, StationOrderRow, Sync, Tables},
};
use crate::{
    esi::EsiHistoryDay,
    opportunities::analysis::{HourFigures, flag_day, median},
    store::{
        BookFields, ExpandedHourRow, FlaggedDayRow, HistoryRecord, HistoryRow as HistoryDay,
        IndicatorColumns, ItemStatsRecord, LatestSnapshot, MedianRow, SeedHistoryRow,
        SeedSnapshotRow, SnapshotRecord, StationOrder, StationStore, StoreError, StoreResult,
        SyncState, WindowRow,
    },
};

pub struct MemStationStore {
    pub(super) shared: std::sync::Arc<Shared>,
}

fn hour_of(at: OffsetDateTime) -> OffsetDateTime {
    at.to_offset(time::UtcOffset::UTC).replace_time(
        Time::from_hms(at.to_offset(time::UtcOffset::UTC).hour(), 0, 0).expect("an hour"),
    )
}

fn day_of(at: OffsetDateTime) -> OffsetDateTime {
    at.to_offset(time::UtcOffset::UTC)
        .replace_time(Time::MIDNIGHT)
}

/// The first bucket start at or after `at`.
fn ceil_to(
    at: OffsetDateTime,
    bucket: fn(OffsetDateTime) -> OffsetDateTime,
    width: Duration,
) -> OffsetDateTime {
    let floor = bucket(at);
    if floor == at { at } else { floor + width }
}

fn tradable(tables: &Tables, region_id: i64, station_id: i64) -> BTreeSet<i64> {
    let mut sides: BTreeMap<i64, (bool, bool)> = BTreeMap::new();
    for o in tables
        .station_orders
        .values()
        .filter(|o| o.region_id == region_id && o.station_id == station_id)
    {
        let entry = sides.entry(o.type_id).or_default();
        if o.is_buy_order {
            entry.0 = true;
        } else {
            entry.1 = true;
        }
    }
    sides
        .into_iter()
        .filter(|(_, (buy, sell))| *buy && *sell)
        .map(|(type_id, _)| type_id)
        .collect()
}

fn sum(values: impl IntoIterator<Item = Numeric>) -> Numeric {
    values
        .into_iter()
        .fold(None::<Numeric>, |acc, v| {
            Some(match acc {
                Some(acc) => acc.add(&v),
                None => v,
            })
        })
        .unwrap_or_else(Numeric::zero)
}

fn minimum(values: impl IntoIterator<Item = Numeric>) -> Numeric {
    values.into_iter().min().expect("a bucket has a row")
}

fn maximum(values: impl IntoIterator<Item = Numeric>) -> Numeric {
    values.into_iter().max().expect("a bucket has a row")
}

fn average(values: impl IntoIterator<Item = Numeric>) -> Numeric {
    let values: Vec<Numeric> = values.into_iter().collect();
    let count = Numeric::int(values.len() as i64);
    sum(values).div(&count).expect("a bucket has a row")
}

/// The hourly rollup of the snapshots in `[start, end)`: one bucket per (item, hour) that has a
/// snapshot, replacing what was materialized there.
fn refresh_hourly(tables: &mut Tables, start: OffsetDateTime, end: OffsetDateTime) {
    tables
        .hourly
        .retain(|(_, _, _, bucket), _| !(*bucket >= start && *bucket < end));
    let mut groups: BTreeMap<(i64, i64, i64, OffsetDateTime), Vec<&SnapshotRow>> = BTreeMap::new();
    for s in &tables.station_snapshots {
        let bucket = hour_of(s.recorded_at);
        if bucket >= start && bucket < end {
            groups
                .entry((s.region_id, s.station_id, s.type_id, bucket))
                .or_default()
                .push(s);
        }
    }
    let built: Vec<_> = groups
        .into_iter()
        .map(|(key, rows)| {
            (
                key,
                Bucket {
                    best_buy_min: minimum(rows.iter().map(|r| r.best_buy.clone())),
                    best_buy_max: maximum(rows.iter().map(|r| r.best_buy.clone())),
                    best_buy_avg: average(rows.iter().map(|r| r.best_buy.clone())),
                    best_sell_min: minimum(rows.iter().map(|r| r.best_sell.clone())),
                    best_sell_max: maximum(rows.iter().map(|r| r.best_sell.clone())),
                    best_sell_avg: average(rows.iter().map(|r| r.best_sell.clone())),
                    spread_min: minimum(rows.iter().map(|r| r.spread.clone())),
                    spread_max: maximum(rows.iter().map(|r| r.spread.clone())),
                    spread_avg: average(rows.iter().map(|r| r.spread.clone())),
                    buy_taken: rows.iter().map(|r| r.buy_taken).sum(),
                    sell_taken: rows.iter().map(|r| r.sell_taken).sum(),
                    snapshots: rows.len() as i64,
                    hours_covered: 0,
                },
            )
        })
        .collect();
    tables.hourly.extend(built);
}

/// The daily rollup of the materialized hourly buckets in `[start, end)`.
fn refresh_daily(tables: &mut Tables, start: OffsetDateTime, end: OffsetDateTime) {
    tables
        .daily
        .retain(|(_, _, _, bucket), _| !(*bucket >= start && *bucket < end));
    let mut groups: BTreeMap<(i64, i64, i64, OffsetDateTime), Vec<&Bucket>> = BTreeMap::new();
    for ((region, station, type_id, hour), bucket) in &tables.hourly {
        let day = day_of(*hour);
        if day >= start && day < end {
            groups
                .entry((*region, *station, *type_id, day))
                .or_default()
                .push(bucket);
        }
    }
    let built: Vec<_> = groups
        .into_iter()
        .map(|(key, hours)| {
            let snapshots: i64 = hours.iter().map(|h| h.snapshots).sum();
            let weighted = |value: fn(&Bucket) -> &Numeric| {
                sum(hours
                    .iter()
                    .map(|h| value(h).mul(&Numeric::int(h.snapshots))))
                .div(&Numeric::int(snapshots))
                .expect("a day has snapshots")
            };
            (
                key,
                Bucket {
                    best_buy_min: minimum(hours.iter().map(|h| h.best_buy_min.clone())),
                    best_buy_max: maximum(hours.iter().map(|h| h.best_buy_max.clone())),
                    best_buy_avg: weighted(|h| &h.best_buy_avg),
                    best_sell_min: minimum(hours.iter().map(|h| h.best_sell_min.clone())),
                    best_sell_max: maximum(hours.iter().map(|h| h.best_sell_max.clone())),
                    best_sell_avg: weighted(|h| &h.best_sell_avg),
                    spread_min: minimum(hours.iter().map(|h| h.spread_min.clone())),
                    spread_max: maximum(hours.iter().map(|h| h.spread_max.clone())),
                    spread_avg: weighted(|h| &h.spread_avg),
                    buy_taken: hours.iter().map(|h| h.buy_taken).sum(),
                    sell_taken: hours.iter().map(|h| h.sell_taken).sum(),
                    snapshots,
                    hours_covered: hours.len() as i64,
                },
            )
        })
        .collect();
    tables.daily.extend(built);
}

/// A row of the `station_daily_figures` view: `(type, day, spread, high)`.
pub(super) struct DailyFigure {
    pub day: Date,
    pub spread: Option<Numeric>,
    pub high: Numeric,
}

/// The view's rows of one region: every history day, with the daily rollup where it covers 20
/// hours or more and the history's own range otherwise.
pub(super) fn daily_figures(
    tables: &Tables,
    region_id: i64,
    from: Date,
    only_type: Option<i64>,
) -> Vec<DailyFigure> {
    tables
        .market_history
        .iter()
        .filter(|((region, type_id, day), _)| {
            *region == region_id && *day >= from && only_type.is_none_or(|t| t == *type_id)
        })
        .map(|((_, type_id, day), history)| {
            let midnight = day.with_time(Time::MIDNIGHT).assume_utc();
            let rollup = tables
                .daily
                .iter()
                .find(|((region, _, t, bucket), _)| {
                    *region == region_id && t == type_id && *bucket == midnight
                })
                .map(|(_, bucket)| bucket)
                .filter(|bucket| bucket.hours_covered >= 20);
            match rollup {
                Some(bucket) => DailyFigure {
                    day: *day,
                    spread: Some(bucket.spread_max.clone()),
                    high: bucket.best_sell_max.clone(),
                },
                None => DailyFigure {
                    day: *day,
                    spread: history.highest.sub(&history.lowest).div(&history.lowest),
                    high: history.highest.clone(),
                },
            }
        })
        .collect()
}

fn text_to_numeric(text: &str) -> StoreResult<Numeric> {
    Numeric::parse(text).ok_or_else(|| {
        StoreError::Backend(format!("invalid input syntax for type numeric: \"{text}\""))
    })
}

#[async_trait]
impl StationStore for MemStationStore {
    async fn claim_book_refresh(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
        lease: Duration,
    ) -> StoreResult<bool> {
        let now = micros(now);
        let mut tables = self.shared.lock();
        let row = tables
            .station_sync
            .entry((region_id, station_id, "book".to_owned()))
            .or_insert_with(|| Sync::scheduled(now));
        if row.next_poll_at <= now {
            row.next_poll_at = now + lease;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn tradable_items(&self, region_id: i64, station_id: i64) -> StoreResult<Vec<i64>> {
        Ok(tradable(&self.shared.lock(), region_id, station_id)
            .into_iter()
            .collect())
    }

    async fn replace_station_book(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
        rows: &[StationOrder],
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<()> {
        let now = micros(now);
        let mut tables = self.shared.lock();
        let mut ids = BTreeSet::new();
        let duplicated = !rows.iter().all(|r| ids.insert(r.order_id));
        let foreign = rows.iter().any(|r| {
            tables
                .station_orders
                .get(&r.order_id)
                .is_some_and(|o| !(o.region_id == region_id && o.station_id == station_id))
        });
        if duplicated || foreign {
            return Err(StoreError::Backend(
                "duplicate key value violates unique constraint \"station_orders_pkey\"".to_owned(),
            ));
        }

        // Snapshots of the tradable items, from the new book and the one it replaces.
        let mut by_type: BTreeMap<i64, Vec<&StationOrder>> = BTreeMap::new();
        for row in rows {
            by_type.entry(row.type_id).or_default().push(row);
        }
        let previous: BTreeMap<i64, &StationOrderRow> = tables
            .station_orders
            .iter()
            .filter(|(_, o)| o.region_id == region_id && o.station_id == station_id)
            .map(|(id, o)| (*id, o))
            .collect();
        let mut snapshots = Vec::new();
        for (type_id, orders) in &by_type {
            let buys: Vec<&&StationOrder> = orders.iter().filter(|o| o.is_buy_order).collect();
            let sells: Vec<&&StationOrder> = orders.iter().filter(|o| !o.is_buy_order).collect();
            let (Some(best_buy), Some(best_sell)) = (
                buys.iter().map(|o| o.price_cents).max(),
                sells.iter().map(|o| o.price_cents).min(),
            ) else {
                continue;
            };
            if best_buy <= 0 {
                continue;
            }
            let taken = |side: &[&&StationOrder]| -> i64 {
                side.iter()
                    .filter_map(|n| {
                        previous
                            .get(&n.order_id)
                            .map(|o| (o.volume_remain - n.volume_remain).max(0))
                    })
                    .sum()
            };
            let near_buy: Vec<&&&StationOrder> = buys
                .iter()
                .filter(|o| i128::from(o.price_cents) * 100 >= i128::from(best_buy) * 99)
                .collect();
            let near_sell: Vec<&&&StationOrder> = sells
                .iter()
                .filter(|o| i128::from(o.price_cents) * 100 <= i128::from(best_sell) * 101)
                .collect();
            let (buy, sell) = (Numeric::cents(best_buy), Numeric::cents(best_sell));
            let Some(spread) = sell.sub(&buy).div(&buy) else {
                continue;
            };
            snapshots.push(SnapshotRow {
                recorded_at: now,
                region_id,
                station_id,
                type_id: *type_id,
                best_buy: buy,
                best_sell: sell,
                spread,
                buy_near_units: near_buy.iter().map(|o| o.volume_remain).sum(),
                buy_near_orders: near_buy.len() as i32,
                sell_near_units: near_sell.iter().map(|o| o.volume_remain).sum(),
                sell_near_orders: near_sell.len() as i32,
                buy_units: buys.iter().map(|o| o.volume_remain).sum(),
                sell_units: sells.iter().map(|o| o.volume_remain).sum(),
                buy_taken: taken(&buys),
                sell_taken: taken(&sells),
            });
        }
        tables.station_snapshots.extend(snapshots);

        tables
            .station_orders
            .retain(|_, o| !(o.region_id == region_id && o.station_id == station_id));
        for row in rows {
            tables.station_orders.insert(
                row.order_id,
                StationOrderRow {
                    region_id,
                    station_id,
                    type_id: row.type_id,
                    is_buy_order: row.is_buy_order,
                    price_cents: row.price_cents,
                    volume_remain: row.volume_remain,
                },
            );
        }
        let row = tables
            .station_sync
            .entry((region_id, station_id, "book".to_owned()))
            .or_insert_with(|| Sync::scheduled(micros(next_poll_at)));
        row.next_poll_at = micros(next_poll_at);
        row.last_success_at = Some(now);
        row.last_attempt_at = Some(now);
        row.last_error = None;
        row.esi_expires_at = esi_expires_at.map(micros);
        row.failures = 0;
        Ok(())
    }

    async fn record_book_failure(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
        reason: &str,
        backoff_start: Duration,
        interval: Duration,
    ) -> StoreResult<()> {
        let now = micros(now);
        let mut tables = self.shared.lock();
        match tables
            .station_sync
            .get_mut(&(region_id, station_id, "book".to_owned()))
        {
            Some(row) => {
                row.next_poll_at = now + backoff(backoff_start, row.failures, interval);
                row.last_attempt_at = Some(now);
                row.last_error = Some(reason.to_owned());
                row.failures += 1;
            }
            None => {
                let mut row = Sync::scheduled(now + backoff_start);
                row.last_attempt_at = Some(now);
                row.last_error = Some(reason.to_owned());
                row.failures = 1;
                tables
                    .station_sync
                    .insert((region_id, station_id, "book".to_owned()), row);
            }
        }
        Ok(())
    }

    async fn record_book_deferral(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
    ) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        let row = tables
            .station_sync
            .entry((region_id, station_id, "book".to_owned()))
            .or_insert_with(|| Sync::scheduled(micros(next_poll_at)));
        row.next_poll_at = micros(next_poll_at);
        row.last_attempt_at = Some(micros(now));
        Ok(())
    }

    async fn track_history(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
    ) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        for type_id in tradable(&tables, region_id, station_id) {
            tables
                .history_sync
                .entry((region_id, type_id))
                .or_insert_with(|| Sync::scheduled(micros(now)));
        }
        Ok(())
    }

    async fn claim_due_history(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
        lease: Duration,
        limit: i64,
    ) -> StoreResult<Vec<i64>> {
        let now = micros(now);
        let mut tables = self.shared.lock();
        let tradable = tradable(&tables, region_id, station_id);
        let mut due: Vec<(OffsetDateTime, i64)> = tables
            .history_sync
            .iter()
            .filter(|((region, type_id), s)| {
                *region == region_id && s.next_poll_at <= now && tradable.contains(type_id)
            })
            .map(|((_, type_id), s)| (s.next_poll_at, *type_id))
            .collect();
        due.sort();
        due.truncate(limit.max(0) as usize);
        let mut claimed = Vec::new();
        for (_, type_id) in due {
            if let Some(row) = tables.history_sync.get_mut(&(region_id, type_id)) {
                row.next_poll_at = now + lease;
            }
            claimed.push(type_id);
        }
        Ok(claimed)
    }

    async fn replace_history_days(
        &self,
        region_id: i64,
        type_id: i64,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
        keep_from: Date,
        kept: &[EsiHistoryDay],
    ) -> StoreResult<()> {
        let mut days = Vec::new();
        for d in kept {
            let day = Date::parse(
                &d.date,
                &time::format_description::well_known::Iso8601::DATE,
            )
            .map_err(|_| {
                StoreError::Backend(format!(
                    "invalid input syntax for type date: \"{}\"",
                    d.date
                ))
            })?;
            days.push((day, d));
        }
        let mut seen = BTreeSet::new();
        if !days.iter().all(|(day, _)| seen.insert(*day)) {
            return Err(StoreError::Backend(
                "ON CONFLICT DO UPDATE command cannot affect row a second time".to_owned(),
            ));
        }
        let mut tables = self.shared.lock();
        for (day, d) in days {
            tables.market_history.insert(
                (region_id, type_id, day),
                HistoryRow {
                    average: Numeric::cents(d.average_cents),
                    highest: Numeric::cents(d.highest_cents),
                    lowest: Numeric::cents(d.lowest_cents),
                    volume: d.volume,
                    order_count: d.order_count,
                },
            );
        }
        if let Some(row) = tables.history_sync.get_mut(&(region_id, type_id)) {
            row.next_poll_at = micros(next_poll_at);
            row.last_success_at = Some(micros(now));
            row.last_attempt_at = Some(micros(now));
            row.last_error = None;
            row.esi_expires_at = Some(micros(next_poll_at));
            row.failures = 0;
        }
        tables
            .market_history
            .retain(|(region, _, day), _| !(*region == region_id && *day < keep_from));
        Ok(())
    }

    async fn schedule_history(
        &self,
        region_id: i64,
        type_id: i64,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
    ) -> StoreResult<()> {
        if let Some(row) = self
            .shared
            .lock()
            .history_sync
            .get_mut(&(region_id, type_id))
        {
            row.next_poll_at = micros(next_poll_at);
            row.last_attempt_at = Some(micros(now));
            row.last_error = None;
            row.failures = 0;
        }
        Ok(())
    }

    async fn record_history_failure(
        &self,
        region_id: i64,
        type_id: i64,
        now: OffsetDateTime,
        reason: &str,
        backoff_start: Duration,
        max_backoff: Duration,
    ) -> StoreResult<()> {
        if let Some(row) = self
            .shared
            .lock()
            .history_sync
            .get_mut(&(region_id, type_id))
        {
            row.next_poll_at = micros(now) + backoff(backoff_start, row.failures, max_backoff);
            row.last_attempt_at = Some(micros(now));
            row.last_error = Some(reason.to_owned());
            row.failures += 1;
        }
        Ok(())
    }

    async fn finish_history_pass(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
        any_ok: bool,
        last_error: Option<&str>,
    ) -> StoreResult<()> {
        let now = micros(now);
        let mut tables = self.shared.lock();
        let row = tables
            .station_sync
            .entry((region_id, station_id, "history".to_owned()))
            .or_insert_with(|| {
                let mut row = Sync::scheduled(now);
                row.last_success_at = any_ok.then_some(now);
                row
            });
        if any_ok {
            row.last_success_at = Some(now);
        }
        row.last_attempt_at = Some(now);
        row.last_error = last_error.map(str::to_owned);
        Ok(())
    }

    async fn prune_item_stats(&self, region_id: i64, station_id: i64) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        let tradable = tradable(&tables, region_id, station_id);
        tables.item_stats.retain(|(region, station, type_id), _| {
            !(*region == region_id && *station == station_id) || tradable.contains(type_id)
        });
        Ok(())
    }

    async fn latest_snapshots(
        &self,
        region_id: i64,
        station_id: i64,
        since: OffsetDateTime,
    ) -> StoreResult<Vec<LatestSnapshot>> {
        let tables = self.shared.lock();
        let tradable = tradable(&tables, region_id, station_id);
        let mut newest: BTreeMap<i64, &SnapshotRow> = BTreeMap::new();
        for s in tables.station_snapshots.iter().filter(|s| {
            s.region_id == region_id
                && s.station_id == station_id
                && s.recorded_at >= since
                && tradable.contains(&s.type_id)
        }) {
            let entry = newest.entry(s.type_id).or_insert(s);
            if s.recorded_at > entry.recorded_at {
                *entry = s;
            }
        }
        Ok(newest
            .into_iter()
            .map(|(type_id, s)| {
                (
                    type_id,
                    s.best_buy.to_string(),
                    s.best_sell.to_string(),
                    s.buy_near_orders,
                    s.sell_near_orders,
                )
            })
            .collect())
    }

    async fn units_taken(
        &self,
        region_id: i64,
        station_id: i64,
        from: OffsetDateTime,
    ) -> StoreResult<Vec<(i64, i64, i64)>> {
        let tables = self.shared.lock();
        let cut = tables
            .daily
            .iter()
            .filter(|((region, station, _, _), _)| *region == region_id && *station == station_id)
            .map(|((_, _, _, bucket), _)| *bucket)
            .max()
            .map_or(from, |latest| latest + Duration::days(1));
        let mut taken: BTreeMap<i64, (i64, i64)> = BTreeMap::new();
        for ((region, station, type_id, bucket), b) in &tables.daily {
            if *region == region_id && *station == station_id && *bucket >= from && *bucket < cut {
                let entry = taken.entry(*type_id).or_default();
                entry.0 += b.buy_taken;
                entry.1 += b.sell_taken;
            }
        }
        for ((region, station, type_id, bucket), b) in &tables.hourly {
            if *region == region_id && *station == station_id && *bucket >= cut.max(from) {
                let entry = taken.entry(*type_id).or_default();
                entry.0 += b.buy_taken;
                entry.1 += b.sell_taken;
            }
        }
        Ok(taken.into_iter().map(|(t, (b, s))| (t, b, s)).collect())
    }

    async fn upsert_book_fields(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
        rows: &[BookFields],
    ) -> StoreResult<()> {
        let mut seen = BTreeSet::new();
        if !rows.iter().all(|r| seen.insert(r.type_id)) {
            return Err(StoreError::Backend(
                "ON CONFLICT DO UPDATE command cannot affect row a second time".to_owned(),
            ));
        }
        let parsed: Vec<(Numeric, Numeric)> = rows
            .iter()
            .map(|r| {
                Ok((
                    text_to_numeric(&r.best_buy)?,
                    text_to_numeric(&r.best_sell)?,
                ))
            })
            .collect::<StoreResult<_>>()?;
        let mut tables = self.shared.lock();
        for (row, (best_buy, best_sell)) in rows.iter().zip(parsed) {
            let stats = tables
                .item_stats
                .entry((region_id, station_id, row.type_id))
                .or_default();
            stats.book_at = Some(micros(now));
            stats.best_buy = Some(best_buy.round(2));
            stats.best_sell = Some(best_sell.round(2));
            stats.buy_near_orders = Some(row.buy_near_orders);
            stats.sell_near_orders = Some(row.sell_near_orders);
            stats.rho = Some(row.rho);
        }
        Ok(())
    }

    async fn history_rows(
        &self,
        region_id: i64,
        station_id: i64,
        from: Date,
    ) -> StoreResult<Vec<HistoryDay>> {
        let tables = self.shared.lock();
        Ok(tables
            .market_history
            .iter()
            .filter(|((region, type_id, day), _)| {
                *region == region_id
                    && *day >= from
                    && tables
                        .item_stats
                        .contains_key(&(region_id, station_id, *type_id))
            })
            .map(|((_, type_id, day), h)| (*type_id, *day, h.average.to_f64(), h.volume))
            .collect())
    }

    async fn item_stat_ids(&self, region_id: i64, station_id: i64) -> StoreResult<Vec<i64>> {
        let tables = self.shared.lock();
        Ok(tables
            .item_stats
            .keys()
            .filter(|(region, station, _)| *region == region_id && *station == station_id)
            .map(|(_, _, type_id)| *type_id)
            .collect())
    }

    async fn update_indicators(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
        columns: &IndicatorColumns,
    ) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        for (i, type_id) in columns.type_id.iter().enumerate() {
            if let Some(stats) = tables
                .item_stats
                .get_mut(&(region_id, station_id, *type_id))
            {
                stats.indicators_at = Some(micros(now));
                stats.volume_7d = columns.volume_7d[i];
                stats.volume_30d = columns.volume_30d[i];
                stats.sigma = columns.sigma[i];
                stats.returns = columns.returns[i];
                stats.sma5 = columns.sma5[i];
                stats.sma20 = columns.sma20[i];
                stats.cross_day = columns.cross_day[i];
                stats.volume_rising = columns.volume_rising[i];
                stats.percent_b = columns.percent_b[i];
                stats.rsi14 = columns.rsi14[i];
            }
        }
        Ok(())
    }

    async fn latest_hour(
        &self,
        region_id: i64,
        station_id: i64,
    ) -> StoreResult<Option<OffsetDateTime>> {
        let tables = self.shared.lock();
        Ok(tables
            .hourly
            .keys()
            .filter(|(region, station, _, _)| *region == region_id && *station == station_id)
            .map(|(_, _, _, bucket)| *bucket)
            .max())
    }

    async fn expansions_stale(
        &self,
        region_id: i64,
        station_id: i64,
        latest: OffsetDateTime,
    ) -> StoreResult<bool> {
        let tables = self.shared.lock();
        Ok(tables
            .item_stats
            .iter()
            .filter(|((region, station, _), _)| *region == region_id && *station == station_id)
            .any(|(_, stats)| stats.expansion_at != Some(latest)))
    }

    async fn own_hourly_days(
        &self,
        region_id: i64,
        station_id: i64,
        from: OffsetDateTime,
    ) -> StoreResult<i64> {
        let tables = self.shared.lock();
        let days: BTreeSet<OffsetDateTime> = tables
            .hourly
            .keys()
            .filter(|(region, station, _, bucket)| {
                *region == region_id && *station == station_id && *bucket >= from
            })
            .map(|(_, _, _, bucket)| day_of(*bucket))
            .collect();
        Ok(days.len() as i64)
    }

    async fn hourly_points(
        &self,
        region_id: i64,
        station_id: i64,
        from: OffsetDateTime,
    ) -> StoreResult<Vec<(i64, OffsetDateTime, f64, f64, i64)>> {
        let tables = self.shared.lock();
        let mut points: Vec<(i64, OffsetDateTime, f64, f64, i64)> = tables
            .hourly
            .iter()
            .filter(|((region, station, _, bucket), _)| {
                *region == region_id && *station == station_id && *bucket >= from
            })
            .map(|((_, _, type_id, bucket), b)| {
                (
                    *type_id,
                    *bucket,
                    b.spread_avg.to_f64(),
                    b.spread_max.to_f64(),
                    b.best_sell_max
                        .mul(&Numeric::int(100))
                        .to_i64()
                        .unwrap_or_default(),
                )
            })
            .collect();
        points.sort_by_key(|p| (p.0, p.1));
        Ok(points)
    }

    async fn unflagged_days(
        &self,
        region_id: i64,
        station_id: i64,
        latest: OffsetDateTime,
    ) -> StoreResult<Vec<Date>> {
        let tables = self.shared.lock();
        // Only the days after the newest flagged one, like the PostgreSQL store.
        let newest = tables
            .flagged_days
            .keys()
            .filter(|(region, station, _, _)| *region == region_id && *station == station_id)
            .map(|(_, _, _, day)| *day)
            .max();
        let days: BTreeSet<Date> = tables
            .hourly
            .keys()
            .filter(|(region, station, _, _)| *region == region_id && *station == station_id)
            .map(|(_, _, _, bucket)| bucket.to_offset(time::UtcOffset::UTC).date())
            .filter(|day| newest.is_none_or(|newest| *day > newest))
            .filter(|day| {
                day.midnight().assume_utc() + Duration::days(1) + Duration::hours(1) <= latest
            })
            .collect();
        Ok(days.into_iter().collect())
    }

    async fn flag_days(&self, region_id: i64, station_id: i64, days: &[Date]) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        for day in days {
            let start = day.midnight().assume_utc();
            let end = start + Duration::days(1);
            let mut by_item: BTreeMap<i64, Vec<HourFigures>> = BTreeMap::new();
            for ((region, station, type_id, bucket), b) in &tables.hourly {
                if *region == region_id
                    && *station == station_id
                    && *bucket >= start
                    && *bucket < end
                {
                    let cents = |n: &Numeric| n.mul(&Numeric::int(100));
                    by_item.entry(*type_id).or_default().push(HourFigures {
                        start: *bucket,
                        spread_max: b.spread_max.to_f64(),
                        best_buy_avg: cents(&b.best_buy_avg).to_f64(),
                        best_sell_avg: cents(&b.best_sell_avg).to_f64(),
                        best_buy_min_cents: cents(&b.best_buy_min).to_i64().unwrap_or_default(),
                        best_sell_max_cents: cents(&b.best_sell_max).to_i64().unwrap_or_default(),
                        buy_taken: b.buy_taken,
                        sell_taken: b.sell_taken,
                    });
                }
            }
            for (type_id, hours) in by_item {
                let key = (region_id, station_id, type_id, *day);
                if tables.flagged_days.contains_key(&key) {
                    continue;
                }
                let flags = flag_day(&hours);
                tables
                    .flagged_days
                    .insert(key, (flags.hours as i32, flags.expanded.len() as i32));
                for hour in flags.expanded {
                    tables
                        .expanded_hours
                        .entry((region_id, station_id, type_id, hour.start))
                        .or_insert(hour);
                }
            }
        }
        Ok(())
    }

    async fn flagged_days(
        &self,
        region_id: i64,
        station_id: i64,
        from: Date,
    ) -> StoreResult<Vec<FlaggedDayRow>> {
        let tables = self.shared.lock();
        Ok(tables
            .flagged_days
            .iter()
            .filter(|((region, station, _, day), _)| {
                *region == region_id && *station == station_id && *day >= from
            })
            .map(|((_, _, type_id, day), (hours, expanded))| FlaggedDayRow {
                type_id: *type_id,
                day: *day,
                hours: *hours,
                expanded_hours: *expanded,
            })
            .collect())
    }

    async fn expanded_hours(
        &self,
        region_id: i64,
        station_id: i64,
        from: OffsetDateTime,
    ) -> StoreResult<Vec<ExpandedHourRow>> {
        let tables = self.shared.lock();
        Ok(tables
            .expanded_hours
            .iter()
            .filter(|((region, station, _, start), _)| {
                *region == region_id && *station == station_id && *start >= from
            })
            .map(|((_, _, type_id, _), hour)| ExpandedHourRow {
                type_id: *type_id,
                hour: *hour,
            })
            .collect())
    }

    async fn medians(
        &self,
        region_id: i64,
        station_id: i64,
        from: OffsetDateTime,
    ) -> StoreResult<Vec<MedianRow>> {
        let tables = self.shared.lock();
        let mut by_item: BTreeMap<i64, (Vec<f64>, Vec<f64>)> = BTreeMap::new();
        for ((region, station, type_id, bucket), b) in &tables.hourly {
            if *region == region_id && *station == station_id && *bucket >= from {
                let entry = by_item.entry(*type_id).or_default();
                entry
                    .0
                    .push(b.best_buy_avg.mul(&Numeric::int(100)).to_f64());
                entry
                    .1
                    .push(b.best_sell_avg.mul(&Numeric::int(100)).to_f64());
            }
        }
        Ok(by_item
            .into_iter()
            .filter_map(|(type_id, (buy, sell))| {
                Some(MedianRow {
                    type_id,
                    buy_cents: median(buy)?,
                    sell_cents: median(sell)?,
                })
            })
            .collect())
    }

    async fn replace_windows(
        &self,
        region_id: i64,
        station_id: i64,
        latest: OffsetDateTime,
        windows: &[WindowRow],
    ) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        tables
            .item_windows
            .retain(|(region, station, _)| !(*region == region_id && *station == station_id));
        tables
            .item_windows
            .extend(windows.iter().map(|w| (region_id, station_id, w.clone())));
        for ((region, station, _), stats) in tables.item_stats.iter_mut() {
            if *region == region_id && *station == station_id {
                stats.expansion_at = Some(micros(latest));
            }
        }
        Ok(())
    }

    async fn windows(&self, region_id: i64, station_id: i64) -> StoreResult<Vec<WindowRow>> {
        let tables = self.shared.lock();
        let mut windows: Vec<WindowRow> = tables
            .item_windows
            .iter()
            .filter(|(region, station, _)| *region == region_id && *station == station_id)
            .map(|(_, _, w)| w.clone())
            .collect();
        windows.sort_by_key(|w| (w.type_id, w.columns(), w.direction));
        Ok(windows)
    }

    async fn prune_flags(&self, region_id: i64, station_id: i64, before: Date) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        let cut = before.midnight().assume_utc();
        tables.flagged_days.retain(|(region, station, _, day), _| {
            !(*region == region_id && *station == station_id && *day < before)
        });
        tables
            .expanded_hours
            .retain(|(region, station, _, start), _| {
                !(*region == region_id && *station == station_id && *start < cut)
            });
        Ok(())
    }

    async fn refresh_rollups(&self, from: OffsetDateTime, to: OffsetDateTime) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        // The window is shrunk to whole buckets, and must hold at least one.
        for (bucket, width, hourly) in [
            (
                hour_of as fn(OffsetDateTime) -> OffsetDateTime,
                Duration::hours(1),
                true,
            ),
            (day_of, Duration::days(1), false),
        ] {
            let start = ceil_to(from, bucket, width);
            let end = bucket(to);
            if start >= end {
                return Err(StoreError::Backend("refresh window too small".to_owned()));
            }
            if hourly {
                refresh_hourly(&mut tables, start, end);
            } else {
                refresh_daily(&mut tables, start, end);
            }
        }
        Ok(())
    }

    async fn seed_snapshots(
        &self,
        region_id: i64,
        station_id: i64,
        rows: &[SeedSnapshotRow],
    ) -> StoreResult<()> {
        let mut built = Vec::new();
        for s in rows {
            let buy = Numeric::from_f64(s.best_buy);
            let sell = Numeric::from_f64(s.best_sell);
            let spread = sell
                .sub(&buy)
                .div(&buy)
                .ok_or_else(|| StoreError::Backend("division by zero".to_owned()))?;
            built.push(SnapshotRow {
                recorded_at: micros(s.recorded_at),
                region_id,
                station_id,
                type_id: s.type_id,
                best_buy: buy.round(2),
                best_sell: sell.round(2),
                spread,
                buy_near_units: s.buy_near_units,
                buy_near_orders: s.buy_near_orders,
                sell_near_units: s.sell_near_units,
                sell_near_orders: s.sell_near_orders,
                buy_units: s.buy_units,
                sell_units: s.sell_units,
                buy_taken: s.buy_taken,
                sell_taken: s.sell_taken,
            });
        }
        self.shared.lock().station_snapshots.extend(built);
        Ok(())
    }

    async fn seed_history(&self, region_id: i64, rows: &[SeedHistoryRow]) -> StoreResult<()> {
        let mut seen = BTreeSet::new();
        if !rows.iter().all(|r| seen.insert((r.type_id, r.day))) {
            // Each statement affects a row once.
        }
        let mut tables = self.shared.lock();
        for h in rows {
            tables.market_history.insert(
                (region_id, h.type_id, h.day),
                HistoryRow {
                    average: Numeric::from_f64(h.average).round(2),
                    highest: Numeric::from_f64(h.highest).round(2),
                    lowest: Numeric::from_f64(h.lowest).round(2),
                    volume: h.volume,
                    order_count: h.order_count,
                },
            );
        }
        Ok(())
    }
    async fn station_sync_state(
        &self,
        region_id: i64,
        station_id: i64,
        kind: &str,
    ) -> StoreResult<Option<SyncState>> {
        Ok(self
            .shared
            .lock()
            .station_sync
            .get(&(region_id, station_id, kind.to_owned()))
            .map(Sync::state))
    }

    async fn history_sync(&self, region_id: i64, type_id: i64) -> StoreResult<Option<SyncState>> {
        Ok(self
            .shared
            .lock()
            .history_sync
            .get(&(region_id, type_id))
            .map(Sync::state))
    }
    async fn book(&self, region_id: i64, station_id: i64) -> StoreResult<Vec<StationOrder>> {
        let tables = self.shared.lock();
        Ok(tables
            .station_orders
            .iter()
            .filter(|(_, o)| o.region_id == region_id && o.station_id == station_id)
            .map(|(id, o)| StationOrder {
                order_id: *id,
                type_id: o.type_id,
                is_buy_order: o.is_buy_order,
                price_cents: o.price_cents,
                volume_remain: o.volume_remain,
            })
            .collect())
    }

    async fn snapshots(&self, region_id: i64, station_id: i64) -> StoreResult<Vec<SnapshotRecord>> {
        let tables = self.shared.lock();
        let mut rows: Vec<SnapshotRecord> = tables
            .station_snapshots
            .iter()
            .filter(|s| s.region_id == region_id && s.station_id == station_id)
            .map(|s| SnapshotRecord {
                recorded_at: s.recorded_at,
                type_id: s.type_id,
                best_buy: s.best_buy.to_string(),
                best_sell: s.best_sell.to_string(),
                spread: s.spread.to_string(),
                buy_near_units: s.buy_near_units,
                buy_near_orders: s.buy_near_orders,
                sell_near_units: s.sell_near_units,
                sell_near_orders: s.sell_near_orders,
                buy_units: s.buy_units,
                sell_units: s.sell_units,
                buy_taken: s.buy_taken,
                sell_taken: s.sell_taken,
            })
            .collect();
        rows.sort_by_key(|r| (r.recorded_at, r.type_id));
        Ok(rows)
    }

    async fn history_table(&self, region_id: i64, type_id: i64) -> StoreResult<Vec<HistoryRecord>> {
        let tables = self.shared.lock();
        Ok(tables
            .market_history
            .iter()
            .filter(|((region, t, _), _)| *region == region_id && *t == type_id)
            .map(|((_, t, day), h)| HistoryRecord {
                type_id: *t,
                day: *day,
                average: h.average.to_string(),
                highest: h.highest.to_string(),
                lowest: h.lowest.to_string(),
                volume: h.volume,
                order_count: h.order_count,
            })
            .collect())
    }

    async fn item_stats_row(
        &self,
        region_id: i64,
        station_id: i64,
        type_id: i64,
    ) -> StoreResult<Option<ItemStatsRecord>> {
        let tables = self.shared.lock();
        Ok(tables
            .item_stats
            .get(&(region_id, station_id, type_id))
            .map(|s| ItemStatsRecord {
                book_at: s.book_at,
                best_buy: s.best_buy.as_ref().map(ToString::to_string),
                best_sell: s.best_sell.as_ref().map(ToString::to_string),
                buy_near_orders: s.buy_near_orders,
                sell_near_orders: s.sell_near_orders,
                rho: s.rho,
                indicators_at: s.indicators_at,
                volume_7d: s.volume_7d,
                volume_30d: s.volume_30d,
                sigma: s.sigma,
                returns: s.returns,
                sma5: s.sma5,
                sma20: s.sma20,
                cross_day: s.cross_day,
                volume_rising: s.volume_rising,
                percent_b: s.percent_b,
                rsi14: s.rsi14,
                expansion_at: s.expansion_at,
            }))
    }
    async fn hourly_buckets(
        &self,
        region_id: i64,
        station_id: i64,
    ) -> StoreResult<Vec<OffsetDateTime>> {
        let tables = self.shared.lock();
        let buckets: BTreeSet<OffsetDateTime> = tables
            .hourly
            .keys()
            .filter(|(region, station, _, _)| *region == region_id && *station == station_id)
            .map(|(_, _, _, bucket)| *bucket)
            .collect();
        Ok(buckets.into_iter().collect())
    }

    async fn daily_buckets(
        &self,
        region_id: i64,
        station_id: i64,
    ) -> StoreResult<Vec<OffsetDateTime>> {
        let tables = self.shared.lock();
        let buckets: BTreeSet<OffsetDateTime> = tables
            .daily
            .keys()
            .filter(|(region, station, _, _)| *region == region_id && *station == station_id)
            .map(|(_, _, _, bucket)| *bucket)
            .collect();
        Ok(buckets.into_iter().collect())
    }
    async fn seed_item_stats(
        &self,
        region_id: i64,
        station_id: i64,
        type_id: i64,
        r: &ItemStatsRecord,
    ) -> StoreResult<()> {
        let numeric = |text: &Option<String>| -> StoreResult<Option<Numeric>> {
            text.as_deref().map(text_to_numeric).transpose()
        };
        let row = ItemStatsRow {
            book_at: r.book_at.map(micros),
            best_buy: numeric(&r.best_buy)?.map(|n| n.round(2)),
            best_sell: numeric(&r.best_sell)?.map(|n| n.round(2)),
            buy_near_orders: r.buy_near_orders,
            sell_near_orders: r.sell_near_orders,
            rho: r.rho,
            indicators_at: r.indicators_at.map(micros),
            volume_7d: r.volume_7d,
            volume_30d: r.volume_30d,
            sigma: r.sigma,
            returns: r.returns,
            sma5: r.sma5,
            sma20: r.sma20,
            cross_day: r.cross_day,
            volume_rising: r.volume_rising,
            percent_b: r.percent_b,
            rsi14: r.rsi14,
            expansion_at: r.expansion_at.map(micros),
        };
        self.shared
            .lock()
            .item_stats
            .insert((region_id, station_id, type_id), row);
        Ok(())
    }
}

#[allow(dead_code)]
fn unused(_: ItemStatsRow) {}
