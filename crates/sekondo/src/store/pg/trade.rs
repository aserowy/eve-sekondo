use async_trait::async_trait;
use sqlx::{PgConnection, PgPool};
use std::collections::BTreeMap;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    freshness::FreshnessRecord,
    store::pg::fee::SALE_FEE_RATES_SQL,
    store::{RememberedSellOrder, StoreResult, TradeSeed, TradeStore},
    trade::{Lot, RememberedOrder, Sale, allocate, allocate_sales},
    trades::TradeRow,
};

pub struct PgTradeStore {
    pool: PgPool,
}

impl PgTradeStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TradeStore for PgTradeStore {
    async fn list_trades(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        from: OffsetDateTime,
        to: OffsetDateTime,
    ) -> StoreResult<Vec<TradeRow>> {
        let sql = format!(
            "SELECT t.* FROM (
             SELECT p.transaction_id, false AS is_sale, p.character_id, c.name AS character_name,
                    p.type_id, tn.name AS type_name, p.location_id, ln.name AS location_name,
                    p.quantity, (p.unit_price * 100)::bigint AS unit_price_cents,
                    p.bought_at AS at,
                    NULL::bigint AS margin_cents, NULL::bigint AS covered_units,
                    NULL::bigint AS cost_per_unit_cents
             FROM purchases p
             JOIN characters c ON c.character_id = p.character_id
             LEFT JOIN names tn ON tn.id = p.type_id
             LEFT JOIN names ln ON ln.id = p.location_id
             WHERE c.account_id = $2 AND p.character_id = ANY($1) AND p.bought_at BETWEEN $3 AND $4
             UNION ALL
             SELECT s.transaction_id, true, s.character_id, c.name,
                    s.type_id, tn.name, s.location_id, ln.name,
                    s.quantity, (s.unit_price * 100)::bigint,
                    s.sold_at,
                    CASE WHEN sc.covered_units = s.quantity
                              AND f.sales_tax IS NOT NULL AND f.broker_fee IS NOT NULL
                         THEN (round(s.quantity * s.unit_price
                                     * (1 - f.sales_tax / 100 - f.broker_fee / 100) - sc.cost, 2)
                               * 100)::bigint
                    END,
                    sc.covered_units,
                    CASE WHEN sc.covered_units = s.quantity AND s.quantity > 0
                         THEN round(sc.cost * 100 / s.quantity)::bigint
                    END
             FROM sales s
             JOIN characters c ON c.character_id = s.character_id
             LEFT JOIN names tn ON tn.id = s.type_id
             LEFT JOIN names ln ON ln.id = s.location_id
             LEFT JOIN sale_costs sc ON sc.transaction_id = s.transaction_id
             LEFT JOIN ({SALE_FEE_RATES_SQL}) f
                    ON f.character_id = s.character_id AND f.location_id = s.location_id
             WHERE c.account_id = $2 AND s.character_id = ANY($1) AND s.sold_at BETWEEN $3 AND $4
         ) t
         ORDER BY t.at DESC, t.transaction_id DESC"
        );
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(character_ids)
            .bind(account_id)
            .bind(from)
            .bind(to)
            .fetch_all(&self.pool)
            .await?)
    }

    async fn list_transactions_freshness(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
    ) -> StoreResult<Vec<FreshnessRecord>> {
        Ok(sqlx::query_as(
            "SELECT c.character_id, c.name, c.needs_relogin, s.last_success_at, s.next_poll_at
         FROM characters c
         LEFT JOIN wealth_sync s ON s.character_id = c.character_id AND s.kind = 'transactions'
         WHERE c.account_id = $1 AND c.character_id = ANY($2)
         ORDER BY c.created_at, c.character_id",
        )
        .bind(account_id)
        .bind(character_ids)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn seed_trade(&self, character_id: i64, t: &TradeSeed) -> StoreResult<()> {
        let sql = if t.is_sale {
            "INSERT INTO sales (transaction_id, character_id, type_id, location_id, quantity,
                 unit_price, sold_at)
             VALUES ($1, $2, $3, $4, $5, $6::numeric / 100, $7)
             ON CONFLICT (transaction_id) DO UPDATE SET
                 character_id = excluded.character_id, type_id = excluded.type_id,
                 location_id = excluded.location_id, quantity = excluded.quantity,
                 unit_price = excluded.unit_price, sold_at = excluded.sold_at"
        } else {
            "INSERT INTO purchases (transaction_id, character_id, type_id, location_id, quantity,
                 unit_price, bought_at)
             VALUES ($1, $2, $3, $4, $5, $6::numeric / 100, $7)
             ON CONFLICT (transaction_id) DO UPDATE SET
                 character_id = excluded.character_id, type_id = excluded.type_id,
                 location_id = excluded.location_id, quantity = excluded.quantity,
                 unit_price = excluded.unit_price, bought_at = excluded.bought_at"
        };
        sqlx::query(sql)
            .bind(t.transaction_id)
            .bind(character_id)
            .bind(t.type_id)
            .bind(t.location_id)
            .bind(t.quantity)
            .bind(t.unit_price_cents)
            .bind(t.at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn seed_remembered_order(
        &self,
        character_id: i64,
        order: &RememberedSellOrder,
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO sell_order_history (order_id, character_id, type_id, first_issued, volume_total)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (order_id) DO UPDATE SET
                 character_id = excluded.character_id, type_id = excluded.type_id,
                 first_issued = excluded.first_issued, volume_total = excluded.volume_total",
        )
        .bind(order.order_id)
        .bind(character_id)
        .bind(order.type_id)
        .bind(order.first_issued)
        .bind(order.volume_total)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn seed_order_cost(
        &self,
        order_id: i64,
        covered_units: i64,
        cost_cents: i64,
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO sell_order_costs (order_id, covered_units, cost)
             VALUES ($1, $2, $3::numeric / 100)
             ON CONFLICT (order_id) DO UPDATE SET
                 covered_units = excluded.covered_units, cost = excluded.cost",
        )
        .bind(order_id)
        .bind(covered_units)
        .bind(cost_cents)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn seed_sale_cost(
        &self,
        transaction_id: i64,
        covered_units: i64,
        cost_cents: i64,
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO sale_costs (transaction_id, covered_units, cost)
             VALUES ($1, $2, $3::numeric / 100)
             ON CONFLICT (transaction_id) DO UPDATE SET
                 covered_units = excluded.covered_units, cost = excluded.cost",
        )
        .bind(transaction_id)
        .bind(covered_units)
        .bind(cost_cents)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete_sale(&self, transaction_id: i64) -> StoreResult<()> {
        sqlx::query("DELETE FROM sales WHERE transaction_id = $1")
            .bind(transaction_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn settle(&self, character_id: i64, type_ids: &[i64]) -> StoreResult<()> {
        let mut tx = self.pool.begin().await?;
        recompute(&mut tx, character_id, type_ids).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn order_costs(&self) -> StoreResult<Vec<(i64, i64, i64)>> {
        Ok(sqlx::query_as(
            "SELECT order_id, covered_units, (cost * 100)::bigint FROM sell_order_costs
             ORDER BY order_id",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    async fn sale_costs(&self) -> StoreResult<Vec<(i64, i64, i64)>> {
        Ok(sqlx::query_as(
            "SELECT transaction_id, covered_units, (cost * 100)::bigint FROM sale_costs
             ORDER BY transaction_id",
        )
        .fetch_all(&self.pool)
        .await?)
    }
}

/// Redoes both allocations of one character for these items from all stored lots, remembered
/// orders and sales, and replaces their stored costs. Every remembered order and every sale of the
/// items gets a row, also one that took nothing. Run it in the transaction that changed the lots,
/// the orders or the sales.
pub async fn recompute(
    conn: &mut PgConnection,
    character_id: i64,
    type_ids: &[i64],
) -> Result<(), sqlx::Error> {
    if type_ids.is_empty() {
        return Ok(());
    }
    let lot_rows: Vec<(i64, i64, OffsetDateTime, i64, i64)> = sqlx::query_as(
        "SELECT type_id, transaction_id, bought_at, quantity, (unit_price * 100)::bigint
         FROM purchases WHERE character_id = $1 AND type_id = ANY($2)",
    )
    .bind(character_id)
    .bind(type_ids)
    .fetch_all(&mut *conn)
    .await?;
    let order_rows: Vec<(i64, i64, OffsetDateTime, i64)> = sqlx::query_as(
        "SELECT type_id, order_id, first_issued, volume_total
         FROM sell_order_history WHERE character_id = $1 AND type_id = ANY($2)",
    )
    .bind(character_id)
    .bind(type_ids)
    .fetch_all(&mut *conn)
    .await?;
    let sale_rows: Vec<(i64, i64, OffsetDateTime, i64)> = sqlx::query_as(
        "SELECT type_id, transaction_id, sold_at, quantity
         FROM sales WHERE character_id = $1 AND type_id = ANY($2)",
    )
    .bind(character_id)
    .bind(type_ids)
    .fetch_all(&mut *conn)
    .await?;

    #[derive(Default)]
    struct OfType {
        lots: Vec<Lot>,
        orders: Vec<RememberedOrder>,
        sales: Vec<Sale>,
    }
    let mut per_type: BTreeMap<i64, OfType> = BTreeMap::new();
    for (type_id, transaction_id, bought_at, quantity, unit_price_cents) in lot_rows {
        per_type.entry(type_id).or_default().lots.push(Lot {
            transaction_id,
            bought_at,
            quantity,
            unit_price_cents,
        });
    }
    for (type_id, order_id, first_issued, volume_total) in order_rows {
        per_type
            .entry(type_id)
            .or_default()
            .orders
            .push(RememberedOrder {
                order_id,
                first_issued,
                volume_total,
            });
    }
    for (type_id, transaction_id, sold_at, quantity) in sale_rows {
        per_type.entry(type_id).or_default().sales.push(Sale {
            transaction_id,
            sold_at,
            quantity,
        });
    }

    let mut order_ids = Vec::new();
    let mut covered = Vec::new();
    let mut cents = Vec::new();
    let mut sale_ids = Vec::new();
    let mut sale_covered = Vec::new();
    let mut sale_cents = Vec::new();
    for of_type in per_type.values() {
        for cost in allocate(&of_type.lots, &of_type.orders) {
            order_ids.push(cost.order_id);
            covered.push(cost.covered_units);
            // `numeric` takes what an `i128` holds; the text carries it without a lossy cast.
            cents.push(cost.cost_cents.to_string());
        }
        for cost in allocate_sales(&of_type.lots, &of_type.sales) {
            sale_ids.push(cost.transaction_id);
            sale_covered.push(cost.covered_units);
            sale_cents.push(cost.cost_cents.to_string());
        }
    }

    sqlx::query(
        "DELETE FROM sell_order_costs
         WHERE order_id IN (SELECT order_id FROM sell_order_history
                            WHERE character_id = $1 AND type_id = ANY($2))",
    )
    .bind(character_id)
    .bind(type_ids)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO sell_order_costs (order_id, covered_units, cost)
         SELECT c.order_id, c.covered_units, c.cents::numeric / 100
         FROM unnest($1::bigint[], $2::bigint[], $3::text[]) AS c(order_id, covered_units, cents)",
    )
    .bind(&order_ids)
    .bind(&covered)
    .bind(&cents)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "DELETE FROM sale_costs
         WHERE transaction_id IN (SELECT transaction_id FROM sales
                                  WHERE character_id = $1 AND type_id = ANY($2))",
    )
    .bind(character_id)
    .bind(type_ids)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO sale_costs (transaction_id, covered_units, cost)
         SELECT c.transaction_id, c.covered_units, c.cents::numeric / 100
         FROM unnest($1::bigint[], $2::bigint[], $3::text[])
              AS c(transaction_id, covered_units, cents)",
    )
    .bind(&sale_ids)
    .bind(&sale_covered)
    .bind(&sale_cents)
    .execute(&mut *conn)
    .await?;
    Ok(())
}
