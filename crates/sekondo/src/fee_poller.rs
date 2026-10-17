//! The fee poller: keeps every linked character's trade skills and NPC standings in step with
//! ESI, and learns who owns the NPC stations where characters have orders. It mirrors the wealth
//! poller but has its own schedule and failure state, so a fee problem never touches the order or
//! wealth polls, and the other way round.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use futures_util::StreamExt;
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    clock::Clock,
    esi::{EsiClient, EsiError, EsiResponse, EsiSkills, EsiStanding},
    events::{Event, EventBus},
    fees::{self, ACCOUNTING, BROKER_RELATIONS},
    names::STRUCTURE_ID_MIN,
    poller::{
        BACKOFF_START, CLAIM_LEASE, PollOutcome, SCHEDULER_TICK, is_foreign_key_violation, jitter,
        spread,
    },
    tokens::{TokenError, TokenService},
};

/// How many polls run at the same time.
const DEFAULT_CONCURRENCY: usize = 4;
/// Skills and standings are polled at most this often: ESI caches both for about as long.
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
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Skills, Kind::Standings];

    /// The value in `fee_sync.kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Skills => "skills",
            Kind::Standings => "standings",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == raw)
    }
}

pub struct FeePoller {
    pool: PgPool,
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
        pool: PgPool,
        tokens: Arc<TokenService>,
        esi: Arc<EsiClient>,
        events: Arc<dyn EventBus>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            pool,
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
        let token = match self.tokens.access_token(character_id).await {
            Ok(token) => token,
            Err(TokenError::NeedsRelogin) => {
                // Make it due again, so it is polled right after it logged in.
                let _ = sqlx::query(
                    "UPDATE fee_sync SET next_poll_at = LEAST(next_poll_at, $3)
                     WHERE character_id = $1 AND kind = $2",
                )
                .bind(character_id)
                .bind(kind.as_str())
                .bind(now)
                .execute(&self.pool)
                .await;
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
        match sqlx::query_scalar::<_, Uuid>(
            "SELECT account_id FROM characters WHERE character_id = $1",
        )
        .bind(character_id)
        .fetch_optional(&self.pool)
        .await
        {
            Ok(Some(account)) => self.publish(account),
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
        error: sqlx::Error,
    ) -> PollOutcome {
        if is_foreign_key_violation(&error) {
            tracing::info!(character_id, "character was unlinked during its poll");
            return PollOutcome::Dropped;
        }
        self.record_failure(character_id, kind, now, &format!("database error: {error}"))
            .await
    }

    /// Replaces the stored skills with the two that matter, in one transaction with the sync
    /// state. Answers whether what the page shows changed.
    async fn store_skills(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        response: &EsiResponse<EsiSkills>,
    ) -> Result<bool, sqlx::Error> {
        let (ids, levels): (Vec<i64>, Vec<i16>) = response
            .body
            .skills
            .iter()
            .filter(|s| matches!(s.skill_id, ACCOUNTING | BROKER_RELATIONS))
            .map(|s| (s.skill_id, s.active_skill_level))
            .unzip();
        let mut tx = self.pool.begin().await?;
        let before = fees::for_characters(&mut *tx, &[character_id]).await?;
        sqlx::query("DELETE FROM character_skills WHERE character_id = $1")
            .bind(character_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO character_skills (character_id, skill_id, active_level)
             SELECT $1, s, l FROM unnest($2::bigint[], $3::smallint[]) AS u(s, l)",
        )
        .bind(character_id)
        .bind(&ids)
        .bind(&levels)
        .execute(&mut *tx)
        .await?;
        self.record_success(
            &mut tx,
            character_id,
            Kind::Skills,
            polled_at,
            response.expires_at,
        )
        .await?;
        let after = fees::for_characters(&mut *tx, &[character_id]).await?;
        tx.commit().await?;
        Ok(before != after)
    }

    /// Replaces the stored standings with the NPC corporation and faction ones (agents are
    /// dropped), in one transaction with the sync state. Answers whether what the page shows
    /// changed.
    async fn store_standings(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        response: &EsiResponse<Vec<EsiStanding>>,
    ) -> Result<bool, sqlx::Error> {
        let kept: Vec<&EsiStanding> = response
            .body
            .iter()
            .filter(|s| matches!(s.from_type.as_str(), "npc_corp" | "faction"))
            .collect();
        let from_id: Vec<i64> = kept.iter().map(|s| s.from_id).collect();
        let from_type: Vec<&str> = kept.iter().map(|s| s.from_type.as_str()).collect();
        let standing: Vec<&str> = kept.iter().map(|s| s.standing.as_str()).collect();
        let mut tx = self.pool.begin().await?;
        let before = fees::for_characters(&mut *tx, &[character_id]).await?;
        sqlx::query("DELETE FROM character_standings WHERE character_id = $1")
            .bind(character_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO character_standings (character_id, from_id, from_type, standing)
             SELECT $1, i, t, s::numeric
             FROM unnest($2::bigint[], $3::text[], $4::text[]) AS u(i, t, s)
             ON CONFLICT DO NOTHING",
        )
        .bind(character_id)
        .bind(&from_id)
        .bind(&from_type)
        .bind(&standing)
        .execute(&mut *tx)
        .await?;
        self.record_success(
            &mut tx,
            character_id,
            Kind::Standings,
            polled_at,
            response.expires_at,
        )
        .await?;
        let after = fees::for_characters(&mut *tx, &[character_id]).await?;
        tx.commit().await?;
        Ok(before != after)
    }

    async fn record_success(
        &self,
        tx: &mut sqlx::PgConnection,
        character_id: i64,
        kind: Kind,
        polled_at: OffsetDateTime,
        expires_at: Option<OffsetDateTime>,
    ) -> Result<(), sqlx::Error> {
        let next_poll_at =
            (polled_at + FEE_INTERVAL).max(expires_at.unwrap_or(polled_at)) + jitter(FEE_INTERVAL);
        sqlx::query(
            "INSERT INTO fee_sync
                 (character_id, kind, next_poll_at, last_success_at, last_attempt_at, last_error,
                  esi_expires_at, failures)
             VALUES ($1, $2, $3, $4, $4, NULL, $5, 0)
             ON CONFLICT (character_id, kind) DO UPDATE SET
                 next_poll_at = excluded.next_poll_at,
                 last_success_at = excluded.last_success_at,
                 last_attempt_at = excluded.last_attempt_at,
                 last_error = NULL,
                 esi_expires_at = excluded.esi_expires_at,
                 failures = 0",
        )
        .bind(character_id)
        .bind(kind.as_str())
        .bind(next_poll_at)
        .bind(polled_at)
        .bind(expires_at)
        .execute(tx)
        .await?;
        Ok(())
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
        let result = sqlx::query(
            "INSERT INTO fee_sync (character_id, kind, next_poll_at, last_attempt_at, last_error, failures)
             VALUES ($1, $2, $3, $4, $5, 1)
             ON CONFLICT (character_id, kind) DO UPDATE SET
                 next_poll_at = $4 + LEAST(
                     $6::float8 * power(2, LEAST(fee_sync.failures, 20)) * interval '1 second',
                     $7::float8 * interval '1 second'),
                 last_attempt_at = $4,
                 last_error = $5,
                 failures = fee_sync.failures + 1",
        )
        .bind(character_id)
        .bind(kind.as_str())
        .bind(now + BACKOFF_START)
        .bind(now)
        .bind(reason)
        .bind(BACKOFF_START.as_seconds_f64())
        .bind(FEE_INTERVAL.as_seconds_f64())
        .execute(&self.pool)
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
        let result = sqlx::query(
            "INSERT INTO fee_sync (character_id, kind, next_poll_at, last_attempt_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (character_id, kind) DO UPDATE SET
                 next_poll_at = excluded.next_poll_at,
                 last_attempt_at = excluded.last_attempt_at",
        )
        .bind(character_id)
        .bind(kind.as_str())
        .bind(until + jitter(FEE_INTERVAL))
        .bind(now)
        .execute(&self.pool)
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
    /// placed and whose owner is not known yet, one ESI call per station whoever's orders they
    /// are. An account whose fees change because of an owner gets `fees-updated`. A lookup that
    /// fails is retried on a later tick, later each time. Answers how many owners were learned.
    pub async fn resolve_stations(&self) -> usize {
        let now = self.clock.now();
        let unknown: Vec<i64> = match sqlx::query_scalar(
            "SELECT DISTINCT o.location_id FROM current_orders o
             WHERE o.location_id < $1
               AND NOT EXISTS (SELECT 1 FROM station_owners s WHERE s.station_id = o.location_id)
             ORDER BY o.location_id",
        )
        .bind(STRUCTURE_ID_MIN)
        .fetch_all(&self.pool)
        .await
        {
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
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        let characters: Vec<(i64, Uuid)> = sqlx::query_as(
            "SELECT DISTINCT o.character_id, c.account_id FROM current_orders o
             JOIN characters c ON c.character_id = o.character_id
             WHERE o.location_id = $1",
        )
        .bind(station_id)
        .fetch_all(&mut *tx)
        .await?;
        let ids: Vec<i64> = characters.iter().map(|(id, _)| *id).collect();
        let before = fees::for_characters(&mut *tx, &ids).await?;
        sqlx::query(
            "INSERT INTO station_owners (station_id, owner_id, resolved_at) VALUES ($1, $2, $3)
             ON CONFLICT (station_id) DO NOTHING",
        )
        .bind(station_id)
        .bind(owner_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        let after = fees::for_characters(&mut *tx, &ids).await?;
        tx.commit().await?;

        let accounts: HashSet<Uuid> = characters
            .iter()
            .filter(|(id, _)| {
                let of = |rows: &[fees::FeeRow]| {
                    rows.iter()
                        .filter(|r| r.character_id == *id)
                        .cloned()
                        .collect::<Vec<_>>()
                };
                of(&before) != of(&after)
            })
            .map(|(_, account)| *account)
            .collect();
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
        polled
    }

    /// Polls both kinds of every character of the account now and looks up the owners of its
    /// stations, whatever the schedule says. For the browser tests.
    #[cfg(feature = "test-hooks")]
    pub async fn refresh_account(&self, account_id: Uuid) -> Result<(), sqlx::Error> {
        let characters: Vec<i64> =
            sqlx::query_scalar("SELECT character_id FROM characters WHERE account_id = $1")
                .bind(account_id)
                .fetch_all(&self.pool)
                .await?;
        for character_id in characters {
            for kind in Kind::ALL {
                self.poll(character_id, kind).await;
            }
        }
        self.station_retries.lock().unwrap().clear();
        self.resolve_stations().await;
        Ok(())
    }

    /// Gives every character and kind without a schedule a first poll time inside the next
    /// interval, spread by the character and the kind.
    async fn schedule_new(&self, now: OffsetDateTime) -> Result<(), sqlx::Error> {
        for kind in Kind::ALL {
            let unscheduled: Vec<i64> = sqlx::query_scalar(
                "SELECT c.character_id FROM characters c
                 WHERE NOT c.needs_relogin
                   AND NOT EXISTS (SELECT 1 FROM fee_sync s
                                   WHERE s.character_id = c.character_id AND s.kind = $1)",
            )
            .bind(kind.as_str())
            .fetch_all(&self.pool)
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
            sqlx::query(
                "INSERT INTO fee_sync (character_id, kind, next_poll_at)
                 SELECT c, $1, t FROM unnest($2::bigint[], $3::timestamptz[]) AS u(c, t)
                 ON CONFLICT (character_id, kind) DO NOTHING",
            )
            .bind(kind.as_str())
            .bind(&unscheduled)
            .bind(&times)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    /// Takes the due pairs of characters that do not need a re-login. They are pushed back by a
    /// lease, so neither this nor another tick takes them again while their poll runs.
    async fn claim_due(&self, now: OffsetDateTime) -> Result<Vec<(i64, Kind)>, sqlx::Error> {
        let claimed: Vec<(i64, String)> = sqlx::query_as(
            "UPDATE fee_sync w SET next_poll_at = $1 + $2::float8 * interval '1 second'
             WHERE (w.character_id, w.kind) IN (
                 SELECT s.character_id, s.kind FROM fee_sync s
                 JOIN characters c USING (character_id)
                 WHERE NOT c.needs_relogin AND s.next_poll_at <= $1
                 ORDER BY s.next_poll_at
                 LIMIT $3
                 FOR UPDATE OF s SKIP LOCKED)
             RETURNING w.character_id, w.kind",
        )
        .bind(now)
        .bind(CLAIM_LEASE.as_seconds_f64())
        .bind((self.concurrency * 8) as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(claimed
            .into_iter()
            .filter_map(|(id, kind)| Kind::parse(&kind).map(|kind| (id, kind)))
            .collect())
    }
}
