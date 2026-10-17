//! The background poller: keeps every linked character's current orders in step with ESI,
//! records a snapshot of each poll, and schedules the next one.

use std::sync::Arc;

use futures_util::StreamExt;
use secrecy::SecretString;
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};

use crate::{
    clock::Clock,
    esi::{EsiClient, EsiError, EsiOrder, EsiResponse},
    events::{Event, EventBus},
    names,
    tokens::{TokenError, TokenService},
};

/// The most a poll is pushed back so characters drift apart. Never negative: a character is
/// never polled before the interval or ESI's cache expiry has passed.
pub(crate) const MAX_JITTER: Duration = Duration::seconds(30);
pub use crate::names::STRUCTURE_ID_MIN;
/// How many characters are polled at the same time.
const DEFAULT_CONCURRENCY: usize = 4;
/// A claimed character is left alone this long, so a poll that dies cannot starve it for good.
pub(crate) const CLAIM_LEASE: Duration = Duration::minutes(5);
/// How often the scheduler looks for due characters.
pub const SCHEDULER_TICK: std::time::Duration = std::time::Duration::from_secs(5);
/// The first retry after a failure; doubles per consecutive failure, capped at the interval.
pub const BACKOFF_START: Duration = Duration::minutes(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollOutcome {
    /// The orders were replaced and a snapshot recorded.
    Updated,
    /// The character has to log in again; nothing was requested.
    NeedsRelogin,
    /// A temporary failure was recorded; the orders are unchanged.
    Failed,
    /// ESI asked us to wait; rescheduled without counting a failure.
    Deferred,
    /// The character was unlinked while its poll ran.
    Dropped,
}

pub struct Poller {
    pool: PgPool,
    tokens: Arc<TokenService>,
    esi: Arc<EsiClient>,
    events: Arc<dyn EventBus>,
    clock: Arc<dyn Clock>,
    interval: Duration,
    concurrency: usize,
}

/// Columns of one order, as the parallel arrays `unnest` takes.
#[derive(Default)]
struct OrderColumns {
    order_id: Vec<i64>,
    type_id: Vec<i64>,
    region_id: Vec<i64>,
    location_id: Vec<i64>,
    is_buy_order: Vec<bool>,
    is_corporation: Vec<bool>,
    price_cents: Vec<i64>,
    volume_remain: Vec<i64>,
    volume_total: Vec<i64>,
    min_volume: Vec<i64>,
    range: Vec<String>,
    escrow_cents: Vec<Option<i64>>,
    issued: Vec<OffsetDateTime>,
    duration: Vec<i32>,
}

impl OrderColumns {
    fn new(orders: &[EsiOrder]) -> Self {
        let mut c = Self::default();
        for o in orders {
            c.order_id.push(o.order_id);
            c.type_id.push(o.type_id);
            c.region_id.push(o.region_id);
            c.location_id.push(o.location_id);
            c.is_buy_order.push(o.is_buy_order);
            c.is_corporation.push(o.is_corporation);
            c.price_cents.push(o.price_cents);
            c.volume_remain.push(o.volume_remain);
            c.volume_total.push(o.volume_total);
            c.min_volume.push(o.min_volume);
            c.range.push(o.range.clone());
            c.escrow_cents.push(o.escrow_cents);
            c.issued.push(o.issued);
            c.duration.push(o.duration);
        }
        c
    }
}

/// `SELECT` over the order arrays bound from `$3` on. Prices and escrow arrive as cents and are
/// turned into exact `numeric` here.
const ORDER_ROWS: &str = "SELECT o.order_id, o.type_id, o.region_id, o.location_id, o.is_buy_order,
        o.is_corporation, o.price_cents::numeric / 100 AS price, o.volume_remain, o.volume_total,
        o.min_volume, o.range, o.escrow_cents::numeric / 100 AS escrow, o.issued, o.duration
    FROM unnest($3::bigint[], $4::bigint[], $5::bigint[], $6::bigint[], $7::bool[], $8::bool[],
        $9::bigint[], $10::bigint[], $11::bigint[], $12::bigint[], $13::text[], $14::bigint[],
        $15::timestamptz[], $16::int[])
        AS o(order_id, type_id, region_id, location_id, is_buy_order, is_corporation, price_cents,
        volume_remain, volume_total, min_volume, range, escrow_cents, issued, duration)";

const ORDER_COLUMNS: &str =
    "order_id, type_id, region_id, location_id, is_buy_order, is_corporation,
    price, volume_remain, volume_total, min_volume, range, escrow, issued, duration";

macro_rules! bind_orders {
    ($query:expr, $c:expr) => {
        $query
            .bind(&$c.order_id)
            .bind(&$c.type_id)
            .bind(&$c.region_id)
            .bind(&$c.location_id)
            .bind(&$c.is_buy_order)
            .bind(&$c.is_corporation)
            .bind(&$c.price_cents)
            .bind(&$c.volume_remain)
            .bind(&$c.volume_total)
            .bind(&$c.min_volume)
            .bind(&$c.range)
            .bind(&$c.escrow_cents)
            .bind(&$c.issued)
            .bind(&$c.duration)
    };
}

pub(crate) fn is_foreign_key_violation(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .and_then(|db| db.code())
        .is_some_and(|code| code == "23503")
}

impl Poller {
    pub fn new(
        pool: PgPool,
        tokens: Arc<TokenService>,
        esi: Arc<EsiClient>,
        events: Arc<dyn EventBus>,
        clock: Arc<dyn Clock>,
        interval: Duration,
    ) -> Self {
        Self {
            pool,
            tokens,
            esi,
            events,
            clock,
            interval,
            concurrency: DEFAULT_CONCURRENCY,
        }
    }

    pub fn with_concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency.max(1);
        self
    }

    /// Polls one character now, whatever its schedule says. Never panics or errors: every way a
    /// poll can end is a [`PollOutcome`], and the database records the details. Tells the
    /// character's account's open pages that something changed (orders or freshness).
    pub async fn poll_character(&self, character_id: i64) -> PollOutcome {
        let outcome = self.run_poll(character_id).await;
        if outcome != PollOutcome::Dropped {
            self.publish(character_id).await;
        }
        outcome
    }

    async fn publish(&self, character_id: i64) {
        match sqlx::query_scalar::<_, uuid::Uuid>(
            "SELECT account_id FROM characters WHERE character_id = $1",
        )
        .bind(character_id)
        .fetch_optional(&self.pool)
        .await
        {
            Ok(Some(account)) => self.events.publish(account, Event::OrdersUpdated),
            Ok(None) => {}
            Err(e) => tracing::error!(character_id, "cannot look up the account to signal: {e}"),
        }
    }

    async fn run_poll(&self, character_id: i64) -> PollOutcome {
        let now = self.clock.now();
        let token = match self.tokens.access_token(character_id).await {
            Ok(token) => token,
            Err(TokenError::NeedsRelogin) => {
                // Make the character due again, so it is polled right after it logged in.
                let _ = sqlx::query(
                    "UPDATE order_sync SET next_poll_at = LEAST(next_poll_at, $2) WHERE character_id = $1",
                )
                .bind(character_id)
                .bind(now)
                .execute(&self.pool)
                .await;
                return PollOutcome::NeedsRelogin;
            }
            Err(TokenError::UnknownCharacter) => return PollOutcome::Dropped,
            Err(TokenError::Temporary(reason)) => {
                return self.record_failure(character_id, now, &reason).await;
            }
        };
        let response = match self.esi.orders(character_id, &token).await {
            Ok(response) => response,
            Err(EsiError::Deferred { until }) => {
                return self.record_deferral(character_id, now, until).await;
            }
            Err(e) => return self.record_failure(character_id, now, &e.to_string()).await,
        };
        match self.store(character_id, now, &response).await {
            Ok(()) => {
                // After the orders are safe: a name that cannot be resolved never fails a poll.
                self.resolve_names(&token, &response.body, now).await;
                PollOutcome::Updated
            }
            Err(e) if is_foreign_key_violation(&e) => {
                tracing::info!(character_id, "character was unlinked during its poll");
                PollOutcome::Dropped
            }
            Err(e) => {
                self.record_failure(character_id, now, &format!("database error: {e}"))
                    .await
            }
        }
    }

    /// Replaces the character's current orders, appends the snapshot and updates the sync state,
    /// all in one transaction.
    async fn store(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        response: &EsiResponse<Vec<EsiOrder>>,
    ) -> Result<(), sqlx::Error> {
        let columns = OrderColumns::new(&response.body);
        let mut tx = self.pool.begin().await?;

        sqlx::query("DELETE FROM current_orders WHERE character_id = $1")
            .bind(character_id)
            .execute(&mut *tx)
            .await?;
        let insert_current = format!(
            "INSERT INTO current_orders (character_id, updated_at, {ORDER_COLUMNS})
             SELECT $1, $2, rows.* FROM ({ORDER_ROWS}) AS rows"
        );
        bind_orders!(
            sqlx::query(sqlx::AssertSqlSafe(insert_current))
                .bind(character_id)
                .bind(polled_at),
            columns
        )
        .execute(&mut *tx)
        .await?;

        let insert_snapshot = format!(
            "INSERT INTO order_snapshots (character_id, polled_at, {ORDER_COLUMNS})
             SELECT $1, $2, rows.* FROM ({ORDER_ROWS}) AS rows"
        );
        bind_orders!(
            sqlx::query(sqlx::AssertSqlSafe(insert_snapshot))
                .bind(character_id)
                .bind(polled_at),
            columns
        )
        .execute(&mut *tx)
        .await?;

        // Every personal sell order is remembered as first seen: a reprice moves `issued` in ESI
        // but not here, and a closed order stays.
        let new_types: Vec<i64> = sqlx::query_scalar(
            "INSERT INTO sell_order_history (order_id, character_id, type_id, first_issued, volume_total)
             SELECT order_id, character_id, type_id, issued, volume_total
             FROM current_orders
             WHERE character_id = $1 AND NOT is_buy_order AND NOT is_corporation
             ON CONFLICT (order_id) DO NOTHING
             RETURNING type_id",
        )
        .bind(character_id)
        .fetch_all(&mut *tx)
        .await?;
        // What a new order took is settled right away, in the same transaction.
        let mut new_types = new_types;
        new_types.sort_unstable();
        new_types.dedup();
        crate::trade::recompute(&mut tx, character_id, &new_types).await?;

        // Computed from the rows just written, so the history and the current figures agree, and
        // a poll without orders records zeros. Two polls in the same instant keep the later one
        // rather than failing the whole poll.
        sqlx::query(
            "INSERT INTO order_poll_totals
                 (character_id, polled_at, sell_value, escrow, sell_corp, escrow_corp, sell_count, buy_count)
             SELECT $1, $2,
                    COALESCE(SUM(price * volume_remain) FILTER (WHERE NOT is_buy_order), 0),
                    COALESCE(SUM(COALESCE(escrow, 0)) FILTER (WHERE is_buy_order), 0),
                    COALESCE(SUM(price * volume_remain) FILTER (WHERE NOT is_buy_order AND is_corporation), 0),
                    COALESCE(SUM(COALESCE(escrow, 0)) FILTER (WHERE is_buy_order AND is_corporation), 0),
                    COUNT(*) FILTER (WHERE NOT is_buy_order),
                    COUNT(*) FILTER (WHERE is_buy_order)
             FROM current_orders WHERE character_id = $1
             ON CONFLICT (character_id, polled_at) DO UPDATE SET
                 sell_value = excluded.sell_value,
                 escrow = excluded.escrow,
                 sell_corp = excluded.sell_corp,
                 escrow_corp = excluded.escrow_corp,
                 sell_count = excluded.sell_count,
                 buy_count = excluded.buy_count",
        )
        .bind(character_id)
        .bind(polled_at)
        .execute(&mut *tx)
        .await?;

        let next_poll_at = self.next_poll_at(polled_at, response.expires_at);
        sqlx::query(
            "INSERT INTO order_sync
                 (character_id, next_poll_at, last_success_at, last_attempt_at, last_error, esi_expires_at, failures)
             VALUES ($1, $2, $3, $3, NULL, $4, 0)
             ON CONFLICT (character_id) DO UPDATE SET
                 next_poll_at = excluded.next_poll_at,
                 last_success_at = excluded.last_success_at,
                 last_attempt_at = excluded.last_attempt_at,
                 last_error = NULL,
                 esi_expires_at = excluded.esi_expires_at,
                 failures = 0",
        )
        .bind(character_id)
        .bind(next_poll_at)
        .bind(polled_at)
        .bind(response.expires_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }

    /// `max(polled_at + interval, ESI's Expires)`, pushed back by a little jitter.
    fn next_poll_at(
        &self,
        polled_at: OffsetDateTime,
        expires_at: Option<OffsetDateTime>,
    ) -> OffsetDateTime {
        let earliest = (polled_at + self.interval).max(expires_at.unwrap_or(polled_at));
        earliest + self.jitter()
    }

    fn jitter(&self) -> Duration {
        jitter(self.interval)
    }

    async fn record_failure(
        &self,
        character_id: i64,
        now: OffsetDateTime,
        reason: &str,
    ) -> PollOutcome {
        tracing::warn!(character_id, "order poll failed: {reason}");
        let result = sqlx::query(
            "INSERT INTO order_sync (character_id, next_poll_at, last_attempt_at, last_error, failures)
             VALUES ($1, $2, $3, $4, 1)
             ON CONFLICT (character_id) DO UPDATE SET
                 next_poll_at = $3 + LEAST(
                     $5::float8 * power(2, LEAST(order_sync.failures, 20)) * interval '1 second',
                     $6::float8 * interval '1 second'),
                 last_attempt_at = $3,
                 last_error = $4,
                 failures = order_sync.failures + 1",
        )
        .bind(character_id)
        .bind(now + BACKOFF_START.min(self.interval))
        .bind(now)
        .bind(reason)
        .bind(BACKOFF_START.as_seconds_f64())
        .bind(self.interval.as_seconds_f64())
        .execute(&self.pool)
        .await;
        match result {
            Ok(_) => PollOutcome::Failed,
            Err(e) if is_foreign_key_violation(&e) => PollOutcome::Dropped,
            Err(e) => {
                tracing::error!(character_id, "cannot record a poll failure: {e}");
                PollOutcome::Failed
            }
        }
    }

    async fn record_deferral(
        &self,
        character_id: i64,
        now: OffsetDateTime,
        until: OffsetDateTime,
    ) -> PollOutcome {
        let result = sqlx::query(
            "INSERT INTO order_sync (character_id, next_poll_at, last_attempt_at)
             VALUES ($1, $2, $3)
             ON CONFLICT (character_id) DO UPDATE SET
                 next_poll_at = excluded.next_poll_at,
                 last_attempt_at = excluded.last_attempt_at",
        )
        .bind(character_id)
        .bind(until + self.jitter())
        .bind(now)
        .execute(&self.pool)
        .await;
        match result {
            Err(e) if is_foreign_key_violation(&e) => PollOutcome::Dropped,
            Err(e) => {
                tracing::error!(character_id, "cannot record a deferred poll: {e}");
                PollOutcome::Deferred
            }
            Ok(_) => PollOutcome::Deferred,
        }
    }
}

impl Poller {
    /// Caches the names the orders refer to that are not known yet (design §4).
    async fn resolve_names(&self, token: &SecretString, orders: &[EsiOrder], now: OffsetDateTime) {
        names::resolve(
            &self.pool,
            &self.esi,
            token,
            orders
                .iter()
                .flat_map(|o| [o.type_id, o.region_id, o.location_id]),
            now,
        )
        .await;
    }
}

// ---- the scheduler (design §2) ----

/// A little random delay, never negative, so polls drift apart.
pub(crate) fn jitter(interval: Duration) -> Duration {
    let max = MAX_JITTER.min(interval / 10).whole_milliseconds().max(1) as u64;
    Duration::milliseconds((rand::random::<u64>() % (max + 1)) as i64)
}

/// A stable spread over the interval, so characters linked together do not poll together.
fn first_offset(character_id: i64, interval: Duration) -> Duration {
    spread(&character_id.to_be_bytes(), interval)
}

/// A stable offset inside the interval for whatever the key names.
pub(crate) fn spread(key: &[u8], interval: Duration) -> Duration {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(key);
    let hash = u64::from_be_bytes(digest[..8].try_into().expect("8 bytes"));
    Duration::seconds((hash % interval.whole_seconds().max(1) as u64) as i64)
}

impl Poller {
    /// Runs [`Poller::tick`] every [`SCHEDULER_TICK`] until the task is aborted.
    pub fn spawn_scheduler(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(SCHEDULER_TICK);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                self.tick().await;
            }
        })
    }

    /// Schedules characters that have no schedule yet, then polls every due character (a few at
    /// a time) and returns how many were polled.
    pub async fn tick(&self) -> usize {
        let now = self.clock.now();
        if let Err(e) = self.schedule_new(now).await {
            tracing::error!("cannot schedule new characters: {e}");
        }
        let due = match self.claim_due(now).await {
            Ok(due) => due,
            Err(e) => {
                tracing::error!("cannot claim due characters: {e}");
                return 0;
            }
        };
        let polled = due.len();
        futures_util::stream::iter(due)
            .for_each_concurrent(self.concurrency, |character_id| async move {
                self.poll_character(character_id).await;
            })
            .await;
        polled
    }

    /// Gives every character without a schedule a first poll time inside the next interval.
    async fn schedule_new(&self, now: OffsetDateTime) -> Result<(), sqlx::Error> {
        let unscheduled: Vec<i64> = sqlx::query_scalar(
            "SELECT c.character_id FROM characters c
             WHERE NOT c.needs_relogin
               AND NOT EXISTS (SELECT 1 FROM order_sync s WHERE s.character_id = c.character_id)",
        )
        .fetch_all(&self.pool)
        .await?;
        if unscheduled.is_empty() {
            return Ok(());
        }
        let times: Vec<OffsetDateTime> = unscheduled
            .iter()
            .map(|id| now + first_offset(*id, self.interval))
            .collect();
        sqlx::query(
            "INSERT INTO order_sync (character_id, next_poll_at)
             SELECT * FROM unnest($1::bigint[], $2::timestamptz[])
             ON CONFLICT (character_id) DO NOTHING",
        )
        .bind(&unscheduled)
        .bind(&times)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Takes the due characters that do not need a re-login. They are pushed back by a lease, so
    /// neither this nor another tick takes them again while their poll runs.
    async fn claim_due(&self, now: OffsetDateTime) -> Result<Vec<i64>, sqlx::Error> {
        sqlx::query_scalar(
            "UPDATE order_sync SET next_poll_at = $1 + $2::float8 * interval '1 second'
             WHERE character_id IN (
                 SELECT s.character_id FROM order_sync s
                 JOIN characters c USING (character_id)
                 WHERE NOT c.needs_relogin AND s.next_poll_at <= $1
                 ORDER BY s.next_poll_at
                 LIMIT $3
                 FOR UPDATE OF s SKIP LOCKED)
             RETURNING character_id",
        )
        .bind(now)
        .bind(CLAIM_LEASE.as_seconds_f64())
        .bind((self.concurrency * 8) as i64)
        .fetch_all(&self.pool)
        .await
    }
}
