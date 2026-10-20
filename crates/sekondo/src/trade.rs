//! Trade margin: matching the purchases of a character to its sell orders first in, first out,
//! and storing what each sell order cost.

use time::OffsetDateTime;

/// One purchase of an item: what a sell order can take units from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lot {
    pub transaction_id: i64,
    pub bought_at: OffsetDateTime,
    pub quantity: i64,
    pub unit_price_cents: i64,
}

/// A sell order as first seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RememberedOrder {
    pub order_id: i64,
    pub first_issued: OffsetDateTime,
    pub volume_total: i64,
}

/// The units an order took from the lots and what they cost together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderCost {
    pub order_id: i64,
    pub covered_units: i64,
    /// Cents. A quantity times a unit price can exceed `i64`.
    pub cost_cents: i128,
}

/// A sale: what it can take units from the lots for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sale {
    pub transaction_id: i64,
    pub sold_at: OffsetDateTime,
    pub quantity: i64,
}

/// The units a sale took from the lots and what they cost together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaleCost {
    pub transaction_id: i64,
    pub covered_units: i64,
    /// Cents. A quantity times a unit price can exceed `i64`.
    pub cost_cents: i128,
}

/// Something that takes units from the lots: when it can start taking, and how many it wants.
struct Demand {
    id: i64,
    at: OffsetDateTime,
    wanted: i64,
}

/// What a demand took: the units and their total price in cents.
struct Taken {
    covered_units: i64,
    cost_cents: i128,
}

/// Matches the demands to the lots first in, first out (design §3, §5).
///
/// The demands must be in the order they take. Each takes up to what it wants from the oldest lots
/// that were bought at or before it and that earlier demands left (the transaction ID breaking
/// ties). The result is in the order of the demands.
fn take_fifo(lots: &[Lot], demands: &[Demand]) -> Vec<Taken> {
    let mut lots: Vec<&Lot> = lots.iter().collect();
    lots.sort_by_key(|lot| (lot.bought_at, lot.transaction_id));

    // A cursor into the lots and what is left of the lot it points at. Demands only get later, so
    // a lot one cannot take yet is also beyond every earlier demand.
    let mut next = 0;
    let mut left = lots.first().map_or(0, |lot| lot.quantity);
    demands
        .iter()
        .map(|demand| {
            let mut wanted = demand.wanted;
            let mut cost_cents: i128 = 0;
            while wanted > 0 && next < lots.len() && lots[next].bought_at <= demand.at {
                let taken = wanted.min(left);
                cost_cents += i128::from(taken) * i128::from(lots[next].unit_price_cents);
                wanted -= taken;
                left -= taken;
                if left == 0 {
                    next += 1;
                    left = lots.get(next).map_or(0, |lot| lot.quantity);
                }
            }
            Taken {
                covered_units: demand.wanted - wanted,
                cost_cents,
            }
        })
        .collect()
}

/// Matches the lots and the orders of one character and one item (design §5).
///
/// The orders go in the order they were first issued (the order ID breaking ties). Each takes up to
/// its total volume from the oldest lots that were bought at or before it was issued and that
/// earlier orders left (the transaction ID breaking ties). The result is in that order.
pub fn allocate(lots: &[Lot], orders: &[RememberedOrder]) -> Vec<OrderCost> {
    let mut orders: Vec<&RememberedOrder> = orders.iter().collect();
    orders.sort_by_key(|order| (order.first_issued, order.order_id));
    let demands: Vec<Demand> = orders
        .iter()
        .map(|order| Demand {
            id: order.order_id,
            at: order.first_issued,
            wanted: order.volume_total,
        })
        .collect();
    demands
        .iter()
        .zip(take_fifo(lots, &demands))
        .map(|(demand, taken)| OrderCost {
            order_id: demand.id,
            covered_units: taken.covered_units,
            cost_cents: taken.cost_cents,
        })
        .collect()
}

/// Matches the lots and the sales of one character and one item, apart from the orders (design §3).
///
/// The sales go in the order of their date (the transaction ID breaking ties). Each takes up to its
/// quantity from the oldest lots bought at or before it that earlier sales left. The result is in
/// that order.
pub fn allocate_sales(lots: &[Lot], sales: &[Sale]) -> Vec<SaleCost> {
    let mut sales: Vec<&Sale> = sales.iter().collect();
    sales.sort_by_key(|sale| (sale.sold_at, sale.transaction_id));
    let demands: Vec<Demand> = sales
        .iter()
        .map(|sale| Demand {
            id: sale.transaction_id,
            at: sale.sold_at,
            wanted: sale.quantity,
        })
        .collect();
    demands
        .iter()
        .zip(take_fifo(lots, &demands))
        .map(|(demand, taken)| SaleCost {
            transaction_id: demand.id,
            covered_units: taken.covered_units,
            cost_cents: taken.cost_cents,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use super::*;

    fn lot(id: i64, bought_at: OffsetDateTime, quantity: i64, price: i64) -> Lot {
        Lot {
            transaction_id: id,
            bought_at,
            quantity,
            unit_price_cents: price * 100,
        }
    }

    fn order(id: i64, first_issued: OffsetDateTime, volume: i64) -> RememberedOrder {
        RememberedOrder {
            order_id: id,
            first_issued,
            volume_total: volume,
        }
    }

    /// `(order, covered units, cost in whole ISK)`.
    fn summary(costs: &[OrderCost]) -> Vec<(i64, i64, i128)> {
        costs
            .iter()
            .map(|c| (c.order_id, c.covered_units, c.cost_cents / 100))
            .collect()
    }

    const DAY1: OffsetDateTime = datetime!(2026-10-01 12:00 UTC);
    const DAY2: OffsetDateTime = datetime!(2026-10-02 12:00 UTC);
    const DAY3: OffsetDateTime = datetime!(2026-10-03 12:00 UTC);
    const DAY4: OffsetDateTime = datetime!(2026-10-04 12:00 UTC);
    const DAY5: OffsetDateTime = datetime!(2026-10-05 12:00 UTC);

    #[test]
    fn one_order_takes_from_two_lots_oldest_first() {
        let lots = [lot(1, DAY1, 10, 100), lot(2, DAY2, 10, 120)];

        let costs = allocate(&lots, &[order(1, DAY3, 15)]);

        assert_eq!(summary(&costs), [(1, 15, 1_600)]);
    }

    #[test]
    fn a_later_order_gets_what_is_left() {
        let lots = [lot(1, DAY1, 10, 100), lot(2, DAY2, 10, 120)];

        let costs = allocate(&lots, &[order(1, DAY3, 15), order(2, DAY4, 10)]);

        assert_eq!(summary(&costs), [(1, 15, 1_600), (2, 5, 600)]);
    }

    #[test]
    fn purchases_after_the_order_are_not_taken() {
        let lots = [lot(1, DAY1, 4, 100), lot(2, DAY5, 100, 50)];

        let costs = allocate(&lots, &[order(1, DAY3, 10)]);

        assert_eq!(summary(&costs), [(1, 4, 400)]);
    }

    #[test]
    fn a_purchase_at_the_very_moment_of_the_order_is_taken() {
        let costs = allocate(&[lot(1, DAY3, 10, 100)], &[order(1, DAY3, 10)]);

        assert_eq!(summary(&costs), [(1, 10, 1_000)]);
    }

    #[test]
    fn an_order_without_lots_covers_nothing() {
        let costs = allocate(&[], &[order(1, DAY3, 10)]);

        assert_eq!(summary(&costs), [(1, 0, 0)]);
    }

    #[test]
    fn ties_are_broken_by_the_ids_whatever_the_input_order() {
        // Same moment: the lower transaction ID is the older lot, the lower order ID goes first.
        let lots = [lot(2, DAY1, 5, 200), lot(1, DAY1, 5, 100)];
        let orders = [order(8, DAY2, 5), order(7, DAY2, 5)];

        let costs = allocate(&lots, &orders);

        assert_eq!(summary(&costs), [(7, 5, 500), (8, 5, 1_000)]);
    }

    #[test]
    fn one_lot_is_split_between_orders() {
        let lots = [lot(1, DAY1, 10, 100)];

        let costs = allocate(
            &lots,
            &[order(1, DAY2, 4), order(2, DAY3, 4), order(3, DAY4, 4)],
        );

        assert_eq!(summary(&costs), [(1, 4, 400), (2, 4, 400), (3, 2, 200)]);
    }

    #[test]
    fn an_order_issued_before_a_lot_does_not_hold_back_a_later_order() {
        // The first order cannot take the day 3 lot, the second one can.
        let lots = [lot(1, DAY3, 10, 100)];

        let costs = allocate(&lots, &[order(1, DAY2, 10), order(2, DAY4, 10)]);

        assert_eq!(summary(&costs), [(1, 0, 0), (2, 10, 1_000)]);
    }

    #[test]
    fn costs_are_summed_in_128_bits() {
        // 4 * 10^12 units at 9 * 10^12 cents: 3.6 * 10^25, far above `i64::MAX`.
        let big = Lot {
            transaction_id: 1,
            bought_at: DAY1,
            quantity: 4_000_000_000_000,
            unit_price_cents: 9_000_000_000_000,
        };

        let costs = allocate(&[big], &[order(1, DAY2, 4_000_000_000_000)]);

        assert_eq!(costs[0].cost_cents, 36_000_000_000_000_000_000_000_000);
        assert_eq!(costs[0].covered_units, 4_000_000_000_000);
    }

    fn sale(id: i64, sold_at: OffsetDateTime, quantity: i64) -> Sale {
        Sale {
            transaction_id: id,
            sold_at,
            quantity,
        }
    }

    /// `(sale, covered units, cost in whole ISK)`.
    fn sale_summary(costs: &[SaleCost]) -> Vec<(i64, i64, i128)> {
        costs
            .iter()
            .map(|c| (c.transaction_id, c.covered_units, c.cost_cents / 100))
            .collect()
    }

    #[test]
    fn one_sale_takes_from_two_lots_oldest_first() {
        let lots = [lot(1, DAY1, 10, 100), lot(2, DAY2, 10, 120)];

        let costs = allocate_sales(&lots, &[sale(1, DAY3, 15)]);

        assert_eq!(sale_summary(&costs), [(1, 15, 1_600)]);
    }

    #[test]
    fn a_later_sale_gets_what_is_left() {
        let lots = [lot(1, DAY1, 10, 100), lot(2, DAY2, 10, 120)];

        let costs = allocate_sales(&lots, &[sale(1, DAY3, 15), sale(2, DAY4, 10)]);

        assert_eq!(sale_summary(&costs), [(1, 15, 1_600), (2, 5, 600)]);
    }

    #[test]
    fn purchases_after_the_sale_are_not_taken() {
        let lots = [lot(1, DAY1, 4, 100), lot(2, DAY5, 100, 50)];

        let costs = allocate_sales(&lots, &[sale(1, DAY3, 10)]);

        assert_eq!(sale_summary(&costs), [(1, 4, 400)]);
    }

    #[test]
    fn sale_ties_are_broken_by_the_ids_whatever_the_input_order() {
        let lots = [lot(2, DAY1, 5, 200), lot(1, DAY1, 5, 100)];
        let sales = [sale(8, DAY2, 5), sale(7, DAY2, 5)];

        let costs = allocate_sales(&lots, &sales);

        assert_eq!(sale_summary(&costs), [(7, 5, 500), (8, 5, 1_000)]);
    }

    #[test]
    fn one_lot_is_split_across_sales() {
        let lots = [lot(1, DAY1, 10, 100)];

        let costs = allocate_sales(
            &lots,
            &[sale(1, DAY2, 4), sale(2, DAY3, 4), sale(3, DAY4, 4)],
        );

        assert_eq!(
            sale_summary(&costs),
            [(1, 4, 400), (2, 4, 400), (3, 2, 200)]
        );
    }

    #[test]
    fn a_sale_without_lots_covers_nothing() {
        let costs = allocate_sales(&[], &[sale(1, DAY3, 10)]);

        assert_eq!(sale_summary(&costs), [(1, 0, 0)]);
    }

    #[test]
    fn sale_costs_are_summed_in_128_bits() {
        let big = Lot {
            transaction_id: 1,
            bought_at: DAY1,
            quantity: 4_000_000_000_000,
            unit_price_cents: 9_000_000_000_000,
        };

        let costs = allocate_sales(&[big], &[sale(1, DAY2, 4_000_000_000_000)]);

        assert_eq!(costs[0].cost_cents, 36_000_000_000_000_000_000_000_000);
    }
}
