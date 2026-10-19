//! The trades overview: the personal wallet transactions of an account's selected characters, and
//! what the sales among them earned.

use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{fees::SALE_FEE_RATES_SQL, selection::CharacterSelection};

/// One purchase or sale.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TradeRow {
    pub transaction_id: i64,
    pub is_sale: bool,
    pub character_id: i64,
    pub character_name: String,
    pub type_id: i64,
    pub type_name: Option<String>,
    pub location_id: i64,
    pub location_name: Option<String>,
    pub quantity: i64,
    pub unit_price_cents: i64,
    pub at: OffsetDateTime,
    /// Only a sale whose cost covers its whole quantity and whose fees are known has one.
    pub margin_cents: Option<i64>,
    /// How many units of a sale have a known cost.
    pub covered_units: Option<i64>,
    /// The average cost per unit of a sale whose cost covers its whole quantity.
    pub cost_per_unit_cents: Option<i64>,
}

/// The transactions dated within `[from, to]` of the selected characters, restricted to the
/// account in the query itself so a selection can never reach another account's data. Newest
/// first, the higher transaction ID first among equal dates.
///
/// The margin of a sale is `Q × P × (1 − t − b) − cost`, with the current rates (design §4).
/// `numeric` `round` is half away from zero, as for the orders.
pub async fn list(
    pool: &PgPool,
    account_id: Uuid,
    selection: &CharacterSelection,
    from: OffsetDateTime,
    to: OffsetDateTime,
) -> Result<Vec<TradeRow>, sqlx::Error> {
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
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(selection.ids())
        .bind(account_id)
        .bind(from)
        .bind(to)
        .fetch_all(pool)
        .await
}
