//! Every database access of the app goes through the stores bundled here.

use std::sync::Arc;

use async_trait::async_trait;
use time::{Date, Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    dashboard::{CharacterFigures, HistoryPoint, TopItem},
    esi::{EsiAsset, EsiHistoryDay, EsiMarketOrder, EsiOrder, EsiPrice, EsiTransaction},
    fee_poller,
    fees::{CharacterFee, FeeRow},
    freshness::{FreshnessRecord, WealthFreshnessRecord},
    opportunities::{settings::Settings, tables::Stats},
    orders::{OrderRow, Side, Sort},
    range::Range,
    trades::TradeRow,
    wealth::{Current, InventoryRow, InventorySort, WealthPoint},
    wealth_poller::Kind,
};

#[cfg(feature = "memory-store")]
pub mod memory;
pub mod pg;

use tower_sessions::{
    session::{Id, Record},
    session_store,
};

/// What a store call failed with. The backend's own error type never leaves the store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Backend(String),
    /// A row refers to one that does not exist (any more), such as an unlinked character's.
    #[error("a referenced row does not exist")]
    MissingParent,
}

pub type StoreResult<T> = Result<T, StoreError>;

/// One handle per domain store.
#[derive(Clone)]
pub struct Stores {
    pub accounts: Arc<dyn AccountStore>,
    pub sessions: Arc<dyn SessionData>,
    pub orders: Arc<dyn OrderStore>,
    pub market: Arc<dyn MarketStore>,
    pub station: Arc<dyn StationStore>,
    pub opportunities: Arc<dyn OpportunityStore>,
    pub wealth: Arc<dyn WealthStore>,
    pub fees: Arc<dyn FeeStore>,
    pub trades: Arc<dyn TradeStore>,
}

#[derive(Debug, Clone)]
pub struct CharacterRecord {
    pub character_id: i64,
    pub account_id: Uuid,
    pub name: String,
    pub owner_hash: String,
    pub scopes: Vec<String>,
    pub needs_relogin: bool,
}

/// The identity of a character as the SSO verified it.
#[derive(Debug, Clone)]
pub struct NewCharacter {
    pub character_id: i64,
    pub name: String,
    pub owner_hash: String,
    pub scopes: Vec<String>,
}

/// Encrypted tokens, as stored.
#[derive(Debug, Clone)]
pub struct SealedTokens {
    pub refresh_token: Vec<u8>,
    pub access_token: Option<Vec<u8>>,
    pub access_expires_at: Option<OffsetDateTime>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DeleteOutcome {
    pub deleted: bool,
    pub account_deleted: bool,
}

/// Accounts, characters and their encrypted tokens.
#[async_trait]
pub trait AccountStore: Send + Sync {
    async fn create_account(&self) -> StoreResult<Uuid>;

    /// Inserts the character under `account_id`, or refreshes an existing one in place. An existing
    /// character is never moved to another account, and its needs-re-login flag is cleared.
    async fn upsert_character(
        &self,
        account_id: Uuid,
        character: &NewCharacter,
        refresh_token: Vec<u8>,
        access_token: Vec<u8>,
        access_expires_at: OffsetDateTime,
    ) -> StoreResult<()>;

    /// Creates an account holding just this character, atomically.
    async fn create_account_with_character(
        &self,
        character: &NewCharacter,
        refresh_token: Vec<u8>,
        access_token: Vec<u8>,
        access_expires_at: OffsetDateTime,
    ) -> StoreResult<Uuid>;

    async fn find_character(&self, character_id: i64) -> StoreResult<Option<CharacterRecord>>;

    async fn list_characters(&self, account_id: Uuid) -> StoreResult<Vec<CharacterRecord>>;

    async fn load_tokens(&self, character_id: i64) -> StoreResult<Option<SealedTokens>>;

    /// Deletes a character (tokens and per-character data cascade). Removes the account too when
    /// it was the account's last character.
    async fn delete_character(&self, character_id: i64) -> StoreResult<DeleteOutcome>;

    /// Like `delete_character`, but only when the character belongs to `account_id`.
    async fn unlink_character(
        &self,
        account_id: Uuid,
        character_id: i64,
    ) -> StoreResult<DeleteOutcome>;

    /// Every account that has a character.
    async fn account_ids(&self) -> StoreResult<Vec<Uuid>>;

    /// How many accounts exist, also ones without a character.
    async fn account_count(&self) -> StoreResult<i64>;

    async fn mark_needs_relogin(&self, character_id: i64) -> StoreResult<()>;

    async fn set_scopes(&self, character_id: i64, scopes: &[String]) -> StoreResult<()>;

    /// Stores freshly refreshed tokens and the scopes they carry.
    async fn save_tokens(
        &self,
        character_id: i64,
        refresh_token: Vec<u8>,
        access_token: Vec<u8>,
        access_expires_at: OffsetDateTime,
        scopes: &[String],
    ) -> StoreResult<()>;

    /// Sets when the stored access token expires, as a fixture: an expired one makes the next
    /// `access_token` call refresh.
    async fn seed_access_expiry(
        &self,
        character_id: i64,
        access_expires_at: OffsetDateTime,
    ) -> StoreResult<()>;
}

/// Session records, as tower-sessions reads and writes them. A record is gone once its expiry date
/// has passed.
#[async_trait]
pub trait SessionData: Send + Sync {
    /// Stores a new record. A taken id is replaced by a fresh one in `record`.
    async fn create(&self, record: &mut Record) -> session_store::Result<()>;
    async fn save(&self, record: &Record) -> session_store::Result<()>;
    async fn load(&self, id: &Id) -> session_store::Result<Option<Record>>;
    async fn delete(&self, id: &Id) -> session_store::Result<()>;
    async fn delete_expired(&self) -> session_store::Result<()>;
}

/// The current orders of the characters and everything the order poller records.
#[async_trait]
pub trait OrderStore: Send + Sync {
    /// The current orders of the selected characters, restricted to the account so a selection can
    /// never reach another account's data.
    async fn list_orders(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        side: Side,
        sort: Sort,
    ) -> StoreResult<Vec<OrderRow>>;

    async fn list_freshness(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
    ) -> StoreResult<Vec<FreshnessRecord>>;

    /// One row per selected character of the account, in link order, including characters without
    /// orders.
    async fn character_figures(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
    ) -> StoreResult<Vec<CharacterFigures>>;

    /// The items with the most ISK in the sell orders (their value) or buy orders (their escrow),
    /// summed over characters and locations. Highest first, ties by name.
    async fn top_items(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        side: Side,
        limit: i64,
    ) -> StoreResult<Vec<TopItem>>;

    /// The series over `range` ending at `now`, one point per bucket, from each character's most
    /// recent poll at or before the bucket's end.
    async fn history(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        range: Range,
        now: OffsetDateTime,
    ) -> StoreResult<Vec<HistoryPoint>>;

    /// Replaces the character's current orders, appends the snapshot, remembers new sell orders,
    /// settles what they took, records the poll totals and updates the sync state, atomically.
    async fn replace_orders(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        orders: &[EsiOrder],
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<()>;

    /// Makes the character due now unless it is due sooner already.
    async fn make_due(&self, character_id: i64, now: OffsetDateTime) -> StoreResult<()>;

    /// Records a failed poll: the next one comes after a backoff that doubles per consecutive
    /// failure, from `backoff_start` up to `interval`.
    async fn record_failure(
        &self,
        character_id: i64,
        now: OffsetDateTime,
        reason: &str,
        backoff_start: Duration,
        interval: Duration,
    ) -> StoreResult<()>;

    async fn record_deferral(
        &self,
        character_id: i64,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
    ) -> StoreResult<()>;

    /// The characters without a schedule that do not need a re-login.
    async fn list_unscheduled(&self) -> StoreResult<Vec<i64>>;

    /// Schedules the characters, keeping the schedule of any that has one.
    async fn insert_schedule(
        &self,
        character_ids: &[i64],
        next_poll_at: &[OffsetDateTime],
    ) -> StoreResult<()>;

    /// Takes the due characters that do not need a re-login, oldest first, and pushes them back
    /// by `lease`, so no tick takes them again while their poll runs.
    async fn claim_due(
        &self,
        now: OffsetDateTime,
        lease: Duration,
        limit: i64,
    ) -> StoreResult<Vec<i64>>;
    /// The sync state of the character's order poll.
    async fn order_sync(&self, character_id: i64) -> StoreResult<Option<SyncState>>;
    /// How many snapshot rows each poll of the character recorded, oldest poll first.
    async fn snapshot_counts(&self, character_id: i64) -> StoreResult<Vec<(OffsetDateTime, i64)>>;

    /// The totals of each poll of the character, oldest first.
    async fn poll_totals(&self, character_id: i64) -> StoreResult<Vec<PollTotalsRow>>;

    /// The personal sell orders remembered of the character, by order ID.
    async fn remembered_orders(&self, character_id: i64) -> StoreResult<Vec<RememberedSellOrder>>;

    /// Stores one current order as it is, as a fixture: the character's other orders stay, and
    /// no snapshot, remembered order or total is recorded. An order of the same ID is replaced.
    async fn seed_order(&self, character_id: i64, order: &EsiOrder) -> StoreResult<()>;
}

/// The sync state of one poll, as the `*_sync` tables hold it.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct SyncState {
    pub next_poll_at: OffsetDateTime,
    pub last_success_at: Option<OffsetDateTime>,
    pub last_attempt_at: Option<OffsetDateTime>,
    pub last_error: Option<String>,
    pub esi_expires_at: Option<OffsetDateTime>,
    pub failures: i32,
}

/// What the totals of one order poll were, in cents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollTotalsRow {
    pub polled_at: OffsetDateTime,
    pub sell_cents: i64,
    pub escrow_cents: i64,
    pub sell_corp_cents: i64,
    pub escrow_corp_cents: i64,
    pub sell_count: i64,
    pub buy_count: i64,
}

/// A personal sell order as first seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RememberedSellOrder {
    pub order_id: i64,
    pub type_id: i64,
    pub first_issued: OffsetDateTime,
    pub volume_total: i64,
}

/// A row of the name cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameState {
    pub id: i64,
    pub name: Option<String>,
    pub retry_after: Option<OffsetDateTime>,
}

/// A purchase or a sale as it is stored, with the price in cents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeSeed {
    pub transaction_id: i64,
    pub is_sale: bool,
    pub type_id: i64,
    pub location_id: i64,
    pub quantity: i64,
    pub unit_price_cents: i64,
    pub at: OffsetDateTime,
}

/// A snapshot of an item at the station, as it is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRecord {
    pub recorded_at: OffsetDateTime,
    pub type_id: i64,
    pub best_buy: String,
    pub best_sell: String,
    /// `(best sell - best buy) / best buy`, as the exact decimal text.
    pub spread: String,
    pub buy_near_units: i64,
    pub buy_near_orders: i32,
    pub sell_near_units: i64,
    pub sell_near_orders: i32,
    pub buy_units: i64,
    pub sell_units: i64,
    pub buy_taken: i64,
    pub sell_taken: i64,
}

/// One day of an item's market history, as it is kept: the prices are exact decimal text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryRecord {
    pub type_id: i64,
    pub day: Date,
    pub average: String,
    pub highest: String,
    pub lowest: String,
    pub volume: i64,
    pub order_count: i64,
}

/// A row of the item stats, with the prices as exact decimal text.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ItemStatsRecord {
    pub book_at: Option<OffsetDateTime>,
    pub best_buy: Option<String>,
    pub best_sell: Option<String>,
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
    pub expansion_kind: Option<String>,
    pub expansion_weekday: Option<i16>,
    pub expansion_hour: Option<i16>,
    pub expansion_sell: Option<String>,
}

/// A public market order of a pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicOrder {
    pub order_id: i64,
    pub region_id: i64,
    pub type_id: i64,
    pub location_id: i64,
    pub is_buy_order: bool,
    pub price_cents: i64,
    pub volume_remain: i64,
}

/// A cached name, or a note that ESI would not give one.
#[derive(Debug, Clone)]
pub struct NameRow {
    pub id: i64,
    pub category: String,
    pub name: Option<String>,
}

/// `(account, order, undercut, gap in cents, undercut volume)` of one current order.
pub type StandingRow = (Uuid, i64, Option<i64>, Option<i64>, Option<i64>);

/// Names, the public market copy and the price list, with the schedules that keep them fresh.
#[async_trait]
pub trait MarketStore: Send + Sync {
    /// The ids among `ids` that have a name or are not to be asked for again yet.
    async fn known_names(&self, ids: &[i64], now: OffsetDateTime) -> StoreResult<Vec<i64>>;

    async fn store_names(
        &self,
        rows: &[NameRow],
        resolved_at: OffsetDateTime,
        retry_after: Option<OffsetDateTime>,
    ) -> StoreResult<()>;

    /// `(id, name)` of the ids that have a cache row; the name is empty while unresolved.
    async fn names_of(&self, ids: &[i64]) -> StoreResult<Vec<(i64, Option<String>)>>;

    /// The cached name of the id, when there is one.
    async fn name_of(&self, id: i64) -> StoreResult<Option<String>>;

    /// The standing, gap and undercut volume of every current order that uses the pair, ordered
    /// by order, as the orders page lists them.
    async fn standings(&self, region_id: i64, type_id: i64) -> StoreResult<Vec<StandingRow>>;

    /// The (region, item) pairs the account's current orders use.
    async fn account_pairs(&self, account_id: Uuid) -> StoreResult<Vec<(i64, i64)>>;

    /// The pairs a current order uses that have no schedule yet.
    async fn untracked_pairs(&self) -> StoreResult<Vec<(i64, i64)>>;

    async fn insert_pair_schedule(
        &self,
        pairs: &[(i64, i64)],
        next_poll_at: &[OffsetDateTime],
    ) -> StoreResult<()>;

    /// Drops the sync state and the copy of pairs no current order uses any more.
    async fn drop_unused_pairs(&self) -> StoreResult<()>;

    /// Takes the due pairs, oldest first, and pushes them back by `lease`.
    async fn claim_due_pairs(
        &self,
        now: OffsetDateTime,
        lease: Duration,
        limit: i64,
    ) -> StoreResult<Vec<(i64, i64)>>;

    /// Replaces the pair's market orders and updates its sync state, atomically.
    async fn replace_market_orders(
        &self,
        region_id: i64,
        type_id: i64,
        now: OffsetDateTime,
        orders: &[EsiMarketOrder],
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<()>;

    async fn record_pair_failure(
        &self,
        region_id: i64,
        type_id: i64,
        now: OffsetDateTime,
        reason: &str,
        backoff_start: Duration,
        interval: Duration,
    ) -> StoreResult<()>;

    async fn record_pair_deferral(
        &self,
        region_id: i64,
        type_id: i64,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
    ) -> StoreResult<()>;

    /// Whether a price refresh is due. When it is, it is pushed back by `lease`, so another tick
    /// does not refresh while this one runs. The very first one is due at once.
    async fn claim_price_refresh(&self, now: OffsetDateTime, lease: Duration) -> StoreResult<bool>;

    /// The accounts that have assets.
    async fn accounts_with_assets(&self) -> StoreResult<Vec<Uuid>>;

    /// Replaces the prices, records the new inventory value of every character that has assets
    /// and updates the sync state, atomically.
    async fn replace_prices(
        &self,
        now: OffsetDateTime,
        prices: &[EsiPrice],
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<()>;

    async fn record_price_failure(
        &self,
        now: OffsetDateTime,
        reason: &str,
        backoff_start: Duration,
        interval: Duration,
    ) -> StoreResult<()>;

    async fn record_price_deferral(
        &self,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
    ) -> StoreResult<()>;
    /// The sync state of the pair's market refresh.
    async fn pair_sync(&self, region_id: i64, type_id: i64) -> StoreResult<Option<SyncState>>;

    /// The sync state of the price refresh.
    async fn price_sync(&self) -> StoreResult<Option<SyncState>>;
    /// Every row of the name cache, by ID.
    async fn names_table(&self) -> StoreResult<Vec<NameState>>;
    /// Every (region, item) pair that has a schedule, in order.
    async fn tracked_pairs(&self) -> StoreResult<Vec<(i64, i64)>>;

    /// Every public order held, by order ID.
    async fn public_orders(&self) -> StoreResult<Vec<PublicOrder>>;

    /// Every item price held, by item: `None` for an item nobody trades.
    async fn prices_table(&self) -> StoreResult<Vec<(i64, Option<i64>)>>;

    /// Sets the average price of an item (in cents, or none), as a fixture: the other prices and
    /// the inventory values stay.
    async fn seed_price(&self, type_id: i64, average_price_cents: Option<i64>) -> StoreResult<()>;
}

/// One order of the station's book, as it is kept.
#[derive(Debug, Clone)]
pub struct StationOrder {
    pub order_id: i64,
    pub type_id: i64,
    pub is_buy_order: bool,
    pub price_cents: i64,
    pub volume_remain: i64,
}

/// The station's order book, its snapshots and its market history, with the schedules that keep
/// them fresh.
#[async_trait]
pub trait StationStore: Send + Sync {
    /// Whether a book refresh is due. When it is, it is pushed back by `lease`, so another tick
    /// leaves it alone while the refresh runs. The very first one is due at once.
    async fn claim_book_refresh(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
        lease: Duration,
    ) -> StoreResult<bool>;

    /// The items whose book at the station holds both a buy and a sell order.
    async fn tradable_items(&self, region_id: i64, station_id: i64) -> StoreResult<Vec<i64>>;

    /// Records the snapshots of the tradable items (looking at the previous book for the units
    /// taken), replaces the station's book and records the success, atomically.
    async fn replace_station_book(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
        rows: &[StationOrder],
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<()>;

    async fn record_book_failure(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
        reason: &str,
        backoff_start: Duration,
        interval: Duration,
    ) -> StoreResult<()>;

    async fn record_book_deferral(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
    ) -> StoreResult<()>;

    /// Gives every tradable item without a schedule a first request at `now`.
    async fn track_history(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
    ) -> StoreResult<()>;

    /// Takes the due tradable items, oldest first, and pushes them back by `lease`.
    async fn claim_due_history(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
        lease: Duration,
        limit: i64,
    ) -> StoreResult<Vec<i64>>;

    /// Upserts the days, records the success and deletes the region's days before `keep_from`,
    /// atomically.
    async fn replace_history_days(
        &self,
        region_id: i64,
        type_id: i64,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
        keep_from: Date,
        kept: &[EsiHistoryDay],
    ) -> StoreResult<()>;

    /// Records when an item is asked next, without counting a failure.
    async fn schedule_history(
        &self,
        region_id: i64,
        type_id: i64,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
    ) -> StoreResult<()>;

    async fn record_history_failure(
        &self,
        region_id: i64,
        type_id: i64,
        now: OffsetDateTime,
        reason: &str,
        backoff_start: Duration,
        max_backoff: Duration,
    ) -> StoreResult<()>;

    /// Records when the history run last finished, and its last error.
    async fn finish_history_pass(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
        any_ok: bool,
        last_error: Option<&str>,
    ) -> StoreResult<()>;

    // ---- item stats, rollups and seeds ----

    /// Drops the item rows of items that are no longer tradable.
    async fn prune_item_stats(&self, region_id: i64, station_id: i64) -> StoreResult<()>;

    /// The newest snapshot since `since` of every tradable item.
    async fn latest_snapshots(
        &self,
        region_id: i64,
        station_id: i64,
        since: OffsetDateTime,
    ) -> StoreResult<Vec<LatestSnapshot>>;

    /// `(item, buy units taken, sell units taken)` since `from`: complete days from the daily
    /// rollup, the rest from the hourly one.
    async fn units_taken(
        &self,
        region_id: i64,
        station_id: i64,
        from: OffsetDateTime,
    ) -> StoreResult<Vec<(i64, i64, i64)>>;

    /// Inserts or replaces the book fields of the items' rows.
    async fn upsert_book_fields(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
        rows: &[BookFields],
    ) -> StoreResult<()>;

    /// The history days since `from` of the items that have a row, oldest day first per item.
    async fn history_rows(
        &self,
        region_id: i64,
        station_id: i64,
        from: Date,
    ) -> StoreResult<Vec<HistoryRow>>;

    async fn item_stat_ids(&self, region_id: i64, station_id: i64) -> StoreResult<Vec<i64>>;

    async fn update_indicators(
        &self,
        region_id: i64,
        station_id: i64,
        now: OffsetDateTime,
        columns: &IndicatorColumns,
    ) -> StoreResult<()>;

    /// The newest hour of the hourly rollup.
    async fn latest_hour(
        &self,
        region_id: i64,
        station_id: i64,
    ) -> StoreResult<Option<OffsetDateTime>>;

    /// Whether any item row was not computed for the hour `latest`.
    async fn expansions_stale(
        &self,
        region_id: i64,
        station_id: i64,
        latest: OffsetDateTime,
    ) -> StoreResult<bool>;

    /// How many distinct days the hourly rollup has since `from`.
    async fn own_hourly_days(
        &self,
        region_id: i64,
        station_id: i64,
        from: OffsetDateTime,
    ) -> StoreResult<i64>;

    /// `(item, hour, average spread, largest spread, highest sell price in cents)` since `from`.
    async fn hourly_points(
        &self,
        region_id: i64,
        station_id: i64,
        from: OffsetDateTime,
    ) -> StoreResult<Vec<(i64, OffsetDateTime, f64, f64, i64)>>;

    /// `(item, day, spread, high in cents)` since `from`.
    async fn daily_points(
        &self,
        region_id: i64,
        from: Date,
    ) -> StoreResult<Vec<(i64, Date, f64, i64)>>;

    async fn update_expansions(
        &self,
        region_id: i64,
        station_id: i64,
        latest: OffsetDateTime,
        columns: &ExpansionColumns,
    ) -> StoreResult<()>;

    /// Materializes the hourly and the daily rollup of the snapshots in `from..to`. Until a
    /// refresh covers them, snapshots do not appear in the rollups.
    async fn refresh_rollups(&self, from: OffsetDateTime, to: OffsetDateTime) -> StoreResult<()>;

    /// Inserts snapshots with chosen times, as a fixture.
    async fn seed_snapshots(
        &self,
        region_id: i64,
        station_id: i64,
        rows: &[SeedSnapshotRow],
    ) -> StoreResult<()>;

    /// Inserts or replaces history days, as a fixture.
    async fn seed_history(&self, region_id: i64, rows: &[SeedHistoryRow]) -> StoreResult<()>;
    /// The sync state of the station's `book` or `history` run.
    async fn station_sync_state(
        &self,
        region_id: i64,
        station_id: i64,
        kind: &str,
    ) -> StoreResult<Option<SyncState>>;

    /// The sync state of one item's history request.
    async fn history_sync(&self, region_id: i64, type_id: i64) -> StoreResult<Option<SyncState>>;
    /// The orders of the station's book, by order ID.
    async fn book(&self, region_id: i64, station_id: i64) -> StoreResult<Vec<StationOrder>>;

    /// Every snapshot of the station, oldest first.
    async fn snapshots(&self, region_id: i64, station_id: i64) -> StoreResult<Vec<SnapshotRecord>>;

    /// The history days of one item of the region, oldest first.
    async fn history_table(&self, region_id: i64, type_id: i64) -> StoreResult<Vec<HistoryRecord>>;

    /// The item's row, with every field.
    async fn item_stats_row(
        &self,
        region_id: i64,
        station_id: i64,
        type_id: i64,
    ) -> StoreResult<Option<ItemStatsRecord>>;

    /// The start of every materialized hourly bucket of the station, oldest first.
    async fn hourly_buckets(
        &self,
        region_id: i64,
        station_id: i64,
    ) -> StoreResult<Vec<OffsetDateTime>>;

    /// The start of every materialized daily bucket of the station, oldest first.
    async fn daily_buckets(
        &self,
        region_id: i64,
        station_id: i64,
    ) -> StoreResult<Vec<OffsetDateTime>>;

    /// Inserts or replaces the whole row of an item, as a fixture.
    async fn seed_item_stats(
        &self,
        region_id: i64,
        station_id: i64,
        type_id: i64,
        row: &ItemStatsRecord,
    ) -> StoreResult<()>;
}

/// `(item, best buy, best sell, buy orders near, sell orders near)` of an item's newest
/// snapshot. The prices are exact decimal text.
pub type LatestSnapshot = (i64, String, String, i32, i32);

/// `(item, day, average price, volume)`.
pub type HistoryRow = (i64, Date, f64, i64);

/// The book fields of one item row.
#[derive(Debug, Clone)]
pub struct BookFields {
    pub type_id: i64,
    pub best_buy: String,
    pub best_sell: String,
    pub buy_near_orders: i32,
    pub sell_near_orders: i32,
    pub rho: f64,
}

/// The indicator fields of the item rows, one entry per item in every column.
#[derive(Debug, Default)]
pub struct IndicatorColumns {
    pub type_id: Vec<i64>,
    pub volume_7d: Vec<Option<f64>>,
    pub volume_30d: Vec<Option<f64>>,
    pub sigma: Vec<Option<f64>>,
    pub returns: Vec<Option<i32>>,
    pub sma5: Vec<Option<f64>>,
    pub sma20: Vec<Option<f64>>,
    pub cross_day: Vec<Option<Date>>,
    pub volume_rising: Vec<Option<bool>>,
    pub percent_b: Vec<Option<f64>>,
    pub rsi14: Vec<Option<f64>>,
}

/// The expansion fields of the item rows, one entry per item in every column. `sell` is in cents.
#[derive(Debug, Default)]
pub struct ExpansionColumns {
    pub type_id: Vec<i64>,
    pub kind: Vec<Option<String>>,
    pub weekday: Vec<Option<i16>>,
    pub hour: Vec<Option<i16>>,
    pub sell: Vec<Option<i64>>,
}

/// One synthetic snapshot of an item.
#[derive(Debug, Clone)]
pub struct SeedSnapshotRow {
    pub recorded_at: OffsetDateTime,
    pub type_id: i64,
    pub best_buy: f64,
    pub best_sell: f64,
    pub buy_near_units: i64,
    pub buy_near_orders: i32,
    pub sell_near_units: i64,
    pub sell_near_orders: i32,
    pub buy_units: i64,
    pub sell_units: i64,
    pub buy_taken: i64,
    pub sell_taken: i64,
}

/// One synthetic history day of an item.
#[derive(Debug, Clone)]
pub struct SeedHistoryRow {
    pub type_id: i64,
    pub day: Date,
    pub average: f64,
    pub highest: f64,
    pub lowest: f64,
    pub volume: i64,
    pub order_count: i64,
}

/// What the station sync last recorded of one kind of its work.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SyncRow {
    pub kind: String,
    pub last_success_at: Option<OffsetDateTime>,
    pub last_error: Option<String>,
}

/// What the Opportunities page reads: the account's filters, the item stats and the data of the
/// detail charts.
#[async_trait]
pub trait OpportunityStore: Send + Sync {
    /// The account's saved filters, or none when it never saved any.
    async fn load_settings(&self, account_id: Uuid) -> StoreResult<Option<Settings>>;

    async fn save_settings(&self, account_id: Uuid, settings: &Settings) -> StoreResult<()>;

    /// The character whose fees the account's analyses prefer, or none when it never chose one.
    async fn load_fee_character(&self, account_id: Uuid) -> StoreResult<Option<i64>>;

    /// Saves the choice, or clears it. Leaves the saved filters alone; an account without any
    /// gets the default filters.
    async fn save_fee_character(
        &self,
        account_id: Uuid,
        character_id: Option<i64>,
    ) -> StoreResult<()>;

    async fn station_sync(&self, region_id: i64, station_id: i64) -> StoreResult<Vec<SyncRow>>;

    /// Days with an own snapshot, counted on the item with the oldest snapshot.
    async fn own_snapshot_days(&self, region_id: i64, station_id: i64) -> StoreResult<i64>;

    /// Every item with book fields, with its name.
    async fn load_stats(&self, region_id: i64, station_id: i64) -> StoreResult<Vec<Stats>>;

    /// `(item, is a buy order)` of the characters' current orders at the location.
    async fn own_orders_at(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        location_id: i64,
    ) -> StoreResult<Vec<(i64, bool)>>;

    /// `(day, average, volume)` of the item since `from`, oldest first.
    async fn history_days(
        &self,
        region_id: i64,
        type_id: i64,
        from: Date,
    ) -> StoreResult<Vec<(Date, f64, f64)>>;

    /// `(hour, best buy, best sell, spread)` averages of the item since `from`, oldest first.
    async fn price_points(
        &self,
        region_id: i64,
        station_id: i64,
        type_id: i64,
        from: OffsetDateTime,
    ) -> StoreResult<Vec<(OffsetDateTime, f64, f64, f64)>>;

    /// `(buy units, buy orders, sell units, sell orders)` near the best prices, of the item's
    /// newest snapshot.
    async fn latest_depth(
        &self,
        region_id: i64,
        station_id: i64,
        type_id: i64,
    ) -> StoreResult<Option<(i64, i32, i64, i32)>>;

    /// `(hour, average spread, largest spread, highest sell price in cents)` of the item since
    /// `from`, oldest first.
    async fn item_hour_points(
        &self,
        region_id: i64,
        station_id: i64,
        type_id: i64,
        from: OffsetDateTime,
    ) -> StoreResult<Vec<(OffsetDateTime, f64, f64, i64)>>;

    /// `(day, spread, high in cents)` of the item since `from`, oldest first.
    async fn item_day_points(
        &self,
        region_id: i64,
        type_id: i64,
        from: Date,
    ) -> StoreResult<Vec<(Date, f64, i64)>>;
}

/// What the characters are worth, and the wallet and assets polls that keep it current.
#[async_trait]
pub trait WealthStore: Send + Sync {
    /// The latest figures summed over the characters, with no upper time bound.
    async fn current(&self, account_id: Uuid, character_ids: &[i64]) -> StoreResult<Current>;

    /// The series over `range` ending at `now`, one point per bucket: each character's latest
    /// figures at or before the bucket's end, summed.
    async fn history(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        range: Range,
        now: OffsetDateTime,
    ) -> StoreResult<Vec<WealthPoint>>;

    /// One row per character, item and top-level location, with the quantities summed, in the
    /// order of `sort`.
    async fn inventory_rows(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        sort: InventorySort,
    ) -> StoreResult<Vec<InventoryRow>>;

    async fn list_wealth_freshness(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
    ) -> StoreResult<Vec<WealthFreshnessRecord>>;

    /// Makes the character's poll of `kind` due now unless it is due sooner already.
    async fn make_due(&self, character_id: i64, kind: Kind, now: OffsetDateTime)
    -> StoreResult<()>;

    /// Whether the next transactions poll must read the whole window of ESI.
    async fn full_read(&self, character_id: i64) -> StoreResult<bool>;

    /// Whether any of the transactions is stored as a purchase or a sale already.
    async fn any_transaction_stored(&self, transaction_ids: &[i64]) -> StoreResult<bool>;

    /// Stores the personal purchases and sales among the transactions, once each, settles the cost
    /// of the sell orders and sales of the items that got a new one, and records the success,
    /// atomically. Returns how many purchases and sales were new.
    async fn store_transactions(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        transactions: &[EsiTransaction],
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<(usize, usize)>;

    /// Appends the balance and records the success. Two polls in the same instant keep the later
    /// one.
    async fn store_wallet(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        balance_cents: i64,
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<()>;

    /// Replaces the character's assets, records the inventory value from the rows just written
    /// and records the success, atomically. `top_locations` holds the top-level location of each
    /// asset, in the order of `assets`.
    async fn store_assets(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        assets: &[EsiAsset],
        top_locations: &[Option<i64>],
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<()>;

    async fn record_failure(
        &self,
        character_id: i64,
        kind: Kind,
        now: OffsetDateTime,
        reason: &str,
        backoff_start: Duration,
        interval: Duration,
    ) -> StoreResult<()>;

    async fn record_deferral(
        &self,
        character_id: i64,
        kind: Kind,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
    ) -> StoreResult<()>;

    /// The characters without a schedule for `kind` that do not need a re-login.
    async fn list_unscheduled(&self, kind: Kind) -> StoreResult<Vec<i64>>;

    async fn insert_schedule(
        &self,
        kind: Kind,
        character_ids: &[i64],
        next_poll_at: &[OffsetDateTime],
    ) -> StoreResult<()>;

    /// Takes the due polls of characters that do not need a re-login, oldest first, and pushes
    /// them back by `lease`.
    async fn claim_due(
        &self,
        now: OffsetDateTime,
        lease: Duration,
        limit: i64,
    ) -> StoreResult<Vec<(i64, Kind)>>;
    /// The sync state of the character's poll of `kind`.
    async fn wealth_sync(&self, character_id: i64, kind: Kind) -> StoreResult<Option<SyncState>>;

    /// Marks the next transactions poll of the character to read the whole window of ESI, as a
    /// fixture.
    async fn seed_full_read(&self, character_id: i64, full_read: bool) -> StoreResult<()>;

    /// Records an inventory value (in cents) at a chosen time, as a fixture. An entry at the same
    /// instant is replaced.
    async fn seed_inventory_value(
        &self,
        character_id: i64,
        recorded_at: OffsetDateTime,
        value_cents: i64,
    ) -> StoreResult<()>;
    /// The inventory values recorded of the character, in cents, oldest first.
    async fn inventory_values(&self, character_id: i64) -> StoreResult<Vec<(OffsetDateTime, i64)>>;

    /// Stores one asset row as it is, as a fixture: the character's other assets and its
    /// inventory value stay. An item that is stored already is replaced, also when it was another
    /// character's.
    async fn seed_asset(
        &self,
        character_id: i64,
        asset: &EsiAsset,
        top_location_id: Option<i64>,
    ) -> StoreResult<()>;

    /// Records the totals of an orders poll as they are, as a fixture. An entry at the same
    /// instant is replaced.
    async fn seed_poll_totals(&self, character_id: i64, totals: &PollTotalsRow) -> StoreResult<()>;
    /// The wallet balances recorded of the character, in cents, oldest first.
    async fn wallet_balances(&self, character_id: i64) -> StoreResult<Vec<(OffsetDateTime, i64)>>;

    /// The IDs of the character's assets, in order.
    async fn asset_ids(&self, character_id: i64) -> StoreResult<Vec<i64>>;
}

/// One standing as the fee poll stores it: `standing` is exact decimal text.
#[derive(Debug, Clone)]
pub struct StandingInput {
    pub from_id: i64,
    pub from_type: String,
    pub standing: String,
}

/// The fee rates of the characters, and the skill, standing and location polls that feed them.
#[async_trait]
pub trait FeeStore: Send + Sync {
    /// The rows of these characters, whoever owns them. For the pollers, which work per character.
    async fn for_characters(&self, character_ids: &[i64]) -> StoreResult<Vec<FeeRow>>;

    /// The rows of these characters that belong to the account, so a selection can never reach
    /// another account's data.
    async fn for_account(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
    ) -> StoreResult<Vec<FeeRow>>;

    /// The fees of these characters at one location, whether or not they have an order there.
    /// Only characters of the account count.
    async fn at_location(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        location_id: i64,
    ) -> StoreResult<Vec<FeeRow>>;

    /// The fees of these characters at their fee location. Only characters of the account count.
    async fn at_fee_location(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
    ) -> StoreResult<Vec<CharacterFee>>;

    /// Whether the character was granted the scope, or none when there is no such character.
    async fn has_scope(&self, character_id: i64, scope: &str) -> StoreResult<Option<bool>>;

    /// Makes the character's poll of `kind` due now unless it is due sooner already.
    async fn make_due(
        &self,
        character_id: i64,
        kind: fee_poller::Kind,
        now: OffsetDateTime,
    ) -> StoreResult<()>;

    /// Replaces the stored skills and records the success, atomically. Answers whether what the
    /// pages show of the character's fees changed.
    async fn store_skills(
        &self,
        character_id: i64,
        skill_ids: &[i64],
        levels: &[i16],
        polled_at: OffsetDateTime,
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<bool>;

    /// Replaces the stored standings and records the success, atomically. Answers whether what
    /// the pages show changed.
    async fn store_standings(
        &self,
        character_id: i64,
        standings: &[StandingInput],
        polled_at: OffsetDateTime,
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<bool>;

    /// Remembers the location (`(id, kind)`, when docked) and records the success, atomically. In
    /// open space the last one stays. Answers whether what the pages show changed.
    async fn store_location(
        &self,
        character_id: i64,
        docked: Option<(i64, &str)>,
        polled_at: OffsetDateTime,
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<bool>;

    async fn record_failure(
        &self,
        character_id: i64,
        kind: fee_poller::Kind,
        now: OffsetDateTime,
        reason: &str,
        backoff_start: Duration,
        interval: Duration,
    ) -> StoreResult<()>;

    async fn record_deferral(
        &self,
        character_id: i64,
        kind: fee_poller::Kind,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
    ) -> StoreResult<()>;

    /// The NPC stations (ids below `structure_id_min`) where a current order is placed, a sale
    /// was made or a character is docked, and whose owner is not known yet.
    async fn stations_without_owner(&self, structure_id_min: i64) -> StoreResult<Vec<i64>>;

    async fn station_owner_known(&self, station_id: i64) -> StoreResult<bool>;

    /// Stores the owner of a station, keeping the first one learned.
    async fn insert_station_owner(
        &self,
        station_id: i64,
        owner_id: i64,
        now: OffsetDateTime,
    ) -> StoreResult<()>;

    /// Stores the owner like `insert_station_owner` and answers the accounts whose characters'
    /// fees at the station changed because of it, atomically.
    async fn store_station_owner(
        &self,
        station_id: i64,
        owner_id: i64,
        now: OffsetDateTime,
    ) -> StoreResult<Vec<Uuid>>;

    /// The characters without a schedule for `kind` that do not need a re-login and have the
    /// scope, when one is asked for.
    async fn list_unscheduled(
        &self,
        kind: fee_poller::Kind,
        scope: Option<&str>,
    ) -> StoreResult<Vec<i64>>;

    async fn insert_schedule(
        &self,
        kind: fee_poller::Kind,
        character_ids: &[i64],
        next_poll_at: &[OffsetDateTime],
    ) -> StoreResult<()>;

    /// Takes the due polls of characters that do not need a re-login (and, for the location, have
    /// its scope), oldest first, and pushes them back by `lease`.
    async fn claim_due(
        &self,
        now: OffsetDateTime,
        lease: Duration,
        limit: i64,
    ) -> StoreResult<Vec<(i64, fee_poller::Kind)>>;
    /// The sync state of the character's poll of `kind`.
    async fn fee_sync(
        &self,
        character_id: i64,
        kind: fee_poller::Kind,
    ) -> StoreResult<Option<SyncState>>;

    /// Stores one skill as it is, as a fixture: the character's other skills and its sync state
    /// stay. A skill that is stored already is replaced.
    async fn seed_skill(&self, character_id: i64, skill_id: i64, level: i16) -> StoreResult<()>;

    /// Stores one standing as it is, as a fixture: the others and the sync state stay. A standing
    /// of the same source is replaced.
    async fn seed_standing(&self, character_id: i64, standing: &StandingInput) -> StoreResult<()>;

    /// Records a successful poll of `kind` at `at` without touching what it stored, as a fixture.
    async fn seed_fee_success(
        &self,
        character_id: i64,
        kind: fee_poller::Kind,
        at: OffsetDateTime,
    ) -> StoreResult<()>;

    /// The `(skill, level)` pairs stored of the character, by skill.
    async fn skills_of(&self, character_id: i64) -> StoreResult<Vec<(i64, i16)>>;

    /// The `(from id, from type, standing text)` rows stored of the character, by source.
    async fn standings_of(&self, character_id: i64) -> StoreResult<Vec<(i64, String, String)>>;

    /// The `(id, kind)` of the place the character was last docked at.
    async fn location_of(&self, character_id: i64) -> StoreResult<Option<(i64, String)>>;

    /// The owner stored for the station.
    async fn station_owner(&self, station_id: i64) -> StoreResult<Option<i64>>;

    /// How many stations have an owner stored.
    async fn station_owner_count(&self) -> StoreResult<i64>;
}

/// The personal purchases and sales, and what the sales earned.
#[async_trait]
pub trait TradeStore: Send + Sync {
    /// The transactions dated within `[from, to]` of the characters, restricted to the account.
    /// Newest first, the higher transaction ID first among equal dates.
    async fn list_trades(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        from: OffsetDateTime,
        to: OffsetDateTime,
    ) -> StoreResult<Vec<TradeRow>>;

    /// How fresh the wallet transactions of each character are.
    async fn list_transactions_freshness(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
    ) -> StoreResult<Vec<FreshnessRecord>>;

    /// Stores a purchase or a sale as it is, as a fixture, without settling any cost. A
    /// transaction of the same ID is replaced.
    async fn seed_trade(&self, character_id: i64, trade: &TradeSeed) -> StoreResult<()>;

    /// Remembers a sell order as it was first seen, as a fixture, without settling its cost. An
    /// order of the same ID is replaced.
    async fn seed_remembered_order(
        &self,
        character_id: i64,
        order: &RememberedSellOrder,
    ) -> StoreResult<()>;

    /// Stores what the allocation gave a remembered order, as a fixture.
    async fn seed_order_cost(
        &self,
        order_id: i64,
        covered_units: i64,
        cost_cents: i64,
    ) -> StoreResult<()>;

    /// Stores what the allocation gave a sale, as a fixture.
    async fn seed_sale_cost(
        &self,
        transaction_id: i64,
        covered_units: i64,
        cost_cents: i64,
    ) -> StoreResult<()>;

    /// Deletes a sale with its cost, as a fixture.
    async fn delete_sale(&self, transaction_id: i64) -> StoreResult<()>;

    /// Redoes the allocation of the character for these items from all stored purchases,
    /// remembered orders and sales, and replaces their stored costs.
    async fn settle(&self, character_id: i64, type_ids: &[i64]) -> StoreResult<()>;

    /// `(order, covered units, cost in cents)` of every stored order cost, by order.
    async fn order_costs(&self) -> StoreResult<Vec<(i64, i64, i64)>>;

    /// `(sale, covered units, cost in cents)` of every stored sale cost, by sale.
    async fn sale_costs(&self) -> StoreResult<Vec<(i64, i64, i64)>>;
}
