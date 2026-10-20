//! What the selected characters are worth: the current figures and their history, from one set
//! of queries, so the dashboard's line is the top edge of the wealth page's stack and the tile
//! is the stack's last point by construction.
//!
//! A character's wealth is its wallet, its personal sell orders, the escrow of its personal buy
//! orders and its inventory. Each series is read as the latest recorded figure at or before a
//! time, so one that is recorded rarely carries forward, and one that was never recorded counts
//! as zero.

use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    selection::CharacterSelection,
    store::{StoreError, WealthStore},
};

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
    store: &dyn WealthStore,
    account_id: Uuid,
    selection: &CharacterSelection,
    sort: InventorySort,
) -> Result<Inventory, StoreError> {
    let rows = store
        .inventory_rows(account_id, selection.ids(), sort)
        .await?;
    let total_cents = rows.iter().filter_map(|r| r.value_cents).sum();
    let unpriced_rows = rows.iter().filter(|r| r.value_cents.is_none()).count();
    Ok(Inventory {
        rows,
        total_cents,
        unpriced_rows,
    })
}
