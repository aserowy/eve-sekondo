//! Names, the public market copy of the pairs the orders use, and the price list.

use sekondo::{
    esi::{EsiAsset, EsiMarketOrder, EsiPrice},
    range::Range,
    store::{NameRow, Stores},
};
use time::{Duration, macros::datetime};

use super::{
    Case,
    support::{REGION, STATION, account_with, boxed, case, now, sell_order},
};

pub fn cases() -> Vec<Case> {
    vec![
        case(
            "market/a-name-is-known-when-resolved-or-not-to-be-asked-again-yet",
            |s| boxed(known_names(s)),
        ),
        case("market/names-are-replaced-by-id-and-one-per-call", |s| {
            boxed(store_names(s))
        }),
        case("market/the-pairs-follow-the-current-orders", |s| {
            boxed(pairs_follow_orders(s))
        }),
        case(
            "market/a-refresh-replaces-the-pairs-orders-and-records-the-state",
            |s| boxed(refresh_replaces(s)),
        ),
        case(
            "market/a-failure-backs-off-and-a-deferral-only-moves-the-poll",
            |s| boxed(pair_failure_and_deferral(s)),
        ),
        case("market/due-pairs-are-claimed-once-and-oldest-first", |s| {
            boxed(claim_pairs(s))
        }),
        case(
            "market/the-first-price-refresh-is-due-at-once-and-leased",
            |s| boxed(claim_prices(s)),
        ),
        case("market/new-prices-revalue-every-inventory", |s| {
            boxed(prices_revalue(s))
        }),
        case("market/the-price-refresh-backs-off-and-defers", |s| {
            boxed(price_failure_and_deferral(s))
        }),
        case("market/the-accounts-with-assets-are-listed", |s| {
            boxed(accounts_with_assets(s))
        }),
        case("market/a-seeded-price-keeps-the-others", |s| {
            boxed(seeded_price(s))
        }),
        case("market/the-public-orders-and-the-prices-are-listed", |s| {
            boxed(listed_tables(s))
        }),
    ]
}

fn name(id: i64, name: Option<&str>) -> NameRow {
    NameRow {
        id,
        category: "inventory_type".to_owned(),
        name: name.map(str::to_owned),
    }
}

fn market_order(id: i64, price_cents: i64) -> EsiMarketOrder {
    EsiMarketOrder {
        order_id: id,
        type_id: 34,
        location_id: STATION,
        system_id: 30_000_142,
        is_buy_order: false,
        price_cents,
        range: "station".to_owned(),
        volume_remain: 5,
    }
}

pub(super) fn asset(item_id: i64, type_id: i64, quantity: i64) -> EsiAsset {
    EsiAsset {
        item_id,
        type_id,
        location_id: STATION,
        location_type: "station".to_owned(),
        location_flag: "Hangar".to_owned(),
        quantity,
        is_singleton: false,
        is_blueprint_copy: false,
    }
}

async fn known_names(stores: &Stores) {
    let t = now();
    stores
        .market
        .store_names(
            &[name(1, Some("Tritanium")), name(2, None)],
            t,
            Some(t + Duration::hours(24)),
        )
        .await
        .unwrap();
    stores
        .market
        .store_names(&[name(3, None)], t, Some(t - Duration::hours(1)))
        .await
        .unwrap();
    stores
        .market
        .store_names(&[name(4, Some("Pyerite"))], t, None)
        .await
        .unwrap();

    let known = stores
        .market
        .known_names(&[1, 2, 3, 4, 5], t)
        .await
        .unwrap();
    let mut known = known;
    known.sort_unstable();
    // 2 is unresolvable until tomorrow, 3 may be asked again, 5 was never asked.
    assert_eq!(known, [1, 2, 4]);
    let later = stores
        .market
        .known_names(&[1, 2, 3, 4], t + Duration::hours(25))
        .await
        .unwrap();
    let mut later = later;
    later.sort_unstable();
    assert_eq!(
        later,
        [1, 4],
        "an unresolvable name is asked again after its retry time"
    );
    assert!(stores.market.known_names(&[], t).await.unwrap().is_empty());
}

async fn store_names(stores: &Stores) {
    let t = now();
    stores
        .market
        .store_names(&[name(1, Some("Old")), name(2, None)], t, None)
        .await
        .unwrap();

    stores
        .market
        .store_names(
            &[name(1, Some("New")), name(3, Some("Third"))],
            t + Duration::minutes(1),
            None,
        )
        .await
        .unwrap();

    assert_eq!(
        stores.market.name_of(1).await.unwrap().as_deref(),
        Some("New")
    );
    assert_eq!(stores.market.name_of(2).await.unwrap(), None);
    assert_eq!(stores.market.name_of(9).await.unwrap(), None);
    let mut named = stores.market.names_of(&[1, 2, 3, 9]).await.unwrap();
    named.sort();
    assert_eq!(
        named,
        [
            (1, Some("New".to_owned())),
            (2, None),
            (3, Some("Third".to_owned()))
        ],
        "an id that has a row but no name is listed without one"
    );
    let rows = stores.market.names_table().await.unwrap();
    assert_eq!(
        rows.iter()
            .map(|n| (n.id, n.name.clone(), n.retry_after))
            .collect::<Vec<_>>(),
        [
            (1, Some("New".to_owned()), None),
            (2, None, None),
            (3, Some("Third".to_owned()), None),
        ]
    );
    // One id twice in one call is no use.
    let twice = stores
        .market
        .store_names(&[name(5, Some("a")), name(5, Some("b"))], t, None)
        .await;
    assert!(twice.is_err());
    assert_eq!(stores.market.name_of(5).await.unwrap(), None);
}

async fn pairs_follow_orders(stores: &Stores) {
    let account = account_with(stores, &[1]).await;
    let other = account_with(stores, &[2]).await;
    let t = now();
    let mut jita_other = sell_order(2, 35);
    jita_other.region_id = 10_000_043;
    stores
        .orders
        .replace_orders(1, t, &[sell_order(1, 34), jita_other], t, None)
        .await
        .unwrap();
    stores
        .orders
        .replace_orders(2, t, &[sell_order(3, 36)], t, None)
        .await
        .unwrap();

    // The pairs of the orders; a pair is not listed twice, and the listing is in order.
    let in_use = vec![(10_000_002, 34), (10_000_002, 36), (10_000_043, 35)];
    let mut untracked = stores.market.untracked_pairs().await.unwrap();
    untracked.sort_unstable();
    assert_eq!(untracked, in_use);
    assert_eq!(
        stores.market.account_pairs(account).await.unwrap(),
        [(10_000_002, 34), (10_000_043, 35)]
    );
    assert_eq!(
        stores.market.account_pairs(other).await.unwrap(),
        [(REGION, 36)]
    );

    stores
        .market
        .insert_pair_schedule(&[(REGION, 34), (REGION, 36)], &[t, t + Duration::hours(1)])
        .await
        .unwrap();
    // A pair that has a schedule keeps it.
    stores
        .market
        .insert_pair_schedule(&[(REGION, 34)], &[t + Duration::days(1)])
        .await
        .unwrap();
    assert_eq!(
        stores
            .market
            .pair_sync(REGION, 34)
            .await
            .unwrap()
            .unwrap()
            .next_poll_at,
        t
    );
    assert_eq!(
        stores.market.untracked_pairs().await.unwrap(),
        [(10_000_043, 35)]
    );

    // The copy of a pair that no order uses any more goes with its sync state.
    stores
        .market
        .replace_market_orders(REGION, 36, t, &[market_order(70, 100)], t, None)
        .await
        .unwrap();
    stores
        .market
        .replace_market_orders(REGION, 34, t, &[market_order(71, 100)], t, None)
        .await
        .unwrap();
    stores.accounts.unlink_character(other, 2).await.unwrap();
    stores.market.drop_unused_pairs().await.unwrap();
    assert!(stores.market.pair_sync(REGION, 36).await.unwrap().is_none());
    assert!(stores.market.pair_sync(REGION, 34).await.unwrap().is_some());
    // Seen through the standings: order 71 of the used pair is still there, 70 is gone.
    stores
        .orders
        .replace_orders(
            1,
            t,
            &[{
                let mut o = sell_order(1, 34);
                o.price_cents = 10_000;
                o
            }],
            t,
            None,
        )
        .await
        .unwrap();
    let standings = stores.market.standings(REGION, 34).await.unwrap();
    assert_eq!(standings.len(), 1);
    assert_eq!(
        standings[0].2,
        Some(1),
        "the public order 71 undercuts ours"
    );
}

async fn refresh_replaces(stores: &Stores) {
    let account = account_with(stores, &[1]).await;
    let t = now();
    let mut ours = sell_order(1, 34);
    ours.price_cents = 10_000;
    stores
        .orders
        .replace_orders(1, t, &[ours], t, None)
        .await
        .unwrap();
    stores
        .market
        .insert_pair_schedule(&[(REGION, 34)], &[t])
        .await
        .unwrap();
    let next = t + Duration::minutes(6);
    let expires = t + Duration::minutes(5);
    stores
        .market
        .record_pair_failure(
            REGION,
            34,
            t - Duration::minutes(1),
            "boom",
            Duration::minutes(1),
            Duration::minutes(5),
        )
        .await
        .unwrap();

    stores
        .market
        .replace_market_orders(
            REGION,
            34,
            t,
            &[
                market_order(10, 9_000),
                market_order(11, 9_500),
                market_order(12, 11_000),
            ],
            next,
            Some(expires),
        )
        .await
        .unwrap();

    let state = stores.market.pair_sync(REGION, 34).await.unwrap().unwrap();
    assert_eq!(state.next_poll_at, next);
    assert_eq!(state.last_success_at, Some(t));
    assert_eq!(state.last_attempt_at, Some(t));
    assert_eq!(state.last_error, None);
    assert_eq!(state.esi_expires_at, Some(expires));
    assert_eq!(state.failures, 0);
    let rows = stores
        .orders
        .list_orders(
            account,
            &[1],
            sekondo::orders::Side::Sell,
            Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(rows[0].undercut, Some(2));
    assert_eq!(rows[0].best_price_cents, Some(9_000));

    // The next refresh replaces the copy.
    stores
        .market
        .replace_market_orders(REGION, 34, t, &[market_order(10, 9_000)], next, None)
        .await
        .unwrap();
    let rows = stores
        .orders
        .list_orders(
            account,
            &[1],
            sekondo::orders::Side::Sell,
            Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(rows[0].undercut, Some(1));
    // A pair nobody tracks gets no sync state from a refresh, and an order that moves pair keeps
    // one copy.
    stores
        .market
        .replace_market_orders(REGION, 99, t, &[market_order(10, 9_000)], next, None)
        .await
        .unwrap();
    assert!(stores.market.pair_sync(REGION, 99).await.unwrap().is_none());
    let rows = stores
        .orders
        .list_orders(
            account,
            &[1],
            sekondo::orders::Side::Sell,
            Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        rows[0].undercut,
        Some(0),
        "the order now belongs to the other pair"
    );
    // Two copies of one order in one answer cannot be stored.
    let twice = stores
        .market
        .replace_market_orders(
            REGION,
            34,
            t,
            &[market_order(20, 1), market_order(20, 2)],
            next,
            None,
        )
        .await;
    assert!(twice.is_err());
}

async fn pair_failure_and_deferral(stores: &Stores) {
    account_with(stores, &[1]).await;
    let t = now();
    stores
        .market
        .insert_pair_schedule(&[(REGION, 34)], &[t])
        .await
        .unwrap();
    let (start, interval) = (Duration::minutes(1), Duration::minutes(5));

    let mut waits = Vec::new();
    for round in 0..5 {
        let at = t + Duration::hours(round);
        stores
            .market
            .record_pair_failure(REGION, 34, at, "boom", start, interval)
            .await
            .unwrap();
        let s = stores.market.pair_sync(REGION, 34).await.unwrap().unwrap();
        waits.push((s.failures, (s.next_poll_at - at).whole_seconds()));
    }
    // The first failure waits one backoff step like the others: 1, 2, 4 minutes, then the cap.
    assert_eq!(waits, [(1, 60), (2, 120), (3, 240), (4, 300), (5, 300)]);

    let until = t + Duration::days(1);
    stores
        .market
        .record_pair_deferral(REGION, 34, t + Duration::days(2), until)
        .await
        .unwrap();
    let s = stores.market.pair_sync(REGION, 34).await.unwrap().unwrap();
    assert_eq!((s.next_poll_at, s.failures), (until, 5));
    assert_eq!(s.last_attempt_at, Some(t + Duration::days(2)));
    assert_eq!(s.last_error.as_deref(), Some("boom"));
    // Without a row nothing is created.
    stores
        .market
        .record_pair_failure(REGION, 77, t, "x", start, interval)
        .await
        .unwrap();
    stores
        .market
        .record_pair_deferral(REGION, 77, t, until)
        .await
        .unwrap();
    assert!(stores.market.pair_sync(REGION, 77).await.unwrap().is_none());
}

async fn claim_pairs(stores: &Stores) {
    let t = now();
    stores
        .market
        .insert_pair_schedule(
            &[(1, 1), (1, 2), (1, 3), (1, 4)],
            &[
                t - Duration::minutes(1),
                t - Duration::minutes(3),
                t + Duration::minutes(3),
                t - Duration::minutes(2),
            ],
        )
        .await
        .unwrap();
    let lease = Duration::minutes(5);

    let first = stores.market.claim_due_pairs(t, lease, 2).await.unwrap();
    assert_eq!(first, [(1, 2), (1, 4)], "the oldest first, at most two");
    assert_eq!(
        stores
            .market
            .pair_sync(1, 2)
            .await
            .unwrap()
            .unwrap()
            .next_poll_at,
        t + lease
    );
    let second = stores.market.claim_due_pairs(t, lease, 10).await.unwrap();
    assert_eq!(second, [(1, 1)]);
    assert!(
        stores
            .market
            .claim_due_pairs(t, lease, 10)
            .await
            .unwrap()
            .is_empty()
    );
}

async fn claim_prices(stores: &Stores) {
    let t = now();
    assert!(stores.market.price_sync().await.unwrap().is_none());

    assert!(
        stores
            .market
            .claim_price_refresh(t, Duration::minutes(5))
            .await
            .unwrap()
    );

    let state = stores.market.price_sync().await.unwrap().unwrap();
    assert_eq!(state.next_poll_at, t + Duration::minutes(5));
    assert_eq!(state.failures, 0);
    assert!(
        !stores
            .market
            .claim_price_refresh(t, Duration::minutes(5))
            .await
            .unwrap()
    );
    assert!(
        stores
            .market
            .claim_price_refresh(t + Duration::minutes(5), Duration::minutes(5))
            .await
            .unwrap()
    );
}

async fn prices_revalue(stores: &Stores) {
    let account = account_with(stores, &[1, 2, 3]).await;
    let t = now();
    let mut copy = asset(3, 34, 1000);
    copy.is_blueprint_copy = true;
    stores
        .wealth
        .store_assets(
            1,
            t - Duration::days(1),
            &[asset(1, 34, 100), asset(2, 35, 10), asset(4, 99, 5), copy],
            &[Some(STATION); 4],
            t,
            None,
        )
        .await
        .unwrap();
    stores
        .wealth
        .store_assets(
            2,
            t - Duration::days(1),
            &[asset(5, 35, 7)],
            &[Some(STATION)],
            t,
            None,
        )
        .await
        .unwrap();

    stores
        .market
        .replace_prices(
            t,
            &[
                EsiPrice {
                    type_id: 34,
                    average_price_cents: Some(550),
                },
                EsiPrice {
                    type_id: 35,
                    average_price_cents: Some(1_250),
                },
                EsiPrice {
                    type_id: 99,
                    average_price_cents: None,
                },
            ],
            t + Duration::days(1),
            Some(t + Duration::hours(23)),
        )
        .await
        .unwrap();

    // Character 1: 100 × 5.50 + 10 × 12.50 (the copy and the unpriced item are worth nothing).
    // Character 2: 7 × 12.50. Character 3 has no assets and gets no new entry.
    let current = stores.wealth.current(account, &[1, 2, 3]).await.unwrap();
    assert_eq!(current.inventory_cents, 55_000 + 12_500 + 8_750);
    let state = stores.market.price_sync().await.unwrap().unwrap();
    assert_eq!(state.next_poll_at, t + Duration::days(1));
    assert_eq!(state.last_success_at, Some(t));
    assert_eq!(state.esi_expires_at, Some(t + Duration::hours(23)));
    assert_eq!((state.failures, state.last_error), (0, None));

    // The list is replaced, not merged: a price that is no longer listed is gone.
    stores
        .market
        .replace_prices(
            t + Duration::minutes(1),
            &[EsiPrice {
                type_id: 35,
                average_price_cents: Some(1_000),
            }],
            t + Duration::days(1),
            None,
        )
        .await
        .unwrap();
    let after = stores.wealth.current(account, &[1, 2, 3]).await.unwrap();
    assert_eq!(after.inventory_cents, 10_000 + 7_000);
    // One entry per instant.
    stores
        .market
        .replace_prices(
            t + Duration::minutes(1),
            &[EsiPrice {
                type_id: 35,
                average_price_cents: Some(2_000),
            }],
            t + Duration::days(1),
            None,
        )
        .await
        .unwrap();
    let again = stores.wealth.current(account, &[1, 2, 3]).await.unwrap();
    assert_eq!(again.inventory_cents, 20_000 + 14_000);
    let points = stores
        .wealth
        .history(account, &[1], Range::Day, t + Duration::hours(1))
        .await
        .unwrap();
    assert_eq!(points.last().unwrap().inventory_cents, 20_000);
    let _ = datetime!(2026-10-20 12:00 UTC);
}

async fn price_failure_and_deferral(stores: &Stores) {
    let t = now();
    let (start, interval) = (Duration::hours(1), Duration::days(1));

    stores
        .market
        .record_price_failure(t, "boom", start, interval)
        .await
        .unwrap();
    let first = stores.market.price_sync().await.unwrap().unwrap();
    assert_eq!(first.failures, 1);
    assert_eq!(first.next_poll_at, t + start);
    assert_eq!(first.last_error.as_deref(), Some("boom"));
    let mut waits = Vec::new();
    for round in 1..=5 {
        let at = t + Duration::days(round);
        stores
            .market
            .record_price_failure(at, "again", start, interval)
            .await
            .unwrap();
        let s = stores.market.price_sync().await.unwrap().unwrap();
        waits.push((s.failures, (s.next_poll_at - at).whole_seconds()));
    }
    assert_eq!(
        waits,
        [
            (2, 7_200),
            (3, 14_400),
            (4, 28_800),
            (5, 57_600),
            (6, 86_400)
        ]
    );

    let until = t + Duration::days(10);
    stores
        .market
        .record_price_deferral(t + Duration::days(9), until)
        .await
        .unwrap();
    let s = stores.market.price_sync().await.unwrap().unwrap();
    assert_eq!((s.next_poll_at, s.failures), (until, 6));
    assert_eq!(s.last_attempt_at, Some(t + Duration::days(9)));
}

async fn accounts_with_assets(stores: &Stores) {
    let a = account_with(stores, &[1]).await;
    let b = account_with(stores, &[2, 3]).await;
    let t = now();
    assert!(
        stores
            .market
            .accounts_with_assets()
            .await
            .unwrap()
            .is_empty()
    );
    for (character, item) in [(2, 1), (3, 2)] {
        stores
            .wealth
            .store_assets(character, t, &[asset(item, 34, 1)], &[None], t, None)
            .await
            .unwrap();
    }
    stores
        .wealth
        .store_assets(1, t, &[], &[], t, None)
        .await
        .unwrap();

    assert_eq!(stores.market.accounts_with_assets().await.unwrap(), [b]);
    let _ = a;
}

async fn listed_tables(stores: &Stores) {
    account_with(stores, &[1]).await;
    let t = now();
    assert!(stores.market.tracked_pairs().await.unwrap().is_empty());
    assert!(stores.market.public_orders().await.unwrap().is_empty());
    assert!(stores.market.prices_table().await.unwrap().is_empty());
    stores
        .market
        .insert_pair_schedule(&[(REGION, 35), (REGION, 34), (1, 99)], &[t, t, t])
        .await
        .unwrap();
    stores
        .market
        .replace_market_orders(
            REGION,
            34,
            t,
            &[market_order(12, 11_000), market_order(10, 9_000)],
            t,
            None,
        )
        .await
        .unwrap();
    stores
        .market
        .replace_prices(
            t,
            &[
                EsiPrice {
                    type_id: 35,
                    average_price_cents: None,
                },
                EsiPrice {
                    type_id: 34,
                    average_price_cents: Some(550),
                },
            ],
            t,
            None,
        )
        .await
        .unwrap();

    assert_eq!(
        stores.market.tracked_pairs().await.unwrap(),
        [(1, 99), (REGION, 34), (REGION, 35)]
    );
    let held = stores.market.public_orders().await.unwrap();
    assert_eq!(
        held.iter()
            .map(|o| (
                o.order_id,
                o.region_id,
                o.type_id,
                o.location_id,
                o.is_buy_order,
                o.price_cents,
                o.volume_remain
            ))
            .collect::<Vec<_>>(),
        [
            (10, REGION, 34, STATION, false, 9_000, 5),
            (12, REGION, 34, STATION, false, 11_000, 5),
        ]
    );
    assert_eq!(
        stores.market.prices_table().await.unwrap(),
        [(34, Some(550)), (35, None)]
    );
}

async fn seeded_price(stores: &Stores) {
    stores.market.seed_price(34, Some(150)).await.unwrap();
    stores.market.seed_price(35, None).await.unwrap();
    stores.market.seed_price(34, Some(275)).await.unwrap();
    assert_eq!(
        stores.market.prices_table().await.unwrap(),
        [(34, Some(275)), (35, None)]
    );
}
