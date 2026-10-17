//! The market poller: keeps a copy of the public market orders of every (region, item) pair that
//! a linked character has a current order for, and refreshes each copy on its own schedule.

use std::sync::Arc;

use futures_util::StreamExt;
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    clock::Clock,
    esi::{EsiClient, EsiError, EsiMarketOrder, EsiResponse},
    events::{Event, EventBus},
    orders::{GAP_CENTS, STANDING_JOIN},
    poller::{BACKOFF_START, CLAIM_LEASE, SCHEDULER_TICK, jitter, spread},
};

/// How often a pair is refreshed: the cache time of ESI's market answers.
pub const MARKET_INTERVAL: Duration = Duration::minutes(5);

/// How many pairs are refreshed at the same time.
const CONCURRENCY: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshOutcome {
    /// The pair's market orders were replaced.
    Updated,
    /// A temporary failure was recorded; the old copy is kept.
    Failed,
    /// ESI asked us to wait; rescheduled without counting a failure.
    Deferred,
}

/// What a pair's refresh may change on screen: per current order using the pair, its standing
/// and gap (design §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// `(account, order, undercut, gap in cents)`, ordered.
    rows: Vec<(Uuid, i64, Option<i64>, Option<i64>)>,
}

impl Snapshot {
    /// The accounts with an order whose standing or gap differs between the two snapshots, or
    /// that has an order in only one of them.
    pub fn changed_accounts(&self, after: &Snapshot) -> Vec<Uuid> {
        let mut changed: Vec<Uuid> = self
            .rows
            .iter()
            .filter(|row| !after.rows.contains(row))
            .chain(after.rows.iter().filter(|row| !self.rows.contains(row)))
            .map(|row| row.0)
            .collect();
        changed.sort_unstable();
        changed.dedup();
        changed
    }
}

/// The standing and gap of every current order that uses the pair, from the same SQL the orders
/// page lists with.
pub async fn snapshot(
    pool: &PgPool,
    region_id: i64,
    type_id: i64,
) -> Result<Snapshot, sqlx::Error> {
    let sql = format!(
        "SELECT c.account_id, o.order_id, st.undercut, {GAP_CENTS}
         FROM current_orders o
         JOIN characters c ON c.character_id = o.character_id
         {STANDING_JOIN}
         WHERE o.region_id = $1 AND o.type_id = $2
         ORDER BY o.order_id"
    );
    let rows = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(region_id)
        .bind(type_id)
        .fetch_all(pool)
        .await?;
    Ok(Snapshot { rows })
}

pub struct MarketPoller {
    pool: PgPool,
    esi: Arc<EsiClient>,
    events: Arc<dyn EventBus>,
    clock: Arc<dyn Clock>,
    interval: Duration,
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
        }
        c
    }
}

impl MarketPoller {
    pub fn new(
        pool: PgPool,
        esi: Arc<EsiClient>,
        events: Arc<dyn EventBus>,
        clock: Arc<dyn Clock>,
        interval: Duration,
    ) -> Self {
        Self {
            pool,
            esi,
            events,
            clock,
            interval,
        }
    }

    /// Runs [`MarketPoller::tick`] every scheduler tick until the task is aborted.
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

    /// Tracks the pairs the current orders use, then refreshes the due ones (a few at a time).
    /// Returns how many pairs were refreshed.
    pub async fn tick(&self) -> usize {
        let now = self.clock.now();
        if let Err(e) = self.track(now).await {
            tracing::error!("cannot track market pairs: {e}");
        }
        let due = match self.claim_due(now).await {
            Ok(due) => due,
            Err(e) => {
                tracing::error!("cannot claim due market pairs: {e}");
                return 0;
            }
        };
        let refreshed = due.len();
        futures_util::stream::iter(due)
            .for_each_concurrent(CONCURRENCY, |(region_id, type_id)| async move {
                self.refresh_pair(region_id, type_id).await;
            })
            .await;
        refreshed
    }

    /// Refreshes every pair the account's current orders use right now, whatever the schedule
    /// says, after tracking them. Returns how many pairs it refreshed. For the test hook.
    pub async fn refresh_account_pairs(&self, account_id: Uuid) -> Result<usize, sqlx::Error> {
        self.track(self.clock.now()).await?;
        let pairs: Vec<(i64, i64)> = sqlx::query_as(
            "SELECT DISTINCT o.region_id, o.type_id FROM current_orders o
             JOIN characters c ON c.character_id = o.character_id
             WHERE c.account_id = $1
             ORDER BY o.region_id, o.type_id",
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?;
        for (region_id, type_id) in &pairs {
            self.refresh_pair(*region_id, *type_id).await;
        }
        Ok(pairs.len())
    }

    /// Gives every pair a current order uses a first refresh time inside the next interval, and
    /// drops the sync state and the copy of pairs no current order uses any more.
    async fn track(&self, now: OffsetDateTime) -> Result<(), sqlx::Error> {
        let missing: Vec<(i64, i64)> = sqlx::query_as(
            "SELECT DISTINCT o.region_id, o.type_id FROM current_orders o
             WHERE NOT EXISTS (SELECT 1 FROM market_sync s
                               WHERE s.region_id = o.region_id AND s.type_id = o.type_id)",
        )
        .fetch_all(&self.pool)
        .await?;
        if !missing.is_empty() {
            let regions: Vec<i64> = missing.iter().map(|p| p.0).collect();
            let types: Vec<i64> = missing.iter().map(|p| p.1).collect();
            let times: Vec<OffsetDateTime> = missing
                .iter()
                .map(|(region, type_id)| {
                    let mut key = region.to_be_bytes().to_vec();
                    key.extend(type_id.to_be_bytes());
                    now + spread(&key, self.interval)
                })
                .collect();
            sqlx::query(
                "INSERT INTO market_sync (region_id, type_id, next_poll_at)
                 SELECT * FROM unnest($1::bigint[], $2::bigint[], $3::timestamptz[])
                 ON CONFLICT (region_id, type_id) DO NOTHING",
            )
            .bind(&regions)
            .bind(&types)
            .bind(&times)
            .execute(&self.pool)
            .await?;
        }
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

    /// Takes the due pairs. They are pushed back by a lease, so neither this nor another tick
    /// takes them again while their refresh runs.
    async fn claim_due(&self, now: OffsetDateTime) -> Result<Vec<(i64, i64)>, sqlx::Error> {
        sqlx::query_as(
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
        .bind(CLAIM_LEASE.as_seconds_f64())
        .bind((CONCURRENCY * 8) as i64)
        .fetch_all(&self.pool)
        .await
    }

    /// Refreshes one pair now, whatever its schedule says. Every way it can end is a
    /// [`RefreshOutcome`], and the database records the details.
    pub async fn refresh_pair(&self, region_id: i64, type_id: i64) -> RefreshOutcome {
        let now = self.clock.now();
        let before = snapshot(&self.pool, region_id, type_id).await;
        let response = match self.esi.market_orders(region_id, type_id).await {
            Ok(response) => response,
            Err(EsiError::Deferred { until }) => {
                return self.record_deferral(region_id, type_id, now, until).await;
            }
            Err(e) => {
                return self
                    .record_failure(region_id, type_id, now, &e.to_string())
                    .await;
            }
        };
        match self.store(region_id, type_id, now, &response).await {
            Ok(()) => {
                self.signal_changes(region_id, type_id, before).await;
                RefreshOutcome::Updated
            }
            Err(e) => {
                self.record_failure(region_id, type_id, now, &format!("database error: {e}"))
                    .await
            }
        }
    }

    /// Tells the accounts whose orders now show a different standing or gap. Without a before
    /// snapshot, or when it cannot be read, nothing is sent: a page also refreshes on reconnect.
    async fn signal_changes(
        &self,
        region_id: i64,
        type_id: i64,
        before: Result<Snapshot, sqlx::Error>,
    ) {
        let (Ok(before), Ok(after)) = (before, snapshot(&self.pool, region_id, type_id).await)
        else {
            tracing::error!(
                region_id,
                type_id,
                "cannot compare the standings of a refresh"
            );
            return;
        };
        for account in before.changed_accounts(&after) {
            self.events.publish(account, Event::MarketUpdated);
        }
    }

    /// Replaces the pair's market orders and updates its sync state in one transaction.
    async fn store(
        &self,
        region_id: i64,
        type_id: i64,
        now: OffsetDateTime,
        response: &EsiResponse<Vec<EsiMarketOrder>>,
    ) -> Result<(), sqlx::Error> {
        let columns = MarketColumns::new(&response.body);
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM market_orders WHERE region_id = $1 AND type_id = $2")
            .bind(region_id)
            .bind(type_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO market_orders
                 (order_id, region_id, type_id, location_id, system_id, is_buy_order, price, range)
             SELECT o.order_id, $1, $2, o.location_id, o.system_id, o.is_buy_order,
                    o.price_cents::numeric / 100, o.range
             FROM unnest($3::bigint[], $4::bigint[], $5::bigint[], $6::bool[], $7::bigint[],
                         $8::text[])
                  AS o(order_id, location_id, system_id, is_buy_order, price_cents, range)
             ON CONFLICT (order_id) DO UPDATE SET
                 region_id = excluded.region_id, type_id = excluded.type_id,
                 location_id = excluded.location_id, system_id = excluded.system_id,
                 is_buy_order = excluded.is_buy_order, price = excluded.price,
                 range = excluded.range",
        )
        .bind(region_id)
        .bind(type_id)
        .bind(&columns.order_id)
        .bind(&columns.location_id)
        .bind(&columns.system_id)
        .bind(&columns.is_buy_order)
        .bind(&columns.price_cents)
        .bind(&columns.range)
        .execute(&mut *tx)
        .await?;
        // `max(now + interval, ESI's Expires)`, pushed back by a little jitter.
        let earliest = (now + self.interval).max(response.expires_at.unwrap_or(now));
        sqlx::query(
            "UPDATE market_sync SET
                 next_poll_at = $3, last_success_at = $4, last_attempt_at = $4,
                 last_error = NULL, esi_expires_at = $5, failures = 0
             WHERE region_id = $1 AND type_id = $2",
        )
        .bind(region_id)
        .bind(type_id)
        .bind(earliest + jitter(self.interval))
        .bind(now)
        .bind(response.expires_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }

    /// Records the failure with the order poll's backoff; the old copy stays.
    async fn record_failure(
        &self,
        region_id: i64,
        type_id: i64,
        now: OffsetDateTime,
        reason: &str,
    ) -> RefreshOutcome {
        tracing::warn!(region_id, type_id, "market refresh failed: {reason}");
        let result = sqlx::query(
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
        .bind(BACKOFF_START.as_seconds_f64())
        .bind(self.interval.as_seconds_f64())
        .execute(&self.pool)
        .await;
        if let Err(e) = result {
            tracing::error!(region_id, type_id, "cannot record a market failure: {e}");
        }
        RefreshOutcome::Failed
    }

    /// The gate paused the route: try again when it lets us, without counting a failure.
    async fn record_deferral(
        &self,
        region_id: i64,
        type_id: i64,
        now: OffsetDateTime,
        until: OffsetDateTime,
    ) -> RefreshOutcome {
        let result = sqlx::query(
            "UPDATE market_sync SET next_poll_at = $3, last_attempt_at = $4
             WHERE region_id = $1 AND type_id = $2",
        )
        .bind(region_id)
        .bind(type_id)
        .bind(until + jitter(self.interval))
        .bind(now)
        .execute(&self.pool)
        .await;
        if let Err(e) = result {
            tracing::error!(region_id, type_id, "cannot record a deferred refresh: {e}");
        }
        RefreshOutcome::Deferred
    }
}
