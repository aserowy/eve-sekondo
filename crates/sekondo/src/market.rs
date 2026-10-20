//! The market poller: keeps a copy of the public market orders of every (region, item) pair that
//! a linked character has a current order for, and refreshes each copy on its own schedule.

use std::sync::Arc;

use futures_util::StreamExt;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    clock::Clock,
    esi::{EsiClient, EsiError, EsiMarketOrder, EsiResponse},
    events::{Event, EventBus},
    poller::{BACKOFF_START, CLAIM_LEASE, SCHEDULER_TICK, jitter, spread},
    store::{MarketStore, StandingRow, StoreError, Stores},
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
/// gap and undercut volume (design §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// `(account, order, undercut, gap in cents, undercut volume)`, ordered.
    rows: Vec<StandingRow>,
}

impl Snapshot {
    /// The accounts with an order whose standing, gap or undercut volume differs between the two snapshots, or
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

/// The standing, gap and undercut volume of every current order that uses the pair, from the same
/// read the orders page lists with.
pub async fn snapshot(
    store: &dyn MarketStore,
    region_id: i64,
    type_id: i64,
) -> Result<Snapshot, StoreError> {
    Ok(Snapshot {
        rows: store.standings(region_id, type_id).await?,
    })
}

pub struct MarketPoller {
    store: Arc<dyn MarketStore>,
    esi: Arc<EsiClient>,
    events: Arc<dyn EventBus>,
    clock: Arc<dyn Clock>,
    interval: Duration,
}

impl MarketPoller {
    pub fn new(
        stores: &Stores,
        esi: Arc<EsiClient>,
        events: Arc<dyn EventBus>,
        clock: Arc<dyn Clock>,
        interval: Duration,
    ) -> Self {
        Self {
            store: stores.market.clone(),
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
    pub async fn refresh_account_pairs(&self, account_id: Uuid) -> Result<usize, StoreError> {
        self.track(self.clock.now()).await?;
        let pairs = self.store.account_pairs(account_id).await?;
        for (region_id, type_id) in &pairs {
            self.refresh_pair(*region_id, *type_id).await;
        }
        Ok(pairs.len())
    }

    /// Gives every pair a current order uses a first refresh time inside the next interval, and
    /// drops the sync state and the copy of pairs no current order uses any more.
    async fn track(&self, now: OffsetDateTime) -> Result<(), StoreError> {
        let missing = self.store.untracked_pairs().await?;
        if !missing.is_empty() {
            let times: Vec<OffsetDateTime> = missing
                .iter()
                .map(|(region, type_id)| {
                    let mut key = region.to_be_bytes().to_vec();
                    key.extend(type_id.to_be_bytes());
                    now + spread(&key, self.interval)
                })
                .collect();
            self.store.insert_pair_schedule(&missing, &times).await?;
        }
        self.store.drop_unused_pairs().await
    }

    /// Takes the due pairs. They are pushed back by a lease, so neither this nor another tick
    /// takes them again while their refresh runs.
    async fn claim_due(&self, now: OffsetDateTime) -> Result<Vec<(i64, i64)>, StoreError> {
        self.store
            .claim_due_pairs(now, CLAIM_LEASE, (CONCURRENCY * 8) as i64)
            .await
    }

    /// Refreshes one pair now, whatever its schedule says. Every way it can end is a
    /// [`RefreshOutcome`], and the database records the details.
    pub async fn refresh_pair(&self, region_id: i64, type_id: i64) -> RefreshOutcome {
        let now = self.clock.now();
        let before = snapshot(&*self.store, region_id, type_id).await;
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

    /// Tells the accounts whose orders now show a different standing, gap or undercut volume. Without a before
    /// snapshot, or when it cannot be read, nothing is sent: a page also refreshes on reconnect.
    async fn signal_changes(
        &self,
        region_id: i64,
        type_id: i64,
        before: Result<Snapshot, StoreError>,
    ) {
        let (Ok(before), Ok(after)) = (before, snapshot(&*self.store, region_id, type_id).await)
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
    ) -> Result<(), StoreError> {
        // `max(now + interval, ESI's Expires)`, pushed back by a little jitter.
        let earliest = (now + self.interval).max(response.expires_at.unwrap_or(now));
        self.store
            .replace_market_orders(
                region_id,
                type_id,
                now,
                &response.body,
                earliest + jitter(self.interval),
                response.expires_at,
            )
            .await
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
        let result = self
            .store
            .record_pair_failure(
                region_id,
                type_id,
                now,
                reason,
                BACKOFF_START,
                self.interval,
            )
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
        let result = self
            .store
            .record_pair_deferral(region_id, type_id, now, until + jitter(self.interval))
            .await;
        if let Err(e) = result {
            tracing::error!(region_id, type_id, "cannot record a deferred refresh: {e}");
        }
        RefreshOutcome::Deferred
    }
}
