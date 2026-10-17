//! The history poller: once a day per tradable item, reads ESI's market history of the station's
//! region and keeps the last 30 days.

use std::sync::Arc;

use futures_util::StreamExt;
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};

use crate::{
    clock::Clock,
    esi::{EsiClient, EsiError, EsiHistoryDay},
    events::{Event, EventBus},
    opportunities::{REGION_ID, STATION_ID, stats},
    poller::{BACKOFF_START, CLAIM_LEASE, SCHEDULER_TICK, jitter},
};

/// How many items are asked at the same time, through the shared gate.
const CONCURRENCY: usize = 2;
/// How many days of history are kept.
pub const KEEP_DAYS: i64 = 30;
/// The longest an item's failures are backed off.
const MAX_BACKOFF: Duration = Duration::hours(1);
/// What an item without history waits before it is asked again.
const NO_HISTORY_WAIT: Duration = Duration::days(1);

/// Items whose book at the station holds both a buy and a sell order.
const TRADABLE: &str = "SELECT type_id FROM station_orders
    WHERE region_id = $1 AND station_id = $2
    GROUP BY type_id HAVING bool_or(is_buy_order) AND bool_or(NOT is_buy_order)";

/// How one item's request ended.
enum Outcome {
    Stored(usize),
    /// ESI has no history for the item: nothing stored, asked again tomorrow.
    NoHistory,
    Failed(String),
    Deferred,
}

pub struct StationHistoryPoller {
    pool: PgPool,
    esi: Arc<EsiClient>,
    events: Arc<dyn EventBus>,
    clock: Arc<dyn Clock>,
}

impl StationHistoryPoller {
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

    /// Runs [`StationHistoryPoller::tick`] every scheduler tick until the task is aborted.
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

    /// Schedules newly tradable items, then asks for every due item until none is due. Returns
    /// how many items were asked.
    pub async fn tick(&self) -> usize {
        let now = self.clock.now();
        if let Err(e) = self.track(now).await {
            tracing::error!("cannot schedule station history: {e}");
            return 0;
        }
        let mut asked = 0;
        let mut stored = 0;
        let mut last_error: Option<String> = None;
        let mut any_ok = false;
        loop {
            let due = match self.claim_due(now).await {
                Ok(due) if !due.is_empty() => due,
                Ok(_) => break,
                Err(e) => {
                    tracing::error!("cannot claim station history: {e}");
                    break;
                }
            };
            asked += due.len();
            let outcomes: Vec<Outcome> = futures_util::stream::iter(due)
                .map(|type_id| self.fetch(type_id, now))
                .buffer_unordered(CONCURRENCY)
                .collect()
                .await;
            for outcome in outcomes {
                match outcome {
                    Outcome::Stored(n) => {
                        stored += n;
                        any_ok = true;
                    }
                    Outcome::NoHistory => any_ok = true,
                    Outcome::Failed(reason) => last_error = last_error.or(Some(reason)),
                    Outcome::Deferred => {}
                }
            }
        }
        if asked > 0 {
            if let Err(e) = self.finish_pass(now, any_ok, last_error.as_deref()).await {
                tracing::error!("cannot record the station history pass: {e}");
            }
            if stored > 0 {
                if let Err(e) = stats::refresh_indicators(&self.pool, now).await {
                    tracing::error!("cannot update the station indicator fields: {e}");
                }
                // Public data: every open Opportunities page re-fetches, once per pass.
                self.events.publish_all(Event::OpportunitiesUpdated);
            }
            tracing::debug!(asked, stored, "station history pass");
        }
        asked
    }

    /// Gives every tradable item without a schedule a first request at once.
    async fn track(&self, now: OffsetDateTime) -> Result<(), sqlx::Error> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO history_sync (region_id, type_id, next_poll_at)
             SELECT $1, t.type_id, $3 FROM ({TRADABLE}) t
             ON CONFLICT (region_id, type_id) DO NOTHING"
        )))
        .bind(REGION_ID)
        .bind(STATION_ID)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Takes the due tradable items. They are pushed back by a lease, so another tick leaves
    /// them alone while their request runs.
    async fn claim_due(&self, now: OffsetDateTime) -> Result<Vec<i64>, sqlx::Error> {
        sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "UPDATE history_sync SET next_poll_at = $3 + $4::float8 * interval '1 second'
             WHERE region_id = $1 AND type_id IN (
                 SELECT s.type_id FROM history_sync s
                 WHERE s.region_id = $1 AND s.next_poll_at <= $3
                   AND s.type_id IN ({TRADABLE})
                 ORDER BY s.next_poll_at, s.type_id
                 LIMIT $5
                 FOR UPDATE OF s SKIP LOCKED)
             RETURNING type_id"
        )))
        .bind(REGION_ID)
        .bind(STATION_ID)
        .bind(now)
        .bind(CLAIM_LEASE.as_seconds_f64())
        .bind((CONCURRENCY * 8) as i64)
        .fetch_all(&self.pool)
        .await
    }

    async fn fetch(&self, type_id: i64, now: OffsetDateTime) -> Outcome {
        match self.esi.market_history(REGION_ID, type_id).await {
            Ok(response) if response.body.is_empty() => {
                self.schedule(type_id, now, now + NO_HISTORY_WAIT).await;
                Outcome::NoHistory
            }
            Ok(response) => {
                let next = response.expires_at.unwrap_or(now + NO_HISTORY_WAIT);
                match self.store(type_id, now, next, &response.body).await {
                    Ok(stored) => Outcome::Stored(stored),
                    Err(e) => {
                        self.record_failure(type_id, now, &format!("database error: {e}"))
                            .await
                    }
                }
            }
            Err(EsiError::NotFound) => {
                self.schedule(type_id, now, now + NO_HISTORY_WAIT).await;
                Outcome::NoHistory
            }
            Err(EsiError::Deferred { until }) => {
                self.schedule(type_id, now, until + jitter(MAX_BACKOFF))
                    .await;
                Outcome::Deferred
            }
            Err(e) => self.record_failure(type_id, now, &e.to_string()).await,
        }
    }

    /// Upserts the days within the window and records the success, in one transaction.
    async fn store(
        &self,
        type_id: i64,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
        days: &[EsiHistoryDay],
    ) -> Result<usize, sqlx::Error> {
        let keep_from = (now - Duration::days(KEEP_DAYS)).date();
        let kept: Vec<&EsiHistoryDay> = days
            .iter()
            .filter(|d| {
                time::Date::parse(
                    &d.date,
                    &time::format_description::well_known::Iso8601::DATE,
                )
                .is_ok_and(|date| date >= keep_from)
            })
            .collect();
        let dates: Vec<&str> = kept.iter().map(|d| d.date.as_str()).collect();
        let average: Vec<i64> = kept.iter().map(|d| d.average_cents).collect();
        let highest: Vec<i64> = kept.iter().map(|d| d.highest_cents).collect();
        let lowest: Vec<i64> = kept.iter().map(|d| d.lowest_cents).collect();
        let volume: Vec<i64> = kept.iter().map(|d| d.volume).collect();
        let orders: Vec<i64> = kept.iter().map(|d| d.order_count).collect();

        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO market_history
                 (region_id, type_id, day, average, highest, lowest, volume, order_count)
             SELECT $1, $2, h.day::date, h.average / 100.0, h.highest / 100.0, h.lowest / 100.0,
                    h.volume, h.order_count
             FROM unnest($3::text[], $4::bigint[], $5::bigint[], $6::bigint[], $7::bigint[], $8::bigint[])
                  AS h(day, average, highest, lowest, volume, order_count)
             ON CONFLICT (region_id, type_id, day) DO UPDATE SET
                 average = excluded.average, highest = excluded.highest, lowest = excluded.lowest,
                 volume = excluded.volume, order_count = excluded.order_count",
        )
        .bind(REGION_ID)
        .bind(type_id)
        .bind(&dates)
        .bind(&average)
        .bind(&highest)
        .bind(&lowest)
        .bind(&volume)
        .bind(&orders)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE history_sync SET next_poll_at = $3, last_success_at = $4, last_attempt_at = $4,
                 last_error = NULL, esi_expires_at = $3, failures = 0
             WHERE region_id = $1 AND type_id = $2",
        )
        .bind(REGION_ID)
        .bind(type_id)
        .bind(next_poll_at)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        // Older days go with the daily pass.
        sqlx::query("DELETE FROM market_history WHERE region_id = $1 AND day < $2")
            .bind(REGION_ID)
            .bind(keep_from)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(kept.len())
    }

    /// Records when an item is asked next, without counting a failure.
    async fn schedule(&self, type_id: i64, now: OffsetDateTime, next_poll_at: OffsetDateTime) {
        let result = sqlx::query(
            "UPDATE history_sync SET next_poll_at = $3, last_attempt_at = $4, last_error = NULL,
                 failures = 0
             WHERE region_id = $1 AND type_id = $2",
        )
        .bind(REGION_ID)
        .bind(type_id)
        .bind(next_poll_at)
        .bind(now)
        .execute(&self.pool)
        .await;
        if let Err(e) = result {
            tracing::error!(type_id, "cannot schedule station history: {e}");
        }
    }

    /// Records the failure with the other polls' backoff; what is stored for the item stays.
    async fn record_failure(&self, type_id: i64, now: OffsetDateTime, reason: &str) -> Outcome {
        tracing::warn!(type_id, "station history failed: {reason}");
        let result = sqlx::query(
            "UPDATE history_sync SET
                 next_poll_at = $3 + LEAST(
                     $5::float8 * power(2, LEAST(failures, 20)) * interval '1 second',
                     $6::float8 * interval '1 second'),
                 last_attempt_at = $3, last_error = $4, failures = failures + 1
             WHERE region_id = $1 AND type_id = $2",
        )
        .bind(REGION_ID)
        .bind(type_id)
        .bind(now)
        .bind(reason)
        .bind(BACKOFF_START.as_seconds_f64())
        .bind(MAX_BACKOFF.as_seconds_f64())
        .execute(&self.pool)
        .await;
        if let Err(e) = result {
            tracing::error!(type_id, "cannot record a station history failure: {e}");
        }
        Outcome::Failed(reason.to_owned())
    }

    /// Records when the history run last finished, and its last error.
    async fn finish_pass(
        &self,
        now: OffsetDateTime,
        any_ok: bool,
        last_error: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO station_sync
                 (region_id, station_id, kind, next_poll_at, last_success_at, last_attempt_at, last_error)
             VALUES ($1, $2, 'history', $3, CASE WHEN $4 THEN $3 END, $3, $5)
             ON CONFLICT (region_id, station_id, kind) DO UPDATE SET
                 last_success_at = CASE WHEN $4 THEN $3 ELSE station_sync.last_success_at END,
                 last_attempt_at = $3, last_error = $5",
        )
        .bind(REGION_ID)
        .bind(STATION_ID)
        .bind(now)
        .bind(any_ok)
        .bind(last_error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
