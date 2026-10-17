//! The wealth poller: keeps every linked character's wallet balance (and, later, its assets) in
//! step with ESI. It mirrors the order poller but has a schedule and a failure state of its own,
//! so a wallet or assets problem never touches the orders, and the other way round.

use std::{collections::HashMap, sync::Arc};

use futures_util::StreamExt;
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};

use crate::{
    clock::Clock,
    esi::{EsiAsset, EsiClient, EsiError, EsiResponse},
    events::{Event, EventBus},
    names::{self, STRUCTURE_ID_MIN},
    poller::{
        BACKOFF_START, CLAIM_LEASE, PollOutcome, SCHEDULER_TICK, is_foreign_key_violation, jitter,
        spread,
    },
    tokens::{TokenError, TokenService},
};

/// How many polls run at the same time.
const DEFAULT_CONCURRENCY: usize = 4;
/// ESI caches the assets for an hour, and they are heavy: never poll them more often.
pub const ASSETS_INTERVAL: Duration = Duration::hours(1);
/// How many containers deep an asset is followed before its location counts as unknown. Guards
/// against cycles in the answer.
const MAX_NESTING: usize = 16;

/// What is polled per character. Each kind has its own schedule and failure state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Wallet,
    Assets,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Wallet, Kind::Assets];

    /// The value in `wealth_sync.kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Wallet => "wallet",
            Kind::Assets => "assets",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == raw)
    }

    /// How long after a poll the next one is earliest due.
    fn interval(self, order_interval: Duration) -> Duration {
        match self {
            Kind::Wallet => order_interval,
            Kind::Assets => ASSETS_INTERVAL,
        }
    }
}

pub struct WealthPoller {
    pool: PgPool,
    tokens: Arc<TokenService>,
    esi: Arc<EsiClient>,
    events: Arc<dyn EventBus>,
    clock: Arc<dyn Clock>,
    /// The order poll's interval: the wallet follows its cadence.
    interval: Duration,
    concurrency: usize,
}

impl WealthPoller {
    pub fn new(
        pool: PgPool,
        tokens: Arc<TokenService>,
        esi: Arc<EsiClient>,
        events: Arc<dyn EventBus>,
        clock: Arc<dyn Clock>,
        interval: Duration,
    ) -> Self {
        Self {
            pool,
            tokens,
            esi,
            events,
            clock,
            interval,
            concurrency: DEFAULT_CONCURRENCY,
        }
    }

    /// Polls one kind of one character now, whatever its schedule says. Never panics or errors:
    /// every way a poll can end is a [`PollOutcome`], and the database records the details.
    pub async fn poll(&self, character_id: i64, kind: Kind) -> PollOutcome {
        let outcome = self.run_poll(character_id, kind).await;
        if outcome == PollOutcome::Updated {
            self.publish(character_id).await;
        }
        outcome
    }

    /// Tells the character's account's open pages that its wealth changed.
    async fn publish(&self, character_id: i64) {
        match sqlx::query_scalar::<_, uuid::Uuid>(
            "SELECT account_id FROM characters WHERE character_id = $1",
        )
        .bind(character_id)
        .fetch_optional(&self.pool)
        .await
        {
            Ok(Some(account)) => self.events.publish(account, Event::WealthUpdated),
            Ok(None) => {}
            Err(e) => tracing::error!(character_id, "cannot look up the account to signal: {e}"),
        }
    }

    async fn run_poll(&self, character_id: i64, kind: Kind) -> PollOutcome {
        let now = self.clock.now();
        let token = match self.tokens.access_token(character_id).await {
            Ok(token) => token,
            Err(TokenError::NeedsRelogin) => {
                // Make it due again, so it is polled right after it logged in.
                let _ = sqlx::query(
                    "UPDATE wealth_sync SET next_poll_at = LEAST(next_poll_at, $3)
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
        match kind {
            Kind::Wallet => match self.esi.wallet(character_id, &token).await {
                Ok(response) => match self.store_wallet(character_id, now, &response).await {
                    Ok(()) => PollOutcome::Updated,
                    Err(e) => self.store_failed(character_id, kind, now, e).await,
                },
                Err(e) => self.esi_failed(character_id, kind, now, e).await,
            },
            Kind::Assets => match self.esi.assets(character_id, &token).await {
                Ok(response) => match self.store_assets(character_id, now, &response).await {
                    Ok(()) => {
                        // After the assets are safe: a name that cannot be resolved never fails
                        // a poll. Structures are named with the owner's token.
                        let tops = top_locations(&response.body);
                        let wanted: Vec<i64> = response
                            .body
                            .iter()
                            .flat_map(|a| [Some(a.type_id), tops[&a.item_id]])
                            .flatten()
                            .collect();
                        names::resolve(&self.pool, &self.esi, &token, wanted, now).await;
                        PollOutcome::Updated
                    }
                    Err(e) => self.store_failed(character_id, kind, now, e).await,
                },
                Err(e) => self.esi_failed(character_id, kind, now, e).await,
            },
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

    /// Appends the balance and updates the sync state, in one transaction. Two polls in the same
    /// instant keep the later one rather than failing.
    async fn store_wallet(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        response: &EsiResponse<i64>,
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO wallet_balances (character_id, polled_at, balance)
             VALUES ($1, $2, $3::numeric / 100)
             ON CONFLICT (character_id, polled_at) DO UPDATE SET balance = excluded.balance",
        )
        .bind(character_id)
        .bind(polled_at)
        .bind(response.body)
        .execute(&mut *tx)
        .await?;
        self.record_success(
            &mut tx,
            character_id,
            Kind::Wallet,
            polled_at,
            response.expires_at,
        )
        .await?;
        tx.commit().await
    }

    /// Replaces the character's assets with what ESI returned, records the inventory value from
    /// the rows just written and updates the sync state, in one transaction.
    async fn store_assets(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        response: &EsiResponse<Vec<EsiAsset>>,
    ) -> Result<(), sqlx::Error> {
        let tops = top_locations(&response.body);
        let item_id: Vec<i64> = response.body.iter().map(|a| a.item_id).collect();
        let type_id: Vec<i64> = response.body.iter().map(|a| a.type_id).collect();
        let location_id: Vec<i64> = response.body.iter().map(|a| a.location_id).collect();
        let location_type: Vec<&str> = response
            .body
            .iter()
            .map(|a| a.location_type.as_str())
            .collect();
        let location_flag: Vec<&str> = response
            .body
            .iter()
            .map(|a| a.location_flag.as_str())
            .collect();
        let quantity: Vec<i64> = response.body.iter().map(|a| a.quantity).collect();
        let singleton: Vec<bool> = response.body.iter().map(|a| a.is_singleton).collect();
        let copy: Vec<bool> = response.body.iter().map(|a| a.is_blueprint_copy).collect();
        let top: Vec<Option<i64>> = response.body.iter().map(|a| tops[&a.item_id]).collect();

        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM current_assets WHERE character_id = $1")
            .bind(character_id)
            .execute(&mut *tx)
            .await?;
        // An item that changed hands may still be on its previous owner's list: it moves.
        sqlx::query(
            "INSERT INTO current_assets
                 (item_id, character_id, type_id, location_id, location_type, location_flag,
                  quantity, is_singleton, is_blueprint_copy, top_location_id)
             SELECT a.item_id, $1, a.type_id, a.location_id, a.location_type, a.location_flag,
                    a.quantity, a.is_singleton, a.is_blueprint_copy, a.top_location_id
             FROM unnest($2::bigint[], $3::bigint[], $4::bigint[], $5::text[], $6::text[],
                         $7::bigint[], $8::bool[], $9::bool[], $10::bigint[])
                  AS a(item_id, type_id, location_id, location_type, location_flag, quantity,
                       is_singleton, is_blueprint_copy, top_location_id)
             ON CONFLICT (item_id) DO UPDATE SET
                 character_id = excluded.character_id,
                 type_id = excluded.type_id,
                 location_id = excluded.location_id,
                 location_type = excluded.location_type,
                 location_flag = excluded.location_flag,
                 quantity = excluded.quantity,
                 is_singleton = excluded.is_singleton,
                 is_blueprint_copy = excluded.is_blueprint_copy,
                 top_location_id = excluded.top_location_id",
        )
        .bind(character_id)
        .bind(&item_id)
        .bind(&type_id)
        .bind(&location_id)
        .bind(&location_type)
        .bind(&location_flag)
        .bind(&quantity)
        .bind(&singleton)
        .bind(&copy)
        .bind(&top)
        .execute(&mut *tx)
        .await?;
        // Two entries in the same instant keep the later one.
        let entry = format!(
            "INSERT INTO inventory_values (character_id, recorded_at, value)
             SELECT $1, $2, {INVENTORY_VALUE_SUM}
             FROM current_assets a LEFT JOIN item_prices p USING (type_id)
             WHERE a.character_id = $1
             ON CONFLICT (character_id, recorded_at) DO UPDATE SET value = excluded.value"
        );
        sqlx::query(sqlx::AssertSqlSafe(entry))
            .bind(character_id)
            .bind(polled_at)
            .execute(&mut *tx)
            .await?;
        self.record_success(
            &mut tx,
            character_id,
            Kind::Assets,
            polled_at,
            response.expires_at,
        )
        .await?;
        tx.commit().await
    }

    async fn record_success(
        &self,
        tx: &mut sqlx::PgConnection,
        character_id: i64,
        kind: Kind,
        polled_at: OffsetDateTime,
        expires_at: Option<OffsetDateTime>,
    ) -> Result<(), sqlx::Error> {
        let interval = kind.interval(self.interval);
        let next_poll_at =
            (polled_at + interval).max(expires_at.unwrap_or(polled_at)) + jitter(interval);
        sqlx::query(
            "INSERT INTO wealth_sync
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
            "wealth poll failed: {reason}"
        );
        let interval = kind.interval(self.interval);
        let result = sqlx::query(
            "INSERT INTO wealth_sync (character_id, kind, next_poll_at, last_attempt_at, last_error, failures)
             VALUES ($1, $2, $3, $4, $5, 1)
             ON CONFLICT (character_id, kind) DO UPDATE SET
                 next_poll_at = $4 + LEAST(
                     $6::float8 * power(2, LEAST(wealth_sync.failures, 20)) * interval '1 second',
                     $7::float8 * interval '1 second'),
                 last_attempt_at = $4,
                 last_error = $5,
                 failures = wealth_sync.failures + 1",
        )
        .bind(character_id)
        .bind(kind.as_str())
        .bind(now + BACKOFF_START.min(interval))
        .bind(now)
        .bind(reason)
        .bind(BACKOFF_START.as_seconds_f64())
        .bind(interval.as_seconds_f64())
        .execute(&self.pool)
        .await;
        match result {
            Ok(_) => PollOutcome::Failed,
            Err(e) if is_foreign_key_violation(&e) => PollOutcome::Dropped,
            Err(e) => {
                tracing::error!(character_id, "cannot record a wealth poll failure: {e}");
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
            "INSERT INTO wealth_sync (character_id, kind, next_poll_at, last_attempt_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (character_id, kind) DO UPDATE SET
                 next_poll_at = excluded.next_poll_at,
                 last_attempt_at = excluded.last_attempt_at",
        )
        .bind(character_id)
        .bind(kind.as_str())
        .bind(until + jitter(kind.interval(self.interval)))
        .bind(now)
        .execute(&self.pool)
        .await;
        match result {
            Err(e) if is_foreign_key_violation(&e) => PollOutcome::Dropped,
            Err(e) => {
                tracing::error!(character_id, "cannot record a deferred wealth poll: {e}");
                PollOutcome::Deferred
            }
            Ok(_) => PollOutcome::Deferred,
        }
    }
}

/// The inventory value of the assets joined to `a` and the stored prices joined to `p`: the sum
/// of quantity times average price over everything that is priced and not a blueprint copy.
pub(crate) const INVENTORY_VALUE_SUM: &str = "COALESCE(SUM(a.quantity * p.average_price)
     FILTER (WHERE NOT a.is_blueprint_copy AND p.average_price IS NOT NULL), 0)";

/// The top-level location of every asset: the location of the outermost item that holds it, or
/// its own location when it is not inside another item. A station, a solar system or a structure
/// (ESI reports those with the type `item` or `other`, told apart by their ID) counts; anything
/// else, a missing parent or nesting beyond [`MAX_NESTING`] included, is `None`.
fn top_locations(assets: &[EsiAsset]) -> HashMap<i64, Option<i64>> {
    let places: HashMap<i64, (i64, &str)> = assets
        .iter()
        .map(|a| (a.item_id, (a.location_id, a.location_type.as_str())))
        .collect();
    assets
        .iter()
        .map(|asset| {
            let (mut id, mut kind) = (asset.location_id, asset.location_type.as_str());
            for _ in 0..MAX_NESTING {
                match (kind, places.get(&id)) {
                    ("item", Some(&(parent, parent_kind))) => (id, kind) = (parent, parent_kind),
                    _ => break,
                }
            }
            let top = match kind {
                "station" | "solar_system" => Some(id),
                "item" if places.contains_key(&id) => None,
                _ if id >= STRUCTURE_ID_MIN => Some(id),
                _ => None,
            };
            (asset.item_id, top)
        })
        .collect()
}

// ---- the scheduler ----

impl WealthPoller {
    /// Runs [`WealthPoller::tick`] every [`SCHEDULER_TICK`] until the task is aborted.
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

    /// Schedules what has no schedule yet, then polls everything due (a few at a time) and
    /// returns how many polls ran.
    pub async fn tick(&self) -> usize {
        let now = self.clock.now();
        if let Err(e) = self.schedule_new(now).await {
            tracing::error!("cannot schedule wealth polls: {e}");
        }
        let due = match self.claim_due(now).await {
            Ok(due) => due,
            Err(e) => {
                tracing::error!("cannot claim due wealth polls: {e}");
                return 0;
            }
        };
        let polled = due.len();
        futures_util::stream::iter(due)
            .for_each_concurrent(self.concurrency, |(character_id, kind)| async move {
                self.poll(character_id, kind).await;
            })
            .await;
        polled
    }

    /// Gives every character and kind without a schedule a first poll time inside the next
    /// interval, spread by the character and the kind.
    async fn schedule_new(&self, now: OffsetDateTime) -> Result<(), sqlx::Error> {
        for kind in Kind::ALL {
            let unscheduled: Vec<i64> = sqlx::query_scalar(
                "SELECT c.character_id FROM characters c
                 WHERE NOT c.needs_relogin
                   AND NOT EXISTS (SELECT 1 FROM wealth_sync s
                                   WHERE s.character_id = c.character_id AND s.kind = $1)",
            )
            .bind(kind.as_str())
            .fetch_all(&self.pool)
            .await?;
            if unscheduled.is_empty() {
                continue;
            }
            let interval = kind.interval(self.interval);
            let times: Vec<OffsetDateTime> = unscheduled
                .iter()
                .map(|id| {
                    let mut key = id.to_be_bytes().to_vec();
                    key.extend_from_slice(kind.as_str().as_bytes());
                    now + spread(&key, interval)
                })
                .collect();
            sqlx::query(
                "INSERT INTO wealth_sync (character_id, kind, next_poll_at)
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
            "UPDATE wealth_sync w SET next_poll_at = $1 + $2::float8 * interval '1 second'
             WHERE (w.character_id, w.kind) IN (
                 SELECT s.character_id, s.kind FROM wealth_sync s
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

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(item_id: i64, location_id: i64, location_type: &str) -> EsiAsset {
        EsiAsset {
            item_id,
            type_id: 34,
            location_id,
            location_type: location_type.to_owned(),
            location_flag: "Hangar".to_owned(),
            quantity: 1,
            is_singleton: false,
            is_blueprint_copy: false,
        }
    }

    fn top(assets: &[EsiAsset], item_id: i64) -> Option<i64> {
        top_locations(assets)[&item_id]
    }

    const STATION: i64 = 60003760;
    const SYSTEM: i64 = 30000142;
    const STRUCTURE: i64 = 1_000_000_000_001;

    #[test]
    fn an_item_in_a_station_is_there() {
        assert_eq!(top(&[asset(1, STATION, "station")], 1), Some(STATION));
    }

    #[test]
    fn an_item_in_a_container_in_a_hangar_is_in_the_station() {
        let assets = [asset(1, STATION, "station"), asset(2, 1, "item")];
        assert_eq!(top(&assets, 2), Some(STATION));
    }

    #[test]
    fn a_module_fitted_to_a_ship_in_space_is_in_the_solar_system() {
        let assets = [
            asset(1, SYSTEM, "solar_system"),
            asset(2, 1, "item"),
            asset(3, 2, "item"),
        ];
        assert_eq!(top(&assets, 3), Some(SYSTEM));
    }

    #[test]
    fn a_structure_counts_as_one_even_when_esi_calls_its_type_item_or_other() {
        let assets = [
            asset(1, STRUCTURE, "item"),
            asset(2, 1, "item"),
            asset(3, STRUCTURE + 1, "other"),
        ];
        assert_eq!(top(&assets, 1), Some(STRUCTURE));
        assert_eq!(top(&assets, 2), Some(STRUCTURE));
        assert_eq!(top(&assets, 3), Some(STRUCTURE + 1));
    }

    #[test]
    fn an_unknown_parent_or_a_cycle_gives_no_location() {
        let lost = [asset(1, 99, "item"), asset(2, 1, "item")];
        assert_eq!(top(&lost, 1), None);
        assert_eq!(top(&lost, 2), None);
        let other = [asset(1, 5, "other")];
        assert_eq!(top(&other, 1), None);
        let cycle = [asset(1, 2, "item"), asset(2, 1, "item")];
        assert_eq!(top(&cycle, 1), None);
        assert_eq!(top(&cycle, 2), None);
    }

    #[test]
    fn a_very_deep_nesting_is_cut_off() {
        let mut assets = vec![asset(1, STATION, "station")];
        for id in 2..=40 {
            assets.push(asset(id, id - 1, "item"));
        }
        assert_eq!(top(&assets, 10), Some(STATION));
        assert_eq!(top(&assets, 40), None, "deeper than the hop limit");
    }
}
