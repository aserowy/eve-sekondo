use std::collections::BTreeMap;

use async_trait::async_trait;
use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    Shared,
    fee::rates,
    micros,
    numeric::Numeric,
    tables::{CostRow, SellOrderRow, Tables, TransactionRow},
};
use crate::{
    freshness::FreshnessRecord,
    store::{RememberedSellOrder, StoreError, StoreResult, TradeSeed, TradeStore},
    trade::{Lot, RememberedOrder, Sale, allocate, allocate_sales},
    trades::TradeRow,
};

pub struct MemTradeStore {
    pub(super) shared: std::sync::Arc<Shared>,
}

/// Redoes both allocations of one character for these items from all stored lots, remembered
/// orders and sales, and replaces their stored costs.
pub(super) fn recompute(tables: &mut Tables, character_id: i64, type_ids: &[i64]) {
    if type_ids.is_empty() {
        return;
    }
    #[derive(Default)]
    struct OfType {
        lots: Vec<Lot>,
        orders: Vec<RememberedOrder>,
        sales: Vec<Sale>,
    }
    let mut per_type: BTreeMap<i64, OfType> = BTreeMap::new();
    for (id, row) in &tables.purchases {
        if row.character_id == character_id && type_ids.contains(&row.type_id) {
            per_type.entry(row.type_id).or_default().lots.push(Lot {
                transaction_id: *id,
                bought_at: row.at,
                quantity: row.quantity,
                unit_price_cents: row.unit_price_cents,
            });
        }
    }
    for (id, row) in &tables.sell_order_history {
        if row.character_id == character_id && type_ids.contains(&row.type_id) {
            per_type
                .entry(row.type_id)
                .or_default()
                .orders
                .push(RememberedOrder {
                    order_id: *id,
                    first_issued: row.first_issued,
                    volume_total: row.volume_total,
                });
        }
    }
    for (id, row) in &tables.sales {
        if row.character_id == character_id && type_ids.contains(&row.type_id) {
            per_type.entry(row.type_id).or_default().sales.push(Sale {
                transaction_id: *id,
                sold_at: row.at,
                quantity: row.quantity,
            });
        }
    }

    let orders: Vec<i64> = tables
        .sell_order_history
        .iter()
        .filter(|(_, o)| o.character_id == character_id && type_ids.contains(&o.type_id))
        .map(|(id, _)| *id)
        .collect();
    for id in orders {
        tables.sell_order_costs.remove(&id);
    }
    let sales: Vec<i64> = tables
        .sales
        .iter()
        .filter(|(_, s)| s.character_id == character_id && type_ids.contains(&s.type_id))
        .map(|(id, _)| *id)
        .collect();
    for id in sales {
        tables.sale_costs.remove(&id);
    }
    for of_type in per_type.values() {
        for cost in allocate(&of_type.lots, &of_type.orders) {
            tables.sell_order_costs.insert(
                cost.order_id,
                CostRow {
                    covered_units: cost.covered_units,
                    cost_cents: cost.cost_cents,
                },
            );
        }
        for cost in allocate_sales(&of_type.lots, &of_type.sales) {
            tables.sale_costs.insert(
                cost.transaction_id,
                CostRow {
                    covered_units: cost.covered_units,
                    cost_cents: cost.cost_cents,
                },
            );
        }
    }
}

fn name_of(tables: &Tables, id: i64) -> Option<String> {
    tables.names.get(&id).and_then(|n| n.name.clone())
}

/// `round(Q × P × (1 − t/100 − b/100) − cost, 2) × 100` as a `bigint`, with the SQL's numeric
/// arithmetic.
fn sale_margin(
    quantity: i64,
    unit_price_cents: i64,
    sales_tax: &Numeric,
    broker_fee: &Numeric,
    cost_cents: i128,
) -> Option<i64> {
    let hundred = Numeric::int(100);
    let rate = Numeric::int(1)
        .sub(&sales_tax.div(&hundred)?)
        .sub(&broker_fee.div(&hundred)?);
    let proceeds = Numeric::int(quantity)
        .mul(&Numeric::cents(unit_price_cents))
        .mul(&rate);
    proceeds
        .sub(&Numeric::cents(cost_cents))
        .round(2)
        .mul(&hundred)
        .to_i64()
}

#[async_trait]
impl TradeStore for MemTradeStore {
    async fn list_trades(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        from: OffsetDateTime,
        to: OffsetDateTime,
    ) -> StoreResult<Vec<TradeRow>> {
        let tables = self.shared.lock();
        let account_character = |id: i64| {
            character_ids.contains(&id)
                && tables
                    .characters
                    .get(&id)
                    .is_some_and(|c| c.account_id == account_id)
        };
        let mut rows = Vec::new();
        for (id, p) in &tables.purchases {
            if account_character(p.character_id) && (from..=to).contains(&p.at) {
                rows.push(TradeRow {
                    transaction_id: *id,
                    is_sale: false,
                    character_id: p.character_id,
                    character_name: tables.characters[&p.character_id].name.clone(),
                    type_id: p.type_id,
                    type_name: name_of(&tables, p.type_id),
                    location_id: p.location_id,
                    location_name: name_of(&tables, p.location_id),
                    quantity: p.quantity,
                    unit_price_cents: p.unit_price_cents,
                    at: p.at,
                    margin_cents: None,
                    covered_units: None,
                    cost_per_unit_cents: None,
                });
            }
        }
        for (id, s) in &tables.sales {
            if account_character(s.character_id) && (from..=to).contains(&s.at) {
                let cost = tables.sale_costs.get(id);
                let covered = cost.filter(|c| c.covered_units == s.quantity);
                let (tax, fee) = rates(&tables, s.character_id, s.location_id);
                let margin = match (covered, &tax, &fee) {
                    (Some(c), Some(tax), Some(fee)) => {
                        sale_margin(s.quantity, s.unit_price_cents, tax, fee, c.cost_cents)
                    }
                    _ => None,
                };
                let per_unit = covered.filter(|_| s.quantity > 0).and_then(|c| {
                    Numeric::cents(c.cost_cents)
                        .mul(&Numeric::int(100))
                        .div(&Numeric::int(s.quantity))?
                        .to_i64()
                });
                rows.push(TradeRow {
                    transaction_id: *id,
                    is_sale: true,
                    character_id: s.character_id,
                    character_name: tables.characters[&s.character_id].name.clone(),
                    type_id: s.type_id,
                    type_name: name_of(&tables, s.type_id),
                    location_id: s.location_id,
                    location_name: name_of(&tables, s.location_id),
                    quantity: s.quantity,
                    unit_price_cents: s.unit_price_cents,
                    at: s.at,
                    margin_cents: margin,
                    covered_units: cost.map(|c| c.covered_units),
                    cost_per_unit_cents: per_unit,
                });
            }
        }
        rows.sort_by(|a, b| {
            b.at.cmp(&a.at)
                .then(b.transaction_id.cmp(&a.transaction_id))
        });
        Ok(rows)
    }

    async fn list_transactions_freshness(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
    ) -> StoreResult<Vec<FreshnessRecord>> {
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
                let sync = tables
                    .wealth_sync
                    .get(&(c.character_id, "transactions".to_owned()));
                FreshnessRecord {
                    character_id: c.character_id,
                    name: c.name.clone(),
                    needs_relogin: c.needs_relogin,
                    last_success_at: sync.and_then(|s| s.last_success_at),
                    next_poll_at: sync.map(|s| s.next_poll_at),
                }
            })
            .collect())
    }

    async fn seed_trade(&self, character_id: i64, t: &TradeSeed) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        if !tables.characters.contains_key(&character_id) {
            return Err(StoreError::MissingParent);
        }
        let row = TransactionRow {
            character_id,
            type_id: t.type_id,
            location_id: t.location_id,
            quantity: t.quantity,
            unit_price_cents: t.unit_price_cents,
            at: micros(t.at),
        };
        if t.is_sale {
            tables.sales.insert(t.transaction_id, row);
        } else {
            tables.purchases.insert(t.transaction_id, row);
        }
        Ok(())
    }

    async fn seed_remembered_order(
        &self,
        character_id: i64,
        order: &RememberedSellOrder,
    ) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        if !tables.characters.contains_key(&character_id) {
            return Err(StoreError::MissingParent);
        }
        tables.sell_order_history.insert(
            order.order_id,
            SellOrderRow {
                character_id,
                type_id: order.type_id,
                first_issued: micros(order.first_issued),
                volume_total: order.volume_total,
            },
        );
        Ok(())
    }

    async fn seed_order_cost(
        &self,
        order_id: i64,
        covered_units: i64,
        cost_cents: i64,
    ) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        if !tables.sell_order_history.contains_key(&order_id) {
            return Err(StoreError::MissingParent);
        }
        tables.sell_order_costs.insert(
            order_id,
            CostRow {
                covered_units,
                cost_cents: i128::from(cost_cents),
            },
        );
        Ok(())
    }

    async fn seed_sale_cost(
        &self,
        transaction_id: i64,
        covered_units: i64,
        cost_cents: i64,
    ) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        if !tables.sales.contains_key(&transaction_id) {
            return Err(StoreError::MissingParent);
        }
        tables.sale_costs.insert(
            transaction_id,
            CostRow {
                covered_units,
                cost_cents: i128::from(cost_cents),
            },
        );
        Ok(())
    }

    async fn delete_sale(&self, transaction_id: i64) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        tables.sales.remove(&transaction_id);
        tables.sale_costs.remove(&transaction_id);
        Ok(())
    }

    async fn settle(&self, character_id: i64, type_ids: &[i64]) -> StoreResult<()> {
        recompute(&mut self.shared.lock(), character_id, type_ids);
        Ok(())
    }

    async fn order_costs(&self) -> StoreResult<Vec<(i64, i64, i64)>> {
        let tables = self.shared.lock();
        Ok(tables
            .sell_order_costs
            .iter()
            .map(|(id, c)| (*id, c.covered_units, c.cost_cents as i64))
            .collect())
    }

    async fn sale_costs(&self) -> StoreResult<Vec<(i64, i64, i64)>> {
        let tables = self.shared.lock();
        Ok(tables
            .sale_costs
            .iter()
            .map(|(id, c)| (*id, c.covered_units, c.cost_cents as i64))
            .collect())
    }
}
