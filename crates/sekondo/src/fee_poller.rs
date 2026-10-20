//! The fee poller: keeps every linked character's trade skills, NPC standings and location in step
//! with ESI, and learns who owns the NPC stations where characters have orders. It mirrors the wealth
//! poller but has its own schedule and failure state, so a fee problem never touches the order or
//! wealth polls, and the other way round.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use futures_util::StreamExt;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    clock::Clock,
    esi::{
        EsiCharacterLocation, EsiClient, EsiError, EsiLocation, EsiResponse, EsiSkills, EsiStanding,
    },
    events::{Event, EventBus},
    fees::{self, ACCOUNTING, BROKER_RELATIONS},
    names::{self, STRUCTURE_ID_MIN},
    oauth::LOCATION_SCOPE,
    opportunities::STATION_ID,
    poller::{
        BACKOFF_START, CLAIM_LEASE, PollOutcome, SCHEDULER_TICK, is_foreign_key_violation, jitter,
        spread,
    },
    store::{AccountStore, FeeStore, MarketStore, StandingInput, StoreError, Stores},
    tokens::{TokenError, TokenService},
};

/// How many polls run at the same time.
const DEFAULT_CONCURRENCY: usize = 4;
/// Skills, standings and the location are polled at most this often: ESI caches them for about as long.
pub const FEE_INTERVAL: Duration = Duration::hours(1);
/// How many unknown stations are looked up per tick.
const STATIONS_PER_TICK: i64 = 10;
/// A station lookup that keeps failing is retried at most this rarely.
const STATION_RETRY_MAX: Duration = Duration::hours(1);

/// What is polled per character. Each kind has its own schedule and failure state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Skills,
    Standings,
    /// Needs the optional location scope: characters without it are not polled for it.
    Location,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Skills, Kind::Standings, Kind::Location];

    /// The value in `fee_sync.kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Skills => "skills",
            Kind::Standings => "standings",
            Kind::Location => "location",
        }
    }

    /// The scope a character must have been granted to be polled for this kind, if any.
    pub(crate) fn optional_scope(self) -> Option<&'static str> {
        match self {
            Kind::Location => Some(LOCATION_SCOPE),
            Kind::Skills | Kind::Standings => None,
        }
    }

    pub(crate) fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == raw)
    }
}

pub struct FeePoller {
    fees: Arc<dyn FeeStore>,
    accounts: Arc<dyn AccountStore>,
    market: Arc<dyn MarketStore>,
    tokens: Arc<TokenService>,
    esi: Arc<EsiClient>,
    events: Arc<dyn EventBus>,
    clock: Arc<dyn Clock>,
    concurrency: usize,
    /// Per station whose lookup failed: how many times in a row, and when to try again.
    station_retries: Mutex<HashMap<i64, (u32, OffsetDateTime)>>,
}

impl FeePoller {
    pub fn new(
        stores: &Stores,
        tokens: Arc<TokenService>,
        esi: Arc<EsiClient>,
        events: Arc<dyn EventBus>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            fees: stores.fees.clone(),
            accounts: stores.accounts.clone(),
            market: stores.market.clone(),
            tokens,
            esi,
            events,
            clock,
            concurrency: DEFAULT_CONCURRENCY,
            station_retries: Mutex::new(HashMap::new()),
        }
    }

    /// Polls one kind of one character now, whatever its schedule says. Never panics or errors:
    /// every way a poll can end is a [`PollOutcome`], and the database records the details.
    pub async fn poll(&self, character_id: i64, kind: Kind) -> PollOutcome {
        let now = self.clock.now();
        if let Some(scope) = kind.optional_scope() {
            match self.fees.has_scope(character_id, scope).await {
                Ok(Some(true)) => {}
                Ok(Some(false)) => return PollOutcome::Skipped,
                Ok(None) => return PollOutcome::Dropped,
                Err(e) => {
                    return self
                        .record_failure(character_id, kind, now, &format!("database error: {e}"))
                        .await;
                }
            }
        }
        let token = match self.tokens.access_token(character_id).await {
            Ok(token) => token,
            Err(TokenError::NeedsRelogin) => {
                // Make it due again, so it is polled right after it logged in.
                let _ = self.fees.make_due(character_id, kind, now).await;
                return PollOutcome::NeedsRelogin;
            }
            Err(TokenError::UnknownCharacter) => return PollOutcome::Dropped,
            Err(TokenError::Temporary(reason)) => {
                return self.record_failure(character_id, kind, now, &reason).await;
            }
        };
        let stored = match kind {
            Kind::Skills => match self.esi.skills(character_id, &token).await {
                Ok(response) => self.store_skills(character_id, now, &response).await,
                Err(e) => return self.esi_failed(character_id, kind, now, e).await,
            },
            Kind::Standings => match self.esi.standings(character_id, &token).await {
                Ok(response) => self.store_standings(character_id, now, &response).await,
                Err(e) => return self.esi_failed(character_id, kind, now, e).await,
            },
            Kind::Location => match self.esi.character_location(character_id, &token).await {
                Ok(response) => {
                    // Named like order locations, so the fee cards can say where they are.
                    if let Some(docked) = response.body.docked() {
                        let id = match docked {
                            EsiLocation::Station(id) | EsiLocation::Structure(id) => id,
                        };
                        names::resolve(&*self.market, &self.esi, &token, [id], now).await;
                    }
                    self.store_location(character_id, now, &response).await
                }
                Err(e) => return self.esi_failed(character_id, kind, now, e).await,
            },
        };
        match stored {
            Ok(changed) => {
                if changed {
                    self.publish_for_character(character_id).await;
                }
                PollOutcome::Updated
            }
            Err(e) => self.store_failed(character_id, kind, now, e).await,
        }
    }

    /// Tells the account's open pages that a fee of one of its characters changed.
    fn publish(&self, account: Uuid) {
        self.events.publish(account, Event::FeesUpdated);
    }

    async fn publish_for_character(&self, character_id: i64) {
        match self.accounts.find_character(character_id).await {
            Ok(Some(character)) => self.publish(character.account_id),
            Ok(None) => {}
            Err(e) => tracing::error!(character_id, "cannot look up the account to signal: {e}"),
        }
    }

    async fn esi_failed(
        &self,
        character_id: i64,
        kind: Kind,
        now: OffsetDateTime,
        error: EsiError,
    ) -> PollOutcome {
        match error {
            EsiError::Deferred { until } => {
                self.record_deferral(character_id, kind, now, until).await
            }
            e => {
                self.record_failure(character_id, kind, now, &e.to_string())
                    .await
            }
        }
    }

    async fn store_failed(
        &self,
        character_id: i64,
        kind: Kind,
        now: OffsetDateTime,
        error: StoreError,
    ) -> PollOutcome {
        if is_foreign_key_violation(&error) {
            tracing::info!(character_id, "character was unlinked during its poll");
            return PollOutcome::Dropped;
        }
        self.record_failure(character_id, kind, now, &format!("database error: {error}"))
            .await
    }

    /// `max(polled_at + interval, ESI's Expires)`, pushed back by a little jitter.
    fn next_poll_at(
        polled_at: OffsetDateTime,
        expires_at: Option<OffsetDateTime>,
    ) -> OffsetDateTime {
        (polled_at + FEE_INTERVAL).max(expires_at.unwrap_or(polled_at)) + jitter(FEE_INTERVAL)
    }

    /// Replaces the stored skills with the two that matter, in one transaction with the sync
    /// state. Answers whether what the page shows changed.
    async fn store_skills(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        response: &EsiResponse<EsiSkills>,
    ) -> Result<bool, StoreError> {
        let (ids, levels): (Vec<i64>, Vec<i16>) = response
            .body
            .skills
            .iter()
            .filter(|s| matches!(s.skill_id, ACCOUNTING | BROKER_RELATIONS))
            .map(|s| (s.skill_id, s.active_skill_level))
            .unzip();
        self.fees
            .store_skills(
                character_id,
                &ids,
                &levels,
                polled_at,
                Self::next_poll_at(polled_at, response.expires_at),
                response.expires_at,
            )
            .await
    }

    /// Replaces the stored standings with the NPC corporation and faction ones (agents are
    /// dropped), in one transaction with the sync state. Answers whether what the page shows
    /// changed.
    async fn store_standings(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        response: &EsiResponse<Vec<EsiStanding>>,
    ) -> Result<bool, StoreError> {
        let kept: Vec<StandingInput> = response
            .body
            .iter()
            .filter(|s| matches!(s.from_type.as_str(), "npc_corp" | "faction"))
            .map(|s| StandingInput {
                from_id: s.from_id,
                from_type: s.from_type.clone(),
                standing: s.standing.clone(),
            })
            .collect();
        self.fees
            .store_standings(
                character_id,
                &kept,
                polled_at,
                Self::next_poll_at(polled_at, response.expires_at),
                response.expires_at,
            )
            .await
    }

    /// Remembers the station or structure the character is docked in. In open space the last
    /// one stays. Answers whether what the page shows changed.
    async fn store_location(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        response: &EsiResponse<EsiCharacterLocation>,
    ) -> Result<bool, StoreError> {
        let docked = response.body.docked().map(|docked| match docked {
            EsiLocation::Station(id) => (id, "station"),
            EsiLocation::Structure(id) => (id, "structure"),
        });
        self.fees
            .store_location(
                character_id,
                docked,
                polled_at,
                Self::next_poll_at(polled_at, response.expires_at),
                response.expires_at,
            )
            .await
    }

    async fn record_failure(
        &self,
        character_id: i64,
        kind: Kind,
        now: OffsetDateTime,
        reason: &str,
    ) -> PollOutcome {
        tracing::warn!(
            character_id,
            kind = kind.as_str(),
            "fee poll failed: {reason}"
        );
        let result = self
            .fees
            .record_failure(character_id, kind, now, reason, BACKOFF_START, FEE_INTERVAL)
            .await;
        match result {
            Ok(_) => PollOutcome::Failed,
            Err(e) if is_foreign_key_violation(&e) => PollOutcome::Dropped,
            Err(e) => {
                tracing::error!(character_id, "cannot record a fee poll failure: {e}");
                PollOutcome::Failed
            }
        }
    }

    async fn record_deferral(
        &self,
        character_id: i64,
        kind: Kind,
        now: OffsetDateTime,
        until: OffsetDateTime,
    ) -> PollOutcome {
        let result = self
            .fees
            .record_deferral(character_id, kind, now, until + jitter(FEE_INTERVAL))
            .await;
        match result {
            Err(e) if is_foreign_key_violation(&e) => PollOutcome::Dropped,
            Err(e) => {
                tracing::error!(character_id, "cannot record a deferred fee poll: {e}");
                PollOutcome::Deferred
            }
            Ok(_) => PollOutcome::Deferred,
        }
    }
}

// ---- station owners ----

impl FeePoller {
    /// Looks up the owner of up to [`STATIONS_PER_TICK`] NPC stations where a current order is
    /// placed, a sale was made or a character is docked, and whose owner is not known yet, one ESI call per station
    /// whoever's they are. An account whose fees change because of an owner gets `fees-updated`. A lookup that
    /// fails is retried on a later tick, later each time. Answers how many owners were learned.
    pub async fn resolve_stations(&self) -> usize {
        let now = self.clock.now();
        let unknown: Vec<i64> = match self.fees.stations_without_owner(STRUCTURE_ID_MIN).await {
            Ok(unknown) => unknown,
            Err(e) => {
                tracing::error!("cannot list the stations without an owner: {e}");
                return 0;
            }
        };
        let due: Vec<i64> = {
            let retries = self.station_retries.lock().unwrap();
            unknown
                .into_iter()
                .filter(|id| retries.get(id).is_none_or(|(_, after)| *after <= now))
                .take(STATIONS_PER_TICK as usize)
                .collect()
        };
        let mut learned = 0;
        for station_id in due {
            match self.esi.station(station_id).await {
                Ok(station) => match self.store_owner(station_id, station.owner, now).await {
                    Ok(()) => {
                        self.station_retries.lock().unwrap().remove(&station_id);
                        learned += 1;
                    }
                    Err(e) => self.station_failed(station_id, now, &e.to_string()),
                },
                Err(EsiError::Deferred { until }) => {
                    let failures = self
                        .station_retries
                        .lock()
                        .unwrap()
                        .get(&station_id)
                        .map_or(0, |(f, _)| *f);
                    self.station_retries
                        .lock()
                        .unwrap()
                        .insert(station_id, (failures, until));
                }
                Err(e) => self.station_failed(station_id, now, &e.to_string()),
            }
        }
        learned
    }

    /// Learns the owner of the built-in trading station even when no character has an order
    /// there. A character's fees at the station then follow from its skills and standings, so
    /// every account is told once.
    async fn ensure_built_in_owner(&self, now: OffsetDateTime) {
        let retry_after = self
            .station_retries
            .lock()
            .unwrap()
            .get(&STATION_ID)
            .map(|(_, after)| *after);
        if retry_after.is_some_and(|after| after > now) {
            return;
        }
        match fees::ensure_station_owner(&*self.fees, &self.esi, STATION_ID, now).await {
            Ok(learned) => {
                self.station_retries.lock().unwrap().remove(&STATION_ID);
                if learned {
                    match self.accounts.account_ids().await {
                        Ok(accounts) => accounts.into_iter().for_each(|a| self.publish(a)),
                        Err(e) => tracing::error!("cannot list the accounts to signal: {e}"),
                    }
                }
            }
            Err(EsiError::Deferred { until }) => {
                let mut retries = self.station_retries.lock().unwrap();
                let failures = retries.get(&STATION_ID).map_or(0, |(f, _)| *f);
                retries.insert(STATION_ID, (failures, until));
            }
            Err(e) => self.station_failed(STATION_ID, now, &e.to_string()),
        }
    }

    fn station_failed(&self, station_id: i64, now: OffsetDateTime, reason: &str) {
        let mut retries = self.station_retries.lock().unwrap();
        let failures = retries.get(&station_id).map_or(0, |(f, _)| *f) + 1;
        let wait = (BACKOFF_START * 2_i32.pow(failures.min(10) - 1)).min(STATION_RETRY_MAX);
        tracing::warn!(station_id, "station owner lookup failed: {reason}");
        retries.insert(station_id, (failures, now + wait));
    }

    /// Stores the owner and signals every account whose characters' fees at the station changed
    /// because of it, in one transaction.
    async fn store_owner(
        &self,
        station_id: i64,
        owner_id: i64,
        now: OffsetDateTime,
    ) -> Result<(), StoreError> {
        let accounts = self
            .fees
            .store_station_owner(station_id, owner_id, now)
            .await?;
        for account in accounts {
            self.publish(account);
        }
        Ok(())
    }
}

// ---- the scheduler ----

impl FeePoller {
    /// Runs [`FeePoller::tick`] every [`SCHEDULER_TICK`] until the task is aborted.
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

    /// Schedules what has no schedule yet, polls everything due (a few at a time) and looks up
    /// station owners. Answers how many polls ran.
    pub async fn tick(&self) -> usize {
        let now = self.clock.now();
        if let Err(e) = self.schedule_new(now).await {
            tracing::error!("cannot schedule fee polls: {e}");
        }
        let due = match self.claim_due(now).await {
            Ok(due) => due,
            Err(e) => {
                tracing::error!("cannot claim due fee polls: {e}");
                Vec::new()
            }
        };
        let polled = due.len();
        futures_util::stream::iter(due)
            .for_each_concurrent(self.concurrency, |(character_id, kind)| async move {
                self.poll(character_id, kind).await;
            })
            .await;
        self.resolve_stations().await;
        self.ensure_built_in_owner(now).await;
        polled
    }

    /// Polls both kinds of every character of the account now and looks up the owners of its
    /// stations, whatever the schedule says. For the browser tests.
    #[cfg(feature = "test-hooks")]
    pub async fn refresh_account(&self, account_id: Uuid) -> Result<(), StoreError> {
        let characters: Vec<i64> = self
            .accounts
            .list_characters(account_id)
            .await?
            .into_iter()
            .map(|c| c.character_id)
            .collect();
        for character_id in characters {
            for kind in Kind::ALL {
                self.poll(character_id, kind).await;
            }
        }
        self.station_retries.lock().unwrap().clear();
        self.resolve_stations().await;
        self.ensure_built_in_owner(self.clock.now()).await;
        Ok(())
    }

    /// Gives every character and kind without a schedule a first poll time inside the next
    /// interval, spread by the character and the kind.
    async fn schedule_new(&self, now: OffsetDateTime) -> Result<(), StoreError> {
        for kind in Kind::ALL {
            let unscheduled = self
                .fees
                .list_unscheduled(kind, kind.optional_scope())
                .await?;
            if unscheduled.is_empty() {
                continue;
            }
            let times: Vec<OffsetDateTime> = unscheduled
                .iter()
                .map(|id| {
                    let mut key = id.to_be_bytes().to_vec();
                    key.extend_from_slice(kind.as_str().as_bytes());
                    now + spread(&key, FEE_INTERVAL)
                })
                .collect();
            self.fees
                .insert_schedule(kind, &unscheduled, &times)
                .await?;
        }
        Ok(())
    }

    /// Takes the due pairs of characters that do not need a re-login. They are pushed back by a
    /// lease, so neither this nor another tick takes them again while their poll runs.
    async fn claim_due(&self, now: OffsetDateTime) -> Result<Vec<(i64, Kind)>, StoreError> {
        self.fees
            .claim_due(now, CLAIM_LEASE, (self.concurrency * 8) as i64)
            .await
    }
}
