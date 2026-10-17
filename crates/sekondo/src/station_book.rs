//! The station book poller: reads the whole public order book of the station's region, keeps the
//! orders placed at the station, and records a top-of-book snapshot of every tradable item.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use futures_util::StreamExt;
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};

use crate::{
    clock::Clock,
    esi::{EsiBookPage, EsiClient, EsiError, EsiMarketOrder},
    events::{Event, EventBus},
    market::RefreshOutcome,
    names,
    opportunities::{REGION_ID, STATION_ID, stats},
    poller::{BACKOFF_START, CLAIM_LEASE, SCHEDULER_TICK, jitter},
};

/// How often the book is refreshed at the earliest: the cache time of ESI's market answers.
pub const BOOK_INTERVAL: Duration = Duration::minutes(5);

/// How many pages are read at the same time, through the shared gate.
const PAGE_CONCURRENCY: usize = 8;
/// A read that crossed an ESI cache generation is repeated at once, at most this often.
const GENERATION_RETRIES: usize = 2;

/// Where the pages of the region's book come from. The ESI client, or a script in tests.
#[async_trait]
pub trait BookPages: Send + Sync {
    async fn page(&self, region_id: i64, page: u32) -> Result<EsiBookPage, EsiError>;
}

#[async_trait]
impl BookPages for EsiClient {
    async fn page(&self, region_id: i64, page: u32) -> Result<EsiBookPage, EsiError> {
        self.region_orders_page(region_id, page).await
    }
}

/// Why a read of the book did not give a book.
enum ReadError {
    Esi(EsiError),
    /// Every page of every attempt belonged to more than one cache generation.
    MixedGenerations,
}

pub struct StationBookPoller {
    pool: PgPool,
    pages: Arc<dyn BookPages>,
    /// For the names of the items; without it (a scripted source) no names are looked up.
    esi: Option<Arc<EsiClient>>,
    events: Arc<dyn EventBus>,
    clock: Arc<dyn Clock>,
    interval: Duration,
}

impl StationBookPoller {
    pub fn new(
        pool: PgPool,
        esi: Arc<EsiClient>,
        events: Arc<dyn EventBus>,
        clock: Arc<dyn Clock>,
        interval: Duration,
    ) -> Self {
        Self {
            esi: Some(esi.clone()),
            ..Self::with_source(pool, esi, events, clock, interval)
        }
    }

    pub fn with_source(
        pool: PgPool,
        pages: Arc<dyn BookPages>,
        events: Arc<dyn EventBus>,
        clock: Arc<dyn Clock>,
        interval: Duration,
    ) -> Self {
        Self {
            pool,
            pages,
            esi: None,
            events,
            clock,
            interval,
        }
    }

    /// Runs [`StationBookPoller::tick`] every scheduler tick until the task is aborted.
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

    /// Refreshes the book when it is due (at once on a fresh database). Returns how many
    /// refreshes it started.
    pub async fn tick(&self) -> usize {
        let now = self.clock.now();
        let claimed = async {
            sqlx::query(
                "INSERT INTO station_sync (region_id, station_id, kind, next_poll_at)
                 VALUES ($1, $2, 'book', $3) ON CONFLICT DO NOTHING",
            )
            .bind(REGION_ID)
            .bind(STATION_ID)
            .bind(now)
            .execute(&self.pool)
            .await?;
            // Pushed back by a lease, so another tick leaves it alone while the refresh runs.
            sqlx::query(
                "UPDATE station_sync SET next_poll_at = $3 + $4::float8 * interval '1 second'
                 WHERE region_id = $1 AND station_id = $2 AND kind = 'book' AND next_poll_at <= $3",
            )
            .bind(REGION_ID)
            .bind(STATION_ID)
            .bind(now)
            .bind(CLAIM_LEASE.as_seconds_f64())
            .execute(&self.pool)
            .await
            .map(|done| done.rows_affected() > 0)
        }
        .await;
        match claimed {
            Ok(true) => {
                self.refresh().await;
                1
            }
            Ok(false) => 0,
            Err(e) => {
                tracing::error!("cannot claim the station book: {e}");
                0
            }
        }
    }

    /// Refreshes the book now, whatever the schedule says. Every way it can end is a
    /// [`RefreshOutcome`], and the database records the details.
    pub async fn refresh(&self) -> RefreshOutcome {
        let now = self.clock.now();
        match self.read_book().await {
            Ok((orders, expires_at)) => match self.store(now, &orders, expires_at).await {
                Ok(()) => {
                    self.update_stats(now).await;
                    self.resolve_names(now).await;
                    // Public data: every open Opportunities page re-fetches.
                    self.events.publish_all(Event::OpportunitiesUpdated);
                    RefreshOutcome::Updated
                }
                Err(e) => {
                    self.record_failure(now, &format!("database error: {e}"))
                        .await
                }
            },
            Err(ReadError::Esi(EsiError::Deferred { until })) => {
                self.record_deferral(now, until).await
            }
            Err(ReadError::Esi(e)) => self.record_failure(now, &e.to_string()).await,
            Err(ReadError::MixedGenerations) => {
                self.record_failure(
                    now,
                    "the order book changed its cache generation during every read",
                )
                .await
            }
        }
    }

    /// Caches the names of the tradable items that are not known yet, so the tables can show
    /// them. Never fails the refresh.
    async fn resolve_names(&self, now: OffsetDateTime) {
        let Some(esi) = &self.esi else { return };
        let tradable: Vec<i64> = match sqlx::query_scalar(
            "SELECT type_id FROM station_orders WHERE region_id = $1 AND station_id = $2
             GROUP BY type_id HAVING bool_or(is_buy_order) AND bool_or(NOT is_buy_order)",
        )
        .bind(REGION_ID)
        .bind(STATION_ID)
        .fetch_all(&self.pool)
        .await
        {
            Ok(tradable) => tradable,
            Err(e) => {
                tracing::error!("cannot list the tradable items to name: {e}");
                return;
            }
        };
        names::resolve_public_names(&self.pool, esi, tradable, now).await;
    }

    /// The book fields of every item, and the expansion fields when the hourly rollup advanced.
    /// A failure here never fails the refresh: the book and its snapshot are safe already.
    async fn update_stats(&self, now: OffsetDateTime) {
        if let Err(e) = stats::refresh_book_fields(&self.pool, now).await {
            tracing::error!("cannot update the station book fields: {e}");
        }
        if let Err(e) = stats::refresh_expansions_if_advanced(&self.pool, now).await {
            tracing::error!("cannot update the station expansion fields: {e}");
        }
    }

    /// Every order of the region's book, once, from pages of one cache generation.
    async fn read_book(&self) -> Result<(Vec<EsiMarketOrder>, Option<OffsetDateTime>), ReadError> {
        for _ in 0..=GENERATION_RETRIES {
            let first = self
                .pages
                .page(REGION_ID, 1)
                .await
                .map_err(ReadError::Esi)?;
            let rest: Vec<Result<EsiBookPage, EsiError>> =
                futures_util::stream::iter(2..=first.pages)
                    .map(|page| self.pages.page(REGION_ID, page))
                    .buffer_unordered(PAGE_CONCURRENCY)
                    .collect()
                    .await;
            let mut pages = vec![first];
            let mut deferred: Option<OffsetDateTime> = None;
            let mut failure = None;
            for result in rest {
                match result {
                    Ok(page) => pages.push(page),
                    Err(EsiError::Deferred { until }) => {
                        deferred = Some(deferred.map_or(until, |d| d.max(until)));
                    }
                    Err(e) => failure = failure.or(Some(e)),
                }
            }
            if let Some(e) = failure {
                return Err(ReadError::Esi(e));
            }
            if let Some(until) = deferred {
                return Err(ReadError::Esi(EsiError::Deferred { until }));
            }
            let generation = pages[0].generation;
            if pages.iter().any(|p| p.generation != generation) {
                continue;
            }
            let expires_at = pages.iter().filter_map(|p| p.expires_at).min();
            let orders = pages.into_iter().flat_map(|p| p.orders).collect();
            return Ok((orders, expires_at));
        }
        Err(ReadError::MixedGenerations)
    }

    /// Records the snapshots, replaces the station's book and records the success, all in one
    /// transaction.
    async fn store(
        &self,
        now: OffsetDateTime,
        orders: &[EsiMarketOrder],
        expires_at: Option<OffsetDateTime>,
    ) -> Result<(), sqlx::Error> {
        // The orders at the station. An order can show up on two pages when the book moved
        // under the read; the last one wins.
        let at_station: HashMap<i64, &EsiMarketOrder> = orders
            .iter()
            .filter(|o| o.location_id == STATION_ID)
            .map(|o| (o.order_id, o))
            .collect();
        let mut rows: Vec<&EsiMarketOrder> = at_station.into_values().collect();
        rows.sort_unstable_by_key(|o| o.order_id);
        let order_id: Vec<i64> = rows.iter().map(|o| o.order_id).collect();
        let type_id: Vec<i64> = rows.iter().map(|o| o.type_id).collect();
        let is_buy: Vec<bool> = rows.iter().map(|o| o.is_buy_order).collect();
        let price_cents: Vec<i64> = rows.iter().map(|o| o.price_cents).collect();
        let volume_remain: Vec<i64> = rows.iter().map(|o| o.volume_remain).collect();

        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "CREATE TEMP TABLE new_station_orders ON COMMIT DROP AS
             SELECT o.order_id, o.type_id, o.is_buy_order, o.price_cents::numeric / 100 AS price,
                    o.volume_remain
             FROM unnest($1::bigint[], $2::bigint[], $3::bool[], $4::bigint[], $5::bigint[])
                  AS o(order_id, type_id, is_buy_order, price_cents, volume_remain)",
        )
        .bind(&order_id)
        .bind(&type_id)
        .bind(&is_buy)
        .bind(&price_cents)
        .bind(&volume_remain)
        .execute(&mut *tx)
        .await?;

        // Snapshots of the tradable items (both sides present). Within 1% of a side's best is
        // compared as price * 100 against best * 99 or 101, so it is exact. Units taken look at
        // orders present in the previous book and this one, before the book is replaced.
        sqlx::query(
            "WITH best AS (
                 SELECT type_id,
                        max(price) FILTER (WHERE is_buy_order) AS best_buy,
                        min(price) FILTER (WHERE NOT is_buy_order) AS best_sell
                 FROM new_station_orders GROUP BY type_id
                 HAVING bool_or(is_buy_order) AND bool_or(NOT is_buy_order)),
             sides AS (
                 SELECT n.type_id,
                        COALESCE(sum(n.volume_remain) FILTER (WHERE n.is_buy_order), 0)::bigint AS buy_units,
                        COALESCE(sum(n.volume_remain) FILTER (WHERE NOT n.is_buy_order), 0)::bigint AS sell_units,
                        COALESCE(sum(n.volume_remain) FILTER (WHERE n.is_buy_order AND n.price * 100 >= b.best_buy * 99), 0)::bigint AS buy_near_units,
                        count(*) FILTER (WHERE n.is_buy_order AND n.price * 100 >= b.best_buy * 99)::int AS buy_near_orders,
                        COALESCE(sum(n.volume_remain) FILTER (WHERE NOT n.is_buy_order AND n.price * 100 <= b.best_sell * 101), 0)::bigint AS sell_near_units,
                        count(*) FILTER (WHERE NOT n.is_buy_order AND n.price * 100 <= b.best_sell * 101)::int AS sell_near_orders
                 FROM new_station_orders n JOIN best b USING (type_id)
                 GROUP BY n.type_id),
             taken AS (
                 SELECT n.type_id,
                        COALESCE(sum(greatest(o.volume_remain - n.volume_remain, 0)) FILTER (WHERE n.is_buy_order), 0)::bigint AS buy_taken,
                        COALESCE(sum(greatest(o.volume_remain - n.volume_remain, 0)) FILTER (WHERE NOT n.is_buy_order), 0)::bigint AS sell_taken
                 FROM new_station_orders n
                 JOIN station_orders o ON o.order_id = n.order_id
                  AND o.region_id = $1 AND o.station_id = $2
                 GROUP BY n.type_id)
             INSERT INTO station_snapshots
                 (recorded_at, region_id, station_id, type_id, best_buy, best_sell, spread,
                  buy_near_units, buy_near_orders, sell_near_units, sell_near_orders,
                  buy_units, sell_units, buy_taken, sell_taken)
             SELECT $3, $1, $2, b.type_id, b.best_buy, b.best_sell,
                    (b.best_sell - b.best_buy) / NULLIF(b.best_buy, 0),
                    s.buy_near_units, s.buy_near_orders, s.sell_near_units, s.sell_near_orders,
                    s.buy_units, s.sell_units,
                    COALESCE(t.buy_taken, 0), COALESCE(t.sell_taken, 0)
             FROM best b JOIN sides s USING (type_id) LEFT JOIN taken t USING (type_id)
             WHERE b.best_buy > 0",
        )
        .bind(REGION_ID)
        .bind(STATION_ID)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM station_orders WHERE region_id = $1 AND station_id = $2")
            .bind(REGION_ID)
            .bind(STATION_ID)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO station_orders
                 (region_id, station_id, order_id, type_id, is_buy_order, price, volume_remain)
             SELECT $1, $2, order_id, type_id, is_buy_order, price, volume_remain
             FROM new_station_orders",
        )
        .bind(REGION_ID)
        .bind(STATION_ID)
        .execute(&mut *tx)
        .await?;

        // `max(now + interval, ESI's Expires)`, pushed back by a little jitter.
        let earliest = (now + self.interval).max(expires_at.unwrap_or(now));
        sqlx::query(
            "INSERT INTO station_sync
                 (region_id, station_id, kind, next_poll_at, last_success_at, last_attempt_at,
                  last_error, esi_expires_at, failures)
             VALUES ($1, $2, 'book', $3, $4, $4, NULL, $5, 0)
             ON CONFLICT (region_id, station_id, kind) DO UPDATE SET
                 next_poll_at = excluded.next_poll_at,
                 last_success_at = excluded.last_success_at,
                 last_attempt_at = excluded.last_attempt_at,
                 last_error = NULL, esi_expires_at = excluded.esi_expires_at, failures = 0",
        )
        .bind(REGION_ID)
        .bind(STATION_ID)
        .bind(earliest + jitter(self.interval))
        .bind(now)
        .bind(expires_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }

    /// Records the failure with the other polls' backoff; the old book stays and no snapshot is
    /// recorded.
    async fn record_failure(&self, now: OffsetDateTime, reason: &str) -> RefreshOutcome {
        tracing::warn!("station book refresh failed: {reason}");
        let result = sqlx::query(
            "INSERT INTO station_sync
                 (region_id, station_id, kind, next_poll_at, last_attempt_at, last_error, failures)
             VALUES ($1, $2, 'book', $3 + $5::float8 * interval '1 second', $3, $4, 1)
             ON CONFLICT (region_id, station_id, kind) DO UPDATE SET
                 next_poll_at = $3 + LEAST(
                     $5::float8 * power(2, LEAST(station_sync.failures, 20)) * interval '1 second',
                     $6::float8 * interval '1 second'),
                 last_attempt_at = $3, last_error = $4,
                 failures = station_sync.failures + 1",
        )
        .bind(REGION_ID)
        .bind(STATION_ID)
        .bind(now)
        .bind(reason)
        .bind(BACKOFF_START.as_seconds_f64())
        .bind(self.interval.as_seconds_f64())
        .execute(&self.pool)
        .await;
        if let Err(e) = result {
            tracing::error!("cannot record a station book failure: {e}");
        }
        RefreshOutcome::Failed
    }

    /// The gate paused the route: try again when it lets us, without counting a failure.
    async fn record_deferral(&self, now: OffsetDateTime, until: OffsetDateTime) -> RefreshOutcome {
        let result = sqlx::query(
            "INSERT INTO station_sync (region_id, station_id, kind, next_poll_at, last_attempt_at)
             VALUES ($1, $2, 'book', $3, $4)
             ON CONFLICT (region_id, station_id, kind) DO UPDATE SET
                 next_poll_at = excluded.next_poll_at, last_attempt_at = excluded.last_attempt_at",
        )
        .bind(REGION_ID)
        .bind(STATION_ID)
        .bind(until + jitter(self.interval))
        .bind(now)
        .execute(&self.pool)
        .await;
        if let Err(e) = result {
            tracing::error!("cannot record a deferred station book refresh: {e}");
        }
        RefreshOutcome::Deferred
    }
}
