//! The price poller: keeps one public copy of ESI's market prices for everybody, refreshes it
//! once a day, and revalues every character's inventory with each refresh.

use std::sync::Arc;

use sqlx::PgPool;
use time::{Duration, OffsetDateTime};

use crate::{
    clock::Clock,
    esi::{EsiClient, EsiError, EsiPrice, EsiResponse},
    events::{Event, EventBus},
    market::RefreshOutcome,
    poller::{BACKOFF_START, CLAIM_LEASE, SCHEDULER_TICK, jitter},
    wealth_poller::INVENTORY_VALUE_SUM,
};

/// How often the prices are refreshed.
pub const PRICES_INTERVAL: Duration = Duration::days(1);

pub struct PricePoller {
    pool: PgPool,
    esi: Arc<EsiClient>,
    events: Arc<dyn EventBus>,
    clock: Arc<dyn Clock>,
}

impl PricePoller {
    pub fn new(
        pool: PgPool,
        esi: Arc<EsiClient>,
        events: Arc<dyn EventBus>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            pool,
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
        let claimed = async {
            sqlx::query(
                "INSERT INTO price_sync (singleton, next_poll_at) VALUES (true, $1)
                 ON CONFLICT (singleton) DO NOTHING",
            )
            .bind(now)
            .execute(&self.pool)
            .await?;
            // Pushed back by a lease, so another tick does not refresh while this one runs.
            sqlx::query(
                "UPDATE price_sync SET next_poll_at = $1 + $2::float8 * interval '1 second'
                 WHERE next_poll_at <= $1",
            )
            .bind(now)
            .bind(CLAIM_LEASE.as_seconds_f64())
            .execute(&self.pool)
            .await
        }
        .await;
        match claimed {
            Ok(done) if done.rows_affected() > 0 => {
                self.refresh().await;
                1
            }
            Ok(_) => 0,
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
        match sqlx::query_scalar::<_, uuid::Uuid>(
            "SELECT DISTINCT c.account_id FROM current_assets a
             JOIN characters c ON c.character_id = a.character_id",
        )
        .fetch_all(&self.pool)
        .await
        {
            Ok(accounts) => {
                for account in accounts {
                    self.events.publish(account, Event::WealthUpdated);
                }
            }
            Err(e) => tracing::error!("cannot look up the accounts to signal: {e}"),
        }
    }

    /// Replaces the prices, records the new inventory value of every character that has assets
    /// and updates the sync state, in one transaction.
    async fn store(
        &self,
        now: OffsetDateTime,
        response: &EsiResponse<Vec<EsiPrice>>,
    ) -> Result<(), sqlx::Error> {
        let type_id: Vec<i64> = response.body.iter().map(|p| p.type_id).collect();
        let cents: Vec<Option<i64>> = response
            .body
            .iter()
            .map(|p| p.average_price_cents)
            .collect();
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM item_prices")
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO item_prices (type_id, average_price)
             SELECT p.type_id, p.cents::numeric / 100
             FROM unnest($1::bigint[], $2::bigint[]) AS p(type_id, cents)
             ON CONFLICT (type_id) DO UPDATE SET average_price = excluded.average_price",
        )
        .bind(&type_id)
        .bind(&cents)
        .execute(&mut *tx)
        .await?;
        // Two entries in the same instant keep the later one.
        let revalue = format!(
            "INSERT INTO inventory_values (character_id, recorded_at, value)
             SELECT a.character_id, $1, {INVENTORY_VALUE_SUM}
             FROM current_assets a LEFT JOIN item_prices p USING (type_id)
             GROUP BY a.character_id
             ON CONFLICT (character_id, recorded_at) DO UPDATE SET value = excluded.value"
        );
        sqlx::query(sqlx::AssertSqlSafe(revalue))
            .bind(now)
            .execute(&mut *tx)
            .await?;
        // `max(now + interval, ESI's Expires)`, pushed back by a little jitter.
        let earliest = (now + PRICES_INTERVAL).max(response.expires_at.unwrap_or(now));
        sqlx::query(
            "INSERT INTO price_sync
                 (singleton, next_poll_at, last_success_at, last_attempt_at, last_error,
                  esi_expires_at, failures)
             VALUES (true, $1, $2, $2, NULL, $3, 0)
             ON CONFLICT (singleton) DO UPDATE SET
                 next_poll_at = excluded.next_poll_at,
                 last_success_at = excluded.last_success_at,
                 last_attempt_at = excluded.last_attempt_at,
                 last_error = NULL,
                 esi_expires_at = excluded.esi_expires_at,
                 failures = 0",
        )
        .bind(earliest + jitter(PRICES_INTERVAL))
        .bind(now)
        .bind(response.expires_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }

    /// Records the failure with the order poll's backoff; the old prices stay.
    async fn record_failure(&self, now: OffsetDateTime, reason: &str) -> RefreshOutcome {
        tracing::warn!("price refresh failed: {reason}");
        let result = sqlx::query(
            "INSERT INTO price_sync (singleton, next_poll_at, last_attempt_at, last_error, failures)
             VALUES (true, $1, $2, $3, 1)
             ON CONFLICT (singleton) DO UPDATE SET
                 next_poll_at = $2 + LEAST(
                     $4::float8 * power(2, LEAST(price_sync.failures, 20)) * interval '1 second',
                     $5::float8 * interval '1 second'),
                 last_attempt_at = $2,
                 last_error = $3,
                 failures = price_sync.failures + 1",
        )
        .bind(now + BACKOFF_START)
        .bind(now)
        .bind(reason)
        .bind(BACKOFF_START.as_seconds_f64())
        .bind(PRICES_INTERVAL.as_seconds_f64())
        .execute(&self.pool)
        .await;
        if let Err(e) = result {
            tracing::error!("cannot record a price failure: {e}");
        }
        RefreshOutcome::Failed
    }

    /// The gate paused the route: try again when it lets us, without counting a failure.
    async fn record_deferral(&self, now: OffsetDateTime, until: OffsetDateTime) -> RefreshOutcome {
        let result = sqlx::query(
            "INSERT INTO price_sync (singleton, next_poll_at, last_attempt_at) VALUES (true, $1, $2)
             ON CONFLICT (singleton) DO UPDATE SET
                 next_poll_at = excluded.next_poll_at,
                 last_attempt_at = excluded.last_attempt_at",
        )
        .bind(until + jitter(PRICES_INTERVAL))
        .bind(now)
        .execute(&self.pool)
        .await;
        if let Err(e) = result {
            tracing::error!("cannot record a deferred price refresh: {e}");
        }
        RefreshOutcome::Deferred
    }
}
