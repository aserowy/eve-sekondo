//! The trades overview: the personal wallet transactions of an account's selected characters, and
//! what the sales among them earned.

use time::OffsetDateTime;

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
