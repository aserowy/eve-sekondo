use async_trait::async_trait;
use sqlx::PgPool;
use time::{Date, Duration, OffsetDateTime};

use crate::{
    esi::EsiHistoryDay,
    store::{
        BookFields, ExpansionColumns, HistoryRecord, HistoryRow, IndicatorColumns, ItemStatsRecord,
        LatestSnapshot, SeedHistoryRow, SeedSnapshotRow, SnapshotRecord, StationOrder,
        StationStore, StoreResult, SyncState,
    },
};

/// Items whose book at the station holds both a buy and a sell order.
const TRADABLE: &str = "SELECT type_id FROM station_orders
    WHERE region_id = $1 AND station_id = $2
    GROUP BY type_id HAVING bool_or(is_buy_order) AND bool_or(NOT is_buy_order)";

pub struct PgStationStore {
    pool: PgPool,
}

impl PgStationStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl StationStore for PgStationStore {
    async fn claim_book_refresh(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
        lease: Duration,
    ) -> StoreResult<bool> {
        sqlx::query(
            "INSERT INTO station_sync (region_id, station_id, kind, next_poll_at)
             VALUES ($1, $2, 'book', $3) ON CONFLICT DO NOTHING",
        )
        .bind(region_id)
        .bind(station_id)
        .bind(now)
        .execute(&self.pool)
        .await?;
        // Pushed back by a lease, so another tick leaves it alone while the refresh runs.
        let done = sqlx::query(
            "UPDATE station_sync SET next_poll_at = $3 + $4::float8 * interval '1 second'
             WHERE region_id = $1 AND station_id = $2 AND kind = 'book' AND next_poll_at <= $3",
        )
        .bind(region_id)
        .bind(station_id)
        .bind(now)
        .bind(lease.as_seconds_f64())
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() > 0)
    }

    async fn tradable_items(&self, region_id: i64, station_id: i64) -> StoreResult<Vec<i64>> {
        Ok(sqlx::query_scalar(
            "SELECT type_id FROM station_orders WHERE region_id = $1 AND station_id = $2
             GROUP BY type_id HAVING bool_or(is_buy_order) AND bool_or(NOT is_buy_order)",
        )
        .bind(region_id)
        .bind(station_id)
        .fetch_all(&self.pool)
        .await?)
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
        let order_id: Vec<i64> = rows.iter().map(|o| o.order_id).collect();
        let type_id: Vec<i64> = rows.iter().map(|o| o.type_id).collect();
        let is_buy: Vec<bool> = rows.iter().map(|o| o.is_buy_order).collect();
        let price_cents: Vec<i64> = rows.iter().map(|o| o.price_cents).collect();
        let volume_remain: Vec<i64> = rows.iter().map(|o| o.volume_remain).collect();

        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "CREATE TEMP TABLE new_station_orders ON COMMIT DROP AS
             SELECT o.order_id, o.type_id, o.is_buy_order, o.price_cents::numeric / 100 AS price,
                    o.volume_remain
             FROM unnest($1::bigint[], $2::bigint[], $3::bool[], $4::bigint[], $5::bigint[])
                  AS o(order_id, type_id, is_buy_order, price_cents, volume_remain)",
        )
        .bind(&order_id)
        .bind(&type_id)
        .bind(&is_buy)
        .bind(&price_cents)
        .bind(&volume_remain)
        .execute(&mut *tx)
        .await?;

        // Snapshots of the tradable items (both sides present). Within 1% of a side's best is
        // compared as price * 100 against best * 99 or 101, so it is exact. Units taken look at
        // orders present in the previous book and this one, before the book is replaced.
        sqlx::query(
            "WITH best AS (
                 SELECT type_id,
                        max(price) FILTER (WHERE is_buy_order) AS best_buy,
                        min(price) FILTER (WHERE NOT is_buy_order) AS best_sell
                 FROM new_station_orders GROUP BY type_id
                 HAVING bool_or(is_buy_order) AND bool_or(NOT is_buy_order)),
             sides AS (
                 SELECT n.type_id,
                        COALESCE(sum(n.volume_remain) FILTER (WHERE n.is_buy_order), 0)::bigint AS buy_units,
                        COALESCE(sum(n.volume_remain) FILTER (WHERE NOT n.is_buy_order), 0)::bigint AS sell_units,
                        COALESCE(sum(n.volume_remain) FILTER (WHERE n.is_buy_order AND n.price * 100 >= b.best_buy * 99), 0)::bigint AS buy_near_units,
                        count(*) FILTER (WHERE n.is_buy_order AND n.price * 100 >= b.best_buy * 99)::int AS buy_near_orders,
                        COALESCE(sum(n.volume_remain) FILTER (WHERE NOT n.is_buy_order AND n.price * 100 <= b.best_sell * 101), 0)::bigint AS sell_near_units,
                        count(*) FILTER (WHERE NOT n.is_buy_order AND n.price * 100 <= b.best_sell * 101)::int AS sell_near_orders
                 FROM new_station_orders n JOIN best b USING (type_id)
                 GROUP BY n.type_id),
             taken AS (
                 SELECT n.type_id,
                        COALESCE(sum(greatest(o.volume_remain - n.volume_remain, 0)) FILTER (WHERE n.is_buy_order), 0)::bigint AS buy_taken,
                        COALESCE(sum(greatest(o.volume_remain - n.volume_remain, 0)) FILTER (WHERE NOT n.is_buy_order), 0)::bigint AS sell_taken
                 FROM new_station_orders n
                 JOIN station_orders o ON o.order_id = n.order_id
                  AND o.region_id = $1 AND o.station_id = $2
                 GROUP BY n.type_id)
             INSERT INTO station_snapshots
                 (recorded_at, region_id, station_id, type_id, best_buy, best_sell, spread,
                  buy_near_units, buy_near_orders, sell_near_units, sell_near_orders,
                  buy_units, sell_units, buy_taken, sell_taken)
             SELECT $3, $1, $2, b.type_id, b.best_buy, b.best_sell,
                    (b.best_sell - b.best_buy) / NULLIF(b.best_buy, 0),
                    s.buy_near_units, s.buy_near_orders, s.sell_near_units, s.sell_near_orders,
                    s.buy_units, s.sell_units,
                    COALESCE(t.buy_taken, 0), COALESCE(t.sell_taken, 0)
             FROM best b JOIN sides s USING (type_id) LEFT JOIN taken t USING (type_id)
             WHERE b.best_buy > 0",
        )
        .bind(region_id)
        .bind(station_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM station_orders WHERE region_id = $1 AND station_id = $2")
            .bind(region_id)
            .bind(station_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO station_orders
                 (region_id, station_id, order_id, type_id, is_buy_order, price, volume_remain)
             SELECT $1, $2, order_id, type_id, is_buy_order, price, volume_remain
             FROM new_station_orders",
        )
        .bind(region_id)
        .bind(station_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM station_orders WHERE region_id = $1 AND station_id = $2")
            .bind(region_id)
            .bind(station_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO station_orders
                 (region_id, station_id, order_id, type_id, is_buy_order, price, volume_remain)
             SELECT $1, $2, order_id, type_id, is_buy_order, price, volume_remain
             FROM new_station_orders",
        )
        .bind(region_id)
        .bind(station_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO station_sync
                 (region_id, station_id, kind, next_poll_at, last_success_at, last_attempt_at,
                  last_error, esi_expires_at, failures)
             VALUES ($1, $2, 'book', $3, $4, $4, NULL, $5, 0)
             ON CONFLICT (region_id, station_id, kind) DO UPDATE SET
                 next_poll_at = excluded.next_poll_at,
                 last_success_at = excluded.last_success_at,
                 last_attempt_at = excluded.last_attempt_at,
                 last_error = NULL, esi_expires_at = excluded.esi_expires_at, failures = 0",
        )
        .bind(region_id)
        .bind(station_id)
        .bind(next_poll_at)
        .bind(now)
        .bind(esi_expires_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
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
        sqlx::query(
            "INSERT INTO station_sync
                 (region_id, station_id, kind, next_poll_at, last_attempt_at, last_error, failures)
             VALUES ($1, $2, 'book', $3 + $5::float8 * interval '1 second', $3, $4, 1)
             ON CONFLICT (region_id, station_id, kind) DO UPDATE SET
                 next_poll_at = $3 + LEAST(
                     $5::float8 * power(2, LEAST(station_sync.failures, 20)) * interval '1 second',
                     $6::float8 * interval '1 second'),
                 last_attempt_at = $3, last_error = $4,
                 failures = station_sync.failures + 1",
        )
        .bind(region_id)
        .bind(station_id)
        .bind(now)
        .bind(reason)
        .bind(backoff_start.as_seconds_f64())
        .bind(interval.as_seconds_f64())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn record_book_deferral(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO station_sync (region_id, station_id, kind, next_poll_at, last_attempt_at)
             VALUES ($1, $2, 'book', $3, $4)
             ON CONFLICT (region_id, station_id, kind) DO UPDATE SET
                 next_poll_at = excluded.next_poll_at, last_attempt_at = excluded.last_attempt_at",
        )
        .bind(region_id)
        .bind(station_id)
        .bind(next_poll_at)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn track_history(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
    ) -> StoreResult<()> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO history_sync (region_id, type_id, next_poll_at)
             SELECT $1, t.type_id, $3 FROM ({TRADABLE}) t
             ON CONFLICT (region_id, type_id) DO NOTHING"
        )))
        .bind(region_id)
        .bind(station_id)
        .bind(now)
        .execute(&self.pool)
        .await?;
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
        Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "UPDATE history_sync SET next_poll_at = $3 + $4::float8 * interval '1 second'
             WHERE region_id = $1 AND type_id IN (
                 SELECT s.type_id FROM history_sync s
                 WHERE s.region_id = $1 AND s.next_poll_at <= $3
                   AND s.type_id IN ({TRADABLE})
                 ORDER BY s.next_poll_at, s.type_id
                 LIMIT $5
                 FOR UPDATE OF s SKIP LOCKED)
             RETURNING type_id"
        )))
        .bind(region_id)
        .bind(station_id)
        .bind(now)
        .bind(lease.as_seconds_f64())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?)
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
        let dates: Vec<&str> = kept.iter().map(|d| d.date.as_str()).collect();
        let average: Vec<i64> = kept.iter().map(|d| d.average_cents).collect();
        let highest: Vec<i64> = kept.iter().map(|d| d.highest_cents).collect();
        let lowest: Vec<i64> = kept.iter().map(|d| d.lowest_cents).collect();
        let volume: Vec<i64> = kept.iter().map(|d| d.volume).collect();
        let orders: Vec<i64> = kept.iter().map(|d| d.order_count).collect();

        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO market_history
                 (region_id, type_id, day, average, highest, lowest, volume, order_count)
             SELECT $1, $2, h.day::date, h.average / 100.0, h.highest / 100.0, h.lowest / 100.0,
                    h.volume, h.order_count
             FROM unnest($3::text[], $4::bigint[], $5::bigint[], $6::bigint[], $7::bigint[], $8::bigint[])
                  AS h(day, average, highest, lowest, volume, order_count)
             ON CONFLICT (region_id, type_id, day) DO UPDATE SET
                 average = excluded.average, highest = excluded.highest, lowest = excluded.lowest,
                 volume = excluded.volume, order_count = excluded.order_count",
        )
        .bind(region_id)
        .bind(type_id)
        .bind(&dates)
        .bind(&average)
        .bind(&highest)
        .bind(&lowest)
        .bind(&volume)
        .bind(&orders)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE history_sync SET next_poll_at = $3, last_success_at = $4, last_attempt_at = $4,
                 last_error = NULL, esi_expires_at = $3, failures = 0
             WHERE region_id = $1 AND type_id = $2",
        )
        .bind(region_id)
        .bind(type_id)
        .bind(next_poll_at)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        // Older days go with the daily pass.
        sqlx::query("DELETE FROM market_history WHERE region_id = $1 AND day < $2")
            .bind(region_id)
            .bind(keep_from)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn schedule_history(
        &self,
        region_id: i64,
        type_id: i64,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
    ) -> StoreResult<()> {
        sqlx::query(
            "UPDATE history_sync SET next_poll_at = $3, last_attempt_at = $4, last_error = NULL,
                 failures = 0
             WHERE region_id = $1 AND type_id = $2",
        )
        .bind(region_id)
        .bind(type_id)
        .bind(next_poll_at)
        .bind(now)
        .execute(&self.pool)
        .await?;
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
        sqlx::query(
            "UPDATE history_sync SET
                 next_poll_at = $3 + LEAST(
                     $5::float8 * power(2, LEAST(failures, 20)) * interval '1 second',
                     $6::float8 * interval '1 second'),
                 last_attempt_at = $3, last_error = $4, failures = failures + 1
             WHERE region_id = $1 AND type_id = $2",
        )
        .bind(region_id)
        .bind(type_id)
        .bind(now)
        .bind(reason)
        .bind(backoff_start.as_seconds_f64())
        .bind(max_backoff.as_seconds_f64())
        .execute(&self.pool)
        .await?;
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
        sqlx::query(
            "INSERT INTO station_sync
                 (region_id, station_id, kind, next_poll_at, last_success_at, last_attempt_at, last_error)
             VALUES ($1, $2, 'history', $3, CASE WHEN $4 THEN $3 END, $3, $5)
             ON CONFLICT (region_id, station_id, kind) DO UPDATE SET
                 last_success_at = CASE WHEN $4 THEN $3 ELSE station_sync.last_success_at END,
                 last_attempt_at = $3, last_error = $5",
        )
        .bind(region_id)
        .bind(station_id)
        .bind(now)
        .bind(any_ok)
        .bind(last_error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn prune_item_stats(&self, region_id: i64, station_id: i64) -> StoreResult<()> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM station_item_stats
             WHERE region_id = $1 AND station_id = $2 AND type_id NOT IN ({TRADABLE})"
        )))
        .bind(region_id)
        .bind(station_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn latest_snapshots(
        &self,
        region_id: i64,
        station_id: i64,
        since: OffsetDateTime,
    ) -> StoreResult<Vec<LatestSnapshot>> {
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT DISTINCT ON (type_id) type_id, best_buy::text, best_sell::text,
                buy_near_orders, sell_near_orders
         FROM station_snapshots
         WHERE region_id = $1 AND station_id = $2 AND recorded_at >= $3
           AND type_id IN ({TRADABLE})
         ORDER BY type_id, recorded_at DESC"
        )))
        .bind(region_id)
        .bind(station_id)
        .bind(since)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn units_taken(
        &self,
        region_id: i64,
        station_id: i64,
        from: OffsetDateTime,
    ) -> StoreResult<Vec<(i64, i64, i64)>> {
        Ok(sqlx::query_as(
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
        .bind(region_id)
        .bind(station_id)
        .bind(from)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn upsert_book_fields(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
        rows: &[BookFields],
    ) -> StoreResult<()> {
        let type_ids: Vec<i64> = rows.iter().map(|r| r.type_id).collect();
        let best_buy: Vec<&str> = rows.iter().map(|r| r.best_buy.as_str()).collect();
        let best_sell: Vec<&str> = rows.iter().map(|r| r.best_sell.as_str()).collect();
        let buy_near: Vec<i32> = rows.iter().map(|r| r.buy_near_orders).collect();
        let sell_near: Vec<i32> = rows.iter().map(|r| r.sell_near_orders).collect();
        let rho: Vec<f64> = rows.iter().map(|r| r.rho).collect();
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
        .bind(region_id)
        .bind(station_id)
        .bind(now)
        .bind(&type_ids)
        .bind(&best_buy)
        .bind(&best_sell)
        .bind(&buy_near)
        .bind(&sell_near)
        .bind(&rho)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn history_rows(
        &self,
        region_id: i64,
        station_id: i64,
        from: Date,
    ) -> StoreResult<Vec<HistoryRow>> {
        Ok(sqlx::query_as(
            "SELECT h.type_id, h.day, h.average::float8, h.volume FROM market_history h
         WHERE h.region_id = $1 AND h.day >= $2
           AND h.type_id IN (SELECT type_id FROM station_item_stats WHERE region_id = $1 AND station_id = $3)
         ORDER BY h.type_id, h.day",
        )
        .bind(region_id)
        .bind(from)
        .bind(station_id)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn item_stat_ids(&self, region_id: i64, station_id: i64) -> StoreResult<Vec<i64>> {
        Ok(sqlx::query_scalar(
            "SELECT type_id FROM station_item_stats WHERE region_id = $1 AND station_id = $2",
        )
        .bind(region_id)
        .bind(station_id)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn update_indicators(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
        columns: &IndicatorColumns,
    ) -> StoreResult<()> {
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
        .bind(region_id)
        .bind(station_id)
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
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn latest_hour(
        &self,
        region_id: i64,
        station_id: i64,
    ) -> StoreResult<Option<OffsetDateTime>> {
        Ok(sqlx::query_scalar(
            "SELECT max(bucket) FROM station_hourly WHERE region_id = $1 AND station_id = $2",
        )
        .bind(region_id)
        .bind(station_id)
        .fetch_one(&self.pool)
        .await?)
    }

    async fn expansions_stale(
        &self,
        region_id: i64,
        station_id: i64,
        latest: OffsetDateTime,
    ) -> StoreResult<bool> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM station_item_stats
                        WHERE region_id = $1 AND station_id = $2
                          AND expansion_at IS DISTINCT FROM $3)",
        )
        .bind(region_id)
        .bind(station_id)
        .bind(latest)
        .fetch_one(&self.pool)
        .await?)
    }

    async fn own_hourly_days(
        &self,
        region_id: i64,
        station_id: i64,
        from: OffsetDateTime,
    ) -> StoreResult<i64> {
        Ok(sqlx::query_scalar(
            "SELECT count(DISTINCT date_trunc('day', bucket)) FROM station_hourly
         WHERE region_id = $1 AND station_id = $2 AND bucket >= $3",
        )
        .bind(region_id)
        .bind(station_id)
        .bind(from)
        .fetch_one(&self.pool)
        .await?)
    }

    async fn hourly_points(
        &self,
        region_id: i64,
        station_id: i64,
        from: OffsetDateTime,
    ) -> StoreResult<Vec<(i64, OffsetDateTime, f64, f64, i64)>> {
        Ok(sqlx::query_as(
            "SELECT type_id, bucket, spread_avg::float8, spread_max::float8,
                    round(best_sell_max * 100)::bigint
             FROM station_hourly
             WHERE region_id = $1 AND station_id = $2 AND bucket >= $3
             ORDER BY type_id, bucket",
        )
        .bind(region_id)
        .bind(station_id)
        .bind(from)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn daily_points(
        &self,
        region_id: i64,
        from: Date,
    ) -> StoreResult<Vec<(i64, Date, f64, i64)>> {
        Ok(sqlx::query_as(
            "SELECT type_id, day, spread::float8, round(high * 100)::bigint
         FROM station_daily_figures
         WHERE region_id = $1 AND day >= $2
         ORDER BY type_id, day",
        )
        .bind(region_id)
        .bind(from)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn update_expansions(
        &self,
        region_id: i64,
        station_id: i64,
        latest: OffsetDateTime,
        columns: &ExpansionColumns,
    ) -> StoreResult<()> {
        sqlx::query(
            "UPDATE station_item_stats t SET
             expansion_at = $3, expansion_kind = e.kind, expansion_weekday = e.weekday,
             expansion_hour = e.hour, expansion_sell = e.sell / 100.0
         FROM unnest($4::bigint[], $5::text[], $6::smallint[], $7::smallint[], $8::bigint[])
              AS e(type_id, kind, weekday, hour, sell)
         WHERE t.region_id = $1 AND t.station_id = $2 AND t.type_id = e.type_id",
        )
        .bind(region_id)
        .bind(station_id)
        .bind(latest)
        .bind(&columns.type_id)
        .bind(&columns.kind)
        .bind(&columns.weekday)
        .bind(&columns.hour)
        .bind(&columns.sell)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn refresh_rollups(&self, from: OffsetDateTime, to: OffsetDateTime) -> StoreResult<()> {
        for view in ["station_hourly", "station_daily"] {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "CALL refresh_continuous_aggregate('{view}', $1, $2)"
            )))
            .bind(from)
            .bind(to)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn seed_snapshots(
        &self,
        region_id: i64,
        station_id: i64,
        rows: &[SeedSnapshotRow],
    ) -> StoreResult<()> {
        for s in rows {
            sqlx::query(
                "INSERT INTO station_snapshots
                 (recorded_at, region_id, station_id, type_id, best_buy, best_sell, spread,
                  buy_near_units, buy_near_orders, sell_near_units, sell_near_orders,
                  buy_units, sell_units, buy_taken, sell_taken)
             VALUES ($1, $2, $3, $4, $5::float8::numeric, $6::float8::numeric,
                     ($6::float8::numeric - $5::float8::numeric) / $5::float8::numeric,
                     $7, $8, $9, $10, $11, $12, $13, $14)",
            )
            .bind(s.recorded_at)
            .bind(region_id)
            .bind(station_id)
            .bind(s.type_id)
            .bind(s.best_buy)
            .bind(s.best_sell)
            .bind(s.buy_near_units)
            .bind(s.buy_near_orders)
            .bind(s.sell_near_units)
            .bind(s.sell_near_orders)
            .bind(s.buy_units)
            .bind(s.sell_units)
            .bind(s.buy_taken)
            .bind(s.sell_taken)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    async fn seed_history(&self, region_id: i64, rows: &[SeedHistoryRow]) -> StoreResult<()> {
        for h in rows {
            sqlx::query(
                "INSERT INTO market_history
                 (region_id, type_id, day, average, highest, lowest, volume, order_count)
             VALUES ($1, $2, $3, $4::float8::numeric, $5::float8::numeric, $6::float8::numeric, $7, $8)
             ON CONFLICT (region_id, type_id, day) DO UPDATE SET
                 average = excluded.average, highest = excluded.highest, lowest = excluded.lowest,
                 volume = excluded.volume, order_count = excluded.order_count",
            )
            .bind(region_id)
            .bind(h.type_id)
            .bind(h.day)
            .bind(h.average)
            .bind(h.highest)
            .bind(h.lowest)
            .bind(h.volume)
            .bind(h.order_count)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }
    async fn station_sync_state(
        &self,
        region_id: i64,
        station_id: i64,
        kind: &str,
    ) -> StoreResult<Option<SyncState>> {
        Ok(sqlx::query_as(
            "SELECT next_poll_at, last_success_at, last_attempt_at, last_error, esi_expires_at, failures FROM station_sync
             WHERE region_id = $1 AND station_id = $2 AND kind = $3",
        )
        .bind(region_id)
        .bind(station_id)
        .bind(kind)
        .fetch_optional(&self.pool)
        .await?)
    }

    async fn history_sync(&self, region_id: i64, type_id: i64) -> StoreResult<Option<SyncState>> {
        Ok(sqlx::query_as(
            "SELECT next_poll_at, last_success_at, last_attempt_at, last_error, esi_expires_at, failures FROM history_sync WHERE region_id = $1 AND type_id = $2",
        )
        .bind(region_id)
        .bind(type_id)
        .fetch_optional(&self.pool)
        .await?)
    }
    async fn book(&self, region_id: i64, station_id: i64) -> StoreResult<Vec<StationOrder>> {
        let rows: Vec<(i64, i64, bool, i64, i64)> = sqlx::query_as(
            "SELECT order_id, type_id, is_buy_order, (price * 100)::bigint, volume_remain
             FROM station_orders WHERE region_id = $1 AND station_id = $2 ORDER BY order_id",
        )
        .bind(region_id)
        .bind(station_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(order_id, type_id, is_buy_order, price_cents, volume_remain)| StationOrder {
                    order_id,
                    type_id,
                    is_buy_order,
                    price_cents,
                    volume_remain,
                },
            )
            .collect())
    }

    async fn snapshots(&self, region_id: i64, station_id: i64) -> StoreResult<Vec<SnapshotRecord>> {
        #[derive(sqlx::FromRow)]
        struct Row {
            recorded_at: OffsetDateTime,
            type_id: i64,
            best_buy: String,
            best_sell: String,
            spread: String,
            buy_near_units: i64,
            buy_near_orders: i32,
            sell_near_units: i64,
            sell_near_orders: i32,
            buy_units: i64,
            sell_units: i64,
            buy_taken: i64,
            sell_taken: i64,
        }
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT recorded_at, type_id, best_buy::text AS best_buy, best_sell::text AS best_sell,
                    spread::text AS spread, buy_near_units, buy_near_orders, sell_near_units,
                    sell_near_orders, buy_units, sell_units, buy_taken, sell_taken
             FROM station_snapshots WHERE region_id = $1 AND station_id = $2
             ORDER BY recorded_at, type_id",
        )
        .bind(region_id)
        .bind(station_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| SnapshotRecord {
                recorded_at: r.recorded_at,
                type_id: r.type_id,
                best_buy: r.best_buy,
                best_sell: r.best_sell,
                spread: r.spread,
                buy_near_units: r.buy_near_units,
                buy_near_orders: r.buy_near_orders,
                sell_near_units: r.sell_near_units,
                sell_near_orders: r.sell_near_orders,
                buy_units: r.buy_units,
                sell_units: r.sell_units,
                buy_taken: r.buy_taken,
                sell_taken: r.sell_taken,
            })
            .collect())
    }

    async fn history_table(&self, region_id: i64, type_id: i64) -> StoreResult<Vec<HistoryRecord>> {
        let rows: Vec<(i64, Date, String, String, String, i64, i64)> = sqlx::query_as(
            "SELECT type_id, day, average::text, highest::text, lowest::text, volume, order_count
             FROM market_history WHERE region_id = $1 AND type_id = $2 ORDER BY day",
        )
        .bind(region_id)
        .bind(type_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(type_id, day, average, highest, lowest, volume, order_count)| HistoryRecord {
                    type_id,
                    day,
                    average,
                    highest,
                    lowest,
                    volume,
                    order_count,
                },
            )
            .collect())
    }

    async fn item_stats_row(
        &self,
        region_id: i64,
        station_id: i64,
        type_id: i64,
    ) -> StoreResult<Option<ItemStatsRecord>> {
        #[derive(sqlx::FromRow)]
        struct Row {
            book_at: Option<OffsetDateTime>,
            best_buy: Option<String>,
            best_sell: Option<String>,
            buy_near_orders: Option<i32>,
            sell_near_orders: Option<i32>,
            rho: Option<f64>,
            indicators_at: Option<OffsetDateTime>,
            volume_7d: Option<f64>,
            volume_30d: Option<f64>,
            sigma: Option<f64>,
            returns: Option<i32>,
            sma5: Option<f64>,
            sma20: Option<f64>,
            cross_day: Option<Date>,
            volume_rising: Option<bool>,
            percent_b: Option<f64>,
            rsi14: Option<f64>,
            expansion_at: Option<OffsetDateTime>,
            expansion_kind: Option<String>,
            expansion_weekday: Option<i16>,
            expansion_hour: Option<i16>,
            expansion_sell: Option<String>,
        }
        let row: Option<Row> = sqlx::query_as(
            "SELECT book_at, best_buy::text AS best_buy, best_sell::text AS best_sell,
                    buy_near_orders, sell_near_orders, rho, indicators_at, volume_7d, volume_30d,
                    sigma, returns, sma5, sma20, cross_day, volume_rising, percent_b, rsi14,
                    expansion_at, expansion_kind, expansion_weekday, expansion_hour,
                    expansion_sell::text AS expansion_sell
             FROM station_item_stats WHERE region_id = $1 AND station_id = $2 AND type_id = $3",
        )
        .bind(region_id)
        .bind(station_id)
        .bind(type_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| ItemStatsRecord {
            book_at: r.book_at,
            best_buy: r.best_buy,
            best_sell: r.best_sell,
            buy_near_orders: r.buy_near_orders,
            sell_near_orders: r.sell_near_orders,
            rho: r.rho,
            indicators_at: r.indicators_at,
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
            expansion_at: r.expansion_at,
            expansion_kind: r.expansion_kind,
            expansion_weekday: r.expansion_weekday,
            expansion_hour: r.expansion_hour,
            expansion_sell: r.expansion_sell,
        }))
    }
    async fn hourly_buckets(
        &self,
        region_id: i64,
        station_id: i64,
    ) -> StoreResult<Vec<OffsetDateTime>> {
        Ok(sqlx::query_scalar(
            "SELECT DISTINCT bucket FROM station_hourly
             WHERE region_id = $1 AND station_id = $2 ORDER BY bucket",
        )
        .bind(region_id)
        .bind(station_id)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn daily_buckets(
        &self,
        region_id: i64,
        station_id: i64,
    ) -> StoreResult<Vec<OffsetDateTime>> {
        Ok(sqlx::query_scalar(
            "SELECT DISTINCT bucket FROM station_daily
             WHERE region_id = $1 AND station_id = $2 ORDER BY bucket",
        )
        .bind(region_id)
        .bind(station_id)
        .fetch_all(&self.pool)
        .await?)
    }
    async fn seed_item_stats(
        &self,
        region_id: i64,
        station_id: i64,
        type_id: i64,
        r: &ItemStatsRecord,
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO station_item_stats
                 (region_id, station_id, type_id, book_at, best_buy, best_sell, buy_near_orders,
                  sell_near_orders, rho, indicators_at, volume_7d, volume_30d, sigma, returns, sma5,
                  sma20, cross_day, volume_rising, percent_b, rsi14, expansion_at, expansion_kind,
                  expansion_weekday, expansion_hour, expansion_sell)
             VALUES ($1, $2, $3, $4, $5::numeric, $6::numeric, $7, $8, $9, $10, $11, $12, $13, $14,
                     $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25::numeric)
             ON CONFLICT (region_id, station_id, type_id) DO UPDATE SET
                 book_at = excluded.book_at, best_buy = excluded.best_buy,
                 best_sell = excluded.best_sell, buy_near_orders = excluded.buy_near_orders,
                 sell_near_orders = excluded.sell_near_orders, rho = excluded.rho,
                 indicators_at = excluded.indicators_at, volume_7d = excluded.volume_7d,
                 volume_30d = excluded.volume_30d, sigma = excluded.sigma,
                 returns = excluded.returns, sma5 = excluded.sma5, sma20 = excluded.sma20,
                 cross_day = excluded.cross_day, volume_rising = excluded.volume_rising,
                 percent_b = excluded.percent_b, rsi14 = excluded.rsi14,
                 expansion_at = excluded.expansion_at, expansion_kind = excluded.expansion_kind,
                 expansion_weekday = excluded.expansion_weekday,
                 expansion_hour = excluded.expansion_hour,
                 expansion_sell = excluded.expansion_sell",
        )
        .bind(region_id)
        .bind(station_id)
        .bind(type_id)
        .bind(r.book_at)
        .bind(&r.best_buy)
        .bind(&r.best_sell)
        .bind(r.buy_near_orders)
        .bind(r.sell_near_orders)
        .bind(r.rho)
        .bind(r.indicators_at)
        .bind(r.volume_7d)
        .bind(r.volume_30d)
        .bind(r.sigma)
        .bind(r.returns)
        .bind(r.sma5)
        .bind(r.sma20)
        .bind(r.cross_day)
        .bind(r.volume_rising)
        .bind(r.percent_b)
        .bind(r.rsi14)
        .bind(r.expansion_at)
        .bind(&r.expansion_kind)
        .bind(r.expansion_weekday)
        .bind(r.expansion_hour)
        .bind(&r.expansion_sell)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
