use async_trait::async_trait;
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    esi::{EsiMarketOrder, EsiPrice},
    store::{
        MarketStore, NameRow, NameState, PublicOrder, StandingRow, StoreResult, SyncState,
        pg::order::{GAP_CENTS, STANDING_JOIN},
    },
    wealth_poller::INVENTORY_VALUE_SUM,
};

pub struct PgMarketStore {
    pool: PgPool,
}

impl PgMarketStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Columns of the market orders of one pair, as the parallel arrays `unnest` takes.
#[derive(Default)]
struct MarketColumns {
    order_id: Vec<i64>,
    location_id: Vec<i64>,
    system_id: Vec<i64>,
    is_buy_order: Vec<bool>,
    price_cents: Vec<i64>,
    range: Vec<String>,
    volume_remain: Vec<i64>,
}

impl MarketColumns {
    fn new(orders: &[EsiMarketOrder]) -> Self {
        let mut c = Self::default();
        for o in orders {
            c.order_id.push(o.order_id);
            c.location_id.push(o.location_id);
            c.system_id.push(o.system_id);
            c.is_buy_order.push(o.is_buy_order);
            c.price_cents.push(o.price_cents);
            c.range.push(o.range.clone());
            c.volume_remain.push(o.volume_remain);
        }
        c
    }
}

#[async_trait]
impl MarketStore for PgMarketStore {
    async fn known_names(&self, ids: &[i64], now: OffsetDateTime) -> StoreResult<Vec<i64>> {
        Ok(sqlx::query_scalar(
            "SELECT id FROM names WHERE id = ANY($1) AND (name IS NOT NULL OR retry_after > $2)",
        )
        .bind(ids)
        .bind(now)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn store_names(
        &self,
        rows: &[NameRow],
        resolved_at: OffsetDateTime,
        retry_after: Option<OffsetDateTime>,
    ) -> StoreResult<()> {
        let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
        let categories: Vec<&str> = rows.iter().map(|r| r.category.as_str()).collect();
        let names: Vec<Option<&str>> = rows.iter().map(|r| r.name.as_deref()).collect();
        sqlx::query(
            "INSERT INTO names (id, category, name, resolved_at, retry_after)
             SELECT n.id, n.category, n.name, $4, $5
             FROM unnest($1::bigint[], $2::text[], $3::text[]) AS n(id, category, name)
             ON CONFLICT (id) DO UPDATE SET
                 category = excluded.category,
                 name = excluded.name,
                 resolved_at = excluded.resolved_at,
                 retry_after = excluded.retry_after",
        )
        .bind(&ids)
        .bind(&categories)
        .bind(&names)
        .bind(resolved_at)
        .bind(retry_after)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn names_of(&self, ids: &[i64]) -> StoreResult<Vec<(i64, Option<String>)>> {
        Ok(sqlx::query_as::<_, (i64, Option<String>)>(
            "SELECT id, name FROM names WHERE id = ANY($1)",
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn name_of(&self, id: i64) -> StoreResult<Option<String>> {
        let name: Option<Option<String>> =
            sqlx::query_scalar("SELECT name FROM names WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(name.flatten())
    }

    async fn standings(&self, region_id: i64, type_id: i64) -> StoreResult<Vec<StandingRow>> {
        let sql = format!(
            "SELECT c.account_id, o.order_id, st.undercut, {GAP_CENTS}, st.undercut_volume
             FROM current_orders o
             JOIN characters c ON c.character_id = o.character_id
             {STANDING_JOIN}
             WHERE o.region_id = $1 AND o.type_id = $2
             ORDER BY o.order_id"
        );
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(region_id)
            .bind(type_id)
            .fetch_all(&self.pool)
            .await?)
    }

    async fn account_pairs(&self, account_id: Uuid) -> StoreResult<Vec<(i64, i64)>> {
        Ok(sqlx::query_as(
            "SELECT DISTINCT o.region_id, o.type_id FROM current_orders o
             JOIN characters c ON c.character_id = o.character_id
             WHERE c.account_id = $1
             ORDER BY o.region_id, o.type_id",
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn untracked_pairs(&self) -> StoreResult<Vec<(i64, i64)>> {
        Ok(sqlx::query_as(
            "SELECT DISTINCT o.region_id, o.type_id FROM current_orders o
             WHERE NOT EXISTS (SELECT 1 FROM market_sync s
                               WHERE s.region_id = o.region_id AND s.type_id = o.type_id)",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    async fn insert_pair_schedule(
        &self,
        pairs: &[(i64, i64)],
        next_poll_at: &[OffsetDateTime],
    ) -> StoreResult<()> {
        let regions: Vec<i64> = pairs.iter().map(|p| p.0).collect();
        let types: Vec<i64> = pairs.iter().map(|p| p.1).collect();
        sqlx::query(
            "INSERT INTO market_sync (region_id, type_id, next_poll_at)
             SELECT * FROM unnest($1::bigint[], $2::bigint[], $3::timestamptz[])
             ON CONFLICT (region_id, type_id) DO NOTHING",
        )
        .bind(&regions)
        .bind(&types)
        .bind(next_poll_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn drop_unused_pairs(&self) -> StoreResult<()> {
        sqlx::query(
            "DELETE FROM market_sync s
             WHERE NOT EXISTS (SELECT 1 FROM current_orders o
                               WHERE o.region_id = s.region_id AND o.type_id = s.type_id)",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "DELETE FROM market_orders m
             WHERE NOT EXISTS (SELECT 1 FROM current_orders o
                               WHERE o.region_id = m.region_id AND o.type_id = m.type_id)",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn claim_due_pairs(
        &self,
        now: OffsetDateTime,
        lease: Duration,
        limit: i64,
    ) -> StoreResult<Vec<(i64, i64)>> {
        Ok(sqlx::query_as(
            "UPDATE market_sync SET next_poll_at = $1 + $2::float8 * interval '1 second'
             WHERE (region_id, type_id) IN (
                 SELECT region_id, type_id FROM market_sync
                 WHERE next_poll_at <= $1
                 ORDER BY next_poll_at
                 LIMIT $3
                 FOR UPDATE SKIP LOCKED)
             RETURNING region_id, type_id",
        )
        .bind(now)
        .bind(lease.as_seconds_f64())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn replace_market_orders(
        &self,
        region_id: i64,
        type_id: i64,
        now: OffsetDateTime,
        orders: &[EsiMarketOrder],
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<()> {
        let columns = MarketColumns::new(orders);
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM market_orders WHERE region_id = $1 AND type_id = $2")
            .bind(region_id)
            .bind(type_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO market_orders
                 (order_id, region_id, type_id, location_id, system_id, is_buy_order, price, range,
                  volume_remain)
             SELECT o.order_id, $1, $2, o.location_id, o.system_id, o.is_buy_order,
                    o.price_cents::numeric / 100, o.range, o.volume_remain
             FROM unnest($3::bigint[], $4::bigint[], $5::bigint[], $6::bool[], $7::bigint[],
                         $8::text[], $9::bigint[])
                  AS o(order_id, location_id, system_id, is_buy_order, price_cents, range,
                       volume_remain)
             ON CONFLICT (order_id) DO UPDATE SET
                 region_id = excluded.region_id, type_id = excluded.type_id,
                 location_id = excluded.location_id, system_id = excluded.system_id,
                 is_buy_order = excluded.is_buy_order, price = excluded.price,
                 range = excluded.range, volume_remain = excluded.volume_remain",
        )
        .bind(region_id)
        .bind(type_id)
        .bind(&columns.order_id)
        .bind(&columns.location_id)
        .bind(&columns.system_id)
        .bind(&columns.is_buy_order)
        .bind(&columns.price_cents)
        .bind(&columns.range)
        .bind(&columns.volume_remain)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE market_sync SET
                 next_poll_at = $3, last_success_at = $4, last_attempt_at = $4,
                 last_error = NULL, esi_expires_at = $5, failures = 0
             WHERE region_id = $1 AND type_id = $2",
        )
        .bind(region_id)
        .bind(type_id)
        .bind(next_poll_at)
        .bind(now)
        .bind(esi_expires_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn record_pair_failure(
        &self,
        region_id: i64,
        type_id: i64,
        now: OffsetDateTime,
        reason: &str,
        backoff_start: Duration,
        interval: Duration,
    ) -> StoreResult<()> {
        sqlx::query(
            "UPDATE market_sync SET
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
        .bind(interval.as_seconds_f64())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn record_pair_deferral(
        &self,
        region_id: i64,
        type_id: i64,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
    ) -> StoreResult<()> {
        sqlx::query(
            "UPDATE market_sync SET next_poll_at = $3, last_attempt_at = $4
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

    async fn claim_price_refresh(&self, now: OffsetDateTime, lease: Duration) -> StoreResult<bool> {
        sqlx::query(
            "INSERT INTO price_sync (singleton, next_poll_at) VALUES (true, $1)
             ON CONFLICT (singleton) DO NOTHING",
        )
        .bind(now)
        .execute(&self.pool)
        .await?;
        // Pushed back by a lease, so another tick does not refresh while this one runs.
        let done = sqlx::query(
            "UPDATE price_sync SET next_poll_at = $1 + $2::float8 * interval '1 second'
             WHERE next_poll_at <= $1",
        )
        .bind(now)
        .bind(lease.as_seconds_f64())
        .execute(&self.pool)
        .await?;
        Ok(done.rows_affected() > 0)
    }

    async fn accounts_with_assets(&self) -> StoreResult<Vec<Uuid>> {
        Ok(sqlx::query_scalar(
            "SELECT DISTINCT c.account_id FROM current_assets a
             JOIN characters c ON c.character_id = a.character_id",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    async fn replace_prices(
        &self,
        now: OffsetDateTime,
        prices: &[EsiPrice],
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<()> {
        let type_id: Vec<i64> = prices.iter().map(|p| p.type_id).collect();
        let cents: Vec<Option<i64>> = prices.iter().map(|p| p.average_price_cents).collect();
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM item_prices")
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO item_prices (type_id, average_price)
             SELECT p.type_id, p.cents::numeric / 100
             FROM unnest($1::bigint[], $2::bigint[]) AS p(type_id, cents)
             ON CONFLICT (type_id) DO UPDATE SET average_price = excluded.average_price",
        )
        .bind(&type_id)
        .bind(&cents)
        .execute(&mut *tx)
        .await?;
        // Two entries in the same instant keep the later one.
        let revalue = format!(
            "INSERT INTO inventory_values (character_id, recorded_at, value)
             SELECT a.character_id, $1, {INVENTORY_VALUE_SUM}
             FROM current_assets a LEFT JOIN item_prices p USING (type_id)
             GROUP BY a.character_id
             ON CONFLICT (character_id, recorded_at) DO UPDATE SET value = excluded.value"
        );
        sqlx::query(sqlx::AssertSqlSafe(revalue))
            .bind(now)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO price_sync
                 (singleton, next_poll_at, last_success_at, last_attempt_at, last_error,
                  esi_expires_at, failures)
             VALUES (true, $1, $2, $2, NULL, $3, 0)
             ON CONFLICT (singleton) DO UPDATE SET
                 next_poll_at = excluded.next_poll_at,
                 last_success_at = excluded.last_success_at,
                 last_attempt_at = excluded.last_attempt_at,
                 last_error = NULL,
                 esi_expires_at = excluded.esi_expires_at,
                 failures = 0",
        )
        .bind(next_poll_at)
        .bind(now)
        .bind(esi_expires_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn record_price_failure(
        &self,
        now: OffsetDateTime,
        reason: &str,
        backoff_start: Duration,
        interval: Duration,
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO price_sync (singleton, next_poll_at, last_attempt_at, last_error, failures)
             VALUES (true, $1, $2, $3, 1)
             ON CONFLICT (singleton) DO UPDATE SET
                 next_poll_at = $2 + LEAST(
                     $4::float8 * power(2, LEAST(price_sync.failures, 20)) * interval '1 second',
                     $5::float8 * interval '1 second'),
                 last_attempt_at = $2,
                 last_error = $3,
                 failures = price_sync.failures + 1",
        )
        .bind(now + backoff_start)
        .bind(now)
        .bind(reason)
        .bind(backoff_start.as_seconds_f64())
        .bind(interval.as_seconds_f64())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn record_price_deferral(
        &self,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO price_sync (singleton, next_poll_at, last_attempt_at) VALUES (true, $1, $2)
             ON CONFLICT (singleton) DO UPDATE SET
                 next_poll_at = excluded.next_poll_at,
                 last_attempt_at = excluded.last_attempt_at",
        )
        .bind(next_poll_at)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
    async fn pair_sync(&self, region_id: i64, type_id: i64) -> StoreResult<Option<SyncState>> {
        Ok(sqlx::query_as(
            "SELECT next_poll_at, last_success_at, last_attempt_at, last_error, esi_expires_at, failures FROM market_sync WHERE region_id = $1 AND type_id = $2",
        )
        .bind(region_id)
        .bind(type_id)
        .fetch_optional(&self.pool)
        .await?)
    }

    async fn price_sync(&self) -> StoreResult<Option<SyncState>> {
        Ok(sqlx::query_as("SELECT next_poll_at, last_success_at, last_attempt_at, last_error, esi_expires_at, failures FROM price_sync")
            .fetch_optional(&self.pool)
            .await?)
    }
    async fn names_table(&self) -> StoreResult<Vec<NameState>> {
        let rows: Vec<(i64, Option<String>, Option<OffsetDateTime>)> =
            sqlx::query_as("SELECT id, name, retry_after FROM names ORDER BY id")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows
            .into_iter()
            .map(|(id, name, retry_after)| NameState {
                id,
                name,
                retry_after,
            })
            .collect())
    }
    async fn tracked_pairs(&self) -> StoreResult<Vec<(i64, i64)>> {
        Ok(
            sqlx::query_as(
                "SELECT region_id, type_id FROM market_sync ORDER BY region_id, type_id",
            )
            .fetch_all(&self.pool)
            .await?,
        )
    }

    async fn public_orders(&self) -> StoreResult<Vec<PublicOrder>> {
        let rows: Vec<(i64, i64, i64, i64, bool, i64, i64)> = sqlx::query_as(
            "SELECT order_id, region_id, type_id, location_id, is_buy_order, (price * 100)::bigint,
                    volume_remain
             FROM market_orders ORDER BY order_id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    order_id,
                    region_id,
                    type_id,
                    location_id,
                    is_buy_order,
                    price_cents,
                    volume_remain,
                )| {
                    PublicOrder {
                        order_id,
                        region_id,
                        type_id,
                        location_id,
                        is_buy_order,
                        price_cents,
                        volume_remain,
                    }
                },
            )
            .collect())
    }

    async fn prices_table(&self) -> StoreResult<Vec<(i64, Option<i64>)>> {
        Ok(sqlx::query_as(
            "SELECT type_id, (average_price * 100)::bigint FROM item_prices ORDER BY type_id",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    async fn seed_price(&self, type_id: i64, average_price_cents: Option<i64>) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO item_prices (type_id, average_price) VALUES ($1, $2::numeric / 100)
             ON CONFLICT (type_id) DO UPDATE SET average_price = excluded.average_price",
        )
        .bind(type_id)
        .bind(average_price_cents)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
