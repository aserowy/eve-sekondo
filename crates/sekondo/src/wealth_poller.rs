//! The wealth poller: keeps every linked character's wallet balance (and, later, its assets) in
//! step with ESI. It mirrors the order poller but has a schedule and a failure state of its own,
//! so a wallet or assets problem never touches the orders, and the other way round.

use std::{collections::HashMap, sync::Arc};

use futures_util::StreamExt;
use secrecy::SecretString;
use time::{Duration, OffsetDateTime};

use crate::{
    clock::Clock,
    esi::{EsiAsset, EsiClient, EsiError, EsiResponse, EsiTransaction},
    events::{Event, EventBus},
    names::{self, STRUCTURE_ID_MIN},
    poller::{
        BACKOFF_START, CLAIM_LEASE, PollOutcome, SCHEDULER_TICK, is_foreign_key_violation, jitter,
        spread,
    },
    store::{AccountStore, MarketStore, StoreError, Stores, WealthStore},
    tokens::{TokenError, TokenService},
};

/// How many polls run at the same time.
const DEFAULT_CONCURRENCY: usize = 4;
/// ESI caches the assets for an hour, and they are heavy: never poll them more often.
pub const ASSETS_INTERVAL: Duration = Duration::hours(1);
/// How often the wallet transactions are polled: ESI caches them for an hour.
pub const TRANSACTIONS_INTERVAL: Duration = Duration::hours(1);
/// How many answers one transactions poll asks for at most, so a character with a history that
/// never reaches a stored transaction cannot keep it busy for ever.
const MAX_TRANSACTION_REQUESTS: usize = 50;
/// How many containers deep an asset is followed before its location counts as unknown. Guards
/// against cycles in the answer.
const MAX_NESTING: usize = 16;

/// What is polled per character. Each kind has its own schedule and failure state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Wallet,
    Assets,
    /// The personal purchases and sales among the wallet transactions, for the margin of sell
    /// orders and of sales.
    Transactions,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Wallet, Kind::Assets, Kind::Transactions];

    /// The value in `wealth_sync.kind`.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Wallet => "wallet",
            Kind::Assets => "assets",
            Kind::Transactions => "transactions",
        }
    }

    pub(crate) fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == raw)
    }

    /// How long after a poll the next one is earliest due.
    fn interval(self, order_interval: Duration) -> Duration {
        match self {
            Kind::Wallet => order_interval,
            Kind::Assets => ASSETS_INTERVAL,
            Kind::Transactions => TRANSACTIONS_INTERVAL,
        }
    }
}

/// What a transactions poll read from ESI: every transaction of every answer, and the expiry of
/// the first answer.
struct Fetched {
    transactions: Vec<EsiTransaction>,
    expires_at: Option<OffsetDateTime>,
}

/// How many new purchases and sales a transactions poll stored.
struct Stored {
    purchases: usize,
    sales: usize,
}

pub struct WealthPoller {
    wealth: Arc<dyn WealthStore>,
    accounts: Arc<dyn AccountStore>,
    market: Arc<dyn MarketStore>,
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
        stores: &Stores,
        tokens: Arc<TokenService>,
        esi: Arc<EsiClient>,
        events: Arc<dyn EventBus>,
        clock: Arc<dyn Clock>,
        interval: Duration,
    ) -> Self {
        Self {
            wealth: stores.wealth.clone(),
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

    /// Polls one kind of one character now, whatever its schedule says. Never panics or errors:
    /// every way a poll can end is a [`PollOutcome`], and the database records the details.
    pub async fn poll(&self, character_id: i64, kind: Kind) -> PollOutcome {
        let (outcome, signals) = self.run_poll(character_id, kind).await;
        for event in signals {
            self.publish(character_id, event).await;
        }
        outcome
    }

    /// Tells the character's account's open pages what changed.
    async fn publish(&self, character_id: i64, event: Event) {
        match self.accounts.find_character(character_id).await {
            Ok(Some(character)) => self.events.publish(character.account_id, event),
            Ok(None) => {}
            Err(e) => tracing::error!(character_id, "cannot look up the account to signal: {e}"),
        }
    }

    /// The token to poll with, or how the poll ends without one.
    async fn token_for(
        &self,
        character_id: i64,
        kind: Kind,
        now: OffsetDateTime,
    ) -> Result<SecretString, PollOutcome> {
        match self.tokens.access_token(character_id).await {
            Ok(token) => Ok(token),
            Err(TokenError::NeedsRelogin) => {
                // Make it due again, so it is polled right after it logged in.
                let _ = self.wealth.make_due(character_id, kind, now).await;
                Err(PollOutcome::NeedsRelogin)
            }
            Err(TokenError::UnknownCharacter) => Err(PollOutcome::Dropped),
            Err(TokenError::Temporary(reason)) => {
                Err(self.record_failure(character_id, kind, now, &reason).await)
            }
        }
    }

    /// The outcome of a poll and the signal it sends: a wallet or assets poll changes the wealth
    /// views; a transactions poll changes the trades page when it stored anything, and also the
    /// orders page when it stored a purchase.
    async fn run_poll(&self, character_id: i64, kind: Kind) -> (PollOutcome, Vec<Event>) {
        let now = self.clock.now();
        let token = match self.token_for(character_id, kind, now).await {
            Ok(token) => token,
            Err(outcome) => return (outcome, Vec::new()),
        };
        let wealth = |outcome: PollOutcome| {
            let signal = (outcome == PollOutcome::Updated).then_some(Event::WealthUpdated);
            (outcome, signal.into_iter().collect())
        };
        match kind {
            Kind::Wallet => match self.esi.wallet(character_id, &token).await {
                Ok(response) => match self.store_wallet(character_id, now, &response).await {
                    Ok(()) => wealth(PollOutcome::Updated),
                    Err(e) => wealth(self.store_failed(character_id, kind, now, e).await),
                },
                Err(e) => wealth(self.esi_failed(character_id, kind, now, e).await),
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
                        names::resolve(&*self.market, &self.esi, &token, wanted, now).await;
                        wealth(PollOutcome::Updated)
                    }
                    Err(e) => wealth(self.store_failed(character_id, kind, now, e).await),
                },
                Err(e) => wealth(self.esi_failed(character_id, kind, now, e).await),
            },
            Kind::Transactions => self.poll_transactions(character_id, now, &token).await,
        }
    }

    /// Reads the wallet transactions from the newest back until an answer holds a transaction that
    /// is already stored, is empty, or [`MAX_TRANSACTION_REQUESTS`] answers were read. Stores the
    /// personal purchases and sales among them and settles the cost of the sell orders and sales
    /// they feed.
    async fn poll_transactions(
        &self,
        character_id: i64,
        now: OffsetDateTime,
        token: &SecretString,
    ) -> (PollOutcome, Vec<Event>) {
        let kind = Kind::Transactions;
        let full_read = match self.wealth.full_read(character_id).await {
            Ok(full_read) => full_read,
            Err(e) => {
                return (
                    self.store_failed(character_id, kind, now, e).await,
                    Vec::new(),
                );
            }
        };
        let fetched = match self
            .fetch_transactions(character_id, token, full_read)
            .await
        {
            Ok(fetched) => fetched,
            Err(e) => {
                return (
                    self.esi_failed(character_id, kind, now, e).await,
                    Vec::new(),
                );
            }
        };
        match self.store_transactions(character_id, now, &fetched).await {
            Ok(stored) => {
                // After the transactions are safe: a name that cannot be resolved never fails a
                // poll. Structures are named with the owner's token.
                let wanted: Vec<i64> = fetched
                    .transactions
                    .iter()
                    .filter(|t| t.is_personal)
                    .flat_map(|t| [t.type_id, t.location_id])
                    .collect();
                names::resolve(&*self.market, &self.esi, token, wanted, now).await;
                let mut signals = Vec::new();
                if stored.purchases + stored.sales > 0 {
                    signals.push(Event::TradesUpdated);
                }
                if stored.purchases > 0 {
                    signals.push(Event::OrdersUpdated);
                }
                (PollOutcome::Updated, signals)
            }
            Err(e) => (
                self.store_failed(character_id, kind, now, e).await,
                Vec::new(),
            ),
        }
    }

    /// Whether the next transactions poll must read the whole window of ESI, ignoring what is
    /// stored. Set once for the characters that existed before sales were kept.
    async fn fetch_transactions(
        &self,
        character_id: i64,
        token: &SecretString,
        full_read: bool,
    ) -> Result<Fetched, EsiError> {
        let first = self
            .esi
            .wallet_transactions(character_id, token, None)
            .await?;
        let expires_at = first.expires_at;
        let mut transactions = first.body;
        let mut answer = transactions.clone();
        for _ in 1..MAX_TRANSACTION_REQUESTS {
            let ids: Vec<i64> = answer.iter().map(|t| t.transaction_id).collect();
            // `from_id` includes the transaction it names, so the next answer starts just below.
            let Some(oldest) = ids.iter().min().copied() else {
                return Ok(Fetched {
                    transactions,
                    expires_at,
                });
            };
            let known = !full_read
                && self
                    .wealth
                    .any_transaction_stored(&ids)
                    .await
                    .map_err(|e| EsiError::Temporary(format!("database error: {e}")))?;
            if known {
                return Ok(Fetched {
                    transactions,
                    expires_at,
                });
            }
            answer = self
                .esi
                .wallet_transactions(character_id, token, Some(oldest - 1))
                .await?
                .body;
            transactions.extend(answer.iter().cloned());
        }
        tracing::warn!(
            character_id,
            "transactions poll stopped after {MAX_TRANSACTION_REQUESTS} answers"
        );
        Ok(Fetched {
            transactions,
            expires_at,
        })
    }

    /// Stores the personal purchases and sales, once each, settles the cost of the sell orders and
    /// sales of the items that got a new one, and updates the sync state, in one transaction.
    async fn store_transactions(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        fetched: &Fetched,
    ) -> Result<Stored, StoreError> {
        let (purchases, sales) = self
            .wealth
            .store_transactions(
                character_id,
                polled_at,
                &fetched.transactions,
                self.next_poll_at(Kind::Transactions, polled_at, fetched.expires_at),
                fetched.expires_at,
            )
            .await?;
        Ok(Stored { purchases, sales })
    }

    /// `max(polled_at + interval, ESI's Expires)`, pushed back by a little jitter.
    fn next_poll_at(
        &self,
        kind: Kind,
        polled_at: OffsetDateTime,
        expires_at: Option<OffsetDateTime>,
    ) -> OffsetDateTime {
        let interval = kind.interval(self.interval);
        (polled_at + interval).max(expires_at.unwrap_or(polled_at)) + jitter(interval)
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

    /// Appends the balance and updates the sync state, in one transaction. Two polls in the same
    /// instant keep the later one rather than failing.
    async fn store_wallet(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        response: &EsiResponse<i64>,
    ) -> Result<(), StoreError> {
        self.wealth
            .store_wallet(
                character_id,
                polled_at,
                response.body,
                self.next_poll_at(Kind::Wallet, polled_at, response.expires_at),
                response.expires_at,
            )
            .await
    }

    /// Replaces the character's assets with what ESI returned, records the inventory value from
    /// the rows just written and updates the sync state, in one transaction.
    async fn store_assets(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        response: &EsiResponse<Vec<EsiAsset>>,
    ) -> Result<(), StoreError> {
        let tops = top_locations(&response.body);
        let top: Vec<Option<i64>> = response.body.iter().map(|a| tops[&a.item_id]).collect();
        self.wealth
            .store_assets(
                character_id,
                polled_at,
                &response.body,
                &top,
                self.next_poll_at(Kind::Assets, polled_at, response.expires_at),
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
            "wealth poll failed: {reason}"
        );
        let interval = kind.interval(self.interval);
        let result = self
            .wealth
            .record_failure(character_id, kind, now, reason, BACKOFF_START, interval)
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
        let result = self
            .wealth
            .record_deferral(
                character_id,
                kind,
                now,
                until + jitter(kind.interval(self.interval)),
            )
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
    async fn schedule_new(&self, now: OffsetDateTime) -> Result<(), StoreError> {
        for kind in Kind::ALL {
            let unscheduled = self.wealth.list_unscheduled(kind).await?;
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
            self.wealth
                .insert_schedule(kind, &unscheduled, &times)
                .await?;
        }
        Ok(())
    }

    /// Takes the due pairs of characters that do not need a re-login. They are pushed back by a
    /// lease, so neither this nor another tick takes them again while their poll runs.
    async fn claim_due(&self, now: OffsetDateTime) -> Result<Vec<(i64, Kind)>, StoreError> {
        self.wealth
            .claim_due(now, CLAIM_LEASE, (self.concurrency * 8) as i64)
            .await
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
