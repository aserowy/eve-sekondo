use std::{cmp::Ordering, collections::BTreeMap};

use async_trait::async_trait;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::{
    Shared, backoff, bucket_ends, micros,
    tables::{AssetRow, PollTotals, Sync, Tables, TransactionRow},
    trade::recompute,
};
use crate::{
    esi::{EsiAsset, EsiTransaction},
    freshness::WealthFreshnessRecord,
    orders::Dir,
    range::Range,
    store::{PollTotalsRow, StoreError, StoreResult, SyncState, WealthStore},
    wealth::{Current, InventoryRow, InventorySort, InventorySortKey, WealthPoint},
    wealth_poller::Kind,
};

pub struct MemWealthStore {
    pub(super) shared: std::sync::Arc<Shared>,
}

/// The characters of the account among `character_ids`.
fn selected(tables: &Tables, account_id: Uuid, character_ids: &[i64]) -> Vec<i64> {
    tables
        .characters
        .values()
        .filter(|c| c.account_id == account_id && character_ids.contains(&c.character_id))
        .map(|c| c.character_id)
        .collect()
}

/// A character's latest figures at or before `at`: wallet, personal sell value, personal escrow
/// and inventory, each `None` when it was never recorded.
struct Figures {
    wallet: Option<i128>,
    sell: Option<i128>,
    escrow: Option<i128>,
    inventory: Option<i128>,
}

fn figures_at(tables: &Tables, character_id: i64, at: Option<OffsetDateTime>) -> Figures {
    let upto = |t: &OffsetDateTime| at.is_none_or(|at| *t <= at);
    let wallet = tables
        .wallet_balances
        .range(
            (
                character_id,
                OffsetDateTime::UNIX_EPOCH - Duration::days(365 * 200),
            )..,
        )
        .take_while(|((c, _), _)| *c == character_id)
        .filter(|((_, t), _)| upto(t))
        .last()
        .map(|(_, balance)| i128::from(*balance));
    let totals = tables
        .order_poll_totals
        .iter()
        .rfind(|((c, t), _)| *c == character_id && upto(t))
        .map(|(_, totals)| totals);
    let inventory = tables
        .inventory_values
        .iter()
        .rfind(|((c, t), _)| *c == character_id && upto(t))
        .map(|(_, value)| *value);
    Figures {
        wallet,
        sell: totals.map(|t| t.sell_value - t.sell_corp),
        escrow: totals.map(|t| t.escrow - t.escrow_corp),
        inventory,
    }
}

fn to_cents(value: i128) -> i64 {
    i64::try_from(value).expect("an amount in cents fits a bigint")
}

fn name_of(tables: &Tables, id: i64) -> Option<String> {
    tables.names.get(&id).and_then(|n| n.name.clone())
}

fn lowered(name: Option<String>, id: i64) -> String {
    name.unwrap_or_else(|| format!("#{id}")).to_lowercase()
}

/// `ORDER BY` of the inventory list: the sort key in its direction, then the item, the character
/// and the place, ascending.
fn compare(sort: InventorySort, a: &Sorted, b: &Sorted) -> Ordering {
    let direct = |ordering: Ordering| match sort.dir {
        Dir::Asc => ordering,
        Dir::Desc => ordering.reverse(),
    };
    let first = match sort.key {
        InventorySortKey::Price => a
            .unit_price
            .is_none()
            .cmp(&b.unit_price.is_none())
            .then_with(|| direct(a.unit_price.cmp(&b.unit_price))),
        InventorySortKey::Value => a
            .value
            .is_none()
            .cmp(&b.value.is_none())
            .then_with(|| direct(a.value.cmp(&b.value))),
        InventorySortKey::Character => direct(a.character.cmp(&b.character)),
        InventorySortKey::Item => direct(a.item.cmp(&b.item)),
        InventorySortKey::Quantity => direct(a.row.quantity.cmp(&b.row.quantity)),
        InventorySortKey::Location => direct(a.place.cmp(&b.place)),
    };
    first
        .then_with(|| a.item.cmp(&b.item))
        .then_with(|| a.row.character_id.cmp(&b.row.character_id))
        .then_with(|| a.row.location_id.cmp(&b.row.location_id))
}

struct Sorted {
    row: InventoryRow,
    character: String,
    item: String,
    place: String,
    unit_price: Option<i64>,
    value: Option<i128>,
}

fn require_character(tables: &Tables, character_id: i64) -> StoreResult<()> {
    if tables.characters.contains_key(&character_id) {
        Ok(())
    } else {
        Err(StoreError::MissingParent)
    }
}

fn record_success(
    tables: &mut Tables,
    character_id: i64,
    kind: Kind,
    polled_at: OffsetDateTime,
    next_poll_at: OffsetDateTime,
    expires_at: Option<OffsetDateTime>,
) {
    let row = tables
        .wealth_sync
        .entry((character_id, kind.as_str().to_owned()))
        .or_insert_with(|| Sync::scheduled(micros(next_poll_at)));
    row.next_poll_at = micros(next_poll_at);
    row.last_success_at = Some(micros(polled_at));
    row.last_attempt_at = Some(micros(polled_at));
    row.last_error = None;
    row.esi_expires_at = expires_at.map(micros);
    row.failures = 0;
    row.full_read = false;
}

/// Inserts the personal buys or sells among the transactions that are not stored yet. Answers the
/// item type of each new row.
fn insert_transactions(
    tables: &mut Tables,
    character_id: i64,
    transactions: &[EsiTransaction],
    buys: bool,
) -> Vec<i64> {
    let mut types = Vec::new();
    for t in transactions
        .iter()
        .filter(|t| t.is_buy == buys && t.is_personal)
    {
        let table = if buys {
            &mut tables.purchases
        } else {
            &mut tables.sales
        };
        if let std::collections::btree_map::Entry::Vacant(entry) = table.entry(t.transaction_id) {
            entry.insert(TransactionRow {
                character_id,
                type_id: t.type_id,
                location_id: t.location_id,
                quantity: t.quantity,
                unit_price_cents: t.unit_price_cents,
                at: micros(t.bought_at),
            });
            types.push(t.type_id);
        }
    }
    types
}

/// The value of a character's assets at the stored prices: quantity times average price of
/// everything that is priced and not a blueprint copy.
pub(super) fn inventory_value(tables: &Tables, character_id: i64) -> i128 {
    tables
        .current_assets
        .values()
        .filter(|a| a.character_id == character_id && !a.is_blueprint_copy)
        .filter_map(|a| {
            tables
                .item_prices
                .get(&a.type_id)
                .copied()
                .flatten()
                .map(|price| i128::from(a.quantity) * i128::from(price))
        })
        .sum()
}

#[async_trait]
impl WealthStore for MemWealthStore {
    async fn current(&self, account_id: Uuid, character_ids: &[i64]) -> StoreResult<Current> {
        let tables = self.shared.lock();
        let mut current = Current::default();
        for id in selected(&tables, account_id, character_ids) {
            let f = figures_at(&tables, id, None);
            current.wallet_cents += f.wallet.map_or(0, to_cents);
            current.sell_cents += f.sell.map_or(0, to_cents);
            current.escrow_cents += f.escrow.map_or(0, to_cents);
            current.inventory_cents += f.inventory.map_or(0, to_cents);
        }
        Ok(current)
    }

    async fn history(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        range: Range,
        now: OffsetDateTime,
    ) -> StoreResult<Vec<WealthPoint>> {
        let tables = self.shared.lock();
        let characters = selected(&tables, account_id, character_ids);
        if characters.is_empty() {
            return Ok(Vec::new());
        }
        Ok(bucket_ends(range, micros(now))
            .into_iter()
            .map(|at| {
                let mut point = WealthPoint {
                    at,
                    wallet_cents: 0,
                    sell_cents: 0,
                    escrow_cents: 0,
                    inventory_cents: 0,
                    contributors: 0,
                };
                for id in &characters {
                    let f = figures_at(&tables, *id, Some(at));
                    point.wallet_cents += f.wallet.map_or(0, to_cents);
                    point.sell_cents += f.sell.map_or(0, to_cents);
                    point.escrow_cents += f.escrow.map_or(0, to_cents);
                    point.inventory_cents += f.inventory.map_or(0, to_cents);
                    if f.wallet.is_some() || f.sell.is_some() || f.inventory.is_some() {
                        point.contributors += 1;
                    }
                }
                point
            })
            .collect())
    }

    async fn inventory_rows(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        sort: InventorySort,
    ) -> StoreResult<Vec<InventoryRow>> {
        let tables = self.shared.lock();
        let selected = selected(&tables, account_id, character_ids);
        // One row per character, item and top-level location; blueprint copies on their own.
        let mut groups: BTreeMap<(i64, i64, i64, bool, bool), i64> = BTreeMap::new();
        for a in tables
            .current_assets
            .values()
            .filter(|a| selected.contains(&a.character_id))
        {
            let place = a.top_location_id.unwrap_or(a.location_id);
            *groups
                .entry((
                    a.character_id,
                    a.type_id,
                    place,
                    a.top_location_id.is_some(),
                    a.is_blueprint_copy,
                ))
                .or_default() += a.quantity;
        }
        let mut rows: Vec<Sorted> = groups
            .into_iter()
            .map(
                |((character_id, type_id, place, top_known, copy), quantity)| {
                    let unit_price = if copy {
                        None
                    } else {
                        tables.item_prices.get(&type_id).copied().flatten()
                    };
                    let value = unit_price.map(|p| i128::from(p) * i128::from(quantity));
                    let character = tables.characters[&character_id].name.clone();
                    let type_name = name_of(&tables, type_id);
                    let location_name = name_of(&tables, place);
                    Sorted {
                        character: character.to_lowercase(),
                        item: lowered(type_name.clone(), type_id),
                        place: lowered(location_name.clone(), place),
                        unit_price,
                        value,
                        row: InventoryRow {
                            character_id,
                            character_name: character,
                            type_id,
                            type_name,
                            location_id: place,
                            location_name,
                            location_known: tables.names.contains_key(&place),
                            top_known,
                            quantity,
                            unit_price_cents: unit_price,
                            value_cents: value.map(to_cents),
                        },
                    }
                },
            )
            .collect();
        rows.sort_by(|a, b| compare(sort, a, b));
        Ok(rows.into_iter().map(|s| s.row).collect())
    }

    async fn list_wealth_freshness(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
    ) -> StoreResult<Vec<WealthFreshnessRecord>> {
        let tables = self.shared.lock();
        let mut characters: Vec<_> = tables
            .characters
            .values()
            .filter(|c| c.account_id == account_id && character_ids.contains(&c.character_id))
            .collect();
        characters.sort_by_key(|c| (c.created_at, c.character_id));
        Ok(characters
            .into_iter()
            .map(|c| {
                let sync = |kind: &str| tables.wealth_sync.get(&(c.character_id, kind.to_owned()));
                let (wallet, assets) = (sync("wallet"), sync("assets"));
                WealthFreshnessRecord {
                    character_id: c.character_id,
                    name: c.name.clone(),
                    needs_relogin: c.needs_relogin,
                    wallet_last_success_at: wallet.and_then(|s| s.last_success_at),
                    wallet_next_poll_at: wallet.map(|s| s.next_poll_at),
                    assets_last_success_at: assets.and_then(|s| s.last_success_at),
                    assets_next_poll_at: assets.map(|s| s.next_poll_at),
                }
            })
            .collect())
    }

    async fn make_due(
        &self,
        character_id: i64,
        kind: Kind,
        now: OffsetDateTime,
    ) -> StoreResult<()> {
        if let Some(row) = self
            .shared
            .lock()
            .wealth_sync
            .get_mut(&(character_id, kind.as_str().to_owned()))
        {
            row.next_poll_at = row.next_poll_at.min(micros(now));
        }
        Ok(())
    }

    async fn full_read(&self, character_id: i64) -> StoreResult<bool> {
        Ok(self
            .shared
            .lock()
            .wealth_sync
            .get(&(character_id, "transactions".to_owned()))
            .is_some_and(|s| s.full_read))
    }

    async fn any_transaction_stored(&self, transaction_ids: &[i64]) -> StoreResult<bool> {
        let tables = self.shared.lock();
        Ok(transaction_ids
            .iter()
            .any(|id| tables.purchases.contains_key(id) || tables.sales.contains_key(id)))
    }

    async fn store_transactions(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        transactions: &[EsiTransaction],
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<(usize, usize)> {
        let mut tables = self.shared.lock();
        require_character(&tables, character_id)?;
        let mut purchase_types = insert_transactions(&mut tables, character_id, transactions, true);
        let mut sale_types = insert_transactions(&mut tables, character_id, transactions, false);
        let stored = (purchase_types.len(), sale_types.len());
        purchase_types.append(&mut sale_types);
        purchase_types.sort_unstable();
        purchase_types.dedup();
        recompute(&mut tables, character_id, &purchase_types);
        record_success(
            &mut tables,
            character_id,
            Kind::Transactions,
            polled_at,
            next_poll_at,
            esi_expires_at,
        );
        Ok(stored)
    }

    async fn store_wallet(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        balance_cents: i64,
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        require_character(&tables, character_id)?;
        tables
            .wallet_balances
            .insert((character_id, micros(polled_at)), balance_cents);
        record_success(
            &mut tables,
            character_id,
            Kind::Wallet,
            polled_at,
            next_poll_at,
            esi_expires_at,
        );
        Ok(())
    }

    async fn store_assets(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        assets: &[EsiAsset],
        top_locations: &[Option<i64>],
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<()> {
        let mut seen = std::collections::BTreeSet::new();
        if !assets.iter().all(|a| seen.insert(a.item_id)) {
            return Err(StoreError::Backend(
                "ON CONFLICT DO UPDATE command cannot affect row a second time".to_owned(),
            ));
        }
        let mut tables = self.shared.lock();
        require_character(&tables, character_id)?;
        tables
            .current_assets
            .retain(|_, a| a.character_id != character_id);
        for (asset, top) in assets.iter().zip(top_locations) {
            // An item that changed hands moves from its previous owner's list.
            tables.current_assets.insert(
                asset.item_id,
                AssetRow {
                    character_id,
                    type_id: asset.type_id,
                    location_id: asset.location_id,
                    location_type: asset.location_type.clone(),
                    location_flag: asset.location_flag.clone(),
                    quantity: asset.quantity,
                    is_singleton: asset.is_singleton,
                    is_blueprint_copy: asset.is_blueprint_copy,
                    top_location_id: *top,
                },
            );
        }
        // An aggregate without a group is one row, also when the character has no assets.
        let value = inventory_value(&tables, character_id);
        tables
            .inventory_values
            .insert((character_id, micros(polled_at)), value);
        record_success(
            &mut tables,
            character_id,
            Kind::Assets,
            polled_at,
            next_poll_at,
            esi_expires_at,
        );
        Ok(())
    }

    async fn record_failure(
        &self,
        character_id: i64,
        kind: Kind,
        now: OffsetDateTime,
        reason: &str,
        backoff_start: Duration,
        interval: Duration,
    ) -> StoreResult<()> {
        let now = micros(now);
        let mut tables = self.shared.lock();
        require_character(&tables, character_id)?;
        match tables
            .wealth_sync
            .get_mut(&(character_id, kind.as_str().to_owned()))
        {
            Some(row) => {
                row.next_poll_at = now + backoff(backoff_start, row.failures, interval);
                row.last_attempt_at = Some(now);
                row.last_error = Some(reason.to_owned());
                row.failures += 1;
            }
            None => {
                let mut row = Sync::scheduled(now + backoff_start.min(interval));
                row.last_attempt_at = Some(now);
                row.last_error = Some(reason.to_owned());
                row.failures = 1;
                tables
                    .wealth_sync
                    .insert((character_id, kind.as_str().to_owned()), row);
            }
        }
        Ok(())
    }

    async fn record_deferral(
        &self,
        character_id: i64,
        kind: Kind,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
    ) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        require_character(&tables, character_id)?;
        let row = tables
            .wealth_sync
            .entry((character_id, kind.as_str().to_owned()))
            .or_insert_with(|| Sync::scheduled(micros(next_poll_at)));
        row.next_poll_at = micros(next_poll_at);
        row.last_attempt_at = Some(micros(now));
        Ok(())
    }

    async fn list_unscheduled(&self, kind: Kind) -> StoreResult<Vec<i64>> {
        let tables = self.shared.lock();
        Ok(tables
            .characters
            .values()
            .filter(|c| !c.needs_relogin)
            .filter(|c| {
                !tables
                    .wealth_sync
                    .contains_key(&(c.character_id, kind.as_str().to_owned()))
            })
            .map(|c| c.character_id)
            .collect())
    }

    async fn insert_schedule(
        &self,
        kind: Kind,
        character_ids: &[i64],
        next_poll_at: &[OffsetDateTime],
    ) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        if character_ids
            .iter()
            .any(|id| !tables.characters.contains_key(id))
        {
            return Err(StoreError::MissingParent);
        }
        for (id, at) in character_ids.iter().zip(next_poll_at) {
            tables
                .wealth_sync
                .entry((*id, kind.as_str().to_owned()))
                .or_insert_with(|| Sync::scheduled(micros(*at)));
        }
        Ok(())
    }

    async fn claim_due(
        &self,
        now: OffsetDateTime,
        lease: Duration,
        limit: i64,
    ) -> StoreResult<Vec<(i64, Kind)>> {
        let now = micros(now);
        let mut tables = self.shared.lock();
        let mut due: Vec<(OffsetDateTime, i64, String)> = tables
            .wealth_sync
            .iter()
            .filter(|(_, row)| row.next_poll_at <= now)
            .filter(|((id, _), _)| tables.characters.get(id).is_some_and(|c| !c.needs_relogin))
            .map(|((id, kind), row)| (row.next_poll_at, *id, kind.clone()))
            .collect();
        due.sort();
        due.truncate(limit.max(0) as usize);
        let mut claimed = Vec::new();
        for (_, id, kind) in due {
            if let Some(row) = tables.wealth_sync.get_mut(&(id, kind.clone())) {
                row.next_poll_at = now + lease;
            }
            if let Some(kind) = Kind::parse(&kind) {
                claimed.push((id, kind));
            }
        }
        Ok(claimed)
    }
    async fn wealth_sync(&self, character_id: i64, kind: Kind) -> StoreResult<Option<SyncState>> {
        Ok(self
            .shared
            .lock()
            .wealth_sync
            .get(&(character_id, kind.as_str().to_owned()))
            .map(Sync::state))
    }

    async fn seed_full_read(&self, character_id: i64, full_read: bool) -> StoreResult<()> {
        if let Some(row) = self
            .shared
            .lock()
            .wealth_sync
            .get_mut(&(character_id, "transactions".to_owned()))
        {
            row.full_read = full_read;
        }
        Ok(())
    }

    async fn seed_inventory_value(
        &self,
        character_id: i64,
        recorded_at: OffsetDateTime,
        value_cents: i64,
    ) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        require_character(&tables, character_id)?;
        tables
            .inventory_values
            .insert((character_id, micros(recorded_at)), i128::from(value_cents));
        Ok(())
    }
    async fn inventory_values(&self, character_id: i64) -> StoreResult<Vec<(OffsetDateTime, i64)>> {
        let tables = self.shared.lock();
        Ok(tables
            .inventory_values
            .iter()
            .filter(|((id, _), _)| *id == character_id)
            .map(|((_, at), value)| (*at, to_cents(*value)))
            .collect())
    }

    async fn seed_asset(
        &self,
        character_id: i64,
        a: &EsiAsset,
        top_location_id: Option<i64>,
    ) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        require_character(&tables, character_id)?;
        tables.current_assets.insert(
            a.item_id,
            AssetRow {
                character_id,
                type_id: a.type_id,
                location_id: a.location_id,
                location_type: a.location_type.clone(),
                location_flag: a.location_flag.clone(),
                quantity: a.quantity,
                is_singleton: a.is_singleton,
                is_blueprint_copy: a.is_blueprint_copy,
                top_location_id,
            },
        );
        Ok(())
    }

    async fn seed_poll_totals(&self, character_id: i64, t: &PollTotalsRow) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        require_character(&tables, character_id)?;
        tables.order_poll_totals.insert(
            (character_id, micros(t.polled_at)),
            PollTotals {
                sell_value: i128::from(t.sell_cents),
                escrow: i128::from(t.escrow_cents),
                sell_corp: i128::from(t.sell_corp_cents),
                escrow_corp: i128::from(t.escrow_corp_cents),
                sell_count: t.sell_count,
                buy_count: t.buy_count,
            },
        );
        Ok(())
    }
    async fn wallet_balances(&self, character_id: i64) -> StoreResult<Vec<(OffsetDateTime, i64)>> {
        let tables = self.shared.lock();
        Ok(tables
            .wallet_balances
            .iter()
            .filter(|((id, _), _)| *id == character_id)
            .map(|((_, at), cents)| (*at, *cents))
            .collect())
    }

    async fn asset_ids(&self, character_id: i64) -> StoreResult<Vec<i64>> {
        let tables = self.shared.lock();
        Ok(tables
            .current_assets
            .iter()
            .filter(|(_, asset)| asset.character_id == character_id)
            .map(|(item_id, _)| *item_id)
            .collect())
    }
}
