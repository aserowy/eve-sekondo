//! The tables of the in-memory stores, as plain collections.

use std::collections::BTreeMap;

use time::{Date, OffsetDateTime};
use tower_sessions::session::Record;
use uuid::Uuid;

use super::numeric::Numeric;
use crate::{opportunities::analysis::ExpandedHour, store::WindowRow};

/// A row of `characters`.
#[derive(Debug, Clone)]
pub(super) struct CharacterRow {
    pub character_id: i64,
    pub account_id: Uuid,
    pub name: String,
    pub owner_hash: String,
    pub scopes: Vec<String>,
    pub needs_relogin: bool,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

/// A row of `character_tokens`.
#[derive(Debug, Clone)]
pub(super) struct TokenRow {
    pub refresh_token: Vec<u8>,
    pub access_token: Option<Vec<u8>>,
    pub access_expires_at: Option<OffsetDateTime>,
}

/// The columns every `*_sync` table shares.
#[derive(Debug, Clone)]
pub(super) struct Sync {
    pub next_poll_at: OffsetDateTime,
    pub last_success_at: Option<OffsetDateTime>,
    pub last_attempt_at: Option<OffsetDateTime>,
    pub last_error: Option<String>,
    pub esi_expires_at: Option<OffsetDateTime>,
    pub failures: i32,
    pub full_read: bool,
}

impl Sync {
    pub fn state(&self) -> crate::store::SyncState {
        crate::store::SyncState {
            next_poll_at: self.next_poll_at,
            last_success_at: self.last_success_at,
            last_attempt_at: self.last_attempt_at,
            last_error: self.last_error.clone(),
            esi_expires_at: self.esi_expires_at,
            failures: self.failures,
        }
    }

    /// A row that was only scheduled.
    pub fn scheduled(next_poll_at: OffsetDateTime) -> Self {
        Self {
            next_poll_at,
            last_success_at: None,
            last_attempt_at: None,
            last_error: None,
            esi_expires_at: None,
            failures: 0,
            full_read: false,
        }
    }
}

/// A row of `current_orders` or `order_snapshots`.
#[derive(Debug, Clone)]
#[allow(dead_code)] // columns the real table has, kept so a row is whole
pub(super) struct OrderRow {
    pub character_id: i64,
    pub type_id: i64,
    pub region_id: i64,
    pub location_id: i64,
    pub is_buy_order: bool,
    pub is_corporation: bool,
    pub price_cents: i64,
    pub volume_remain: i64,
    pub volume_total: i64,
    pub min_volume: i64,
    pub range: String,
    pub escrow_cents: Option<i64>,
    pub issued: OffsetDateTime,
    pub duration: i32,
}

/// A row of `order_poll_totals`, in cents.
#[derive(Debug, Clone, Default)]
pub(super) struct PollTotals {
    pub sell_value: i128,
    pub escrow: i128,
    pub sell_corp: i128,
    pub escrow_corp: i128,
    pub sell_count: i64,
    pub buy_count: i64,
}

/// A row of `names`.
#[derive(Debug, Clone)]
#[allow(dead_code)] // columns the real table has, kept so a row is whole
pub(super) struct NameRow {
    pub category: String,
    pub name: Option<String>,
    pub resolved_at: OffsetDateTime,
    pub retry_after: Option<OffsetDateTime>,
}

/// A row of `market_orders`.
#[derive(Debug, Clone)]
pub(super) struct MarketOrderRow {
    pub region_id: i64,
    pub type_id: i64,
    pub location_id: i64,
    pub system_id: i64,
    pub is_buy_order: bool,
    pub price_cents: i64,
    pub range: String,
    pub volume_remain: i64,
}

/// A row of `current_assets`.
#[derive(Debug, Clone)]
#[allow(dead_code)] // columns the real table has, kept so a row is whole
pub(super) struct AssetRow {
    pub character_id: i64,
    pub type_id: i64,
    pub location_id: i64,
    pub location_type: String,
    pub location_flag: String,
    pub quantity: i64,
    pub is_singleton: bool,
    pub is_blueprint_copy: bool,
    pub top_location_id: Option<i64>,
}

/// A row of `purchases` or `sales`.
#[derive(Debug, Clone)]
pub(super) struct TransactionRow {
    pub character_id: i64,
    pub type_id: i64,
    pub location_id: i64,
    pub quantity: i64,
    pub unit_price_cents: i64,
    pub at: OffsetDateTime,
}

/// A row of `sell_order_history`.
#[derive(Debug, Clone)]
pub(super) struct SellOrderRow {
    pub character_id: i64,
    pub type_id: i64,
    pub first_issued: OffsetDateTime,
    pub volume_total: i64,
}

/// A row of `sell_order_costs` or `sale_costs`.
#[derive(Debug, Clone)]
pub(super) struct CostRow {
    pub covered_units: i64,
    pub cost_cents: i128,
}

/// A row of `station_orders`.
#[derive(Debug, Clone)]
pub(super) struct StationOrderRow {
    pub region_id: i64,
    pub station_id: i64,
    pub type_id: i64,
    pub is_buy_order: bool,
    pub price_cents: i64,
    pub volume_remain: i64,
}

/// A row of `station_snapshots`.
#[derive(Debug, Clone)]
pub(super) struct SnapshotRow {
    pub recorded_at: OffsetDateTime,
    pub region_id: i64,
    pub station_id: i64,
    pub type_id: i64,
    pub best_buy: Numeric,
    pub best_sell: Numeric,
    pub spread: Numeric,
    pub buy_near_units: i64,
    pub buy_near_orders: i32,
    pub sell_near_units: i64,
    pub sell_near_orders: i32,
    pub buy_units: i64,
    pub sell_units: i64,
    pub buy_taken: i64,
    pub sell_taken: i64,
}

/// A row of `market_history`.
#[derive(Debug, Clone)]
pub(super) struct HistoryRow {
    pub average: Numeric,
    pub highest: Numeric,
    pub lowest: Numeric,
    pub volume: i64,
    pub order_count: i64,
}

/// A row of `station_item_stats`.
#[derive(Debug, Clone, Default)]
pub(super) struct ItemStatsRow {
    pub book_at: Option<OffsetDateTime>,
    pub best_buy: Option<Numeric>,
    pub best_sell: Option<Numeric>,
    pub buy_near_orders: Option<i32>,
    pub sell_near_orders: Option<i32>,
    pub rho: Option<f64>,
    pub indicators_at: Option<OffsetDateTime>,
    pub volume_7d: Option<f64>,
    pub volume_30d: Option<f64>,
    pub sigma: Option<f64>,
    pub returns: Option<i32>,
    pub sma5: Option<f64>,
    pub sma20: Option<f64>,
    pub cross_day: Option<Date>,
    pub volume_rising: Option<bool>,
    pub percent_b: Option<f64>,
    pub rsi14: Option<f64>,
    pub expansion_at: Option<OffsetDateTime>,
}

/// A row of `opportunity_settings`.
#[derive(Debug, Clone)]
pub(super) struct SettingsRow {
    pub min_margin: Numeric,
    pub min_isk_per_day: Numeric,
    pub search: String,
    pub capital: Numeric,
    pub fee_character_id: Option<i64>,
}

impl SettingsRow {
    pub fn of(
        settings: &crate::opportunities::settings::Settings,
        fee_character_id: Option<i64>,
    ) -> Self {
        Self {
            min_margin: Numeric::from_f64(settings.min_margin),
            min_isk_per_day: Numeric::from_f64(settings.min_isk_per_day),
            search: settings.search.clone(),
            capital: Numeric::from_f64(settings.capital),
            fee_character_id,
        }
    }
}

/// A materialized bucket of `station_hourly` or `station_daily`.
#[derive(Debug, Clone)]
pub(super) struct Bucket {
    pub best_buy_min: Numeric,
    pub best_buy_max: Numeric,
    pub best_buy_avg: Numeric,
    pub best_sell_min: Numeric,
    pub best_sell_max: Numeric,
    pub best_sell_avg: Numeric,
    pub spread_min: Numeric,
    pub spread_max: Numeric,
    pub spread_avg: Numeric,
    pub buy_taken: i64,
    pub sell_taken: i64,
    pub snapshots: i64,
    /// Only on a daily bucket: how many hourly buckets it was built from.
    pub hours_covered: i64,
}

/// Every table, in plain collections. A table that references `characters` loses its rows with
/// the character, as `ON DELETE CASCADE` does.
#[derive(Default)]
pub(super) struct Tables {
    pub accounts: BTreeMap<Uuid, OffsetDateTime>,
    pub characters: BTreeMap<i64, CharacterRow>,
    pub tokens: BTreeMap<i64, TokenRow>,
    pub sessions: BTreeMap<String, Record>,

    pub current_orders: BTreeMap<i64, OrderRow>,
    pub order_snapshots: Vec<(OffsetDateTime, i64, OrderRow)>,
    pub order_poll_totals: BTreeMap<(i64, OffsetDateTime), PollTotals>,
    pub order_sync: BTreeMap<i64, Sync>,

    pub names: BTreeMap<i64, NameRow>,
    pub market_orders: BTreeMap<i64, MarketOrderRow>,
    pub market_sync: BTreeMap<(i64, i64), Sync>,
    pub item_prices: BTreeMap<i64, Option<i64>>,
    pub price_sync: Option<Sync>,

    pub wallet_balances: BTreeMap<(i64, OffsetDateTime), i64>,
    pub current_assets: BTreeMap<i64, AssetRow>,
    pub inventory_values: BTreeMap<(i64, OffsetDateTime), i128>,
    pub wealth_sync: BTreeMap<(i64, String), Sync>,

    pub character_skills: BTreeMap<(i64, i64), i16>,
    pub character_standings: BTreeMap<(i64, i64, String), Numeric>,
    pub character_locations: BTreeMap<i64, (i64, String, OffsetDateTime)>,
    pub station_owners: BTreeMap<i64, (i64, OffsetDateTime)>,
    pub fee_sync: BTreeMap<(i64, String), Sync>,

    pub purchases: BTreeMap<i64, TransactionRow>,
    pub sales: BTreeMap<i64, TransactionRow>,
    pub sell_order_history: BTreeMap<i64, SellOrderRow>,
    pub sell_order_costs: BTreeMap<i64, CostRow>,
    pub sale_costs: BTreeMap<i64, CostRow>,

    pub station_sync: BTreeMap<(i64, i64, String), Sync>,
    pub station_orders: BTreeMap<i64, StationOrderRow>,
    pub station_snapshots: Vec<SnapshotRow>,
    pub market_history: BTreeMap<(i64, i64, Date), HistoryRow>,
    pub history_sync: BTreeMap<(i64, i64), Sync>,
    pub item_stats: BTreeMap<(i64, i64, i64), ItemStatsRow>,
    pub hourly: BTreeMap<(i64, i64, i64, OffsetDateTime), Bucket>,
    pub daily: BTreeMap<(i64, i64, i64, OffsetDateTime), Bucket>,
    /// `station_flagged_days`: the hours with a rollup and the expanded hours of an item's day.
    pub flagged_days: BTreeMap<(i64, i64, i64, Date), (i32, i32)>,
    /// `station_expanded_hours`, keyed by item and hour.
    pub expanded_hours: BTreeMap<(i64, i64, i64, OffsetDateTime), ExpandedHour>,
    /// `station_item_windows`.
    pub item_windows: Vec<(i64, i64, WindowRow)>,

    pub opportunity_settings: BTreeMap<Uuid, SettingsRow>,
}

impl Tables {
    /// Deletes the character and everything that references it.
    pub fn delete_character(&mut self, character_id: i64) {
        self.characters.remove(&character_id);
        self.tokens.remove(&character_id);
        self.current_orders
            .retain(|_, o| o.character_id != character_id);
        self.order_snapshots
            .retain(|(_, _, o)| o.character_id != character_id);
        self.order_poll_totals
            .retain(|(c, _), _| *c != character_id);
        self.order_sync.remove(&character_id);
        self.wallet_balances.retain(|(c, _), _| *c != character_id);
        self.current_assets
            .retain(|_, a| a.character_id != character_id);
        self.inventory_values.retain(|(c, _), _| *c != character_id);
        self.wealth_sync.retain(|(c, _), _| *c != character_id);
        self.character_skills.retain(|(c, _), _| *c != character_id);
        self.character_standings
            .retain(|(c, _, _), _| *c != character_id);
        self.character_locations.remove(&character_id);
        self.fee_sync.retain(|(c, _), _| *c != character_id);
        self.purchases.retain(|_, t| t.character_id != character_id);
        let sales: Vec<i64> = self
            .sales
            .iter()
            .filter(|(_, t)| t.character_id == character_id)
            .map(|(id, _)| *id)
            .collect();
        for id in sales {
            self.sales.remove(&id);
            self.sale_costs.remove(&id);
        }
        let orders: Vec<i64> = self
            .sell_order_history
            .iter()
            .filter(|(_, o)| o.character_id == character_id)
            .map(|(id, _)| *id)
            .collect();
        for id in orders {
            self.sell_order_history.remove(&id);
            self.sell_order_costs.remove(&id);
        }
    }

    /// Deletes the account and everything that references it.
    pub fn delete_account(&mut self, account_id: Uuid) {
        self.accounts.remove(&account_id);
        self.opportunity_settings.remove(&account_id);
        let characters: Vec<i64> = self
            .characters
            .values()
            .filter(|c| c.account_id == account_id)
            .map(|c| c.character_id)
            .collect();
        for character_id in characters {
            self.delete_character(character_id);
        }
    }
}
