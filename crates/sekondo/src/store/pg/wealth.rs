use async_trait::async_trait;
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    esi::{EsiAsset, EsiTransaction},
    freshness::WealthFreshnessRecord,
    orders::Dir,
    range::{BUCKET_CTES, Range},
    store::{PollTotalsRow, StoreResult, SyncState, WealthStore},
    wealth::{Current, InventoryRow, InventorySort, InventorySortKey, WealthPoint},
    wealth_poller::{INVENTORY_VALUE_SUM, Kind},
};

pub struct PgWealthStore {
    pool: PgPool,
}

impl PgWealthStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// The three carry-forward lookups of one character, each at the time `$at` names. `chars` is
/// the selected characters; the figures come out in `numeric`, one row per character.
fn carry_forward(at: &str) -> String {
    format!(
        "LEFT JOIN LATERAL (
             SELECT b.balance FROM wallet_balances b
             WHERE b.character_id = c.character_id AND b.polled_at <= {at}
             ORDER BY b.polled_at DESC LIMIT 1
         ) w ON true
         LEFT JOIN LATERAL (
             SELECT p.sell_value - p.sell_corp AS sell, p.escrow - p.escrow_corp AS escrow
             FROM order_poll_totals p
             WHERE p.character_id = c.character_id AND p.polled_at <= {at}
             ORDER BY p.polled_at DESC LIMIT 1
         ) t ON true
         LEFT JOIN LATERAL (
             SELECT v.value FROM inventory_values v
             WHERE v.character_id = c.character_id AND v.recorded_at <= {at}
             ORDER BY v.recorded_at DESC LIMIT 1
         ) i ON true"
    )
}

/// The SQL `ORDER BY` terms for a direction. Only these literals ever reach the query text.
/// Unpriced rows go last in both directions, so their term does not follow the direction.
fn order_by(key: InventorySortKey, direction: &str) -> String {
    match key {
        InventorySortKey::Price => format!("(g.unit_price IS NULL), g.unit_price {direction}"),
        InventorySortKey::Value => format!("(g.value IS NULL), g.value {direction}"),
        InventorySortKey::Character => format!("lower(c.name) {direction}"),
        InventorySortKey::Item => format!("lower(COALESCE(tn.name, '#' || g.type_id)) {direction}"),
        InventorySortKey::Quantity => format!("g.quantity {direction}"),
        InventorySortKey::Location => {
            format!("lower(COALESCE(ln.name, '#' || g.place_id)) {direction}")
        }
    }
}

#[async_trait]
impl WealthStore for PgWealthStore {
    async fn current(&self, account_id: Uuid, character_ids: &[i64]) -> StoreResult<Current> {
        let sql = format!(
            "SELECT (COALESCE(SUM(w.balance), 0) * 100)::bigint AS wallet_cents,
                (COALESCE(SUM(t.sell), 0) * 100)::bigint AS sell_cents,
                (COALESCE(SUM(t.escrow), 0) * 100)::bigint AS escrow_cents,
                (COALESCE(SUM(i.value), 0) * 100)::bigint AS inventory_cents
         FROM characters c
         {}
         WHERE c.account_id = $1 AND c.character_id = ANY($2)",
            carry_forward("'infinity'::timestamptz")
        );
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(account_id)
            .bind(character_ids)
            .fetch_one(&self.pool)
            .await?)
    }

    async fn history(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        range: Range,
        now: OffsetDateTime,
    ) -> StoreResult<Vec<WealthPoint>> {
        let seconds = |d: Duration| d.as_seconds_f64();
        let sql = format!(
            "{BUCKET_CTES}
         SELECT e.at,
                (COALESCE(SUM(w.balance), 0) * 100)::bigint AS wallet_cents,
                (COALESCE(SUM(t.sell), 0) * 100)::bigint AS sell_cents,
                (COALESCE(SUM(t.escrow), 0) * 100)::bigint AS escrow_cents,
                (COALESCE(SUM(i.value), 0) * 100)::bigint AS inventory_cents,
                COUNT(*) FILTER (WHERE w.balance IS NOT NULL OR t.sell IS NOT NULL
                                    OR i.value IS NOT NULL) AS contributors
         FROM ends e
         CROSS JOIN chars c
         {}
         GROUP BY e.at
         ORDER BY e.at",
            carry_forward("e.at")
        );
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(account_id)
            .bind(character_ids)
            .bind(now)
            .bind(seconds(range.bucket()))
            .bind(seconds(range.span()))
            .fetch_all(&self.pool)
            .await?)
    }

    async fn inventory_rows(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        sort: InventorySort,
    ) -> StoreResult<Vec<InventoryRow>> {
        let direction = match sort.dir {
            Dir::Asc => "ASC",
            Dir::Desc => "DESC",
        };
        let sql = format!(
            "WITH g AS (
             SELECT a.character_id, a.type_id,
                    COALESCE(a.top_location_id, a.location_id) AS place_id,
                    (a.top_location_id IS NOT NULL) AS top_known,
                    a.is_blueprint_copy,
                    SUM(a.quantity)::bigint AS quantity,
                    CASE WHEN a.is_blueprint_copy THEN NULL ELSE p.average_price END AS unit_price
             FROM current_assets a
             JOIN characters c ON c.character_id = a.character_id
             LEFT JOIN item_prices p ON p.type_id = a.type_id
             WHERE c.account_id = $1 AND a.character_id = ANY($2)
             GROUP BY a.character_id, a.type_id, place_id, top_known, a.is_blueprint_copy,
                      unit_price
         )
         SELECT g.character_id, c.name AS character_name, g.type_id, tn.name AS type_name,
                g.place_id AS location_id, ln.name AS location_name,
                (ln.id IS NOT NULL) AS location_known, g.top_known, g.quantity,
                (g.unit_price * 100)::bigint AS unit_price_cents,
                (g.unit_price * g.quantity * 100)::bigint AS value_cents
         FROM (SELECT g.*, g.unit_price * g.quantity AS value FROM g) g
         JOIN characters c ON c.character_id = g.character_id
         LEFT JOIN names tn ON tn.id = g.type_id
         LEFT JOIN names ln ON ln.id = g.place_id
         ORDER BY {}, lower(COALESCE(tn.name, '#' || g.type_id)), c.character_id, g.place_id",
            order_by(sort.key, direction)
        );
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(account_id)
            .bind(character_ids)
            .fetch_all(&self.pool)
            .await?)
    }

    async fn list_wealth_freshness(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
    ) -> StoreResult<Vec<WealthFreshnessRecord>> {
        Ok(sqlx::query_as(
            "SELECT c.character_id, c.name, c.needs_relogin,
                w.last_success_at AS wallet_last_success_at, w.next_poll_at AS wallet_next_poll_at,
                a.last_success_at AS assets_last_success_at, a.next_poll_at AS assets_next_poll_at
         FROM characters c
         LEFT JOIN wealth_sync w ON w.character_id = c.character_id AND w.kind = 'wallet'
         LEFT JOIN wealth_sync a ON a.character_id = c.character_id AND a.kind = 'assets'
         WHERE c.account_id = $1 AND c.character_id = ANY($2)
         ORDER BY c.created_at, c.character_id",
        )
        .bind(account_id)
        .bind(character_ids)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn make_due(
        &self,
        character_id: i64,
        kind: Kind,
        now: OffsetDateTime,
    ) -> StoreResult<()> {
        sqlx::query(
            "UPDATE wealth_sync SET next_poll_at = LEAST(next_poll_at, $3)
                     WHERE character_id = $1 AND kind = $2",
        )
        .bind(character_id)
        .bind(kind.as_str())
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn full_read(&self, character_id: i64) -> StoreResult<bool> {
        let flag: Option<bool> = sqlx::query_scalar(
            "SELECT full_read FROM wealth_sync WHERE character_id = $1 AND kind = 'transactions'",
        )
        .bind(character_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(flag.unwrap_or(false))
    }

    async fn any_transaction_stored(&self, transaction_ids: &[i64]) -> StoreResult<bool> {
        Ok(sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM purchases WHERE transaction_id = ANY($1))
                         OR EXISTS (SELECT 1 FROM sales WHERE transaction_id = ANY($1))",
        )
        .bind(transaction_ids)
        .fetch_one(&self.pool)
        .await?)
    }

    async fn store_transactions(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        transactions: &[EsiTransaction],
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<(usize, usize)> {
        let mut tx = self.pool.begin().await?;
        let mut purchase_types = insert_transactions(
            &mut tx,
            character_id,
            transactions,
            true,
            "purchases",
            "bought_at",
        )
        .await?;
        let mut sale_types = insert_transactions(
            &mut tx,
            character_id,
            transactions,
            false,
            "sales",
            "sold_at",
        )
        .await?;
        let stored = (purchase_types.len(), sale_types.len());
        purchase_types.append(&mut sale_types);
        purchase_types.sort_unstable();
        purchase_types.dedup();
        crate::store::pg::trade::recompute(&mut tx, character_id, &purchase_types).await?;
        record_success(
            &mut tx,
            character_id,
            Kind::Transactions,
            polled_at,
            next_poll_at,
            esi_expires_at,
        )
        .await?;
        tx.commit().await?;
        Ok(stored)
    }

    async fn store_wallet(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        balance_cents: i64,
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO wallet_balances (character_id, polled_at, balance)
             VALUES ($1, $2, $3::numeric / 100)
             ON CONFLICT (character_id, polled_at) DO UPDATE SET balance = excluded.balance",
        )
        .bind(character_id)
        .bind(polled_at)
        .bind(balance_cents)
        .execute(&mut *tx)
        .await?;
        record_success(
            &mut tx,
            character_id,
            Kind::Wallet,
            polled_at,
            next_poll_at,
            esi_expires_at,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn store_assets(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        assets: &[EsiAsset],
        top_locations: &[Option<i64>],
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<()> {
        let item_id: Vec<i64> = assets.iter().map(|a| a.item_id).collect();
        let type_id: Vec<i64> = assets.iter().map(|a| a.type_id).collect();
        let location_id: Vec<i64> = assets.iter().map(|a| a.location_id).collect();
        let location_type: Vec<&str> = assets.iter().map(|a| a.location_type.as_str()).collect();
        let location_flag: Vec<&str> = assets.iter().map(|a| a.location_flag.as_str()).collect();
        let quantity: Vec<i64> = assets.iter().map(|a| a.quantity).collect();
        let singleton: Vec<bool> = assets.iter().map(|a| a.is_singleton).collect();
        let copy: Vec<bool> = assets.iter().map(|a| a.is_blueprint_copy).collect();
        let top: Vec<Option<i64>> = top_locations.to_vec();

        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM current_assets WHERE character_id = $1")
            .bind(character_id)
            .execute(&mut *tx)
            .await?;
        // An item that changed hands may still be on its previous owner's list: it moves.
        sqlx::query(
            "INSERT INTO current_assets
                 (item_id, character_id, type_id, location_id, location_type, location_flag,
                  quantity, is_singleton, is_blueprint_copy, top_location_id)
             SELECT a.item_id, $1, a.type_id, a.location_id, a.location_type, a.location_flag,
                    a.quantity, a.is_singleton, a.is_blueprint_copy, a.top_location_id
             FROM unnest($2::bigint[], $3::bigint[], $4::bigint[], $5::text[], $6::text[],
                         $7::bigint[], $8::bool[], $9::bool[], $10::bigint[])
                  AS a(item_id, type_id, location_id, location_type, location_flag, quantity,
                       is_singleton, is_blueprint_copy, top_location_id)
             ON CONFLICT (item_id) DO UPDATE SET
                 character_id = excluded.character_id,
                 type_id = excluded.type_id,
                 location_id = excluded.location_id,
                 location_type = excluded.location_type,
                 location_flag = excluded.location_flag,
                 quantity = excluded.quantity,
                 is_singleton = excluded.is_singleton,
                 is_blueprint_copy = excluded.is_blueprint_copy,
                 top_location_id = excluded.top_location_id",
        )
        .bind(character_id)
        .bind(&item_id)
        .bind(&type_id)
        .bind(&location_id)
        .bind(&location_type)
        .bind(&location_flag)
        .bind(&quantity)
        .bind(&singleton)
        .bind(&copy)
        .bind(&top)
        .execute(&mut *tx)
        .await?;
        // Two entries in the same instant keep the later one.
        let entry = format!(
            "INSERT INTO inventory_values (character_id, recorded_at, value)
             SELECT $1, $2, {INVENTORY_VALUE_SUM}
             FROM current_assets a LEFT JOIN item_prices p USING (type_id)
             WHERE a.character_id = $1
             ON CONFLICT (character_id, recorded_at) DO UPDATE SET value = excluded.value"
        );
        sqlx::query(sqlx::AssertSqlSafe(entry))
            .bind(character_id)
            .bind(polled_at)
            .execute(&mut *tx)
            .await?;
        record_success(
            &mut tx,
            character_id,
            Kind::Assets,
            polled_at,
            next_poll_at,
            esi_expires_at,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn record_failure(
        &self,
        character_id: i64,
        kind: Kind,
        now: OffsetDateTime,
        reason: &str,
        backoff_start: Duration,
        interval: Duration,
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO wealth_sync (character_id, kind, next_poll_at, last_attempt_at, last_error, failures)
             VALUES ($1, $2, $3, $4, $5, 1)
             ON CONFLICT (character_id, kind) DO UPDATE SET
                 next_poll_at = $4 + LEAST(
                     $6::float8 * power(2, LEAST(wealth_sync.failures, 20)) * interval '1 second',
                     $7::float8 * interval '1 second'),
                 last_attempt_at = $4,
                 last_error = $5,
                 failures = wealth_sync.failures + 1",
        )
        .bind(character_id)
        .bind(kind.as_str())
        .bind(now + backoff_start.min(interval))
        .bind(now)
        .bind(reason)
        .bind(backoff_start.as_seconds_f64())
        .bind(interval.as_seconds_f64())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn record_deferral(
        &self,
        character_id: i64,
        kind: Kind,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO wealth_sync (character_id, kind, next_poll_at, last_attempt_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (character_id, kind) DO UPDATE SET
                 next_poll_at = excluded.next_poll_at,
                 last_attempt_at = excluded.last_attempt_at",
        )
        .bind(character_id)
        .bind(kind.as_str())
        .bind(next_poll_at)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_unscheduled(&self, kind: Kind) -> StoreResult<Vec<i64>> {
        Ok(sqlx::query_scalar(
            "SELECT c.character_id FROM characters c
                 WHERE NOT c.needs_relogin
                   AND NOT EXISTS (SELECT 1 FROM wealth_sync s
                                   WHERE s.character_id = c.character_id AND s.kind = $1)",
        )
        .bind(kind.as_str())
        .fetch_all(&self.pool)
        .await?)
    }

    async fn insert_schedule(
        &self,
        kind: Kind,
        character_ids: &[i64],
        next_poll_at: &[OffsetDateTime],
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO wealth_sync (character_id, kind, next_poll_at)
                 SELECT c, $1, t FROM unnest($2::bigint[], $3::timestamptz[]) AS u(c, t)
                 ON CONFLICT (character_id, kind) DO NOTHING",
        )
        .bind(kind.as_str())
        .bind(character_ids)
        .bind(next_poll_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn claim_due(
        &self,
        now: OffsetDateTime,
        lease: Duration,
        limit: i64,
    ) -> StoreResult<Vec<(i64, Kind)>> {
        let claimed: Vec<(i64, String)> = sqlx::query_as(
            "UPDATE wealth_sync w SET next_poll_at = $1 + $2::float8 * interval '1 second'
             WHERE (w.character_id, w.kind) IN (
                 SELECT s.character_id, s.kind FROM wealth_sync s
                 JOIN characters c USING (character_id)
                 WHERE NOT c.needs_relogin AND s.next_poll_at <= $1
                 ORDER BY s.next_poll_at
                 LIMIT $3
                 FOR UPDATE OF s SKIP LOCKED)
             RETURNING w.character_id, w.kind",
        )
        .bind(now)
        .bind(lease.as_seconds_f64())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(claimed
            .into_iter()
            .filter_map(|(id, kind)| Kind::parse(&kind).map(|kind| (id, kind)))
            .collect())
    }
    async fn wealth_sync(&self, character_id: i64, kind: Kind) -> StoreResult<Option<SyncState>> {
        Ok(sqlx::query_as(
            "SELECT next_poll_at, last_success_at, last_attempt_at, last_error, esi_expires_at, failures FROM wealth_sync WHERE character_id = $1 AND kind = $2",
        )
        .bind(character_id)
        .bind(kind.as_str())
        .fetch_optional(&self.pool)
        .await?)
    }

    async fn seed_full_read(&self, character_id: i64, full_read: bool) -> StoreResult<()> {
        sqlx::query(
            "UPDATE wealth_sync SET full_read = $2 WHERE character_id = $1 AND kind = 'transactions'",
        )
        .bind(character_id)
        .bind(full_read)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn seed_inventory_value(
        &self,
        character_id: i64,
        recorded_at: OffsetDateTime,
        value_cents: i64,
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO inventory_values (character_id, recorded_at, value)
             VALUES ($1, $2, $3::numeric / 100)
             ON CONFLICT (character_id, recorded_at) DO UPDATE SET value = excluded.value",
        )
        .bind(character_id)
        .bind(recorded_at)
        .bind(value_cents)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
    async fn inventory_values(&self, character_id: i64) -> StoreResult<Vec<(OffsetDateTime, i64)>> {
        Ok(sqlx::query_as(
            "SELECT recorded_at, (value * 100)::bigint FROM inventory_values
             WHERE character_id = $1 ORDER BY recorded_at",
        )
        .bind(character_id)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn seed_asset(
        &self,
        character_id: i64,
        a: &EsiAsset,
        top_location_id: Option<i64>,
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO current_assets (item_id, character_id, type_id, location_id, location_type,
                 location_flag, quantity, is_singleton, is_blueprint_copy, top_location_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (item_id) DO UPDATE SET
                 character_id = excluded.character_id, type_id = excluded.type_id,
                 location_id = excluded.location_id, location_type = excluded.location_type,
                 location_flag = excluded.location_flag, quantity = excluded.quantity,
                 is_singleton = excluded.is_singleton,
                 is_blueprint_copy = excluded.is_blueprint_copy,
                 top_location_id = excluded.top_location_id",
        )
        .bind(a.item_id)
        .bind(character_id)
        .bind(a.type_id)
        .bind(a.location_id)
        .bind(&a.location_type)
        .bind(&a.location_flag)
        .bind(a.quantity)
        .bind(a.is_singleton)
        .bind(a.is_blueprint_copy)
        .bind(top_location_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn seed_poll_totals(&self, character_id: i64, t: &PollTotalsRow) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO order_poll_totals
                 (character_id, polled_at, sell_value, escrow, sell_corp, escrow_corp, sell_count,
                  buy_count)
             VALUES ($1, $2, $3::numeric / 100, $4::numeric / 100, $5::numeric / 100,
                     $6::numeric / 100, $7, $8)
             ON CONFLICT (character_id, polled_at) DO UPDATE SET
                 sell_value = excluded.sell_value, escrow = excluded.escrow,
                 sell_corp = excluded.sell_corp, escrow_corp = excluded.escrow_corp,
                 sell_count = excluded.sell_count, buy_count = excluded.buy_count",
        )
        .bind(character_id)
        .bind(t.polled_at)
        .bind(t.sell_cents)
        .bind(t.escrow_cents)
        .bind(t.sell_corp_cents)
        .bind(t.escrow_corp_cents)
        .bind(t.sell_count as i32)
        .bind(t.buy_count as i32)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
    async fn wallet_balances(&self, character_id: i64) -> StoreResult<Vec<(OffsetDateTime, i64)>> {
        Ok(sqlx::query_as(
            "SELECT polled_at, (balance * 100)::bigint FROM wallet_balances
             WHERE character_id = $1 ORDER BY polled_at",
        )
        .bind(character_id)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn asset_ids(&self, character_id: i64) -> StoreResult<Vec<i64>> {
        Ok(sqlx::query_scalar(
            "SELECT item_id FROM current_assets WHERE character_id = $1 ORDER BY item_id",
        )
        .bind(character_id)
        .fetch_all(&self.pool)
        .await?)
    }
}

async fn record_success(
    tx: &mut sqlx::PgConnection,
    character_id: i64,
    kind: Kind,
    polled_at: OffsetDateTime,
    next_poll_at: OffsetDateTime,
    expires_at: Option<OffsetDateTime>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO wealth_sync
                 (character_id, kind, next_poll_at, last_success_at, last_attempt_at, last_error,
                  esi_expires_at, failures)
             VALUES ($1, $2, $3, $4, $4, NULL, $5, 0)
             ON CONFLICT (character_id, kind) DO UPDATE SET
                 next_poll_at = excluded.next_poll_at,
                 last_success_at = excluded.last_success_at,
                 last_attempt_at = excluded.last_attempt_at,
                 last_error = NULL,
                 esi_expires_at = excluded.esi_expires_at,
                 failures = 0,
                 full_read = false",
    )
    .bind(character_id)
    .bind(kind.as_str())
    .bind(next_poll_at)
    .bind(polled_at)
    .bind(expires_at)
    .execute(tx)
    .await?;
    Ok(())
}

/// Inserts the personal purchases (`buys`) or sales among the transactions that are not stored
/// yet. Returns the item type of each new row.
async fn insert_transactions(
    tx: &mut sqlx::PgConnection,
    character_id: i64,
    transactions: &[EsiTransaction],
    buys: bool,
    table: &'static str,
    date_column: &'static str,
) -> Result<Vec<i64>, sqlx::Error> {
    let rows: Vec<&EsiTransaction> = transactions
        .iter()
        .filter(|t| t.is_buy == buys && t.is_personal)
        .collect();
    let ids: Vec<i64> = rows.iter().map(|t| t.transaction_id).collect();
    let types: Vec<i64> = rows.iter().map(|t| t.type_id).collect();
    let locations: Vec<i64> = rows.iter().map(|t| t.location_id).collect();
    let quantities: Vec<i64> = rows.iter().map(|t| t.quantity).collect();
    let prices: Vec<i64> = rows.iter().map(|t| t.unit_price_cents).collect();
    let dates: Vec<OffsetDateTime> = rows.iter().map(|t| t.bought_at).collect();
    // Both tables differ only in the name of the date column.
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {table}
                 (transaction_id, character_id, type_id, location_id, quantity, unit_price, {date_column})
             SELECT p.id, $1, p.type_id, p.location_id, p.quantity, p.price_cents::numeric / 100, p.at
             FROM unnest($2::bigint[], $3::bigint[], $4::bigint[], $5::bigint[], $6::bigint[],
                         $7::timestamptz[])
                  AS p(id, type_id, location_id, quantity, price_cents, at)
             ON CONFLICT (transaction_id) DO NOTHING
             RETURNING type_id"
        )))
        .bind(character_id)
        .bind(&ids)
        .bind(&types)
        .bind(&locations)
        .bind(&quantities)
        .bind(&prices)
        .bind(&dates)
        .fetch_all(tx)
        .await
}
