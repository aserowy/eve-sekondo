//! The price poller: keeps one public copy of ESI's market prices for everybody, refreshes it
//! once a day, and revalues every character's inventory with each refresh.

use std::sync::Arc;

use time::{Duration, OffsetDateTime};

use crate::{
    clock::Clock,
    esi::{EsiClient, EsiError, EsiPrice, EsiResponse},
    events::{Event, EventBus},
    market::RefreshOutcome,
    poller::{BACKOFF_START, CLAIM_LEASE, SCHEDULER_TICK, jitter},
    store::{MarketStore, StoreError, Stores},
};

/// How often the prices are refreshed.
pub const PRICES_INTERVAL: Duration = Duration::days(1);

pub struct PricePoller {
    store: Arc<dyn MarketStore>,
    esi: Arc<EsiClient>,
    events: Arc<dyn EventBus>,
    clock: Arc<dyn Clock>,
}

impl PricePoller {
    pub fn new(
        stores: &Stores,
        esi: Arc<EsiClient>,
        events: Arc<dyn EventBus>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            store: stores.market.clone(),
            esi,
            events,
            clock,
        }
    }

    /// Runs [`PricePoller::tick`] every scheduler tick until the task is aborted.
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

    /// Refreshes the prices when they are due; the very first refresh is due at once, because
    /// nothing can be valued without prices. Returns how many refreshes ran (0 or 1).
    pub async fn tick(&self) -> usize {
        let now = self.clock.now();
        let claimed = self.store.claim_price_refresh(now, CLAIM_LEASE).await;
        match claimed {
            Ok(true) => {
                self.refresh().await;
                1
            }
            Ok(false) => 0,
            Err(e) => {
                tracing::error!("cannot claim the price refresh: {e}");
                0
            }
        }
    }

    /// Refreshes the prices now, whatever the schedule says. Every way it can end is a
    /// [`RefreshOutcome`], and the database records the details.
    pub async fn refresh(&self) -> RefreshOutcome {
        let now = self.clock.now();
        let response = match self.esi.prices().await {
            Ok(response) => response,
            Err(EsiError::Deferred { until }) => return self.record_deferral(now, until).await,
            Err(e) => return self.record_failure(now, &e.to_string()).await,
        };
        match self.store(now, &response).await {
            Ok(()) => {
                self.signal_accounts().await;
                RefreshOutcome::Updated
            }
            Err(e) => {
                self.record_failure(now, &format!("database error: {e}"))
                    .await
            }
        }
    }

    /// Tells every account that has assets: their inventory was revalued.
    async fn signal_accounts(&self) {
        match self.store.accounts_with_assets().await {
            Ok(accounts) => {
                for account in accounts {
                    self.events.publish(account, Event::WealthUpdated);
                }
            }
            Err(e) => tracing::error!("cannot look up the accounts to signal: {e}"),
        }
    }

    /// Replaces the prices, records the new inventory value of every character that has assets
    /// and updates the sync state, atomically.
    async fn store(
        &self,
        now: OffsetDateTime,
        response: &EsiResponse<Vec<EsiPrice>>,
    ) -> Result<(), StoreError> {
        // `max(now + interval, ESI's Expires)`, pushed back by a little jitter.
        let earliest = (now + PRICES_INTERVAL).max(response.expires_at.unwrap_or(now));
        self.store
            .replace_prices(
                now,
                &response.body,
                earliest + jitter(PRICES_INTERVAL),
                response.expires_at,
            )
            .await
    }

    /// Records the failure with the order poll's backoff; the old prices stay.
    async fn record_failure(&self, now: OffsetDateTime, reason: &str) -> RefreshOutcome {
        tracing::warn!("price refresh failed: {reason}");
        let result = self
            .store
            .record_price_failure(now, reason, BACKOFF_START, PRICES_INTERVAL)
            .await;
        if let Err(e) = result {
            tracing::error!("cannot record a price failure: {e}");
        }
        RefreshOutcome::Failed
    }

    /// The gate paused the route: try again when it lets us, without counting a failure.
    async fn record_deferral(&self, now: OffsetDateTime, until: OffsetDateTime) -> RefreshOutcome {
        let result = self
            .store
            .record_price_deferral(now, until + jitter(PRICES_INTERVAL))
            .await;
        if let Err(e) = result {
            tracing::error!("cannot record a deferred price refresh: {e}");
        }
        RefreshOutcome::Deferred
    }
}
