//! The station book poller: reads the whole public order book of the station's region, keeps the
//! orders placed at the station, and records a top-of-book snapshot of every tradable item.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use futures_util::StreamExt;
use time::{Duration, OffsetDateTime};

use crate::{
    clock::Clock,
    esi::{EsiBookPage, EsiClient, EsiError, EsiMarketOrder},
    events::{Event, EventBus},
    market::RefreshOutcome,
    names,
    opportunities::{REGION_ID, STATION_ID, stats},
    poller::{BACKOFF_START, CLAIM_LEASE, SCHEDULER_TICK, jitter},
    store::{MarketStore, StationOrder, StationStore, StoreError, Stores},
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
    market: Arc<dyn MarketStore>,
    station: Arc<dyn StationStore>,
    pages: Arc<dyn BookPages>,
    /// For the names of the items; without it (a scripted source) no names are looked up.
    esi: Option<Arc<EsiClient>>,
    events: Arc<dyn EventBus>,
    clock: Arc<dyn Clock>,
    interval: Duration,
}

impl StationBookPoller {
    pub fn new(
        stores: &Stores,
        esi: Arc<EsiClient>,
        events: Arc<dyn EventBus>,
        clock: Arc<dyn Clock>,
        interval: Duration,
    ) -> Self {
        Self {
            esi: Some(esi.clone()),
            ..Self::with_source(stores, esi, events, clock, interval)
        }
    }

    pub fn with_source(
        stores: &Stores,
        pages: Arc<dyn BookPages>,
        events: Arc<dyn EventBus>,
        clock: Arc<dyn Clock>,
        interval: Duration,
    ) -> Self {
        Self {
            market: stores.market.clone(),
            station: stores.station.clone(),
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
        let claimed = self
            .station
            .claim_book_refresh(REGION_ID, STATION_ID, now, CLAIM_LEASE)
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
        let tradable = match self.station.tradable_items(REGION_ID, STATION_ID).await {
            Ok(tradable) => tradable,
            Err(e) => {
                tracing::error!("cannot list the tradable items to name: {e}");
                return;
            }
        };
        names::resolve_public_names(&*self.market, esi, tradable, now).await;
    }

    /// The book fields of every item, and the expansion fields when the hourly rollup advanced.
    /// A failure here never fails the refresh: the book and its snapshot are safe already.
    async fn update_stats(&self, now: OffsetDateTime) {
        if let Err(e) = stats::refresh_book_fields(&*self.station, now).await {
            tracing::error!("cannot update the station book fields: {e}");
        }
        if let Err(e) = stats::refresh_expansions_if_advanced(&*self.station, now).await {
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
    ) -> Result<(), StoreError> {
        // The orders at the station. An order can show up on two pages when the book moved
        // under the read; the last one wins.
        let at_station: HashMap<i64, &EsiMarketOrder> = orders
            .iter()
            .filter(|o| o.location_id == STATION_ID)
            .map(|o| (o.order_id, o))
            .collect();
        let mut rows: Vec<&EsiMarketOrder> = at_station.into_values().collect();
        rows.sort_unstable_by_key(|o| o.order_id);
        let rows: Vec<StationOrder> = rows
            .into_iter()
            .map(|o| StationOrder {
                order_id: o.order_id,
                type_id: o.type_id,
                is_buy_order: o.is_buy_order,
                price_cents: o.price_cents,
                volume_remain: o.volume_remain,
            })
            .collect();
        // `max(now + interval, ESI's Expires)`, pushed back by a little jitter.
        let earliest = (now + self.interval).max(expires_at.unwrap_or(now));
        self.station
            .replace_station_book(
                REGION_ID,
                STATION_ID,
                now,
                &rows,
                earliest + jitter(self.interval),
                expires_at,
            )
            .await
    }

    /// Records the failure with the other polls' backoff; the old book stays and no snapshot is
    /// recorded.
    async fn record_failure(&self, now: OffsetDateTime, reason: &str) -> RefreshOutcome {
        tracing::warn!("station book refresh failed: {reason}");
        let result = self
            .station
            .record_book_failure(
                REGION_ID,
                STATION_ID,
                now,
                reason,
                BACKOFF_START,
                self.interval,
            )
            .await;
        if let Err(e) = result {
            tracing::error!("cannot record a station book failure: {e}");
        }
        RefreshOutcome::Failed
    }

    /// The gate paused the route: try again when it lets us, without counting a failure.
    async fn record_deferral(&self, now: OffsetDateTime, until: OffsetDateTime) -> RefreshOutcome {
        let result = self
            .station
            .record_book_deferral(REGION_ID, STATION_ID, now, until + jitter(self.interval))
            .await;
        if let Err(e) = result {
            tracing::error!("cannot record a deferred station book refresh: {e}");
        }
        RefreshOutcome::Deferred
    }
}
