//! The wallet, the assets and their valuation, the wallet transactions, and the polls behind them.

use sekondo::{
    esi::{EsiPrice, EsiTransaction},
    orders::Dir,
    range::Range,
    store::{NameRow, StoreError, Stores},
    wealth::{InventorySort, InventorySortKey},
    wealth_poller::Kind,
};
use time::{Duration, OffsetDateTime, macros::datetime};

use super::{
    Case,
    market::asset,
    support::{STATION, account_with, boxed, buy_order, case, now, sell_order},
};

pub fn cases() -> Vec<Case> {
    vec![
        case(
            "wealth/the-wallet-appends-and-keeps-the-later-of-one-instant",
            |s| boxed(wallet_appends(s)),
        ),
        case(
            "wealth/the-current-figures-take-each-series-latest-value",
            |s| boxed(current_figures(s)),
        ),
        case("wealth/the-history-carries-each-series-forward", |s| {
            boxed(history_carries_forward(s))
        }),
        case("wealth/assets-are-replaced-and-valued", |s| {
            boxed(assets_replaced(s))
        }),
        case(
            "wealth/an-item-that-changed-hands-moves-to-its-new-owner",
            |s| boxed(assets_move(s)),
        ),
        case(
            "wealth/inventory-rows-sum-per-character-item-and-place",
            |s| boxed(inventory_groups(s)),
        ),
        case(
            "wealth/inventory-sorts-by-every-column-in-both-directions",
            |s| boxed(inventory_sorting(s)),
        ),
        case(
            "wealth/transactions-are-stored-once-and-only-personal-ones",
            |s| boxed(transactions_once(s)),
        ),
        case(
            "wealth/a-full-read-is-cleared-by-a-transactions-poll",
            |s| boxed(full_read(s)),
        ),
        case("wealth/new-purchases-settle-the-cost-of-sales", |s| {
            boxed(purchases_settle_sales(s))
        }),
        case(
            "wealth/a-failure-backs-off-per-kind-and-a-success-resets",
            |s| boxed(failure_backoff(s)),
        ),
        case(
            "wealth/a-deferral-moves-the-next-poll-without-a-failure",
            |s| boxed(deferral(s)),
        ),
        case(
            "wealth/each-kind-is-scheduled-and-claimed-on-its-own",
            |s| boxed(schedule_and_claim(s)),
        ),
        case(
            "wealth/seeded-assets-and-poll-totals-are-stored-as-they-are",
            |s| boxed(seeded_rows(s)),
        ),
        case(
            "wealth/the-wallet-balances-and-the-asset-ids-are-listed",
            |s| boxed(wallet_and_asset_lists(s)),
        ),
        case("wealth/making-a-poll-due-never-delays-it", |s| {
            boxed(make_due(s))
        }),
        case("wealth/the-inventory-values-are-listed-oldest-first", |s| {
            boxed(inventory_values(s))
        }),
        case("wealth/freshness-lists-the-wallet-and-the-assets", |s| {
            boxed(freshness(s))
        }),
        case(
            "wealth/unlinking-a-character-removes-its-wealth-data",
            |s| boxed(unlinking(s)),
        ),
    ]
}

async fn wallet(stores: &Stores, character: i64, at: OffsetDateTime, cents: i64) {
    stores
        .wealth
        .store_wallet(character, at, cents, at + Duration::minutes(20), None)
        .await
        .unwrap();
}

async fn wallet_appends(stores: &Stores) {
    let account = account_with(stores, &[1]).await;
    let t = datetime!(2026-10-20 10:00 UTC);
    wallet(stores, 1, t, 1_000).await;
    wallet(stores, 1, t + Duration::minutes(20), 2_000).await;
    // Two polls in the same instant keep the later one.
    wallet(stores, 1, t + Duration::minutes(20), 2_500).await;

    let points = stores
        .wealth
        .history(account, &[1], Range::Day, t + Duration::hours(1))
        .await
        .unwrap();

    assert_eq!(points.last().unwrap().wallet_cents, 2_500);
    let at = |h: time::OffsetDateTime| points.iter().find(|p| p.at == h).unwrap();
    assert_eq!(at(datetime!(2026-10-20 10:00 UTC)).wallet_cents, 1_000);
    let state = stores
        .wealth
        .wealth_sync(1, Kind::Wallet)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.last_success_at, Some(t + Duration::minutes(20)));
    assert_eq!(state.next_poll_at, t + Duration::minutes(40));
    assert_eq!((state.failures, state.last_error), (0, None));
    let missing = stores.wealth.store_wallet(404, t, 1, t, None).await;
    assert!(matches!(missing, Err(StoreError::MissingParent)));
}

async fn current_figures(stores: &Stores) {
    let account = account_with(stores, &[1, 2, 3]).await;
    let t = datetime!(2026-10-20 10:00 UTC);
    wallet(stores, 1, t - Duration::hours(1), 100).await;
    wallet(stores, 1, t, 400).await;
    wallet(stores, 2, t, 50).await;
    // Character 1: a personal sell order worth 1000.00 and a corporation one worth 2000.00; a
    // personal buy order in escrow 500.00 and a corporation one in escrow 70.00.
    let mut corp_sell = sell_order(2, 34);
    corp_sell.is_corporation = true;
    corp_sell.price_cents = 20_000;
    let mut corp_buy = buy_order(4, 34);
    corp_buy.is_corporation = true;
    corp_buy.escrow_cents = Some(7_000);
    stores
        .orders
        .replace_orders(
            1,
            t,
            &[sell_order(1, 34), corp_sell, buy_order(3, 34), corp_buy],
            t,
            None,
        )
        .await
        .unwrap();
    stores
        .wealth
        .store_assets(
            1,
            t - Duration::days(2),
            &[asset(1, 34, 10)],
            &[Some(STATION)],
            t,
            None,
        )
        .await
        .unwrap();
    stores
        .market
        .replace_prices(
            t - Duration::days(1),
            &[EsiPrice {
                type_id: 34,
                average_price_cents: Some(300),
            }],
            t,
            None,
        )
        .await
        .unwrap();

    // An inventory value can be recorded at a chosen time; the same instant is replaced.
    stores
        .wealth
        .seed_inventory_value(3, t - Duration::hours(2), 111)
        .await
        .unwrap();
    stores
        .wealth
        .seed_inventory_value(3, t - Duration::hours(2), 222)
        .await
        .unwrap();
    let missing = stores.wealth.seed_inventory_value(404, t, 1).await;
    assert!(matches!(missing, Err(StoreError::MissingParent)));
    let only_three = stores.wealth.current(account, &[3]).await.unwrap();
    assert_eq!(only_three.inventory_cents, 222);
    stores
        .wealth
        .seed_inventory_value(3, t - Duration::hours(2), 0)
        .await
        .unwrap();

    let all = stores.wealth.current(account, &[1, 2, 3]).await.unwrap();

    assert_eq!(all.wallet_cents, 450);
    assert_eq!(all.sell_cents, 100_000, "personal sell orders only");
    assert_eq!(all.escrow_cents, 50_000, "personal buy escrow only");
    assert_eq!(all.inventory_cents, 3_000);
    assert_eq!(all.total_cents(), 450 + 100_000 + 50_000 + 3_000);
    let one = stores.wealth.current(account, &[2]).await.unwrap();
    assert_eq!(
        (one.wallet_cents, one.sell_cents, one.inventory_cents),
        (50, 0, 0)
    );
    let stranger = account_with(stores, &[4]).await;
    let none = stores.wealth.current(stranger, &[1, 2, 3]).await.unwrap();
    assert_eq!(
        none.total_cents(),
        0,
        "selecting another account's characters shows nothing"
    );
    assert_eq!(
        stores
            .wealth
            .current(account, &[])
            .await
            .unwrap()
            .total_cents(),
        0
    );
}

async fn history_carries_forward(stores: &Stores) {
    let account = account_with(stores, &[1, 2]).await;
    let now = datetime!(2026-10-20 12:10 UTC);
    wallet(stores, 1, datetime!(2026-10-20 06:05 UTC), 1_000).await;
    wallet(stores, 1, datetime!(2026-10-20 10:20 UTC), 4_000).await;
    stores
        .orders
        .replace_orders(
            1,
            datetime!(2026-10-20 08:00 UTC),
            &[sell_order(1, 34)],
            now,
            None,
        )
        .await
        .unwrap();
    stores
        .wealth
        .store_assets(
            2,
            datetime!(2026-10-20 11:30 UTC),
            &[asset(1, 34, 1)],
            &[None],
            now,
            None,
        )
        .await
        .unwrap();

    let points = stores
        .wealth
        .history(account, &[1, 2], Range::Day, now)
        .await
        .unwrap();

    assert_eq!(points.len(), 49);
    assert_eq!(points.last().unwrap().at, now);
    let at = |t: time::OffsetDateTime| points.iter().find(|p| p.at == t).unwrap();
    let early = at(datetime!(2026-10-20 06:00 UTC));
    assert_eq!((early.contributors, early.wallet_cents), (0, 0));
    let six = at(datetime!(2026-10-20 07:00 UTC));
    assert_eq!(
        (six.contributors, six.wallet_cents, six.sell_cents),
        (1, 1_000, 0)
    );
    let nine = at(datetime!(2026-10-20 09:00 UTC));
    assert_eq!((nine.wallet_cents, nine.sell_cents), (1_000, 100_000));
    let eleven = at(datetime!(2026-10-20 11:00 UTC));
    assert_eq!(eleven.wallet_cents, 4_000, "10:20 is before 11:00");
    assert_eq!(eleven.contributors, 1);
    // Character 2 has an inventory entry from 11:30, with no prices known yet: worth nothing but
    // known.
    let last = points.last().unwrap();
    assert_eq!(last.contributors, 2);
    assert_eq!(last.inventory_cents, 0);
    assert!(
        stores
            .wealth
            .history(account, &[], Range::Day, now)
            .await
            .unwrap()
            .is_empty()
    );
}

fn name(id: i64, name: &str) -> NameRow {
    NameRow {
        id,
        category: "t".to_owned(),
        name: Some(name.to_owned()),
    }
}

async fn assets_replaced(stores: &Stores) {
    let account = account_with(stores, &[1]).await;
    let t = datetime!(2026-10-20 10:00 UTC);
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
                    average_price_cents: None,
                },
            ],
            t,
            None,
        )
        .await
        .unwrap();
    stores
        .market
        .store_names(&[name(34, "Tritanium"), name(STATION, "Jita 4-4")], t, None)
        .await
        .unwrap();
    let mut copy = asset(3, 34, 1);
    copy.is_blueprint_copy = true;
    stores
        .wealth
        .store_assets(
            1,
            t,
            &[asset(1, 34, 100), asset(2, 35, 4), copy],
            &[Some(STATION), Some(STATION), None],
            t + Duration::hours(1),
            Some(t + Duration::minutes(30)),
        )
        .await
        .unwrap();

    let rows = stores
        .wealth
        .inventory_rows(account, &[1], InventorySort::default())
        .await
        .unwrap();

    let summary: Vec<_> = rows
        .iter()
        .map(|r| {
            (
                r.type_id,
                r.quantity,
                r.unit_price_cents,
                r.value_cents,
                r.top_known,
                r.location_id,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (34, 100, Some(550), Some(55_000), true, STATION),
            (35, 4, None, None, true, STATION),
            (34, 1, None, None, false, STATION),
        ],
        "a copy is unpriced and a row of its own; one without a top location keeps its own"
    );
    assert_eq!(rows[0].type_name.as_deref(), Some("Tritanium"));
    assert_eq!(rows[0].location_name.as_deref(), Some("Jita 4-4"));
    assert!(rows[0].location_known);
    assert_eq!(rows[0].character_name, "Pilot 1");
    assert_eq!(rows[1].type_name, None);
    let state = stores
        .wealth
        .wealth_sync(1, Kind::Assets)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.next_poll_at, t + Duration::hours(1));
    assert_eq!(state.esi_expires_at, Some(t + Duration::minutes(30)));

    // The next poll replaces the list.
    stores
        .wealth
        .store_assets(
            1,
            t + Duration::hours(1),
            &[asset(9, 34, 2)],
            &[Some(STATION)],
            t,
            None,
        )
        .await
        .unwrap();
    let after = stores
        .wealth
        .inventory_rows(account, &[1], InventorySort::default())
        .await
        .unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].quantity, 2);
    let current = stores.wealth.current(account, &[1]).await.unwrap();
    assert_eq!(
        current.inventory_cents, 1_100,
        "the value recorded with the second poll"
    );
    let missing = stores.wealth.store_assets(404, t, &[], &[], t, None).await;
    assert!(matches!(missing, Err(StoreError::MissingParent)));
    let twice = stores
        .wealth
        .store_assets(
            1,
            t,
            &[asset(5, 34, 1), asset(5, 34, 2)],
            &[None, None],
            t,
            None,
        )
        .await;
    assert!(twice.is_err(), "an item is listed once");
    let kept = stores
        .wealth
        .inventory_rows(account, &[1], InventorySort::default())
        .await
        .unwrap();
    assert_eq!(kept.len(), 1, "a failed poll changes nothing");
}

async fn assets_move(stores: &Stores) {
    let account = account_with(stores, &[1, 2]).await;
    let t = now();
    stores
        .wealth
        .store_assets(
            1,
            t,
            &[asset(10, 34, 5), asset(11, 34, 1)],
            &[None, None],
            t,
            None,
        )
        .await
        .unwrap();

    // Character 2 now lists item 10: it moves, and character 1 keeps item 11.
    stores
        .wealth
        .store_assets(2, t, &[asset(10, 34, 5)], &[None], t, None)
        .await
        .unwrap();

    let by_character = |rows: Vec<sekondo::wealth::InventoryRow>| {
        rows.into_iter()
            .map(|r| (r.character_id, r.quantity))
            .collect::<Vec<_>>()
    };
    let mut rows = by_character(
        stores
            .wealth
            .inventory_rows(account, &[1, 2], InventorySort::default())
            .await
            .unwrap(),
    );
    rows.sort_unstable();
    assert_eq!(rows, [(1, 1), (2, 5)]);
}

async fn inventory_groups(stores: &Stores) {
    let account = account_with(stores, &[1, 2]).await;
    let t = now();
    let mut in_system = asset(3, 34, 7);
    in_system.location_id = 30_000_142;
    stores
        .wealth
        .store_assets(
            1,
            t,
            &[
                asset(1, 34, 100),
                asset(2, 34, 50),
                in_system,
                asset(4, 35, 1),
            ],
            &[Some(STATION), Some(STATION), Some(30_000_142), None],
            t,
            None,
        )
        .await
        .unwrap();
    stores
        .wealth
        .store_assets(2, t, &[asset(5, 34, 9)], &[Some(STATION)], t, None)
        .await
        .unwrap();

    let rows = stores
        .wealth
        .inventory_rows(
            account,
            &[1, 2],
            InventorySort {
                key: InventorySortKey::Quantity,
                dir: Dir::Desc,
            },
        )
        .await
        .unwrap();

    let summary: Vec<_> = rows
        .iter()
        .map(|r| (r.character_id, r.type_id, r.location_id, r.quantity))
        .collect();
    assert_eq!(
        summary,
        [
            (1, 34, STATION, 150),
            (2, 34, STATION, 9),
            (1, 34, 30_000_142, 7),
            (1, 35, STATION, 1),
        ]
    );
    let only_two = stores
        .wealth
        .inventory_rows(account, &[2], InventorySort::default())
        .await
        .unwrap();
    assert_eq!(only_two.len(), 1);
    let stranger = account_with(stores, &[3]).await;
    assert!(
        stores
            .wealth
            .inventory_rows(stranger, &[1, 2], InventorySort::default())
            .await
            .unwrap()
            .is_empty()
    );
}

async fn inventory_sorting(stores: &Stores) {
    let account = account_with(stores, &[1, 2]).await;
    let t = now();
    stores
        .market
        .replace_prices(
            t,
            &[
                EsiPrice {
                    type_id: 34,
                    average_price_cents: Some(100),
                },
                EsiPrice {
                    type_id: 35,
                    average_price_cents: Some(30),
                },
            ],
            t,
            None,
        )
        .await
        .unwrap();
    stores
        .market
        .store_names(
            &[
                name(34, "Zydrine"),
                name(35, "apple"),
                name(60_000_001, "Amarr"),
                name(STATION, "jita"),
            ],
            t,
            None,
        )
        .await
        .unwrap();
    // Character 1 "Pilot 1": 34 ×10 in Jita (10.00), 35 ×100 in Amarr (30.00), 36 ×5 unpriced.
    // Character 2 "Pilot 2": 34 ×1 in Amarr (1.00).
    let mut amarr = asset(2, 35, 100);
    amarr.location_id = 60_000_001;
    stores
        .wealth
        .store_assets(
            1,
            t,
            &[asset(1, 34, 10), amarr, asset(3, 36, 5)],
            &[Some(STATION), Some(60_000_001), Some(STATION)],
            t,
            None,
        )
        .await
        .unwrap();
    let mut other = asset(4, 34, 1);
    other.location_id = 60_000_001;
    stores
        .wealth
        .store_assets(2, t, &[other], &[Some(60_000_001)], t, None)
        .await
        .unwrap();
    let list = |key: InventorySortKey, dir: Dir| {
        let wealth = stores.wealth.clone();
        async move {
            wealth
                .inventory_rows(account, &[1, 2], InventorySort { key, dir })
                .await
                .unwrap()
                .into_iter()
                .map(|r| (r.character_id, r.type_id, r.location_id))
                .collect::<Vec<_>>()
        }
    };
    let (jita, amarr_id) = (STATION, 60_000_001);

    // Items: "#36" < "apple" < "zydrine", then the character and the place.
    assert_eq!(
        list(InventorySortKey::Item, Dir::Asc).await,
        [
            (1, 36, jita),
            (1, 35, amarr_id),
            (1, 34, jita),
            (2, 34, amarr_id)
        ]
    );
    assert_eq!(
        list(InventorySortKey::Item, Dir::Desc).await,
        [
            (1, 34, jita),
            (2, 34, amarr_id),
            (1, 35, amarr_id),
            (1, 36, jita)
        ]
    );
    // Value: 10.00, 3000.00 (100 × 0.30), 1.00; unpriced last in both directions.
    assert_eq!(
        list(InventorySortKey::Value, Dir::Desc).await,
        [
            (1, 35, amarr_id),
            (1, 34, jita),
            (2, 34, amarr_id),
            (1, 36, jita)
        ]
    );
    assert_eq!(
        list(InventorySortKey::Value, Dir::Asc).await,
        [
            (2, 34, amarr_id),
            (1, 34, jita),
            (1, 35, amarr_id),
            (1, 36, jita)
        ]
    );
    assert_eq!(
        list(InventorySortKey::Price, Dir::Asc).await,
        [
            (1, 35, amarr_id),
            (1, 34, jita),
            (2, 34, amarr_id),
            (1, 36, jita)
        ]
    );
    assert_eq!(
        list(InventorySortKey::Price, Dir::Desc).await,
        [
            (1, 34, jita),
            (2, 34, amarr_id),
            (1, 35, amarr_id),
            (1, 36, jita)
        ]
    );
    assert_eq!(
        list(InventorySortKey::Quantity, Dir::Desc).await,
        [
            (1, 35, amarr_id),
            (1, 34, jita),
            (1, 36, jita),
            (2, 34, amarr_id)
        ]
    );
    assert_eq!(
        list(InventorySortKey::Character, Dir::Desc).await,
        [
            (2, 34, amarr_id),
            (1, 36, jita),
            (1, 35, amarr_id),
            (1, 34, jita)
        ]
    );
    // Places: "amarr" < "jita".
    assert_eq!(
        list(InventorySortKey::Location, Dir::Asc).await,
        [
            (1, 35, amarr_id),
            (2, 34, amarr_id),
            (1, 36, jita),
            (1, 34, jita)
        ]
    );
}

fn transaction(
    id: i64,
    is_buy: bool,
    personal: bool,
    type_id: i64,
    days_ago: i64,
    quantity: i64,
) -> EsiTransaction {
    EsiTransaction {
        transaction_id: id,
        bought_at: now() - Duration::days(days_ago),
        type_id,
        location_id: STATION,
        quantity,
        unit_price_cents: 8_000,
        is_buy,
        is_personal: personal,
    }
}

async fn transactions_once(stores: &Stores) {
    let account = account_with(stores, &[1, 2]).await;
    let t = now();
    let first = [
        transaction(1, true, true, 34, 5, 10),
        transaction(2, true, false, 34, 5, 10),
        transaction(3, false, true, 34, 4, 3),
        transaction(4, false, false, 34, 4, 3),
    ];

    let stored = stores
        .wealth
        .store_transactions(1, t, &first, t + Duration::hours(1), Some(t))
        .await
        .unwrap();

    assert_eq!(
        stored,
        (1, 1),
        "one personal purchase and one personal sale"
    );
    assert!(stores.wealth.any_transaction_stored(&[1]).await.unwrap());
    assert!(
        stores
            .wealth
            .any_transaction_stored(&[99, 3])
            .await
            .unwrap()
    );
    assert!(
        !stores
            .wealth
            .any_transaction_stored(&[2, 4, 99])
            .await
            .unwrap()
    );
    assert!(!stores.wealth.any_transaction_stored(&[]).await.unwrap());
    let state = stores
        .wealth
        .wealth_sync(1, Kind::Transactions)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.next_poll_at, t + Duration::hours(1));
    assert_eq!(state.esi_expires_at, Some(t));

    // Again with one new purchase: only that one is new. A transaction another character stored
    // is not ours to take.
    let second = [
        transaction(1, true, true, 34, 5, 10),
        transaction(5, true, true, 34, 3, 2),
        transaction(3, false, true, 34, 4, 3),
    ];
    let again = stores
        .wealth
        .store_transactions(1, t, &second, t, None)
        .await
        .unwrap();
    assert_eq!(again, (1, 0));
    let other = stores
        .wealth
        .store_transactions(2, t, &[transaction(5, true, true, 34, 3, 2)], t, None)
        .await
        .unwrap();
    assert_eq!(other, (0, 0));
    let listed = stores
        .trades
        .list_trades(account, &[1, 2], t - Duration::days(30), t)
        .await
        .unwrap();
    let ids: Vec<(i64, bool)> = listed
        .iter()
        .map(|r| (r.transaction_id, r.is_sale))
        .collect();
    assert_eq!(ids, [(5, false), (3, true), (1, false)], "newest first");
    let missing = stores
        .wealth
        .store_transactions(404, t, &first, t, None)
        .await;
    assert!(matches!(missing, Err(StoreError::MissingParent)));
}

async fn full_read(stores: &Stores) {
    account_with(stores, &[1]).await;
    let t = now();
    assert!(!stores.wealth.full_read(1).await.unwrap());
    stores.wealth.seed_full_read(1, true).await.unwrap();
    assert!(
        !stores.wealth.full_read(1).await.unwrap(),
        "there is no transactions row to mark yet"
    );

    stores
        .wealth
        .store_transactions(1, t, &[], t, None)
        .await
        .unwrap();
    stores.wealth.seed_full_read(1, true).await.unwrap();
    assert!(stores.wealth.full_read(1).await.unwrap());
    stores.wealth.store_wallet(1, t, 1, t, None).await.unwrap();
    assert!(
        stores.wealth.full_read(1).await.unwrap(),
        "a wallet poll does not clear it"
    );

    stores
        .wealth
        .store_transactions(1, t, &[], t, None)
        .await
        .unwrap();
    assert!(!stores.wealth.full_read(1).await.unwrap());
}

async fn purchases_settle_sales(stores: &Stores) {
    let account = account_with(stores, &[1]).await;
    let t = now();
    let buy = |id: i64, quantity: i64, price: i64, days_ago: i64| EsiTransaction {
        unit_price_cents: price,
        ..transaction(id, true, true, 34, days_ago, quantity)
    };
    let sale = |id: i64, quantity: i64, price: i64, days_ago: i64| EsiTransaction {
        unit_price_cents: price,
        ..transaction(id, false, true, 34, days_ago, quantity)
    };
    // The sale comes first; the purchase it needs is only polled later.
    stores
        .wealth
        .store_transactions(1, t, &[sale(10, 4, 10_000, 1)], t, None)
        .await
        .unwrap();
    let before = stores
        .trades
        .list_trades(account, &[1], t - Duration::days(30), t)
        .await
        .unwrap();
    assert_eq!(before[0].covered_units, Some(0));
    assert_eq!(before[0].margin_cents, None);

    stores
        .wealth
        .store_transactions(1, t, &[buy(11, 10, 8_000, 5)], t, None)
        .await
        .unwrap();

    let after = stores
        .trades
        .list_trades(account, &[1], t - Duration::days(30), t)
        .await
        .unwrap();
    let sold = after.iter().find(|r| r.is_sale).unwrap();
    assert_eq!(sold.covered_units, Some(4));
    assert_eq!(sold.cost_per_unit_cents, Some(8_000));
    // No fees are known yet: no margin.
    assert_eq!(sold.margin_cents, None);
}

async fn failure_backoff(stores: &Stores) {
    account_with(stores, &[1]).await;
    let t = now();
    let (start, interval) = (Duration::minutes(1), Duration::minutes(20));

    stores
        .wealth
        .record_failure(1, Kind::Wallet, t, "boom", start, interval)
        .await
        .unwrap();
    let first = stores
        .wealth
        .wealth_sync(1, Kind::Wallet)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((first.failures, first.next_poll_at), (1, t + start));
    assert_eq!(first.last_error.as_deref(), Some("boom"));
    assert!(
        stores
            .wealth
            .wealth_sync(1, Kind::Assets)
            .await
            .unwrap()
            .is_none()
    );

    let mut waits = Vec::new();
    for round in 1..=6 {
        let at = t + Duration::hours(round);
        stores
            .wealth
            .record_failure(1, Kind::Wallet, at, "again", start, interval)
            .await
            .unwrap();
        let s = stores
            .wealth
            .wealth_sync(1, Kind::Wallet)
            .await
            .unwrap()
            .unwrap();
        waits.push((s.failures, (s.next_poll_at - at).whole_seconds()));
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

    stores
        .wealth
        .store_wallet(1, t + Duration::days(1), 5, t + Duration::days(1), None)
        .await
        .unwrap();
    let ok = stores
        .wealth
        .wealth_sync(1, Kind::Wallet)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((ok.failures, ok.last_error), (0, None));
    // A first failure waits no longer than the interval.
    stores
        .wealth
        .record_failure(1, Kind::Assets, t, "x", Duration::minutes(30), interval)
        .await
        .unwrap();
    let capped = stores
        .wealth
        .wealth_sync(1, Kind::Assets)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(capped.next_poll_at, t + interval);
    let gone = stores
        .wealth
        .record_failure(404, Kind::Wallet, t, "x", start, interval)
        .await;
    assert!(matches!(gone, Err(StoreError::MissingParent)));
}

async fn deferral(stores: &Stores) {
    account_with(stores, &[1]).await;
    let t = now();
    stores
        .wealth
        .record_failure(
            1,
            Kind::Assets,
            t,
            "boom",
            Duration::minutes(1),
            Duration::hours(1),
        )
        .await
        .unwrap();
    let until = t + Duration::hours(2);

    stores
        .wealth
        .record_deferral(1, Kind::Assets, t + Duration::seconds(3), until)
        .await
        .unwrap();
    stores
        .wealth
        .record_deferral(1, Kind::Wallet, t, until)
        .await
        .unwrap();

    let assets = stores
        .wealth
        .wealth_sync(1, Kind::Assets)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((assets.next_poll_at, assets.failures), (until, 1));
    assert_eq!(assets.last_error.as_deref(), Some("boom"));
    assert_eq!(assets.last_attempt_at, Some(t + Duration::seconds(3)));
    let wallet = stores
        .wealth
        .wealth_sync(1, Kind::Wallet)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (wallet.next_poll_at, wallet.failures, wallet.last_error),
        (until, 0, None)
    );
    let gone = stores
        .wealth
        .record_deferral(404, Kind::Wallet, t, until)
        .await;
    assert!(matches!(gone, Err(StoreError::MissingParent)));
}

async fn schedule_and_claim(stores: &Stores) {
    account_with(stores, &[1, 2, 3]).await;
    stores.accounts.mark_needs_relogin(3).await.unwrap();
    let t = now();
    for kind in Kind::ALL {
        let mut unscheduled = stores.wealth.list_unscheduled(kind).await.unwrap();
        unscheduled.sort_unstable();
        assert_eq!(unscheduled, [1, 2]);
    }

    stores
        .wealth
        .insert_schedule(
            Kind::Wallet,
            &[1, 2],
            &[t - Duration::minutes(10), t + Duration::hours(1)],
        )
        .await
        .unwrap();
    stores
        .wealth
        .insert_schedule(Kind::Assets, &[2], &[t - Duration::minutes(20)])
        .await
        .unwrap();
    stores
        .wealth
        .insert_schedule(Kind::Wallet, &[1], &[t + Duration::days(5)])
        .await
        .unwrap();

    let mut wallet_left = stores.wealth.list_unscheduled(Kind::Wallet).await.unwrap();
    wallet_left.sort_unstable();
    assert!(wallet_left.is_empty());
    assert_eq!(
        stores.wealth.list_unscheduled(Kind::Assets).await.unwrap(),
        [1]
    );
    assert_eq!(
        stores
            .wealth
            .wealth_sync(1, Kind::Wallet)
            .await
            .unwrap()
            .unwrap()
            .next_poll_at,
        t - Duration::minutes(10),
        "a schedule is kept"
    );
    let missing = stores
        .wealth
        .insert_schedule(Kind::Wallet, &[404], &[t])
        .await;
    assert!(matches!(missing, Err(StoreError::MissingParent)));

    let lease = Duration::minutes(5);
    let mut claimed = stores.wealth.claim_due(t, lease, 10).await.unwrap();
    claimed.sort_by_key(|(id, kind)| (*id, kind.as_str()));
    assert_eq!(claimed, [(1, Kind::Wallet), (2, Kind::Assets)]);
    assert!(
        stores
            .wealth
            .claim_due(t, lease, 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        stores
            .wealth
            .wealth_sync(1, Kind::Wallet)
            .await
            .unwrap()
            .unwrap()
            .next_poll_at,
        t + lease
    );
    stores
        .wealth
        .insert_schedule(
            Kind::Transactions,
            &[3, 1],
            &[t - Duration::days(1), t - Duration::days(2)],
        )
        .await
        .unwrap();
    let limited = stores
        .wealth
        .claim_due(t + Duration::days(1), lease, 5)
        .await
        .unwrap();
    assert!(limited.contains(&(1, Kind::Transactions)));
    assert!(
        !limited.iter().any(|(id, _)| *id == 3),
        "a character needing a login is not claimed"
    );
}

async fn make_due(stores: &Stores) {
    account_with(stores, &[1]).await;
    let t = now();
    stores
        .wealth
        .insert_schedule(Kind::Wallet, &[1], &[t + Duration::hours(1)])
        .await
        .unwrap();
    stores
        .wealth
        .insert_schedule(Kind::Assets, &[1], &[t - Duration::hours(1)])
        .await
        .unwrap();

    stores.wealth.make_due(1, Kind::Wallet, t).await.unwrap();
    stores.wealth.make_due(1, Kind::Assets, t).await.unwrap();
    stores
        .wealth
        .make_due(1, Kind::Transactions, t)
        .await
        .unwrap();
    stores.wealth.make_due(404, Kind::Wallet, t).await.unwrap();

    assert_eq!(
        stores
            .wealth
            .wealth_sync(1, Kind::Wallet)
            .await
            .unwrap()
            .unwrap()
            .next_poll_at,
        t
    );
    assert_eq!(
        stores
            .wealth
            .wealth_sync(1, Kind::Assets)
            .await
            .unwrap()
            .unwrap()
            .next_poll_at,
        t - Duration::hours(1)
    );
    assert!(
        stores
            .wealth
            .wealth_sync(1, Kind::Transactions)
            .await
            .unwrap()
            .is_none()
    );
}

async fn freshness(stores: &Stores) {
    let account = account_with(stores, &[1, 2]).await;
    let t = now();
    stores
        .wealth
        .store_wallet(1, t, 1, t + Duration::minutes(20), None)
        .await
        .unwrap();
    stores
        .wealth
        .store_assets(
            1,
            t - Duration::hours(1),
            &[],
            &[],
            t + Duration::hours(2),
            None,
        )
        .await
        .unwrap();
    stores
        .wealth
        .store_transactions(2, t, &[], t + Duration::hours(1), None)
        .await
        .unwrap();
    stores.accounts.mark_needs_relogin(2).await.unwrap();

    let records = stores
        .wealth
        .list_wealth_freshness(account, &[1, 2, 9])
        .await
        .unwrap();

    let summary: Vec<_> = records
        .iter()
        .map(|r| {
            (
                r.character_id,
                r.needs_relogin,
                r.wallet_last_success_at,
                r.wallet_next_poll_at,
                r.assets_last_success_at,
                r.assets_next_poll_at,
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (
                1,
                false,
                Some(t),
                Some(t + Duration::minutes(20)),
                Some(t - Duration::hours(1)),
                Some(t + Duration::hours(2))
            ),
            (2, true, None, None, None, None),
        ]
    );
}

async fn unlinking(stores: &Stores) {
    let account = account_with(stores, &[1, 2]).await;
    let t = now();
    for id in [1, 2] {
        wallet(stores, id, t, 100).await;
        stores
            .wealth
            .store_assets(id, t, &[asset(id, 34, 1)], &[None], t, None)
            .await
            .unwrap();
        stores
            .wealth
            .store_transactions(id, t, &[transaction(id, true, true, 34, 1, 1)], t, None)
            .await
            .unwrap();
    }

    stores.accounts.unlink_character(account, 1).await.unwrap();

    assert!(
        stores
            .wealth
            .wealth_sync(1, Kind::Wallet)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        stores
            .wealth
            .wealth_sync(1, Kind::Assets)
            .await
            .unwrap()
            .is_none()
    );
    assert!(!stores.wealth.any_transaction_stored(&[1]).await.unwrap());
    assert!(stores.wealth.any_transaction_stored(&[2]).await.unwrap());
    let current = stores.wealth.current(account, &[1, 2]).await.unwrap();
    assert_eq!(current.wallet_cents, 100);
    let rows = stores
        .wealth
        .inventory_rows(account, &[1, 2], InventorySort::default())
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    // Its item can be listed by another character now.
    stores
        .wealth
        .store_assets(2, t, &[asset(1, 34, 1)], &[None], t, None)
        .await
        .unwrap();
}

async fn inventory_values(stores: &Stores) {
    account_with(stores, &[1, 2]).await;
    let t = datetime!(2026-10-20 10:00 UTC);
    stores
        .wealth
        .seed_inventory_value(1, t + Duration::hours(1), 300)
        .await
        .unwrap();
    stores.wealth.seed_inventory_value(1, t, 100).await.unwrap();
    stores.wealth.seed_inventory_value(2, t, 999).await.unwrap();

    assert_eq!(
        stores.wealth.inventory_values(1).await.unwrap(),
        [(t, 100), (t + Duration::hours(1), 300)]
    );
    assert_eq!(stores.wealth.inventory_values(2).await.unwrap(), [(t, 999)]);
    assert!(
        stores
            .wealth
            .inventory_values(404)
            .await
            .unwrap()
            .is_empty()
    );
}

async fn seeded_rows(stores: &Stores) {
    let account = account_with(stores, &[1, 2]).await;
    let t = datetime!(2026-10-20 10:00 UTC);

    // Poll totals keep their parts and counts; one at the same instant replaces the other.
    let totals = |sell: i64, buys: i64| sekondo::store::PollTotalsRow {
        polled_at: t,
        sell_cents: sell,
        escrow_cents: 2_000,
        sell_corp_cents: 400,
        escrow_corp_cents: 50,
        sell_count: 3,
        buy_count: buys,
    };
    stores
        .wealth
        .seed_poll_totals(1, &totals(3_000, 2))
        .await
        .unwrap();
    stores
        .wealth
        .seed_poll_totals(1, &totals(7_000, 5))
        .await
        .unwrap();
    assert_eq!(
        stores.orders.poll_totals(1).await.unwrap(),
        [totals(7_000, 5)]
    );
    let current = stores.wealth.current(account, &[1]).await.unwrap();
    assert_eq!((current.sell_cents, current.escrow_cents), (6_600, 1_950));
    let missing = stores.wealth.seed_poll_totals(404, &totals(1, 1)).await;
    assert!(
        matches!(missing, Err(StoreError::MissingParent)),
        "{missing:?}"
    );

    // An asset is stored as it is, next to the others, and the inventory value stays.
    stores
        .market
        .store_names(&[name(STATION, "Jita 4-4")], t, None)
        .await
        .unwrap();
    stores.market.seed_price(34, Some(250)).await.unwrap();
    stores
        .wealth
        .seed_asset(1, &asset(1, 34, 4), Some(STATION))
        .await
        .unwrap();
    let mut elsewhere = asset(2, 34, 6);
    elsewhere.location_id = 99;
    elsewhere.location_flag = "Deliveries".to_owned();
    stores.wealth.seed_asset(1, &elsewhere, None).await.unwrap();
    assert!(stores.wealth.inventory_values(1).await.unwrap().is_empty());
    let rows = stores
        .wealth
        .inventory_rows(account, &[1], InventorySort::default())
        .await
        .unwrap();
    let mut summary: Vec<_> = rows
        .iter()
        .map(|r| (r.quantity, r.location_id, r.top_known, r.value_cents))
        .collect();
    summary.sort();
    assert_eq!(
        summary,
        [(4, STATION, true, Some(1_000)), (6, 99, false, Some(1_500))]
    );

    // The same item stored again replaces the row, also when it was another character's.
    stores
        .wealth
        .seed_asset(2, &asset(1, 34, 1), Some(STATION))
        .await
        .unwrap();
    let one = stores
        .wealth
        .inventory_rows(account, &[1, 2], InventorySort::default())
        .await
        .unwrap();
    let mut owners: Vec<_> = one.iter().map(|r| (r.character_id, r.quantity)).collect();
    owners.sort();
    assert_eq!(owners, [(1, 6), (2, 1)]);
    let missing = stores.wealth.seed_asset(404, &asset(9, 34, 1), None).await;
    assert!(
        matches!(missing, Err(StoreError::MissingParent)),
        "{missing:?}"
    );
}

async fn wallet_and_asset_lists(stores: &Stores) {
    account_with(stores, &[1, 2]).await;
    let t = datetime!(2026-10-20 10:00 UTC);
    assert!(stores.wealth.wallet_balances(1).await.unwrap().is_empty());
    assert!(stores.wealth.asset_ids(1).await.unwrap().is_empty());

    for (at, cents) in [(t + Duration::hours(1), 300), (t, 100)] {
        stores
            .wealth
            .store_wallet(1, at, cents, at, None)
            .await
            .unwrap();
    }
    stores
        .wealth
        .store_wallet(2, t, 999, t, None)
        .await
        .unwrap();
    assert_eq!(
        stores.wealth.wallet_balances(1).await.unwrap(),
        [(t, 100), (t + Duration::hours(1), 300)]
    );

    stores
        .wealth
        .store_assets(
            1,
            t,
            &[asset(9, 34, 1), asset(3, 34, 1), asset(5, 35, 2)],
            &[None, None, None],
            t,
            None,
        )
        .await
        .unwrap();
    stores
        .wealth
        .store_assets(2, t, &[asset(4, 34, 1)], &[None], t, None)
        .await
        .unwrap();
    assert_eq!(stores.wealth.asset_ids(1).await.unwrap(), [3, 5, 9]);
    assert_eq!(stores.wealth.asset_ids(2).await.unwrap(), [4]);
}
