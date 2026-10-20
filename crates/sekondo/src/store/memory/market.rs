use async_trait::async_trait;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::{
    Shared, backoff, micros,
    tables::{MarketOrderRow, NameRow, OrderRow, Sync, Tables},
};
use crate::{
    esi::{EsiMarketOrder, EsiPrice},
    store::{
        MarketStore, NameRow as NameInput, NameState, PublicOrder, StandingRow, StoreError,
        StoreResult, SyncState,
    },
};

pub struct MemMarketStore {
    pub(super) shared: std::sync::Arc<Shared>,
}

/// Where a current order stands among the public orders of its pair (design §5).
#[derive(Debug, Clone, Default)]
pub(super) struct Standing {
    pub undercut: Option<i64>,
    pub best_price_cents: Option<i64>,
    pub undercut_volume: Option<i64>,
}

impl Standing {
    /// The per-unit difference to the best price, for an order that is undercut.
    pub fn gap_cents(&self, price_cents: i64) -> Option<i64> {
        match (self.undercut, self.best_price_cents) {
            (Some(n), Some(best)) if n > 0 => Some((best - price_cents).abs()),
            _ => None,
        }
    }
}

/// A structure ID starts here, as the SQL spells it.
const STRUCTURE: i64 = 100_000_000;

pub(super) fn standing(tables: &Tables, order_id: i64, order: &OrderRow) -> Standing {
    let refreshed = tables
        .market_sync
        .get(&(order.region_id, order.type_id))
        .is_some_and(|s| s.last_success_at.is_some());
    let own = tables.market_orders.get(&order_id);
    let available =
        order.location_id < STRUCTURE && refreshed && (!order.is_buy_order || own.is_some());
    if !available {
        return Standing::default();
    }
    let own_system = own.map(|o| o.system_id);
    let undercutting: Vec<(&i64, &MarketOrderRow)> = tables
        .market_orders
        .iter()
        .filter(|(id, m)| {
            m.region_id == order.region_id && m.type_id == order.type_id && **id != order_id
        })
        .filter(|(_, m)| {
            if order.is_buy_order {
                m.is_buy_order
                    && m.price_cents > order.price_cents
                    && (m.location_id == order.location_id
                        || m.range == "region"
                        || (m.range != "station" && Some(m.system_id) == own_system))
            } else {
                !m.is_buy_order
                    && m.price_cents < order.price_cents
                    && m.location_id == order.location_id
            }
        })
        .collect();
    let best = if order.is_buy_order {
        undercutting.iter().map(|(_, m)| m.price_cents).max()
    } else {
        undercutting.iter().map(|(_, m)| m.price_cents).min()
    };
    let volume = (!undercutting.is_empty()).then(|| {
        undercutting
            .iter()
            .map(|(_, m)| m.volume_remain)
            .sum::<i64>()
    });
    Standing {
        undercut: Some(undercutting.len() as i64),
        best_price_cents: best,
        undercut_volume: volume,
    }
}

fn pairs_in_use(tables: &Tables) -> std::collections::BTreeSet<(i64, i64)> {
    tables
        .current_orders
        .values()
        .map(|o| (o.region_id, o.type_id))
        .collect()
}

#[async_trait]
impl MarketStore for MemMarketStore {
    async fn known_names(&self, ids: &[i64], now: OffsetDateTime) -> StoreResult<Vec<i64>> {
        let tables = self.shared.lock();
        Ok(ids
            .iter()
            .copied()
            .filter(|id| {
                tables
                    .names
                    .get(id)
                    .is_some_and(|n| n.name.is_some() || n.retry_after.is_some_and(|r| r > now))
            })
            .collect())
    }

    async fn store_names(
        &self,
        rows: &[NameInput],
        resolved_at: OffsetDateTime,
        retry_after: Option<OffsetDateTime>,
    ) -> StoreResult<()> {
        let mut seen = std::collections::BTreeSet::new();
        if !rows.iter().all(|r| seen.insert(r.id)) {
            return Err(StoreError::Backend(
                "ON CONFLICT DO UPDATE command cannot affect row a second time".to_owned(),
            ));
        }
        let mut tables = self.shared.lock();
        for row in rows {
            tables.names.insert(
                row.id,
                NameRow {
                    category: row.category.clone(),
                    name: row.name.clone(),
                    resolved_at: micros(resolved_at),
                    retry_after: retry_after.map(micros),
                },
            );
        }
        Ok(())
    }

    async fn names_of(&self, ids: &[i64]) -> StoreResult<Vec<(i64, Option<String>)>> {
        let tables = self.shared.lock();
        let mut found: Vec<(i64, Option<String>)> = ids
            .iter()
            .filter_map(|id| tables.names.get(id).map(|n| (*id, n.name.clone())))
            .collect();
        found.sort();
        found.dedup();
        Ok(found)
    }

    async fn name_of(&self, id: i64) -> StoreResult<Option<String>> {
        Ok(self
            .shared
            .lock()
            .names
            .get(&id)
            .and_then(|n| n.name.clone()))
    }

    async fn standings(&self, region_id: i64, type_id: i64) -> StoreResult<Vec<StandingRow>> {
        let tables = self.shared.lock();
        Ok(tables
            .current_orders
            .iter()
            .filter(|(_, o)| o.region_id == region_id && o.type_id == type_id)
            .filter_map(|(id, o)| {
                let account = tables.characters.get(&o.character_id)?.account_id;
                let st = standing(&tables, *id, o);
                Some((
                    account,
                    *id,
                    st.undercut,
                    st.gap_cents(o.price_cents),
                    st.undercut_volume,
                ))
            })
            .collect())
    }

    async fn account_pairs(&self, account_id: Uuid) -> StoreResult<Vec<(i64, i64)>> {
        let tables = self.shared.lock();
        let pairs: std::collections::BTreeSet<(i64, i64)> = tables
            .current_orders
            .values()
            .filter(|o| {
                tables
                    .characters
                    .get(&o.character_id)
                    .is_some_and(|c| c.account_id == account_id)
            })
            .map(|o| (o.region_id, o.type_id))
            .collect();
        Ok(pairs.into_iter().collect())
    }

    async fn untracked_pairs(&self) -> StoreResult<Vec<(i64, i64)>> {
        let tables = self.shared.lock();
        Ok(pairs_in_use(&tables)
            .into_iter()
            .filter(|pair| !tables.market_sync.contains_key(pair))
            .collect())
    }

    async fn insert_pair_schedule(
        &self,
        pairs: &[(i64, i64)],
        next_poll_at: &[OffsetDateTime],
    ) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        for (pair, at) in pairs.iter().zip(next_poll_at) {
            tables
                .market_sync
                .entry(*pair)
                .or_insert_with(|| Sync::scheduled(micros(*at)));
        }
        Ok(())
    }

    async fn drop_unused_pairs(&self) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        let used = pairs_in_use(&tables);
        tables.market_sync.retain(|pair, _| used.contains(pair));
        tables
            .market_orders
            .retain(|_, m| used.contains(&(m.region_id, m.type_id)));
        Ok(())
    }

    async fn claim_due_pairs(
        &self,
        now: OffsetDateTime,
        lease: Duration,
        limit: i64,
    ) -> StoreResult<Vec<(i64, i64)>> {
        let mut tables = self.shared.lock();
        let mut due: Vec<(OffsetDateTime, (i64, i64))> = tables
            .market_sync
            .iter()
            .filter(|(_, s)| s.next_poll_at <= now)
            .map(|(pair, s)| (s.next_poll_at, *pair))
            .collect();
        due.sort();
        due.truncate(limit.max(0) as usize);
        let mut claimed = Vec::new();
        for (_, pair) in due {
            if let Some(row) = tables.market_sync.get_mut(&pair) {
                row.next_poll_at = now + lease;
            }
            claimed.push(pair);
        }
        Ok(claimed)
    }

    async fn replace_market_orders(
        &self,
        region_id: i64,
        type_id: i64,
        now: OffsetDateTime,
        orders: &[EsiMarketOrder],
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<()> {
        let mut seen = std::collections::BTreeSet::new();
        if !orders.iter().all(|o| seen.insert(o.order_id)) {
            return Err(StoreError::Backend(
                "ON CONFLICT DO UPDATE command cannot affect row a second time".to_owned(),
            ));
        }
        let mut tables = self.shared.lock();
        tables
            .market_orders
            .retain(|_, m| !(m.region_id == region_id && m.type_id == type_id));
        for o in orders {
            tables.market_orders.insert(
                o.order_id,
                MarketOrderRow {
                    region_id,
                    type_id,
                    location_id: o.location_id,
                    system_id: o.system_id,
                    is_buy_order: o.is_buy_order,
                    price_cents: o.price_cents,
                    range: o.range.clone(),
                    volume_remain: o.volume_remain,
                },
            );
        }
        if let Some(row) = tables.market_sync.get_mut(&(region_id, type_id)) {
            row.next_poll_at = micros(next_poll_at);
            row.last_success_at = Some(micros(now));
            row.last_attempt_at = Some(micros(now));
            row.last_error = None;
            row.esi_expires_at = esi_expires_at.map(micros);
            row.failures = 0;
        }
        Ok(())
    }

    async fn record_pair_failure(
        &self,
        region_id: i64,
        type_id: i64,
        now: OffsetDateTime,
        reason: &str,
        backoff_start: Duration,
        interval: Duration,
    ) -> StoreResult<()> {
        if let Some(row) = self
            .shared
            .lock()
            .market_sync
            .get_mut(&(region_id, type_id))
        {
            row.next_poll_at = micros(now) + backoff(backoff_start, row.failures, interval);
            row.last_attempt_at = Some(micros(now));
            row.last_error = Some(reason.to_owned());
            row.failures += 1;
        }
        Ok(())
    }

    async fn record_pair_deferral(
        &self,
        region_id: i64,
        type_id: i64,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
    ) -> StoreResult<()> {
        if let Some(row) = self
            .shared
            .lock()
            .market_sync
            .get_mut(&(region_id, type_id))
        {
            row.next_poll_at = micros(next_poll_at);
            row.last_attempt_at = Some(micros(now));
        }
        Ok(())
    }

    async fn claim_price_refresh(&self, now: OffsetDateTime, lease: Duration) -> StoreResult<bool> {
        let now = micros(now);
        let mut tables = self.shared.lock();
        let row = tables
            .price_sync
            .get_or_insert_with(|| Sync::scheduled(now));
        if row.next_poll_at <= now {
            row.next_poll_at = now + lease;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn accounts_with_assets(&self) -> StoreResult<Vec<Uuid>> {
        let tables = self.shared.lock();
        let mut accounts: Vec<Uuid> = tables
            .current_assets
            .values()
            .filter_map(|a| tables.characters.get(&a.character_id).map(|c| c.account_id))
            .collect();
        accounts.sort_unstable();
        accounts.dedup();
        Ok(accounts)
    }

    async fn replace_prices(
        &self,
        now: OffsetDateTime,
        prices: &[EsiPrice],
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<()> {
        let now = micros(now);
        let mut tables = self.shared.lock();
        tables.item_prices.clear();
        for price in prices {
            tables
                .item_prices
                .insert(price.type_id, price.average_price_cents);
        }
        // Every character that has assets gets a new inventory value.
        let mut values: std::collections::BTreeMap<i64, i128> = std::collections::BTreeMap::new();
        for asset in tables.current_assets.values() {
            let value = values.entry(asset.character_id).or_insert(0);
            if let (false, Some(Some(price))) = (
                asset.is_blueprint_copy,
                tables.item_prices.get(&asset.type_id),
            ) {
                *value += i128::from(asset.quantity) * i128::from(*price);
            }
        }
        for (character_id, value) in values {
            tables.inventory_values.insert((character_id, now), value);
        }
        let row = tables
            .price_sync
            .get_or_insert_with(|| Sync::scheduled(now));
        row.next_poll_at = micros(next_poll_at);
        row.last_success_at = Some(now);
        row.last_attempt_at = Some(now);
        row.last_error = None;
        row.esi_expires_at = esi_expires_at.map(micros);
        row.failures = 0;
        Ok(())
    }

    async fn record_price_failure(
        &self,
        now: OffsetDateTime,
        reason: &str,
        backoff_start: Duration,
        interval: Duration,
    ) -> StoreResult<()> {
        let now = micros(now);
        let mut tables = self.shared.lock();
        match tables.price_sync.as_mut() {
            Some(row) => {
                row.next_poll_at = now + backoff(backoff_start, row.failures, interval);
                row.last_attempt_at = Some(now);
                row.last_error = Some(reason.to_owned());
                row.failures += 1;
            }
            None => {
                let mut row = Sync::scheduled(now + backoff_start);
                row.last_attempt_at = Some(now);
                row.last_error = Some(reason.to_owned());
                row.failures = 1;
                tables.price_sync = Some(row);
            }
        }
        Ok(())
    }

    async fn record_price_deferral(
        &self,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
    ) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        let row = tables
            .price_sync
            .get_or_insert_with(|| Sync::scheduled(micros(next_poll_at)));
        row.next_poll_at = micros(next_poll_at);
        row.last_attempt_at = Some(micros(now));
        Ok(())
    }
    async fn pair_sync(&self, region_id: i64, type_id: i64) -> StoreResult<Option<SyncState>> {
        Ok(self
            .shared
            .lock()
            .market_sync
            .get(&(region_id, type_id))
            .map(Sync::state))
    }

    async fn price_sync(&self) -> StoreResult<Option<SyncState>> {
        Ok(self.shared.lock().price_sync.as_ref().map(Sync::state))
    }
    async fn names_table(&self) -> StoreResult<Vec<NameState>> {
        Ok(self
            .shared
            .lock()
            .names
            .iter()
            .map(|(id, n)| NameState {
                id: *id,
                name: n.name.clone(),
                retry_after: n.retry_after,
            })
            .collect())
    }
    async fn tracked_pairs(&self) -> StoreResult<Vec<(i64, i64)>> {
        Ok(self.shared.lock().market_sync.keys().copied().collect())
    }

    async fn public_orders(&self) -> StoreResult<Vec<PublicOrder>> {
        Ok(self
            .shared
            .lock()
            .market_orders
            .iter()
            .map(|(id, o)| PublicOrder {
                order_id: *id,
                region_id: o.region_id,
                type_id: o.type_id,
                location_id: o.location_id,
                is_buy_order: o.is_buy_order,
                price_cents: o.price_cents,
                volume_remain: o.volume_remain,
            })
            .collect())
    }

    async fn prices_table(&self) -> StoreResult<Vec<(i64, Option<i64>)>> {
        Ok(self
            .shared
            .lock()
            .item_prices
            .iter()
            .map(|(t, p)| (*t, *p))
            .collect())
    }

    async fn seed_price(&self, type_id: i64, average_price_cents: Option<i64>) -> StoreResult<()> {
        self.shared
            .lock()
            .item_prices
            .insert(type_id, average_price_cents);
        Ok(())
    }
}
