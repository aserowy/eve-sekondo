//! `station_item_stats`: everything about a tradable item that does not depend on an account, kept
//! up to date at three points. The maths are the pure functions of [`super::analysis`]; SQL only
//! collects and stores.

use std::collections::HashMap;

use sqlx::PgPool;
use time::{Date, Duration, OffsetDateTime};

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

/// Items whose book at the station holds both a buy and a sell order.
const TRADABLE: &str = "SELECT type_id FROM station_orders
    WHERE region_id = $1 AND station_id = $2
    GROUP BY type_id HAVING bool_or(is_buy_order) AND bool_or(NOT is_buy_order)";

/// After a book refresh: the item rows follow the tradable items, and their book fields (the best
/// prices and orders within 1% from the latest snapshot, and ρ over 30 days) are replaced.
pub async fn refresh_book_fields(pool: &PgPool, now: OffsetDateTime) -> Result<(), sqlx::Error> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DELETE FROM station_item_stats
         WHERE region_id = $1 AND station_id = $2 AND type_id NOT IN ({TRADABLE})"
    )))
    .bind(REGION_ID)
    .bind(STATION_ID)
    .execute(pool)
    .await?;

    let latest: Vec<(i64, String, String, i32, i32)> =
        sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT DISTINCT ON (type_id) type_id, best_buy::text, best_sell::text,
                buy_near_orders, sell_near_orders
         FROM station_snapshots
         WHERE region_id = $1 AND station_id = $2 AND recorded_at >= $3
           AND type_id IN ({TRADABLE})
         ORDER BY type_id, recorded_at DESC"
        )))
        .bind(REGION_ID)
        .bind(STATION_ID)
        .bind(now - Duration::days(1))
        .fetch_all(pool)
        .await?;

    // Units taken over 30 days: complete days from the daily rollup, the rest from the hourly one.
    let taken: Vec<(i64, i64, i64)> = sqlx::query_as(
        "WITH cut AS (
             SELECT COALESCE(max(bucket) + interval '1 day', $3) AS from_hourly
             FROM station_daily WHERE region_id = $1 AND station_id = $2)
         SELECT type_id, sum(buy_taken)::bigint, sum(sell_taken)::bigint FROM (
             SELECT d.type_id, d.buy_taken, d.sell_taken FROM station_daily d, cut
             WHERE d.region_id = $1 AND d.station_id = $2
               AND d.bucket >= $3 AND d.bucket < cut.from_hourly
             UNION ALL
             SELECT h.type_id, h.buy_taken, h.sell_taken FROM station_hourly h, cut
             WHERE h.region_id = $1 AND h.station_id = $2
               AND h.bucket >= GREATEST(cut.from_hourly, $3)) rolled
         GROUP BY type_id",
    )
    .bind(REGION_ID)
    .bind(STATION_ID)
    .bind(now - Duration::days(RHO_DAYS))
    .fetch_all(pool)
    .await?;
    let taken: HashMap<i64, (i64, i64)> = taken.into_iter().map(|(t, b, s)| (t, (b, s))).collect();

    let type_ids: Vec<i64> = latest.iter().map(|r| r.0).collect();
    let best_buy: Vec<&str> = latest.iter().map(|r| r.1.as_str()).collect();
    let best_sell: Vec<&str> = latest.iter().map(|r| r.2.as_str()).collect();
    let buy_near: Vec<i32> = latest.iter().map(|r| r.3).collect();
    let sell_near: Vec<i32> = latest.iter().map(|r| r.4).collect();
    let rho: Vec<f64> = latest
        .iter()
        .map(|r| {
            let (buy, sell) = taken.get(&r.0).copied().unwrap_or((0, 0));
            buy_side_ratio(buy, sell)
        })
        .collect();
    sqlx::query(
        "INSERT INTO station_item_stats
             (region_id, station_id, type_id, book_at, best_buy, best_sell, buy_near_orders,
              sell_near_orders, rho)
         SELECT $1, $2, s.type_id, $3, s.best_buy::numeric, s.best_sell::numeric, s.buy_near,
                s.sell_near, s.rho
         FROM unnest($4::bigint[], $5::text[], $6::text[], $7::int[], $8::int[], $9::float8[])
              AS s(type_id, best_buy, best_sell, buy_near, sell_near, rho)
         ON CONFLICT (region_id, station_id, type_id) DO UPDATE SET
             book_at = excluded.book_at, best_buy = excluded.best_buy,
             best_sell = excluded.best_sell, buy_near_orders = excluded.buy_near_orders,
             sell_near_orders = excluded.sell_near_orders, rho = excluded.rho",
    )
    .bind(REGION_ID)
    .bind(STATION_ID)
    .bind(now)
    .bind(&type_ids)
    .bind(&best_buy)
    .bind(&best_sell)
    .bind(&buy_near)
    .bind(&sell_near)
    .bind(&rho)
    .execute(pool)
    .await?;
    Ok(())
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
pub async fn refresh_indicators(pool: &PgPool, now: OffsetDateTime) -> Result<(), sqlx::Error> {
    let today = now.date();
    let rows: Vec<(i64, Date, f64, i64)> = sqlx::query_as(
        "SELECT h.type_id, h.day, h.average::float8, h.volume FROM market_history h
         WHERE h.region_id = $1 AND h.day >= $2
           AND h.type_id IN (SELECT type_id FROM station_item_stats WHERE region_id = $1 AND station_id = $3)
         ORDER BY h.type_id, h.day",
    )
    .bind(REGION_ID)
    .bind(today - Duration::days(HISTORY_DAYS))
    .bind(STATION_ID)
    .fetch_all(pool)
    .await?;
    let mut series: HashMap<i64, Series> = HashMap::new();
    for (type_id, day, average, volume) in rows {
        let s = series.entry(type_id).or_default();
        s.days.push(day);
        s.averages.push(average);
        s.volumes.push(volume as f64);
    }

    let item_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT type_id FROM station_item_stats WHERE region_id = $1 AND station_id = $2",
    )
    .bind(REGION_ID)
    .bind(STATION_ID)
    .fetch_all(pool)
    .await?;

    let mut columns = IndicatorColumns::default();
    for type_id in &item_ids {
        let empty = Series::default();
        let s = series.get(type_id).unwrap_or(&empty);
        columns.push(*type_id, today, s);
    }
    sqlx::query(
        "UPDATE station_item_stats t SET
             indicators_at = $3, volume_7d = c.volume_7d, volume_30d = c.volume_30d,
             sigma = c.sigma, returns = c.returns, sma5 = c.sma5, sma20 = c.sma20,
             cross_day = c.cross_day, volume_rising = c.volume_rising,
             percent_b = c.percent_b, rsi14 = c.rsi14
         FROM unnest($4::bigint[], $5::float8[], $6::float8[], $7::float8[], $8::int[],
                     $9::float8[], $10::float8[], $11::date[], $12::bool[], $13::float8[],
                     $14::float8[])
              AS c(type_id, volume_7d, volume_30d, sigma, returns, sma5, sma20, cross_day,
                   volume_rising, percent_b, rsi14)
         WHERE t.region_id = $1 AND t.station_id = $2 AND t.type_id = c.type_id",
    )
    .bind(REGION_ID)
    .bind(STATION_ID)
    .bind(now)
    .bind(&columns.type_id)
    .bind(&columns.volume_7d)
    .bind(&columns.volume_30d)
    .bind(&columns.sigma)
    .bind(&columns.returns)
    .bind(&columns.sma5)
    .bind(&columns.sma20)
    .bind(&columns.cross_day)
    .bind(&columns.volume_rising)
    .bind(&columns.percent_b)
    .bind(&columns.rsi14)
    .execute(pool)
    .await?;
    Ok(())
}

#[derive(Default)]
struct IndicatorColumns {
    type_id: Vec<i64>,
    volume_7d: Vec<Option<f64>>,
    volume_30d: Vec<Option<f64>>,
    sigma: Vec<Option<f64>>,
    returns: Vec<Option<i32>>,
    sma5: Vec<Option<f64>>,
    sma20: Vec<Option<f64>>,
    cross_day: Vec<Option<Date>>,
    volume_rising: Vec<Option<bool>>,
    percent_b: Vec<Option<f64>>,
    rsi14: Vec<Option<f64>>,
}

impl IndicatorColumns {
    fn push(&mut self, type_id: i64, today: Date, s: &Series) {
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
        self.type_id.push(type_id);
        self.volume_7d.push(has_history.then(|| volume_over(7)));
        self.volume_30d.push(has_history.then(|| volume_over(30)));
        self.sigma.push(volatility(&s.averages));
        self.returns
            .push(has_history.then(|| (s.averages.len() - 1) as i32));
        self.sma5.push(sma(&s.averages, 5));
        self.sma20.push(sma(&s.averages, 20));
        self.cross_day
            .push(sma_crossing(&s.averages).map(|days_ago| s.days[s.days.len() - 1 - days_ago]));
        self.volume_rising
            .push((s.volumes.len() >= 20).then(|| volume_rising(&s.volumes)));
        self.percent_b
            .push(bollinger(&s.averages).map(|b| b.percent_b));
        self.rsi14.push(rsi14(&s.averages));
    }
}

/// When the hourly rollup advanced (its newest hour differs from the one the item rows were last
/// computed for): the expansion fields of every item row, from the recurring window with the
/// highest sell price. Returns whether it recomputed.
pub async fn refresh_expansions_if_advanced(
    pool: &PgPool,
    now: OffsetDateTime,
) -> Result<bool, sqlx::Error> {
    let latest: Option<OffsetDateTime> = sqlx::query_scalar(
        "SELECT max(bucket) FROM station_hourly WHERE region_id = $1 AND station_id = $2",
    )
    .bind(REGION_ID)
    .bind(STATION_ID)
    .fetch_one(pool)
    .await?;
    let Some(latest) = latest else {
        return Ok(false);
    };
    let stale: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM station_item_stats
                        WHERE region_id = $1 AND station_id = $2
                          AND expansion_at IS DISTINCT FROM $3)",
    )
    .bind(REGION_ID)
    .bind(STATION_ID)
    .bind(latest)
    .fetch_one(pool)
    .await?;
    if !stale {
        return Ok(false);
    }

    let from = now - Duration::days(ROLLUP_DAYS);
    // Hourly patterns need 7 days of own snapshots; until then only the daily figures are read.
    let own_days: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT date_trunc('day', bucket)) FROM station_hourly
         WHERE region_id = $1 AND station_id = $2 AND bucket >= $3",
    )
    .bind(REGION_ID)
    .bind(STATION_ID)
    .bind(from)
    .fetch_one(pool)
    .await?;
    let mut hours: HashMap<i64, Vec<HourPoint>> = HashMap::new();
    if own_days >= analysis::MIN_HOURLY_DAYS as i64 {
        let rows: Vec<(i64, OffsetDateTime, f64, f64, i64)> = sqlx::query_as(
            "SELECT type_id, bucket, spread_avg::float8, spread_max::float8,
                    round(best_sell_max * 100)::bigint
             FROM station_hourly
             WHERE region_id = $1 AND station_id = $2 AND bucket >= $3
             ORDER BY type_id, bucket",
        )
        .bind(REGION_ID)
        .bind(STATION_ID)
        .bind(from)
        .fetch_all(pool)
        .await?;
        for (type_id, start, spread_avg, spread_max, sell_max_cents) in rows {
            hours.entry(type_id).or_default().push(HourPoint {
                start,
                spread_avg,
                spread_max,
                sell_max_cents,
            });
        }
    }
    let rows: Vec<(i64, Date, f64, i64)> = sqlx::query_as(
        "SELECT type_id, day, spread::float8, round(high * 100)::bigint
         FROM station_daily_figures
         WHERE region_id = $1 AND day >= $2
         ORDER BY type_id, day",
    )
    .bind(REGION_ID)
    .bind(from.date())
    .fetch_all(pool)
    .await?;
    let mut days: HashMap<i64, Vec<DayPoint>> = HashMap::new();
    for (type_id, day, spread, high_cents) in rows {
        days.entry(type_id).or_default().push(DayPoint {
            day,
            spread,
            high_cents,
        });
    }

    let item_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT type_id FROM station_item_stats WHERE region_id = $1 AND station_id = $2",
    )
    .bind(REGION_ID)
    .bind(STATION_ID)
    .fetch_all(pool)
    .await?;
    let mut kind: Vec<Option<&str>> = Vec::new();
    let mut weekday: Vec<Option<i16>> = Vec::new();
    let mut hour: Vec<Option<i16>> = Vec::new();
    let mut sell: Vec<Option<i64>> = Vec::new();
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
        kind.push(k);
        weekday.push(w);
        hour.push(h);
        sell.push(best.map(|w| w.sell_cents));
    }
    sqlx::query(
        "UPDATE station_item_stats t SET
             expansion_at = $3, expansion_kind = e.kind, expansion_weekday = e.weekday,
             expansion_hour = e.hour, expansion_sell = e.sell / 100.0
         FROM unnest($4::bigint[], $5::text[], $6::smallint[], $7::smallint[], $8::bigint[])
              AS e(type_id, kind, weekday, hour, sell)
         WHERE t.region_id = $1 AND t.station_id = $2 AND t.type_id = e.type_id",
    )
    .bind(REGION_ID)
    .bind(STATION_ID)
    .bind(latest)
    .bind(&item_ids)
    .bind(&kind)
    .bind(&weekday)
    .bind(&hour)
    .bind(&sell)
    .execute(pool)
    .await?;
    Ok(true)
}
