//! The personal purchases and sales, matched first in, first out, and what the sales earned.

use sekondo::{
    esi::EsiTransaction,
    store::{NameRow, RememberedSellOrder, StoreError, Stores, TradeSeed},
    wealth_poller::Kind,
};
use time::{Duration, OffsetDateTime, macros::datetime};

use super::{
    Case,
    orders::jita_fees,
    support::{STATION, account_with, boxed, case, now},
};

pub fn cases() -> Vec<Case> {
    vec![
        case(
            "trades/trades-are-listed-newest-first-within-the-range",
            |s| boxed(listing(s)),
        ),
        case(
            "trades/a-selection-never-reaches-another-accounts-trades",
            |s| boxed(account_boundary(s)),
        ),
        case(
            "trades/a-sale-shows-its-margin-at-the-rates-of-its-location",
            |s| boxed(sale_margin(s)),
        ),
        case(
            "trades/a-sale-the-purchases-do-not-cover-has-no-margin",
            |s| boxed(partly_covered(s)),
        ),
        case("trades/sales-take-the-oldest-purchases-first", |s| {
            boxed(first_in_first_out(s))
        }),
        case(
            "trades/seeded-trades-are-settled-on-demand-and-their-costs-listed",
            |s| boxed(seeded_trades(s)),
        ),
        case("trades/the-transactions-freshness-follows-the-poll", |s| {
            boxed(freshness(s))
        }),
    ]
}

fn at(
    t: OffsetDateTime,
    id: i64,
    is_buy: bool,
    quantity: i64,
    price_cents: i64,
    minutes: i64,
) -> EsiTransaction {
    EsiTransaction {
        transaction_id: id,
        bought_at: t + Duration::minutes(minutes),
        type_id: 34,
        location_id: STATION,
        quantity,
        unit_price_cents: price_cents,
        is_buy,
        is_personal: true,
    }
}

async fn store(stores: &Stores, character: i64, transactions: &[EsiTransaction]) {
    let t = now();
    stores
        .wealth
        .store_transactions(character, t, transactions, t, None)
        .await
        .unwrap();
}

async fn listing(stores: &Stores) {
    let account = account_with(stores, &[1, 2]).await;
    let t = now() - Duration::days(2);
    stores
        .market
        .store_names(
            &[
                NameRow {
                    id: 34,
                    category: "t".to_owned(),
                    name: Some("Tritanium".to_owned()),
                },
                NameRow {
                    id: STATION,
                    category: "s".to_owned(),
                    name: Some("Jita 4-4".to_owned()),
                },
            ],
            t,
            None,
        )
        .await
        .unwrap();
    store(
        stores,
        1,
        &[
            at(t, 1, true, 10, 8_000, 0),
            at(t, 2, false, 5, 10_000, 60),
            at(t, 3, true, 1, 100, 60),
        ],
    )
    .await;
    store(stores, 2, &[at(t, 4, false, 1, 1, 30)]).await;

    let all = stores
        .trades
        .list_trades(account, &[1, 2], t, t + Duration::hours(2))
        .await
        .unwrap();

    let ids: Vec<i64> = all.iter().map(|r| r.transaction_id).collect();
    assert_eq!(
        ids,
        [3, 2, 4, 1],
        "newest first, the higher ID first among equal times"
    );
    let sale = all.iter().find(|r| r.transaction_id == 2).unwrap();
    assert!(sale.is_sale);
    assert_eq!(
        (sale.character_id, sale.character_name.as_str()),
        (1, "Pilot 1")
    );
    assert_eq!(
        (sale.type_id, sale.type_name.as_deref()),
        (34, Some("Tritanium"))
    );
    assert_eq!(
        (sale.location_id, sale.location_name.as_deref()),
        (STATION, Some("Jita 4-4"))
    );
    assert_eq!((sale.quantity, sale.unit_price_cents), (5, 10_000));
    assert_eq!(sale.at, t + Duration::minutes(60));
    let purchase = all.iter().find(|r| r.transaction_id == 1).unwrap();
    assert!(!purchase.is_sale);
    assert_eq!(
        (
            purchase.margin_cents,
            purchase.covered_units,
            purchase.cost_per_unit_cents
        ),
        (None, None, None)
    );
    // The range is inclusive on both ends.
    let edge = stores
        .trades
        .list_trades(
            account,
            &[1, 2],
            t + Duration::minutes(30),
            t + Duration::minutes(60),
        )
        .await
        .unwrap();
    assert_eq!(
        edge.iter().map(|r| r.transaction_id).collect::<Vec<_>>(),
        [3, 2, 4]
    );
    let one = stores
        .trades
        .list_trades(account, &[2], t, t + Duration::hours(2))
        .await
        .unwrap();
    assert_eq!(one.len(), 1);
    assert!(
        stores
            .trades
            .list_trades(
                account,
                &[1, 2],
                t + Duration::days(1),
                t + Duration::days(2)
            )
            .await
            .unwrap()
            .is_empty()
    );
}

async fn account_boundary(stores: &Stores) {
    let mine = account_with(stores, &[1]).await;
    let theirs = account_with(stores, &[2]).await;
    let t = now() - Duration::hours(1);
    store(stores, 1, &[at(t, 1, true, 1, 100, 0)]).await;
    store(stores, 2, &[at(t, 2, true, 1, 100, 0)]).await;

    let listed = stores
        .trades
        .list_trades(mine, &[1, 2], t - Duration::hours(1), now())
        .await
        .unwrap();

    assert_eq!(
        listed.iter().map(|r| r.transaction_id).collect::<Vec<_>>(),
        [1]
    );
    let nothing = stores
        .trades
        .list_trades(theirs, &[1], t - Duration::hours(1), now())
        .await
        .unwrap();
    assert!(nothing.is_empty());
}

async fn sale_margin(stores: &Stores) {
    let account = account_with(stores, &[1]).await;
    jita_fees(stores, 1).await;
    let t = now() - Duration::days(2);
    // Ten bought at 80.00, four sold at 100.00.
    store(
        stores,
        1,
        &[at(t, 1, true, 10, 8_000, 0), at(t, 2, false, 4, 10_000, 60)],
    )
    .await;

    let listed = stores
        .trades
        .list_trades(account, &[1], t, t + Duration::hours(2))
        .await
        .unwrap();

    let sale = listed.iter().find(|r| r.is_sale).unwrap();
    assert_eq!(sale.covered_units, Some(4));
    assert_eq!(sale.cost_per_unit_cents, Some(8_000));
    // 4 × 100.00 × (1 − 3.375% − 1.50%) − 4 × 80.00 = 380.50 − 320.00.
    assert_eq!(sale.margin_cents, Some(6_050));

    // Another station's rates apply to a sale made there: its owner is not known.
    let mut elsewhere = at(t, 3, false, 2, 10_000, 90);
    elsewhere.location_id = 60_000_001;
    store(stores, 1, &[elsewhere]).await;
    let listed = stores
        .trades
        .list_trades(account, &[1], t, t + Duration::hours(2))
        .await
        .unwrap();
    let sale = listed.iter().find(|r| r.transaction_id == 3).unwrap();
    assert_eq!(sale.covered_units, Some(2));
    assert_eq!(
        sale.margin_cents, None,
        "no broker fee without the station's owner"
    );
    assert_eq!(sale.cost_per_unit_cents, Some(8_000));
}

async fn partly_covered(stores: &Stores) {
    let account = account_with(stores, &[1]).await;
    jita_fees(stores, 1).await;
    let t = now() - Duration::days(2);
    // 3 bought, 5 sold, and a sale before any purchase.
    store(
        stores,
        1,
        &[
            at(t, 1, false, 2, 10_000, -60),
            at(t, 2, true, 3, 8_000, 0),
            at(t, 3, false, 5, 10_000, 60),
        ],
    )
    .await;

    let listed = stores
        .trades
        .list_trades(
            account,
            &[1],
            t - Duration::hours(2),
            t + Duration::hours(2),
        )
        .await
        .unwrap();

    let early = listed.iter().find(|r| r.transaction_id == 1).unwrap();
    assert_eq!(
        (
            early.covered_units,
            early.margin_cents,
            early.cost_per_unit_cents
        ),
        (Some(0), None, None)
    );
    let late = listed.iter().find(|r| r.transaction_id == 3).unwrap();
    assert_eq!(
        (
            late.covered_units,
            late.margin_cents,
            late.cost_per_unit_cents
        ),
        (Some(3), None, None)
    );
}

async fn first_in_first_out(stores: &Stores) {
    let account = account_with(stores, &[1]).await;
    jita_fees(stores, 1).await;
    let t = now() - Duration::days(5);
    // Bought 10 at 80.00, then 10 at 120.00; sold 15 and then 5.
    store(
        stores,
        1,
        &[
            at(t, 1, true, 10, 8_000, 0),
            at(t, 2, true, 10, 12_000, 60),
            at(t, 3, false, 15, 15_000, 120),
            at(t, 4, false, 5, 15_000, 180),
        ],
    )
    .await;

    let listed = stores
        .trades
        .list_trades(account, &[1], t, t + Duration::hours(4))
        .await
        .unwrap();

    let sale = |id: i64| listed.iter().find(|r| r.transaction_id == id).unwrap();
    // 15: 10 × 80.00 + 5 × 120.00 = 1400.00, 93.33 each (round half away from zero).
    assert_eq!(
        (sale(3).covered_units, sale(3).cost_per_unit_cents),
        (Some(15), Some(9_333))
    );
    // 5: 5 × 120.00.
    assert_eq!(
        (sale(4).covered_units, sale(4).cost_per_unit_cents),
        (Some(5), Some(12_000))
    );
    // 15 × 150.00 × 0.95125 − 1400.00 = 2140.3125 − 1400.00 = 740.3125, to cents.
    assert_eq!(sale(3).margin_cents, Some(74_031));
    // 5 × 150.00 × 0.95125 − 600.00 = 713.4375 − 600.00 = 113.4375.
    assert_eq!(sale(4).margin_cents, Some(11_344));
}

async fn freshness(stores: &Stores) {
    let account = account_with(stores, &[1, 2]).await;
    let t = now();
    stores
        .wealth
        .store_transactions(1, t, &[], t + Duration::hours(1), None)
        .await
        .unwrap();
    stores.wealth.store_wallet(2, t, 1, t, None).await.unwrap();
    stores.accounts.mark_needs_relogin(2).await.unwrap();

    let records = stores
        .trades
        .list_transactions_freshness(account, &[1, 2, 9])
        .await
        .unwrap();

    let summary: Vec<_> = records
        .iter()
        .map(|r| {
            (
                r.character_id,
                r.needs_relogin,
                r.last_success_at,
                r.next_poll_at,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (1, false, Some(t), Some(t + Duration::hours(1))),
            (2, true, None, None)
        ],
        "only the transactions poll counts"
    );
    let _ = Kind::Wallet;
}

async fn seeded_trades(stores: &Stores) {
    let account = account_with(stores, &[1, 2]).await;
    let t = datetime!(2026-10-20 12:00 UTC);
    let trade = |id: i64, is_sale: bool, quantity: i64, price: i64, day: i64| TradeSeed {
        transaction_id: id,
        is_sale,
        type_id: 34,
        location_id: STATION,
        quantity,
        unit_price_cents: price,
        at: t - Duration::days(day),
    };
    let order = |id: i64, volume: i64, day: i64| RememberedSellOrder {
        order_id: id,
        type_id: 34,
        first_issued: t - Duration::days(day),
        volume_total: volume,
    };
    for row in [
        trade(1, false, 10, 10_000, 5),
        trade(2, false, 10, 12_000, 4),
    ] {
        stores.trades.seed_trade(1, &row).await.unwrap();
    }
    stores
        .trades
        .seed_trade(2, &trade(3, false, 99, 1, 5))
        .await
        .unwrap();
    stores
        .trades
        .seed_remembered_order(1, &order(11, 15, 3))
        .await
        .unwrap();
    stores
        .trades
        .seed_remembered_order(1, &order(12, 10, 2))
        .await
        .unwrap();
    stores
        .trades
        .seed_trade(1, &trade(31, true, 12, 20_000, 1))
        .await
        .unwrap();

    // Nothing is settled until it is asked for.
    assert!(stores.trades.order_costs().await.unwrap().is_empty());
    assert!(stores.trades.sale_costs().await.unwrap().is_empty());
    stores.trades.settle(1, &[34]).await.unwrap();
    assert_eq!(
        stores.trades.order_costs().await.unwrap(),
        [(11, 15, 160_000), (12, 5, 60_000)]
    );
    assert_eq!(
        stores.trades.sale_costs().await.unwrap(),
        [(31, 12, 124_000)]
    );

    // A seeded transaction is replaced as a whole, a settled item is left to the next settling,
    // and an empty list of items settles nothing.
    stores
        .trades
        .seed_trade(1, &trade(31, true, 2, 20_000, 1))
        .await
        .unwrap();
    stores.trades.settle(1, &[]).await.unwrap();
    assert_eq!(
        stores.trades.sale_costs().await.unwrap(),
        [(31, 12, 124_000)]
    );
    stores.trades.settle(1, &[34]).await.unwrap();
    assert_eq!(stores.trades.sale_costs().await.unwrap(), [(31, 2, 20_000)]);
    let listed = stores
        .trades
        .list_trades(account, &[1, 2], t - Duration::days(30), t)
        .await
        .unwrap();
    assert_eq!(listed.len(), 4);

    // Costs can be planted, and only for what exists.
    stores.trades.seed_order_cost(11, 7, 700).await.unwrap();
    stores.trades.seed_sale_cost(31, 1, 100).await.unwrap();
    assert_eq!(stores.trades.order_costs().await.unwrap()[0], (11, 7, 700));
    assert_eq!(stores.trades.sale_costs().await.unwrap(), [(31, 1, 100)]);
    for missing in [
        stores.trades.seed_order_cost(404, 1, 1).await,
        stores.trades.seed_sale_cost(404, 1, 1).await,
        stores
            .trades
            .seed_trade(404, &trade(9, false, 1, 1, 1))
            .await,
        stores
            .trades
            .seed_remembered_order(404, &order(9, 1, 1))
            .await,
    ] {
        assert!(
            matches!(missing, Err(StoreError::MissingParent)),
            "{missing:?}"
        );
    }

    // A sale goes with its cost; one that is not there is no error.
    stores.trades.delete_sale(31).await.unwrap();
    stores.trades.delete_sale(31).await.unwrap();
    assert!(stores.trades.sale_costs().await.unwrap().is_empty());
}
