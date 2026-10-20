//! The background poller: keeps every linked character's current orders in step with ESI,
//! records a snapshot of each poll, and schedules the next one.

use std::sync::Arc;

use futures_util::StreamExt;
use secrecy::SecretString;
use time::{Duration, OffsetDateTime};

use crate::{
    clock::Clock,
    esi::{EsiClient, EsiError, EsiOrder, EsiResponse},
    events::{Event, EventBus},
    names,
    store::{AccountStore, MarketStore, OrderStore, StoreError, Stores},
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
    /// Nothing to poll: the character lacks an optional scope this poll needs.
    Skipped,
}

pub struct Poller {
    orders: Arc<dyn OrderStore>,
    accounts: Arc<dyn AccountStore>,
    market: Arc<dyn MarketStore>,
    tokens: Arc<TokenService>,
    esi: Arc<EsiClient>,
    events: Arc<dyn EventBus>,
    clock: Arc<dyn Clock>,
    interval: Duration,
    concurrency: usize,
}

pub(crate) fn is_foreign_key_violation(e: &StoreError) -> bool {
    matches!(e, StoreError::MissingParent)
}

impl Poller {
    pub fn new(
        stores: &Stores,
        tokens: Arc<TokenService>,
        esi: Arc<EsiClient>,
        events: Arc<dyn EventBus>,
        clock: Arc<dyn Clock>,
        interval: Duration,
    ) -> Self {
        Self {
            orders: stores.orders.clone(),
            accounts: stores.accounts.clone(),
            market: stores.market.clone(),
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
        match self.accounts.find_character(character_id).await {
            Ok(Some(character)) => self
                .events
                .publish(character.account_id, Event::OrdersUpdated),
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
                let _ = self.orders.make_due(character_id, now).await;
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

    /// Replaces the character's current orders, appends the snapshot and updates the sync state.
    async fn store(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        response: &EsiResponse<Vec<EsiOrder>>,
    ) -> Result<(), StoreError> {
        let next_poll_at = self.next_poll_at(polled_at, response.expires_at);
        self.orders
            .replace_orders(
                character_id,
                polled_at,
                &response.body,
                next_poll_at,
                response.expires_at,
            )
            .await
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
        let result = self
            .orders
            .record_failure(character_id, now, reason, BACKOFF_START, self.interval)
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
        let result = self
            .orders
            .record_deferral(character_id, now, until + self.jitter())
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
            &*self.market,
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
    async fn schedule_new(&self, now: OffsetDateTime) -> Result<(), StoreError> {
        let unscheduled = self.orders.list_unscheduled().await?;
        if unscheduled.is_empty() {
            return Ok(());
        }
        let times: Vec<OffsetDateTime> = unscheduled
            .iter()
            .map(|id| now + first_offset(*id, self.interval))
            .collect();
        self.orders.insert_schedule(&unscheduled, &times).await
    }

    /// Takes the due characters that do not need a re-login. They are pushed back by a lease, so
    /// neither this nor another tick takes them again while their poll runs.
    async fn claim_due(&self, now: OffsetDateTime) -> Result<Vec<i64>, StoreError> {
        self.orders
            .claim_due(now, CLAIM_LEASE, (self.concurrency * 8) as i64)
            .await
    }
}
