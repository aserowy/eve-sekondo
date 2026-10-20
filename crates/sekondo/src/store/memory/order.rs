use std::cmp::Ordering;

use async_trait::async_trait;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::{
    Shared, backoff, bucket_ends,
    fee::rates,
    market::{Standing, standing},
    micros,
    numeric::Numeric,
    tables::{OrderRow, PollTotals, Sync, Tables},
    trade::recompute,
};
use crate::{
    dashboard::{CharacterFigures, HistoryPoint, TopItem},
    esi::EsiOrder,
    freshness::FreshnessRecord,
    orders::{Dir, OrderRow as OrderView, Side, Sort, SortKey},
    range::Range,
    store::{OrderStore, PollTotalsRow, RememberedSellOrder, StoreError, StoreResult, SyncState},
};

pub struct MemOrderStore {
    pub(super) shared: std::sync::Arc<Shared>,
}

/// The characters of the account among `character_ids`, in link order.
fn selected(tables: &Tables, account_id: Uuid, character_ids: &[i64]) -> Vec<i64> {
    let mut characters: Vec<_> = tables
        .characters
        .values()
        .filter(|c| c.account_id == account_id && character_ids.contains(&c.character_id))
        .collect();
    characters.sort_by_key(|c| (c.created_at, c.character_id));
    characters.into_iter().map(|c| c.character_id).collect()
}

fn name_of(tables: &Tables, id: i64) -> Option<String> {
    tables.names.get(&id).and_then(|n| n.name.clone())
}

fn lowered(name: Option<String>, id: i64) -> String {
    name.unwrap_or_else(|| format!("#{id}")).to_lowercase()
}

fn hundred() -> Numeric {
    Numeric::int(100)
}

/// `round(x, 2) × 100` as a `bigint`.
fn hundredths(x: &Numeric) -> Option<i64> {
    x.round(2).mul(&hundred()).to_i64()
}

/// What the orders page shows of one order, with its standing and what it cost.
struct Listed {
    view: OrderView,
    standing: Standing,
    amount_cents: i128,
}

fn cost_of(tables: &Tables, o: &OrderRow, order_id: i64, standing: Standing) -> Listed {
    let personal_sell = !o.is_buy_order && !o.is_corporation;
    let sc = tables.sell_order_costs.get(&order_id);
    let covered_all = sc.filter(|c| c.covered_units == o.volume_total);
    let (tax, fee) = rates(tables, o.character_id, o.location_id);
    let cost = sc.map(|c| Numeric::cents(c.cost_cents));

    let margin_cents = (|| {
        if !personal_sell {
            return None;
        }
        covered_all?;
        let rate = Numeric::int(1)
            .sub(&tax.as_ref()?.div(&hundred())?)
            .sub(&fee.as_ref()?.div(&hundred())?);
        let proceeds = Numeric::int(o.volume_remain)
            .mul(&Numeric::cents(o.price_cents))
            .mul(&rate);
        let cost_part = Numeric::int(o.volume_remain)
            .mul(cost.as_ref()?)
            .div(&Numeric::int(o.volume_total))?;
        hundredths(&proceeds.sub(&cost_part))
    })();
    let covers_volume = covered_all.is_some() && o.volume_total > 0;
    let cost_cents = (|| {
        if !(personal_sell && covers_volume) {
            return None;
        }
        let part = Numeric::int(o.volume_remain)
            .mul(cost.as_ref()?)
            .div(&Numeric::int(o.volume_total))?;
        hundredths(&part)
    })();
    let cost_per_unit_cents = (|| {
        if !covers_volume {
            return None;
        }
        cost.as_ref()?
            .mul(&hundred())
            .div(&Numeric::int(o.volume_total))?
            .round(0)
            .to_i64()
    })();

    let value = i128::from(o.price_cents) * i128::from(o.volume_remain);
    let view = OrderView {
        order_id,
        character_id: o.character_id,
        character_name: tables.characters[&o.character_id].name.clone(),
        type_id: o.type_id,
        type_name: name_of(tables, o.type_id),
        region_id: o.region_id,
        region_name: name_of(tables, o.region_id),
        location_id: o.location_id,
        location_name: name_of(tables, o.location_id),
        location_known: tables.names.contains_key(&o.location_id),
        is_corporation: o.is_corporation,
        price_cents: o.price_cents,
        volume_remain: o.volume_remain,
        volume_total: o.volume_total,
        value_cents: i64::try_from(value).expect("a value in cents fits a bigint"),
        escrow_cents: o.escrow_cents,
        issued: o.issued,
        duration: o.duration,
        undercut: standing.undercut,
        best_price_cents: standing.best_price_cents,
        gap_cents: standing.gap_cents(o.price_cents),
        undercut_volume: standing.undercut_volume,
        covered_units: personal_sell.then(|| sc.map(|c| c.covered_units)).flatten(),
        margin_cents,
        cost_per_unit_cents,
        cost_cents,
    };
    Listed {
        view,
        standing,
        amount_cents: if o.is_buy_order {
            i128::from(o.escrow_cents.unwrap_or(0))
        } else {
            value
        },
    }
}

/// `ORDER BY` of the orders list: the sort key in its direction, then the order ID.
fn compare(sort: Sort, a: &Listed, b: &Listed) -> Ordering {
    let direct = |ordering: Ordering| match sort.dir {
        Dir::Asc => ordering,
        Dir::Desc => ordering.reverse(),
    };
    let (x, y) = (&a.view, &b.view);
    let first = match sort.key {
        SortKey::Item => direct(
            lowered(x.type_name.clone(), x.type_id).cmp(&lowered(y.type_name.clone(), y.type_id)),
        ),
        SortKey::Price => direct(x.price_cents.cmp(&y.price_cents)),
        // Orders without a standing last, then "Top" (not undercut) before the undercut ones.
        SortKey::Standing => a
            .standing
            .undercut
            .is_none()
            .cmp(&b.standing.undercut.is_none())
            .then_with(|| {
                direct(
                    a.standing
                        .undercut
                        .map(|n| n > 0)
                        .cmp(&b.standing.undercut.map(|n| n > 0)),
                )
            })
            .then_with(|| direct(a.standing.undercut.cmp(&b.standing.undercut))),
        SortKey::Gap => a
            .standing
            .undercut
            .is_none()
            .cmp(&b.standing.undercut.is_none())
            .then_with(|| {
                let gap = |l: &Listed| {
                    l.standing
                        .best_price_cents
                        .map_or(0, |best| (best - l.view.price_cents).abs())
                };
                direct(gap(a).cmp(&gap(b)))
            }),
        SortKey::UndercutVolume => a
            .standing
            .undercut
            .is_none()
            .cmp(&b.standing.undercut.is_none())
            .then_with(|| {
                direct(
                    a.standing
                        .undercut_volume
                        .unwrap_or(0)
                        .cmp(&b.standing.undercut_volume.unwrap_or(0)),
                )
            }),
        SortKey::Volume => direct(x.volume_remain.cmp(&y.volume_remain)),
        SortKey::Amount => direct(a.amount_cents.cmp(&b.amount_cents)),
        SortKey::Margin => x
            .margin_cents
            .is_none()
            .cmp(&y.margin_cents.is_none())
            .then_with(|| direct(x.margin_cents.cmp(&y.margin_cents))),
        SortKey::TimeLeft => direct(x.expires_at().cmp(&y.expires_at())),
    };
    first.then_with(|| x.order_id.cmp(&y.order_id))
}

fn require_character(tables: &Tables, character_id: i64) -> StoreResult<()> {
    if tables.characters.contains_key(&character_id) {
        Ok(())
    } else {
        Err(StoreError::MissingParent)
    }
}

fn freshness(tables: &Tables, character_id: i64) -> FreshnessRecord {
    let c = &tables.characters[&character_id];
    let sync = tables.order_sync.get(&character_id);
    FreshnessRecord {
        character_id,
        name: c.name.clone(),
        needs_relogin: c.needs_relogin,
        last_success_at: sync.and_then(|s| s.last_success_at),
        next_poll_at: sync.map(|s| s.next_poll_at),
    }
}

fn to_i64(value: i128) -> i64 {
    i64::try_from(value).expect("an amount in cents fits a bigint")
}

#[async_trait]
impl OrderStore for MemOrderStore {
    async fn list_orders(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        side: Side,
        sort: Sort,
    ) -> StoreResult<Vec<OrderView>> {
        let tables = self.shared.lock();
        let selected = selected(&tables, account_id, character_ids);
        let mut listed: Vec<Listed> = tables
            .current_orders
            .iter()
            .filter(|(_, o)| selected.contains(&o.character_id))
            .filter(|(_, o)| o.is_buy_order == (side == Side::Buy))
            .map(|(id, o)| cost_of(&tables, o, *id, standing(&tables, *id, o)))
            .collect();
        listed.sort_by(|a, b| compare(sort, a, b));
        Ok(listed.into_iter().map(|l| l.view).collect())
    }

    async fn list_freshness(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
    ) -> StoreResult<Vec<FreshnessRecord>> {
        let tables = self.shared.lock();
        Ok(selected(&tables, account_id, character_ids)
            .into_iter()
            .map(|id| freshness(&tables, id))
            .collect())
    }

    async fn character_figures(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
    ) -> StoreResult<Vec<CharacterFigures>> {
        let tables = self.shared.lock();
        Ok(selected(&tables, account_id, character_ids)
            .into_iter()
            .map(|id| {
                let orders: Vec<&OrderRow> = tables
                    .current_orders
                    .values()
                    .filter(|o| o.character_id == id)
                    .collect();
                let sell = |corp_only: bool| -> i128 {
                    orders
                        .iter()
                        .filter(|o| !o.is_buy_order && (!corp_only || o.is_corporation))
                        .map(|o| i128::from(o.price_cents) * i128::from(o.volume_remain))
                        .sum()
                };
                let escrow = |corp_only: bool| -> i128 {
                    orders
                        .iter()
                        .filter(|o| o.is_buy_order && (!corp_only || o.is_corporation))
                        .map(|o| i128::from(o.escrow_cents.unwrap_or(0)))
                        .sum()
                };
                CharacterFigures {
                    freshness: freshness(&tables, id),
                    sell_cents: to_i64(sell(false)),
                    escrow_cents: to_i64(escrow(false)),
                    sell_corp_cents: to_i64(sell(true)),
                    escrow_corp_cents: to_i64(escrow(true)),
                    sell_count: orders.iter().filter(|o| !o.is_buy_order).count() as i64,
                    buy_count: orders.iter().filter(|o| o.is_buy_order).count() as i64,
                }
            })
            .collect())
    }

    async fn top_items(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        side: Side,
        limit: i64,
    ) -> StoreResult<Vec<TopItem>> {
        let tables = self.shared.lock();
        let selected = selected(&tables, account_id, character_ids);
        let mut sums: std::collections::BTreeMap<i64, i128> = std::collections::BTreeMap::new();
        for o in tables
            .current_orders
            .values()
            .filter(|o| selected.contains(&o.character_id))
            .filter(|o| o.is_buy_order == (side == Side::Buy))
        {
            *sums.entry(o.type_id).or_default() += match side {
                Side::Sell => i128::from(o.price_cents) * i128::from(o.volume_remain),
                Side::Buy => i128::from(o.escrow_cents.unwrap_or(0)),
            };
        }
        let mut items: Vec<(i128, String, i64, TopItem)> = sums
            .into_iter()
            .map(|(type_id, cents)| {
                let label = name_of(&tables, type_id).unwrap_or_else(|| format!("#{type_id}"));
                (
                    cents,
                    label.to_lowercase(),
                    type_id,
                    TopItem {
                        type_id,
                        label,
                        cents: to_i64(cents),
                    },
                )
            })
            .collect();
        items.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        items.truncate(limit.max(0) as usize);
        Ok(items.into_iter().map(|(_, _, _, item)| item).collect())
    }

    async fn history(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        range: Range,
        now: OffsetDateTime,
    ) -> StoreResult<Vec<HistoryPoint>> {
        let tables = self.shared.lock();
        let characters = selected(&tables, account_id, character_ids);
        if characters.is_empty() {
            return Ok(Vec::new());
        }
        Ok(bucket_ends(range, micros(now))
            .into_iter()
            .map(|at| {
                let mut point = HistoryPoint {
                    at,
                    sell_cents: 0,
                    escrow_cents: 0,
                    sell_count: 0,
                    buy_count: 0,
                    contributors: 0,
                };
                for id in &characters {
                    let latest = tables
                        .order_poll_totals
                        .iter()
                        .rfind(|((c, t), _)| c == id && *t <= at);
                    if let Some((_, totals)) = latest {
                        point.sell_cents += to_i64(totals.sell_value);
                        point.escrow_cents += to_i64(totals.escrow);
                        point.sell_count += totals.sell_count;
                        point.buy_count += totals.buy_count;
                        point.contributors += 1;
                    }
                }
                point
            })
            .collect())
    }

    async fn replace_orders(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        orders: &[EsiOrder],
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<()> {
        let polled_at = micros(polled_at);
        let mut tables = self.shared.lock();
        require_character(&tables, character_id)?;
        let mut seen = std::collections::BTreeSet::new();
        let taken_by_another = orders.iter().any(|o| {
            tables
                .current_orders
                .get(&o.order_id)
                .is_some_and(|existing| existing.character_id != character_id)
        });
        if !orders.iter().all(|o| seen.insert(o.order_id)) || taken_by_another {
            return Err(StoreError::Backend(
                "duplicate key value violates unique constraint \"current_orders_pkey\"".to_owned(),
            ));
        }

        tables
            .current_orders
            .retain(|_, o| o.character_id != character_id);
        for o in orders {
            let row = OrderRow {
                character_id,
                type_id: o.type_id,
                region_id: o.region_id,
                location_id: o.location_id,
                is_buy_order: o.is_buy_order,
                is_corporation: o.is_corporation,
                price_cents: o.price_cents,
                volume_remain: o.volume_remain,
                volume_total: o.volume_total,
                min_volume: o.min_volume,
                range: o.range.clone(),
                escrow_cents: o.escrow_cents,
                issued: micros(o.issued),
                duration: o.duration,
            };
            tables.current_orders.insert(o.order_id, row.clone());
            tables.order_snapshots.push((polled_at, o.order_id, row));
        }

        // Every personal sell order is remembered as first seen.
        let mut new_types = Vec::new();
        let personal: Vec<(i64, OrderRow)> = tables
            .current_orders
            .iter()
            .filter(|(_, o)| o.character_id == character_id && !o.is_buy_order && !o.is_corporation)
            .map(|(id, o)| (*id, o.clone()))
            .collect();
        for (id, o) in personal {
            if let std::collections::btree_map::Entry::Vacant(entry) =
                tables.sell_order_history.entry(id)
            {
                entry.insert(super::tables::SellOrderRow {
                    character_id,
                    type_id: o.type_id,
                    first_issued: o.issued,
                    volume_total: o.volume_total,
                });
                new_types.push(o.type_id);
            }
        }
        new_types.sort_unstable();
        new_types.dedup();
        recompute(&mut tables, character_id, &new_types);

        let mut totals = PollTotals::default();
        for o in tables
            .current_orders
            .values()
            .filter(|o| o.character_id == character_id)
        {
            if o.is_buy_order {
                let escrow = i128::from(o.escrow_cents.unwrap_or(0));
                totals.escrow += escrow;
                totals.buy_count += 1;
                if o.is_corporation {
                    totals.escrow_corp += escrow;
                }
            } else {
                let value = i128::from(o.price_cents) * i128::from(o.volume_remain);
                totals.sell_value += value;
                totals.sell_count += 1;
                if o.is_corporation {
                    totals.sell_corp += value;
                }
            }
        }
        tables
            .order_poll_totals
            .insert((character_id, polled_at), totals);

        let row = tables
            .order_sync
            .entry(character_id)
            .or_insert_with(|| Sync::scheduled(micros(next_poll_at)));
        row.next_poll_at = micros(next_poll_at);
        row.last_success_at = Some(polled_at);
        row.last_attempt_at = Some(polled_at);
        row.last_error = None;
        row.esi_expires_at = esi_expires_at.map(micros);
        row.failures = 0;
        Ok(())
    }

    async fn make_due(&self, character_id: i64, now: OffsetDateTime) -> StoreResult<()> {
        if let Some(row) = self.shared.lock().order_sync.get_mut(&character_id) {
            row.next_poll_at = row.next_poll_at.min(micros(now));
        }
        Ok(())
    }

    async fn record_failure(
        &self,
        character_id: i64,
        now: OffsetDateTime,
        reason: &str,
        backoff_start: Duration,
        interval: Duration,
    ) -> StoreResult<()> {
        let now = micros(now);
        let mut tables = self.shared.lock();
        require_character(&tables, character_id)?;
        match tables.order_sync.get_mut(&character_id) {
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
                tables.order_sync.insert(character_id, row);
            }
        }
        Ok(())
    }

    async fn record_deferral(
        &self,
        character_id: i64,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
    ) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        require_character(&tables, character_id)?;
        let row = tables
            .order_sync
            .entry(character_id)
            .or_insert_with(|| Sync::scheduled(micros(next_poll_at)));
        row.next_poll_at = micros(next_poll_at);
        row.last_attempt_at = Some(micros(now));
        Ok(())
    }

    async fn list_unscheduled(&self) -> StoreResult<Vec<i64>> {
        let tables = self.shared.lock();
        Ok(tables
            .characters
            .values()
            .filter(|c| !c.needs_relogin && !tables.order_sync.contains_key(&c.character_id))
            .map(|c| c.character_id)
            .collect())
    }

    async fn insert_schedule(
        &self,
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
                .order_sync
                .entry(*id)
                .or_insert_with(|| Sync::scheduled(micros(*at)));
        }
        Ok(())
    }

    async fn claim_due(
        &self,
        now: OffsetDateTime,
        lease: Duration,
        limit: i64,
    ) -> StoreResult<Vec<i64>> {
        let now = micros(now);
        let mut tables = self.shared.lock();
        let mut due: Vec<(OffsetDateTime, i64)> = tables
            .order_sync
            .iter()
            .filter(|(_, row)| row.next_poll_at <= now)
            .filter(|(id, _)| tables.characters.get(id).is_some_and(|c| !c.needs_relogin))
            .map(|(id, row)| (row.next_poll_at, *id))
            .collect();
        due.sort();
        due.truncate(limit.max(0) as usize);
        let mut claimed = Vec::new();
        for (_, id) in due {
            if let Some(row) = tables.order_sync.get_mut(&id) {
                row.next_poll_at = now + lease;
            }
            claimed.push(id);
        }
        Ok(claimed)
    }
    async fn order_sync(&self, character_id: i64) -> StoreResult<Option<SyncState>> {
        Ok(self
            .shared
            .lock()
            .order_sync
            .get(&character_id)
            .map(Sync::state))
    }
    async fn snapshot_counts(&self, character_id: i64) -> StoreResult<Vec<(OffsetDateTime, i64)>> {
        let tables = self.shared.lock();
        let mut counts: std::collections::BTreeMap<OffsetDateTime, i64> =
            std::collections::BTreeMap::new();
        for (polled_at, _, row) in &tables.order_snapshots {
            if row.character_id == character_id {
                *counts.entry(*polled_at).or_default() += 1;
            }
        }
        Ok(counts.into_iter().collect())
    }

    async fn poll_totals(&self, character_id: i64) -> StoreResult<Vec<PollTotalsRow>> {
        let tables = self.shared.lock();
        Ok(tables
            .order_poll_totals
            .iter()
            .filter(|((id, _), _)| *id == character_id)
            .map(|((_, polled_at), t)| PollTotalsRow {
                polled_at: *polled_at,
                sell_cents: to_i64(t.sell_value),
                escrow_cents: to_i64(t.escrow),
                sell_corp_cents: to_i64(t.sell_corp),
                escrow_corp_cents: to_i64(t.escrow_corp),
                sell_count: t.sell_count,
                buy_count: t.buy_count,
            })
            .collect())
    }

    async fn remembered_orders(&self, character_id: i64) -> StoreResult<Vec<RememberedSellOrder>> {
        let tables = self.shared.lock();
        Ok(tables
            .sell_order_history
            .iter()
            .filter(|(_, o)| o.character_id == character_id)
            .map(|(id, o)| RememberedSellOrder {
                order_id: *id,
                type_id: o.type_id,
                first_issued: o.first_issued,
                volume_total: o.volume_total,
            })
            .collect())
    }

    async fn seed_order(&self, character_id: i64, o: &EsiOrder) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        require_character(&tables, character_id)?;
        tables.current_orders.insert(
            o.order_id,
            OrderRow {
                character_id,
                type_id: o.type_id,
                region_id: o.region_id,
                location_id: o.location_id,
                is_buy_order: o.is_buy_order,
                is_corporation: o.is_corporation,
                price_cents: o.price_cents,
                volume_remain: o.volume_remain,
                volume_total: o.volume_total,
                min_volume: o.min_volume,
                range: o.range.clone(),
                escrow_cents: o.escrow_cents,
                issued: micros(o.issued),
                duration: o.duration,
            },
        );
        Ok(())
    }
}
