//! What the selected characters are worth: the current figures and their history, from one set
//! of queries, so the dashboard's line is the top edge of the wealth page's stack and the tile
//! is the stack's last point by construction.
//!
//! A character's wealth is its wallet, its personal sell orders, the escrow of its personal buy
//! orders and its inventory. Each series is read as the latest recorded figure at or before a
//! time, so one that is recorded rarely carries forward, and one that was never recorded counts
//! as zero.

use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    range::{BUCKET_CTES, Range},
    selection::CharacterSelection,
};

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

/// The figures summed over the selection, in cents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, sqlx::FromRow)]
pub struct Current {
    pub wallet_cents: i64,
    pub sell_cents: i64,
    pub escrow_cents: i64,
    pub inventory_cents: i64,
}

impl Current {
    pub fn total_cents(&self) -> i64 {
        self.wallet_cents + self.sell_cents + self.escrow_cents + self.inventory_cents
    }
}

/// The latest figures, with no upper time bound.
pub async fn current(
    pool: &PgPool,
    account_id: Uuid,
    selection: &CharacterSelection,
) -> Result<Current, sqlx::Error> {
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
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(account_id)
        .bind(selection.ids())
        .fetch_one(pool)
        .await
}

/// One point of the wealth graphs: the figures in force at `at`, summed over the selection.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct WealthPoint {
    pub at: OffsetDateTime,
    pub wallet_cents: i64,
    pub sell_cents: i64,
    pub escrow_cents: i64,
    pub inventory_cents: i64,
    /// How many selected characters had any figure at or before `at`.
    pub contributors: i64,
}

impl WealthPoint {
    pub fn total_cents(&self) -> i64 {
        self.wallet_cents + self.sell_cents + self.escrow_cents + self.inventory_cents
    }
}

/// Whether any selected character has a recorded figure by the end of the series.
pub fn has_history(points: &[WealthPoint]) -> bool {
    points.last().is_some_and(|point| point.contributors > 0)
}

/// The series over `range` ending at `now`, one point per bucket: its timestamp is the bucket's
/// end (the last one clamped to `now`) and its figures are each selected character's latest
/// figures at or before it, summed.
pub async fn history(
    pool: &PgPool,
    account_id: Uuid,
    selection: &CharacterSelection,
    range: Range,
    now: OffsetDateTime,
) -> Result<Vec<WealthPoint>, sqlx::Error> {
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
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(account_id)
        .bind(selection.ids())
        .bind(now)
        .bind(seconds(range.bucket()))
        .bind(seconds(range.span()))
        .fetch_all(pool)
        .await
}

// ---- the inventory list ----

use crate::orders::Dir;

/// The columns the inventory list can be sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventorySortKey {
    Character,
    Item,
    Quantity,
    /// The unit price; unpriced rows last.
    Price,
    /// Quantity times unit price; unpriced rows last.
    Value,
    Location,
}

impl InventorySortKey {
    pub const ALL: [Self; 6] = [
        Self::Character,
        Self::Item,
        Self::Quantity,
        Self::Price,
        Self::Value,
        Self::Location,
    ];

    /// The name used in `sort`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Character => "character",
            Self::Item => "item",
            Self::Quantity => "quantity",
            Self::Price => "price",
            Self::Value => "value",
            Self::Location => "location",
        }
    }

    /// The SQL `ORDER BY` terms for a direction. Only these literals ever reach the query text.
    /// Unpriced rows go last in both directions, so their term does not follow the direction.
    fn order_by(self, direction: &str) -> String {
        match self {
            Self::Price => format!("(g.unit_price IS NULL), g.unit_price {direction}"),
            Self::Value => format!("(g.value IS NULL), g.value {direction}"),
            Self::Character => format!("lower(c.name) {direction}"),
            Self::Item => format!("lower(COALESCE(tn.name, '#' || g.type_id)) {direction}"),
            Self::Quantity => format!("g.quantity {direction}"),
            Self::Location => format!("lower(COALESCE(ln.name, '#' || g.place_id)) {direction}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InventorySort {
    pub key: InventorySortKey,
    pub dir: Dir,
}

impl Default for InventorySort {
    /// Value, highest first.
    fn default() -> Self {
        Self {
            key: InventorySortKey::Value,
            dir: Dir::Desc,
        }
    }
}

impl InventorySort {
    /// Unknown or missing values fall back to the default instead of failing the request: an
    /// unknown column gives the whole default sorting, an unknown direction ascending.
    pub fn parse(key: Option<&str>, dir: Option<&str>) -> Self {
        let Some(key) = key.and_then(|k| {
            InventorySortKey::ALL
                .into_iter()
                .find(|candidate| candidate.name() == k)
        }) else {
            return Self::default();
        };
        let dir = match dir {
            Some("desc") => Dir::Desc,
            _ => Dir::Asc,
        };
        Self { key, dir }
    }
}

/// One row: what a character holds of an item at a top-level location.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct InventoryRow {
    pub character_id: i64,
    pub character_name: String,
    pub type_id: i64,
    pub type_name: Option<String>,
    /// The top-level location, or the asset's own location when that could not be resolved.
    pub location_id: i64,
    pub location_name: Option<String>,
    /// There is a row for the location in the name cache (its name may be unresolvable).
    pub location_known: bool,
    /// The top-level location could be determined.
    pub top_known: bool,
    pub quantity: i64,
    /// `None` for an item without an average price and for blueprint copies.
    pub unit_price_cents: Option<i64>,
    /// Quantity times unit price.
    pub value_cents: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Inventory {
    pub rows: Vec<InventoryRow>,
    /// The sum of the values of all rows.
    pub total_cents: i64,
    pub unpriced_rows: usize,
}

/// The selected characters' current assets, one row per character, item and top-level location
/// with the quantities summed. Blueprint copies are rows of their own: they are unpriced, and
/// must not add to the quantity an original is valued with.
pub async fn inventory(
    pool: &PgPool,
    account_id: Uuid,
    selection: &CharacterSelection,
    sort: InventorySort,
) -> Result<Inventory, sqlx::Error> {
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
        sort.key.order_by(direction)
    );
    let rows: Vec<InventoryRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(account_id)
        .bind(selection.ids())
        .fetch_all(pool)
        .await?;
    let total_cents = rows.iter().filter_map(|r| r.value_cents).sum();
    let unpriced_rows = rows.iter().filter(|r| r.value_cents.is_none()).count();
    Ok(Inventory {
        rows,
        total_cents,
        unpriced_rows,
    })
}
