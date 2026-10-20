//! The current orders of the characters, the order poll's schedule and the dashboard's figures.

use sekondo::{
    orders::{Dir, Side, Sort, SortKey},
    range::Range,
    store::{StoreError, Stores},
};
use time::{Duration, macros::datetime};

use super::{
    Case,
    support::{REGION, STATION, account_with, boxed, buy_order, case, character, now, sell_order},
};

pub fn cases() -> Vec<Case> {
    vec![
        case(
            "orders/a-poll-replaces-the-characters-current-orders",
            |s| boxed(replace_keeps_only_the_newest_poll(s)),
        ),
        case(
            "orders/a-poll-needs-its-character-and-changes-nothing-otherwise",
            |s| boxed(replace_needs_the_character(s)),
        ),
        case(
            "orders/a-seeded-order-is-kept-next-to-the-others-and-records-nothing",
            |s| boxed(seeded_order(s)),
        ),
        case("orders/a-poll-records-the-sync-state", |s| {
            boxed(replace_records_sync_state(s))
        }),
        case("orders/the-figures-split-personal-from-corporation", |s| {
            boxed(figures_split_corporation(s))
        }),
        case("orders/characters-without-orders-still-have-figures", |s| {
            boxed(figures_for_idle_characters(s))
        }),
        case("orders/the-top-items-add-up-per-item", |s| {
            boxed(top_items_add_up(s))
        }),
        case("orders/the-history-carries-the-latest-poll-forward", |s| {
            boxed(history_carries_forward(s))
        }),
        case("orders/a-poll-without-orders-records-zero-totals", |s| {
            boxed(empty_poll_records_zeros(s))
        }),
        case("orders/orders-sort-by-the-simple-keys", |s| {
            boxed(sort_simple_keys(s))
        }),
        case(
            "orders/a-sell-order-stands-against-the-public-orders",
            |s| boxed(standing_of_sell_orders(s)),
        ),
        case(
            "orders/a-buy-order-stands-against-the-buy-orders-that-reach-it",
            |s| boxed(standing_of_buy_orders(s)),
        ),
        case(
            "orders/no-standing-without-a-refresh-and-in-a-structure",
            |s| boxed(no_standing(s)),
        ),
        case(
            "orders/orders-sort-by-standing-gap-and-undercut-volume",
            |s| boxed(sort_by_standing(s)),
        ),
        case("orders/the-margin-follows-the-cost-and-the-fees", |s| {
            boxed(margin_of_a_sell_order(s))
        }),
        case("orders/unlinking-a-character-removes-its-orders", |s| {
            boxed(unlinking_removes_orders(s))
        }),
        case("orders/a-failure-backs-off-and-a-success-resets", |s| {
            boxed(failure_backoff(s))
        }),
        case(
            "orders/a-deferral-moves-the-next-poll-without-a-failure",
            |s| boxed(deferral(s)),
        ),
        case(
            "orders/new-characters-are-scheduled-and-due-ones-claimed-once",
            |s| boxed(schedule_and_claim(s)),
        ),
        case("orders/making-a-character-due-never-delays-it", |s| {
            boxed(make_due(s))
        }),
        case(
            "orders/freshness-lists-the-selected-characters-in-link-order",
            |s| boxed(freshness_in_link_order(s)),
        ),
    ]
}

fn sorted(key: SortKey, dir: Dir) -> Sort {
    Sort { key, dir }
}

fn ids(rows: Vec<sekondo::orders::OrderRow>) -> Vec<i64> {
    rows.into_iter().map(|r| r.order_id).collect()
}

async fn replace_keeps_only_the_newest_poll(stores: &Stores) {
    let account = account_with(stores, &[1, 2]).await;
    let t1 = now() - Duration::minutes(20);
    let t2 = now();
    let mut other = sell_order(900, 35);
    other.price_cents = 123;
    stores
        .orders
        .replace_orders(2, t1, &[other], t1 + Duration::minutes(20), None)
        .await
        .unwrap();
    stores
        .orders
        .replace_orders(
            1,
            t1,
            &[sell_order(10, 34), sell_order(11, 35), buy_order(12, 34)],
            t1 + Duration::minutes(20),
            None,
        )
        .await
        .unwrap();

    let mut moved = sell_order(11, 35);
    moved.price_cents = 9_950;
    moved.volume_remain = 4;
    stores
        .orders
        .replace_orders(
            1,
            t2,
            &[moved, buy_order(13, 36)],
            t2 + Duration::minutes(20),
            None,
        )
        .await
        .unwrap();

    let sells = stores
        .orders
        .list_orders(account, &[1, 2], Side::Sell, Sort::default())
        .await
        .unwrap();
    let sell_ids: Vec<i64> = sells.iter().map(|r| r.order_id).collect();
    assert_eq!(sell_ids.len(), 2, "{sell_ids:?}");
    assert!(
        sell_ids.contains(&11) && sell_ids.contains(&900),
        "{sell_ids:?}"
    );
    let eleven = sells.iter().find(|r| r.order_id == 11).unwrap();
    assert_eq!(eleven.price_cents, 9_950);
    assert_eq!(eleven.volume_remain, 4);
    assert_eq!(eleven.volume_total, 10);
    assert_eq!(eleven.value_cents, 39_800);
    assert_eq!(eleven.character_id, 1);
    assert_eq!(eleven.character_name, "Pilot 1");
    let buys = stores
        .orders
        .list_orders(account, &[1, 2], Side::Buy, Sort::default())
        .await
        .unwrap();
    assert_eq!(ids(buys.clone()), [13]);
    assert_eq!(buys[0].escrow_cents, Some(50_000));
    // Every poll keeps its snapshot rows and its totals; a sell order is remembered once.
    assert_eq!(
        stores.orders.snapshot_counts(1).await.unwrap(),
        [(t1, 3), (t2, 2)]
    );
    let totals = stores.orders.poll_totals(1).await.unwrap();
    assert_eq!(
        totals
            .iter()
            .map(|t| (
                t.polled_at,
                t.sell_cents,
                t.escrow_cents,
                t.sell_count,
                t.buy_count
            ))
            .collect::<Vec<_>>(),
        [(t1, 200_000, 50_000, 2, 1), (t2, 39_800, 50_000, 1, 1)]
    );
    let remembered = stores.orders.remembered_orders(1).await.unwrap();
    assert_eq!(
        remembered
            .iter()
            .map(|o| (o.order_id, o.type_id, o.volume_total))
            .collect::<Vec<_>>(),
        [(10, 34, 10), (11, 35, 10)]
    );
    assert!(stores.orders.snapshot_counts(404).await.unwrap().is_empty());
    // Only the selected characters of the account are listed.
    let only_two = stores
        .orders
        .list_orders(account, &[2], Side::Sell, Sort::default())
        .await
        .unwrap();
    assert_eq!(ids(only_two), [900]);
    let stranger = account_with(stores, &[3]).await;
    let none = stores
        .orders
        .list_orders(stranger, &[1, 2], Side::Sell, Sort::default())
        .await
        .unwrap();
    assert!(none.is_empty());
}

async fn replace_needs_the_character(stores: &Stores) {
    let account = account_with(stores, &[1]).await;
    let t = now();
    let result = stores
        .orders
        .replace_orders(404, t, &[sell_order(1, 34)], t, None)
        .await;
    assert!(
        matches!(result, Err(StoreError::MissingParent)),
        "{result:?}"
    );
    assert!(stores.orders.order_sync(404).await.unwrap().is_none());

    stores
        .orders
        .replace_orders(1, t, &[sell_order(1, 34)], t, None)
        .await
        .unwrap();
    // A batch that names an order twice fails as a whole and keeps the previous poll.
    let again = stores
        .orders
        .replace_orders(
            1,
            t + Duration::minutes(1),
            &[sell_order(2, 34), sell_order(2, 35)],
            t,
            None,
        )
        .await;
    assert!(again.is_err());
    let kept = stores
        .orders
        .list_orders(account, &[1], Side::Sell, Sort::default())
        .await
        .unwrap();
    assert_eq!(ids(kept), [1]);
    assert_eq!(
        stores
            .orders
            .order_sync(1)
            .await
            .unwrap()
            .unwrap()
            .last_success_at,
        Some(t)
    );
    // An order another character has is not ours to take.
    account_with(stores, &[2]).await;
    let stolen = stores
        .orders
        .replace_orders(2, t, &[sell_order(1, 34)], t, None)
        .await;
    assert!(stolen.is_err());
}

async fn replace_records_sync_state(stores: &Stores) {
    account_with(stores, &[1]).await;
    let polled = now();
    let next = polled + Duration::minutes(21);
    let expires = polled + Duration::minutes(20);
    stores
        .orders
        .record_failure(
            1,
            polled - Duration::minutes(5),
            "boom",
            Duration::minutes(1),
            Duration::minutes(20),
        )
        .await
        .unwrap();

    stores
        .orders
        .replace_orders(1, polled, &[sell_order(1, 34)], next, Some(expires))
        .await
        .unwrap();

    let state = stores.orders.order_sync(1).await.unwrap().unwrap();
    assert_eq!(state.next_poll_at, next);
    assert_eq!(state.last_success_at, Some(polled));
    assert_eq!(state.last_attempt_at, Some(polled));
    assert_eq!(state.last_error, None);
    assert_eq!(state.esi_expires_at, Some(expires));
    assert_eq!(state.failures, 0);
    assert!(stores.orders.order_sync(2).await.unwrap().is_none());
}

async fn figures_split_corporation(stores: &Stores) {
    let account = account_with(stores, &[1]).await;
    let t = now();
    let mut corp_sell = sell_order(2, 34);
    corp_sell.is_corporation = true;
    corp_sell.price_cents = 20_000;
    corp_sell.volume_remain = 3;
    let mut corp_buy = buy_order(4, 34);
    corp_buy.is_corporation = true;
    corp_buy.escrow_cents = Some(7_000);
    let mut no_escrow = buy_order(5, 34);
    no_escrow.escrow_cents = None;
    stores
        .orders
        .replace_orders(
            1,
            t,
            &[
                sell_order(1, 34),
                corp_sell,
                buy_order(3, 34),
                corp_buy,
                no_escrow,
            ],
            t + Duration::minutes(20),
            None,
        )
        .await
        .unwrap();

    let figures = stores
        .orders
        .character_figures(account, &[1])
        .await
        .unwrap();

    assert_eq!(figures.len(), 1);
    let f = &figures[0];
    assert_eq!(f.freshness.character_id, 1);
    assert_eq!(f.freshness.last_success_at, Some(t));
    assert_eq!(f.freshness.next_poll_at, Some(t + Duration::minutes(20)));
    assert_eq!(f.sell_cents, 10 * 10_000 + 3 * 20_000);
    assert_eq!(f.sell_corp_cents, 3 * 20_000);
    assert_eq!(f.escrow_cents, 50_000 + 7_000);
    assert_eq!(f.escrow_corp_cents, 7_000);
    assert_eq!((f.sell_count, f.buy_count), (2, 3));
}

async fn figures_for_idle_characters(stores: &Stores) {
    let account = account_with(stores, &[1, 2]).await;
    let t = now();
    stores
        .orders
        .replace_orders(2, t, &[sell_order(1, 34)], t, None)
        .await
        .unwrap();

    let figures = stores
        .orders
        .character_figures(account, &[1, 2, 99])
        .await
        .unwrap();

    let by: Vec<(i64, i64, i64, Option<time::OffsetDateTime>)> = figures
        .iter()
        .map(|f| {
            (
                f.freshness.character_id,
                f.sell_cents,
                f.sell_count,
                f.freshness.last_success_at,
            )
        })
        .collect();
    assert_eq!(by, [(1, 0, 0, None), (2, 100_000, 1, Some(t))]);
    assert!(figures[0].freshness.next_poll_at.is_none());
}

async fn top_items_add_up(stores: &Stores) {
    let account = account_with(stores, &[1, 2]).await;
    let t = now();
    stores
        .market
        .store_names(
            &[
                sekondo::store::NameRow {
                    id: 34,
                    category: "inventory_type".to_owned(),
                    name: Some("Tritanium".to_owned()),
                },
                sekondo::store::NameRow {
                    id: 35,
                    category: "inventory_type".to_owned(),
                    name: Some("pyerite".to_owned()),
                },
                sekondo::store::NameRow {
                    id: 37,
                    category: "inventory_type".to_owned(),
                    name: Some("Isogen".to_owned()),
                },
            ],
            t,
            None,
        )
        .await
        .unwrap();
    let mut a = sell_order(1, 34);
    a.price_cents = 1_000;
    let mut b = sell_order(2, 35);
    b.price_cents = 1_000;
    let mut c = sell_order(3, 37);
    c.price_cents = 1_000;
    let mut d = sell_order(4, 36);
    d.price_cents = 3_000;
    stores
        .orders
        .replace_orders(1, t, &[a.clone(), b, buy_order(5, 34)], t, None)
        .await
        .unwrap();
    let mut e = a;
    e.order_id = 6;
    stores
        .orders
        .replace_orders(2, t, &[c, d, e], t, None)
        .await
        .unwrap();

    let sells = stores
        .orders
        .top_items(account, &[1, 2], Side::Sell, 10)
        .await
        .unwrap();

    let summary: Vec<(i64, &str, i64)> = sells
        .iter()
        .map(|i| (i.type_id, i.label.as_str(), i.cents))
        .collect();
    // 34 twice (20.00 ISK each unit pair), 36 unnamed, ties by name, pyerite lower-cased.
    assert_eq!(
        summary,
        [
            (36, "#36", 30_000),
            (34, "Tritanium", 20_000),
            (37, "Isogen", 10_000),
            (35, "pyerite", 10_000),
        ]
    );
    let limited = stores
        .orders
        .top_items(account, &[1, 2], Side::Sell, 2)
        .await
        .unwrap();
    assert_eq!(limited.len(), 2);
    let buys = stores
        .orders
        .top_items(account, &[1, 2], Side::Buy, 10)
        .await
        .unwrap();
    assert_eq!(buys.len(), 1);
    assert_eq!((buys[0].type_id, buys[0].cents), (34, 50_000));
    let only_two = stores
        .orders
        .top_items(account, &[2], Side::Sell, 10)
        .await
        .unwrap();
    assert_eq!(only_two.len(), 3);
}

async fn history_carries_forward(stores: &Stores) {
    let account = account_with(stores, &[1, 2]).await;
    let now = datetime!(2026-10-20 12:10 UTC);
    let first = datetime!(2026-10-20 06:05 UTC);
    let second = datetime!(2026-10-20 10:20 UTC);
    stores
        .orders
        .replace_orders(
            1,
            first,
            &[sell_order(1, 34), buy_order(2, 34)],
            first,
            None,
        )
        .await
        .unwrap();
    let mut smaller = sell_order(1, 34);
    smaller.volume_remain = 5;
    stores
        .orders
        .replace_orders(1, second, &[smaller], second, None)
        .await
        .unwrap();

    let points = stores
        .orders
        .history(account, &[1, 2], Range::Day, now)
        .await
        .unwrap();

    assert_eq!(points.len(), 49, "48 half hours of a day and now");
    assert_eq!(points.last().unwrap().at, now);
    assert_eq!(points[0].at, datetime!(2026-10-19 12:30 UTC));
    assert!(points.windows(2).all(|w| w[0].at < w[1].at));
    let at = |t: time::OffsetDateTime| points.iter().find(|p| p.at == t).unwrap();
    let before = at(datetime!(2026-10-20 06:00 UTC));
    assert_eq!((before.contributors, before.sell_cents), (0, 0));
    let carried = at(datetime!(2026-10-20 07:00 UTC));
    assert_eq!(carried.contributors, 1, "character 2 was never polled");
    assert_eq!(carried.sell_cents, 100_000);
    assert_eq!(carried.escrow_cents, 50_000);
    assert_eq!((carried.sell_count, carried.buy_count), (1, 1));
    let last = points.last().unwrap();
    assert_eq!(last.sell_cents, 50_000);
    assert_eq!(last.escrow_cents, 0, "the second poll had no buy order");
    assert_eq!((last.sell_count, last.buy_count), (1, 0));
    // A bucket that ends exactly at a poll sees it.
    assert_eq!(at(datetime!(2026-10-20 10:30 UTC)).sell_cents, 50_000);
    assert_eq!(at(datetime!(2026-10-20 10:00 UTC)).sell_cents, 100_000);
    let other = account_with(stores, &[3]).await;
    assert!(
        stores
            .orders
            .history(other, &[1, 2], Range::Day, now)
            .await
            .unwrap()
            .is_empty(),
        "nobody of that account is selected"
    );
}

async fn empty_poll_records_zeros(stores: &Stores) {
    let account = account_with(stores, &[1]).await;
    let now = datetime!(2026-10-20 12:10 UTC);
    let first = datetime!(2026-10-20 08:05 UTC);
    let second = datetime!(2026-10-20 09:05 UTC);
    stores
        .orders
        .replace_orders(1, first, &[sell_order(1, 34)], first, None)
        .await
        .unwrap();
    stores
        .orders
        .replace_orders(1, second, &[], second, None)
        .await
        .unwrap();

    let points = stores
        .orders
        .history(account, &[1], Range::Day, now)
        .await
        .unwrap();

    let last = points.last().unwrap();
    assert_eq!(
        (last.sell_cents, last.sell_count, last.contributors),
        (0, 0, 1)
    );
    let earlier = points
        .iter()
        .find(|p| p.at == datetime!(2026-10-20 08:30 UTC))
        .unwrap();
    assert_eq!(earlier.sell_cents, 100_000);
    let listed = stores
        .orders
        .list_orders(account, &[1], Side::Sell, Sort::default())
        .await
        .unwrap();
    assert!(listed.is_empty());
    // Two polls in the same instant keep the later one.
    stores
        .orders
        .replace_orders(1, second, &[sell_order(7, 34)], second, None)
        .await
        .unwrap();
    let again = stores
        .orders
        .history(account, &[1], Range::Day, now)
        .await
        .unwrap();
    assert_eq!(again.last().unwrap().sell_cents, 100_000);
}

async fn sort_simple_keys(stores: &Stores) {
    let account = account_with(stores, &[1]).await;
    let t = now();
    stores
        .market
        .store_names(
            &[
                sekondo::store::NameRow {
                    id: 34,
                    category: "t".to_owned(),
                    name: Some("Zydrine".to_owned()),
                },
                sekondo::store::NameRow {
                    id: 35,
                    category: "t".to_owned(),
                    name: Some("apple".to_owned()),
                },
            ],
            t,
            None,
        )
        .await
        .unwrap();
    let mut a = sell_order(1, 34); // Zydrine, price 30, volume 2, left 10 days
    a.price_cents = 3_000;
    a.volume_remain = 2;
    a.issued = t - Duration::days(80);
    let mut b = sell_order(2, 35); // apple, price 10, volume 9, left 80 days
    b.price_cents = 1_000;
    b.volume_remain = 9;
    b.issued = t - Duration::days(10);
    let mut c = sell_order(3, 36); // #36, price 20, volume 5, left 50 days
    c.price_cents = 2_000;
    c.volume_remain = 5;
    c.issued = t - Duration::days(40);
    stores
        .orders
        .replace_orders(1, t, &[a, b, c], t, None)
        .await
        .unwrap();
    let list = |key: SortKey, dir: Dir| {
        let orders = stores.orders.clone();
        async move {
            ids(orders
                .list_orders(account, &[1], Side::Sell, sorted(key, dir))
                .await
                .unwrap())
        }
    };

    // Items by lower-cased name: "#36" < "apple" < "zydrine".
    assert_eq!(list(SortKey::Item, Dir::Asc).await, [3, 2, 1]);
    assert_eq!(list(SortKey::Item, Dir::Desc).await, [1, 2, 3]);
    assert_eq!(list(SortKey::Price, Dir::Asc).await, [2, 3, 1]);
    assert_eq!(list(SortKey::Price, Dir::Desc).await, [1, 3, 2]);
    assert_eq!(list(SortKey::Volume, Dir::Asc).await, [1, 3, 2]);
    assert_eq!(list(SortKey::Volume, Dir::Desc).await, [2, 3, 1]);
    // Value: 60.00, 90.00, 100.00.
    assert_eq!(list(SortKey::Amount, Dir::Asc).await, [1, 2, 3]);
    assert_eq!(list(SortKey::Amount, Dir::Desc).await, [3, 2, 1]);
    assert_eq!(list(SortKey::TimeLeft, Dir::Asc).await, [1, 3, 2]);
    assert_eq!(list(SortKey::TimeLeft, Dir::Desc).await, [2, 3, 1]);
}

/// Seeds the public book of one pair: `(order, system, buy, price in cents, volume, range, location)`.
async fn public_orders(
    stores: &Stores,
    region: i64,
    type_id: i64,
    orders: &[(i64, i64, bool, i64, i64, &str, i64)],
) {
    let t = now();
    stores.orders.insert_schedule(&[], &[]).await.unwrap();
    let book: Vec<sekondo::esi::EsiMarketOrder> = orders
        .iter()
        .map(
            |(
                order_id,
                system_id,
                is_buy_order,
                price_cents,
                volume_remain,
                range,
                location_id,
            )| {
                sekondo::esi::EsiMarketOrder {
                    order_id: *order_id,
                    type_id,
                    location_id: *location_id,
                    system_id: *system_id,
                    is_buy_order: *is_buy_order,
                    price_cents: *price_cents,
                    range: (*range).to_owned(),
                    volume_remain: *volume_remain,
                }
            },
        )
        .collect();
    stores
        .market
        .insert_pair_schedule(&[(region, type_id)], &[t])
        .await
        .unwrap();
    stores
        .market
        .replace_market_orders(region, type_id, t, &book, t, None)
        .await
        .unwrap();
}

async fn standing_of_sell_orders(stores: &Stores) {
    let account = account_with(stores, &[1]).await;
    let t = now();
    // Ours: 100.00 at the station. Public: two cheaper there, one cheaper elsewhere, one dearer.
    stores
        .orders
        .replace_orders(1, t, &[sell_order(1, 34)], t, None)
        .await
        .unwrap();
    stores
        .market
        .insert_pair_schedule(&[(REGION, 34)], &[t])
        .await
        .unwrap();
    public_orders(
        stores,
        REGION,
        34,
        &[
            (1, 30000142, false, 10_000, 10, "station", STATION),
            (2, 30000142, false, 9_900, 100, "station", STATION),
            (3, 30000142, false, 9_500, 50, "station", STATION),
            (4, 30000142, false, 8_000, 1_000, "station", 60_000_001),
            (5, 30000142, false, 10_500, 7, "station", STATION),
            (6, 30000142, true, 1_000, 7, "station", STATION),
        ],
    )
    .await;

    let rows = stores
        .orders
        .list_orders(account, &[1], Side::Sell, Sort::default())
        .await
        .unwrap();

    let r = &rows[0];
    assert_eq!(r.undercut, Some(2));
    assert_eq!(r.best_price_cents, Some(9_500));
    assert_eq!(r.gap_cents, Some(500));
    assert_eq!(r.undercut_volume, Some(150));
    let standings = stores.market.standings(REGION, 34).await.unwrap();
    assert_eq!(standings, [(account, 1, Some(2), Some(500), Some(150))]);

    // A top order: nobody is cheaper.
    let mut top = sell_order(1, 34);
    top.price_cents = 9_000;
    stores
        .orders
        .replace_orders(1, t, &[top], t, None)
        .await
        .unwrap();
    let rows = stores
        .orders
        .list_orders(account, &[1], Side::Sell, Sort::default())
        .await
        .unwrap();
    assert_eq!(rows[0].undercut, Some(0));
    assert_eq!(rows[0].best_price_cents, None);
    assert_eq!(rows[0].gap_cents, None);
    assert_eq!(rows[0].undercut_volume, None);
}

async fn standing_of_buy_orders(stores: &Stores) {
    let account = account_with(stores, &[1]).await;
    let t = now();
    let mut ours = buy_order(1, 34);
    ours.price_cents = 5_000;
    stores
        .orders
        .replace_orders(1, t, &[ours], t, None)
        .await
        .unwrap();
    // Our own order is part of the public book, in system 100.
    public_orders(
        stores,
        REGION,
        34,
        &[
            (1, 100, true, 5_000, 10, "region", STATION),
            // Better prices that reach us: same station, region range, in range from our system.
            (2, 100, true, 5_100, 10, "station", STATION),
            (3, 200, true, 5_200, 20, "region", 60_000_002),
            (4, 100, true, 5_300, 30, "5", 60_000_003),
            // Better prices that do not: another station with a station range, another system
            // with a jump range, a worse price, and a sell order.
            (5, 100, true, 5_400, 40, "station", 60_000_004),
            (6, 200, true, 5_500, 50, "10", 60_000_005),
            (7, 100, true, 4_900, 60, "region", 60_000_006),
            (8, 100, false, 6_000, 70, "station", STATION),
        ],
    )
    .await;

    let rows = stores
        .orders
        .list_orders(account, &[1], Side::Buy, Sort::default())
        .await
        .unwrap();

    let r = &rows[0];
    assert_eq!(r.undercut, Some(3), "orders 2, 3 and 4 reach it");
    assert_eq!(r.best_price_cents, Some(5_300));
    assert_eq!(r.gap_cents, Some(300));
    assert_eq!(r.undercut_volume, Some(60));
}

async fn no_standing(stores: &Stores) {
    let account = account_with(stores, &[1]).await;
    let t = now();
    let mut structure = sell_order(2, 34);
    structure.location_id = 1_000_000_000_001;
    let mut other_item = sell_order(3, 35);
    other_item.price_cents = 10_000;
    stores
        .orders
        .replace_orders(1, t, &[sell_order(1, 34), structure, other_item], t, None)
        .await
        .unwrap();
    // 34 is tracked but never refreshed; 35 is refreshed.
    stores
        .market
        .insert_pair_schedule(&[(REGION, 34), (REGION, 35)], &[t, t])
        .await
        .unwrap();
    let book = [sekondo::esi::EsiMarketOrder {
        order_id: 50,
        type_id: 34,
        location_id: STATION,
        system_id: 1,
        is_buy_order: false,
        price_cents: 100,
        range: "station".to_owned(),
        volume_remain: 1,
    }];
    stores
        .market
        .replace_market_orders(REGION, 35, t, &[], t, None)
        .await
        .unwrap();

    let rows = stores
        .orders
        .list_orders(account, &[1], Side::Sell, Sort::default())
        .await
        .unwrap();
    let by_id = |id: i64| rows.iter().find(|r| r.order_id == id).unwrap();
    assert_eq!(by_id(1).undercut, None, "not refreshed yet");
    assert_eq!(
        by_id(3).undercut,
        Some(0),
        "refreshed, and nobody is cheaper"
    );

    // Refreshed with cheaper orders: the structure order still has no standing.
    stores
        .market
        .replace_market_orders(REGION, 34, t, &book, t, None)
        .await
        .unwrap();
    let rows = stores
        .orders
        .list_orders(account, &[1], Side::Sell, Sort::default())
        .await
        .unwrap();
    let by_id = |id: i64| rows.iter().find(|r| r.order_id == id).unwrap();
    assert_eq!(by_id(1).undercut, Some(1));
    assert_eq!(by_id(2).undercut, None, "structures have no public book");
    assert_eq!(by_id(2).gap_cents, None);
    // A buy order the public book does not list has none either.
    stores
        .orders
        .replace_orders(1, t, &[buy_order(9, 34)], t, None)
        .await
        .unwrap();
    let buys = stores
        .orders
        .list_orders(account, &[1], Side::Buy, Sort::default())
        .await
        .unwrap();
    assert_eq!(buys[0].undercut, None);
}

async fn sort_by_standing(stores: &Stores) {
    let account = account_with(stores, &[1]).await;
    let t = now();
    let mut orders = Vec::new();
    for (id, type_id) in [(1, 34), (2, 35), (3, 36), (4, 37)] {
        let mut o = sell_order(id, type_id);
        o.price_cents = 10_000;
        orders.push(o);
    }
    stores
        .orders
        .replace_orders(1, t, &orders, t, None)
        .await
        .unwrap();
    // 34: one cheaper (gap 1.00, volume 10); 35: three cheaper (gap 3.00, volume 5);
    // 36: top; 37: never refreshed.
    stores
        .market
        .insert_pair_schedule(&[(REGION, 34), (REGION, 35), (REGION, 36)], &[t, t, t])
        .await
        .unwrap();
    public_orders(
        stores,
        REGION,
        34,
        &[(101, 1, false, 9_900, 10, "station", STATION)],
    )
    .await;
    public_orders(
        stores,
        REGION,
        35,
        &[
            (102, 1, false, 9_000, 2, "station", STATION),
            (103, 1, false, 9_700, 2, "station", STATION),
            (104, 1, false, 9_800, 1, "station", STATION),
        ],
    )
    .await;
    public_orders(stores, REGION, 36, &[]).await;
    let list = |key: SortKey, dir: Dir| {
        let orders = stores.orders.clone();
        async move {
            ids(orders
                .list_orders(account, &[1], Side::Sell, sorted(key, dir))
                .await
                .unwrap())
        }
    };

    // "Top" first, then by how many undercut; no standing last in both directions.
    assert_eq!(list(SortKey::Standing, Dir::Asc).await, [3, 1, 2, 4]);
    assert_eq!(list(SortKey::Standing, Dir::Desc).await, [2, 1, 3, 4]);
    // A top order counts as no difference.
    assert_eq!(list(SortKey::Gap, Dir::Asc).await, [3, 1, 2, 4]);
    assert_eq!(list(SortKey::Gap, Dir::Desc).await, [2, 1, 3, 4]);
    assert_eq!(list(SortKey::UndercutVolume, Dir::Asc).await, [3, 2, 1, 4]);
    assert_eq!(list(SortKey::UndercutVolume, Dir::Desc).await, [1, 2, 3, 4]);
}

/// Alice's fees at Jita 4-4 with Accounting 5, Broker Relations 5 and no standing: 3.375% sales
/// tax and 1.50% broker fee.
pub(super) async fn jita_fees(stores: &Stores, character_id: i64) {
    let t = now();
    stores
        .fees
        .store_skills(character_id, &[16622, 3446], &[5, 5], t, t, None)
        .await
        .unwrap();
    stores
        .fees
        .store_standings(character_id, &[], t, t, None)
        .await
        .unwrap();
    stores
        .fees
        .insert_station_owner(STATION, 1_000_035, t)
        .await
        .unwrap();
}

async fn margin_of_a_sell_order(stores: &Stores) {
    let account = account_with(stores, &[1]).await;
    let t = now();
    jita_fees(stores, 1).await;
    let buy_on_day =
        |id: i64, quantity: i64, price_cents: i64, days_ago: i64| sekondo::esi::EsiTransaction {
            transaction_id: id,
            bought_at: t - Duration::days(days_ago),
            type_id: 34,
            location_id: STATION,
            quantity,
            unit_price_cents: price_cents,
            is_buy: true,
            is_personal: true,
        };
    // 10 units at 80.00, bought before the order was issued a day ago.
    stores
        .wealth
        .store_transactions(1, t, &[buy_on_day(1, 10, 8_000, 3)], t, None)
        .await
        .unwrap();
    let mut order = sell_order(1, 34);
    order.price_cents = 10_000;
    stores
        .orders
        .replace_orders(1, t, &[order.clone()], t, None)
        .await
        .unwrap();

    let rows = stores
        .orders
        .list_orders(account, &[1], Side::Sell, Sort::default())
        .await
        .unwrap();

    let r = &rows[0];
    assert_eq!(r.covered_units, Some(10));
    // 10 × 100.00 × (1 − 3.375% − 1.50%) − 10 × 80.00 = 951.25 − 800.00.
    assert_eq!(r.margin_cents, Some(15_125));
    assert_eq!(r.cost_cents, Some(80_000));
    assert_eq!(r.cost_per_unit_cents, Some(8_000));

    // A part sold already carries its share: 4 of 10 left costs 4 × 80.00.
    let mut partly = order.clone();
    partly.volume_remain = 4;
    stores
        .orders
        .replace_orders(1, t, &[partly], t, None)
        .await
        .unwrap();
    let rows = stores
        .orders
        .list_orders(account, &[1], Side::Sell, Sort::default())
        .await
        .unwrap();
    assert_eq!(rows[0].cost_cents, Some(32_000));
    // 4 × 100.00 × 0.95125 − 4 × 80.00 = 380.50 − 320.00.
    assert_eq!(rows[0].margin_cents, Some(6_050));

    // A cost that covers only 6 of the 10 units gives no margin and no cost.
    stores
        .wealth
        .store_transactions(
            1,
            t + Duration::minutes(1),
            &[sekondo::esi::EsiTransaction {
                transaction_id: 2,
                bought_at: t - Duration::days(2),
                type_id: 36,
                location_id: STATION,
                quantity: 6,
                unit_price_cents: 100,
                is_buy: true,
                is_personal: true,
            }],
            t,
            None,
        )
        .await
        .unwrap();
    let mut other = sell_order(2, 36);
    other.price_cents = 10_000;
    stores
        .orders
        .replace_orders(1, t, &[other], t, None)
        .await
        .unwrap();
    let rows = stores
        .orders
        .list_orders(account, &[1], Side::Sell, Sort::default())
        .await
        .unwrap();
    assert_eq!(rows[0].covered_units, Some(6));
    assert_eq!(rows[0].margin_cents, None);
    assert_eq!(rows[0].cost_cents, None);
    assert_eq!(rows[0].cost_per_unit_cents, None);
    // A corporation order never has a cost or a margin.
    let mut corp = sell_order(3, 34);
    corp.is_corporation = true;
    stores
        .orders
        .replace_orders(1, t, &[corp], t, None)
        .await
        .unwrap();
    let rows = stores
        .orders
        .list_orders(account, &[1], Side::Sell, Sort::default())
        .await
        .unwrap();
    assert_eq!(rows[0].covered_units, None);
    assert_eq!(rows[0].margin_cents, None);
    // And sort by margin puts those without last in both directions.
    stores
        .orders
        .replace_orders(1, t, &[order, sell_order(5, 34)], t, None)
        .await
        .unwrap();
}

async fn unlinking_removes_orders(stores: &Stores) {
    let account = account_with(stores, &[1, 2]).await;
    let t = now();
    stores
        .orders
        .replace_orders(1, t, &[sell_order(1, 34)], t, None)
        .await
        .unwrap();
    stores
        .orders
        .replace_orders(2, t, &[sell_order(2, 34)], t, None)
        .await
        .unwrap();

    stores.accounts.unlink_character(account, 1).await.unwrap();

    let rows = stores
        .orders
        .list_orders(account, &[1, 2], Side::Sell, Sort::default())
        .await
        .unwrap();
    assert_eq!(ids(rows), [2]);
    assert!(stores.orders.order_sync(1).await.unwrap().is_none());
    assert!(stores.orders.order_sync(2).await.unwrap().is_some());
    // The character's order can be taken by another one once it is gone.
    stores
        .orders
        .replace_orders(2, t, &[sell_order(1, 34)], t, None)
        .await
        .unwrap();
    let _ = character(1);
}

async fn failure_backoff(stores: &Stores) {
    account_with(stores, &[1]).await;
    let t = now();
    let (start, interval) = (Duration::minutes(1), Duration::minutes(20));

    stores
        .orders
        .record_failure(1, t, "first", start, interval)
        .await
        .unwrap();
    let one = stores.orders.order_sync(1).await.unwrap().unwrap();
    assert_eq!(one.failures, 1);
    assert_eq!(one.last_error.as_deref(), Some("first"));
    assert_eq!(one.last_attempt_at, Some(t));
    assert_eq!(one.next_poll_at, t + Duration::minutes(1));
    assert_eq!(one.last_success_at, None);

    // The wait doubles per failure and never exceeds the interval.
    let mut waits = Vec::new();
    for round in 2..=7 {
        let at = t + Duration::hours(round);
        stores
            .orders
            .record_failure(1, at, "again", start, interval)
            .await
            .unwrap();
        let state = stores.orders.order_sync(1).await.unwrap().unwrap();
        waits.push((state.failures, (state.next_poll_at - at).whole_seconds()));
    }
    assert_eq!(
        waits,
        [
            (2, 120),
            (3, 240),
            (4, 480),
            (5, 960),
            (6, 1_200),
            (7, 1_200)
        ]
    );

    // A success starts over.
    stores
        .orders
        .replace_orders(1, t + Duration::days(1), &[], t + Duration::days(1), None)
        .await
        .unwrap();
    let ok = stores.orders.order_sync(1).await.unwrap().unwrap();
    assert_eq!((ok.failures, ok.last_error), (0, None));

    // A character that is gone cannot fail.
    let result = stores
        .orders
        .record_failure(404, t, "x", start, interval)
        .await;
    assert!(matches!(result, Err(StoreError::MissingParent)));
    // A first failure waits no longer than the interval.
    account_with(stores, &[2]).await;
    stores
        .orders
        .record_failure(2, t, "first", Duration::minutes(30), interval)
        .await
        .unwrap();
    let capped = stores.orders.order_sync(2).await.unwrap().unwrap();
    assert_eq!(capped.next_poll_at, t + interval);
}

async fn deferral(stores: &Stores) {
    account_with(stores, &[1]).await;
    let t = now();
    stores
        .orders
        .record_failure(1, t, "boom", Duration::minutes(1), Duration::minutes(20))
        .await
        .unwrap();
    let until = t + Duration::minutes(7);

    stores
        .orders
        .record_deferral(1, t + Duration::seconds(5), until)
        .await
        .unwrap();

    let state = stores.orders.order_sync(1).await.unwrap().unwrap();
    assert_eq!(state.next_poll_at, until);
    assert_eq!(state.last_attempt_at, Some(t + Duration::seconds(5)));
    assert_eq!(state.failures, 1, "a deferral is no failure");
    assert_eq!(state.last_error.as_deref(), Some("boom"));
    // A character without a schedule gets one.
    account_with(stores, &[2]).await;
    stores.orders.record_deferral(2, t, until).await.unwrap();
    let fresh = stores.orders.order_sync(2).await.unwrap().unwrap();
    assert_eq!(
        (fresh.next_poll_at, fresh.failures, fresh.last_error),
        (until, 0, None)
    );
    let gone = stores.orders.record_deferral(404, t, until).await;
    assert!(matches!(gone, Err(StoreError::MissingParent)));
}

async fn schedule_and_claim(stores: &Stores) {
    account_with(stores, &[1, 2, 3]).await;
    stores.accounts.mark_needs_relogin(3).await.unwrap();
    let t = now();

    let mut unscheduled = stores.orders.list_unscheduled().await.unwrap();
    unscheduled.sort_unstable();
    assert_eq!(
        unscheduled,
        [1, 2],
        "a character that needs a login is not scheduled"
    );

    stores
        .orders
        .insert_schedule(
            &[1, 2],
            &[t - Duration::minutes(10), t + Duration::minutes(10)],
        )
        .await
        .unwrap();
    assert!(stores.orders.list_unscheduled().await.unwrap().is_empty());
    // Scheduling again keeps the first schedule.
    stores
        .orders
        .insert_schedule(&[1], &[t + Duration::hours(5)])
        .await
        .unwrap();
    assert_eq!(
        stores
            .orders
            .order_sync(1)
            .await
            .unwrap()
            .unwrap()
            .next_poll_at,
        t - Duration::minutes(10)
    );
    let missing = stores.orders.insert_schedule(&[2, 404], &[t, t]).await;
    assert!(matches!(missing, Err(StoreError::MissingParent)));

    let lease = Duration::minutes(5);
    let claimed = stores.orders.claim_due(t, lease, 10).await.unwrap();
    assert_eq!(claimed, [1]);
    assert_eq!(
        stores
            .orders
            .order_sync(1)
            .await
            .unwrap()
            .unwrap()
            .next_poll_at,
        t + lease
    );
    assert!(
        stores
            .orders
            .claim_due(t, lease, 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        stores
            .orders
            .claim_due(t + Duration::minutes(11), lease, 10)
            .await
            .unwrap()
            .len(),
        2
    );
    // A character that needs a login is never claimed; the limit counts.
    stores
        .orders
        .insert_schedule(&[3], &[t - Duration::days(1)])
        .await
        .unwrap();
    let limited = stores
        .orders
        .claim_due(t + Duration::days(1), lease, 1)
        .await
        .unwrap();
    assert_eq!(limited.len(), 1);
    assert_ne!(limited[0], 3);
}

async fn make_due(stores: &Stores) {
    account_with(stores, &[1, 2]).await;
    let t = now();
    stores
        .orders
        .insert_schedule(&[1, 2], &[t + Duration::hours(1), t - Duration::hours(1)])
        .await
        .unwrap();

    stores.orders.make_due(1, t).await.unwrap();
    stores.orders.make_due(2, t).await.unwrap();
    stores.orders.make_due(404, t).await.unwrap();

    assert_eq!(
        stores
            .orders
            .order_sync(1)
            .await
            .unwrap()
            .unwrap()
            .next_poll_at,
        t
    );
    assert_eq!(
        stores
            .orders
            .order_sync(2)
            .await
            .unwrap()
            .unwrap()
            .next_poll_at,
        t - Duration::hours(1),
        "an earlier time stays"
    );
}

async fn freshness_in_link_order(stores: &Stores) {
    let account = stores.accounts.create_account().await.unwrap();
    for id in [20, 10, 30] {
        stores
            .accounts
            .upsert_character(account, &character(id), vec![1], vec![2], now())
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    stores.accounts.mark_needs_relogin(30).await.unwrap();
    let t = now();
    stores
        .orders
        .replace_orders(10, t, &[], t + Duration::minutes(20), None)
        .await
        .unwrap();

    let records = stores
        .orders
        .list_freshness(account, &[10, 20, 30, 99])
        .await
        .unwrap();

    let summary: Vec<_> = records
        .iter()
        .map(|r| {
            (
                r.character_id,
                r.name.as_str(),
                r.needs_relogin,
                r.last_success_at,
                r.next_poll_at,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (20, "Pilot 20", false, None, None),
            (
                10,
                "Pilot 10",
                false,
                Some(t),
                Some(t + Duration::minutes(20))
            ),
            (30, "Pilot 30", true, None, None),
        ]
    );
    let narrowed = stores.orders.list_freshness(account, &[10]).await.unwrap();
    assert_eq!(narrowed.len(), 1);
    let _ = REGION;
}

async fn seeded_order(stores: &Stores) {
    let account = account_with(stores, &[1, 2]).await;
    stores
        .orders
        .seed_order(1, &sell_order(10, 34))
        .await
        .unwrap();
    let mut other = sell_order(11, 35);
    other.location_id = 99;
    other.escrow_cents = Some(12_345);
    stores.orders.seed_order(1, &other).await.unwrap();
    // The same ID again replaces the order, also when it was another character's.
    stores
        .orders
        .seed_order(2, &sell_order(10, 36))
        .await
        .unwrap();

    let listed = |character: i64| async move {
        stores
            .orders
            .list_orders(account, &[character], Side::Sell, Sort::default())
            .await
            .unwrap()
    };
    let mut ids: Vec<_> = listed(1)
        .await
        .iter()
        .map(|o| (o.order_id, o.type_id))
        .collect();
    ids.sort();
    assert_eq!(ids, [(11, 35)]);
    let second = listed(2).await;
    assert_eq!((second[0].order_id, second[0].type_id), (10, 36));

    // Nothing but the order itself was recorded.
    assert!(stores.orders.snapshot_counts(1).await.unwrap().is_empty());
    assert!(stores.orders.poll_totals(1).await.unwrap().is_empty());
    assert!(stores.orders.remembered_orders(1).await.unwrap().is_empty());
    assert!(stores.orders.order_sync(1).await.unwrap().is_none());

    let missing = stores.orders.seed_order(404, &sell_order(12, 34)).await;
    assert!(
        matches!(missing, Err(StoreError::MissingParent)),
        "{missing:?}"
    );
}
