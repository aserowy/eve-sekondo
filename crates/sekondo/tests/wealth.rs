//! The wealth queries against Postgres: the current figures, the history and the inventory list.

mod common;

use sekondo::{
    esi::EsiAsset,
    range::Range,
    selection::CharacterSelection,
    store::{NameRow, NewCharacter, PollTotalsRow, Stores},
    wealth::{self, Current},
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const CAROL: i64 = 90000003;

fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc().replace_nanosecond(0).unwrap()
}

/// An account with these characters.
async fn account(stores: &Stores, characters: &[i64]) -> Uuid {
    let (first, rest) = characters.split_first().expect("a character");
    let character = |id: i64| NewCharacter {
        character_id: id,
        name: "name".to_owned(),
        owner_hash: "owner".to_owned(),
        scopes: Vec::new(),
    };
    let now = OffsetDateTime::now_utc();
    let account = stores
        .accounts
        .create_account_with_character(&character(*first), vec![1], vec![2], now)
        .await
        .unwrap();
    for id in rest {
        stores
            .accounts
            .upsert_character(account, &character(*id), vec![1], vec![2], now)
            .await
            .unwrap();
    }
    account
}

fn m(millions: i64) -> String {
    format!("{millions}000000.00")
}

/// ISK as written in the figures, in cents.
fn cents(isk: &str) -> i64 {
    (isk.parse::<f64>().unwrap() * 100.0).round() as i64
}

async fn wallet(stores: &Stores, character: i64, at: OffsetDateTime, balance: &str) {
    stores
        .wealth
        .store_wallet(character, at, cents(balance), at + Duration::hours(1), None)
        .await
        .unwrap();
}

async fn inventory(stores: &Stores, character: i64, at: OffsetDateTime, value: &str) {
    stores
        .wealth
        .seed_inventory_value(character, at, cents(value))
        .await
        .unwrap();
}

/// Orders recorded by a poll: `sell` and `escrow` in total, of which the corporation's part.
async fn orders(
    stores: &Stores,
    character: i64,
    at: OffsetDateTime,
    (sell, sell_corp): (&str, &str),
    (escrow, escrow_corp): (&str, &str),
) {
    stores
        .wealth
        .seed_poll_totals(
            character,
            &PollTotalsRow {
                polled_at: at,
                sell_cents: cents(sell),
                escrow_cents: cents(escrow),
                sell_corp_cents: cents(sell_corp),
                escrow_corp_cents: cents(escrow_corp),
                sell_count: 1,
                buy_count: 1,
            },
        )
        .await
        .unwrap();
}

fn wealth_store(stores: &Stores) -> std::sync::Arc<dyn sekondo::store::WealthStore> {
    stores.wealth.clone()
}

fn all(characters: &[i64]) -> CharacterSelection {
    CharacterSelection::resolve(characters, None)
}

async fn current(stores: &Stores, account: Uuid, characters: &[i64]) -> Current {
    wealth_store(stores)
        .current(account, all(characters).ids())
        .await
        .unwrap()
}

/// Alice: 100m wallet, 30m personal sell orders, 20m personal escrow, 50m inventory.
async fn alice_with_everything(stores: &Stores, at: OffsetDateTime) -> Uuid {
    let account = account(stores, &[ALICE]).await;
    wallet(stores, ALICE, at, &m(100)).await;
    orders(stores, ALICE, at, (&m(30), "0"), (&m(20), "0")).await;
    inventory(stores, ALICE, at, &m(50)).await;
    account
}

#[tokio::test]
async fn all_four_components_add_up() {
    let stores = common::stores();
    let at = now() - Duration::minutes(10);
    let account = alice_with_everything(&stores, at).await;

    let now = current(&stores, account, &[ALICE]).await;

    assert_eq!(now.wallet_cents, 10_000_000_000);
    assert_eq!(now.sell_cents, 3_000_000_000);
    assert_eq!(now.escrow_cents, 2_000_000_000);
    assert_eq!(now.inventory_cents, 5_000_000_000);
    assert_eq!(now.total_cents(), 20_000_000_000);
}

#[tokio::test]
async fn corporation_orders_do_not_count() {
    let stores = common::stores();
    let at = now() - Duration::minutes(10);
    let account = account(&stores, &[ALICE]).await;
    wallet(&stores, ALICE, at, &m(100)).await;
    // 30m personal and 40m corporation sell orders; 20m personal and 15m corporation escrow.
    orders(&stores, ALICE, at, (&m(70), &m(40)), (&m(35), &m(15))).await;
    inventory(&stores, ALICE, at, &m(50)).await;

    let now = current(&stores, account, &[ALICE]).await;

    assert_eq!(
        (now.sell_cents, now.escrow_cents),
        (3_000_000_000, 2_000_000_000)
    );
    assert_eq!(now.total_cents(), 20_000_000_000);
}

#[tokio::test]
async fn assets_that_were_never_fetched_count_as_zero() {
    let stores = common::stores();
    let at = now() - Duration::minutes(10);
    let account = account(&stores, &[ALICE]).await;
    wallet(&stores, ALICE, at, &m(100)).await;
    orders(&stores, ALICE, at, (&m(30), "0"), (&m(20), "0")).await;

    let now = current(&stores, account, &[ALICE]).await;

    assert_eq!(now.inventory_cents, 0);
    assert_eq!(now.total_cents(), 15_000_000_000);
}

#[tokio::test]
async fn the_latest_figure_of_each_series_counts_whenever_it_was_recorded() {
    let stores = common::stores();
    let t = now();
    let account = account(&stores, &[ALICE]).await;
    wallet(&stores, ALICE, t - Duration::hours(3), &m(1)).await;
    wallet(&stores, ALICE, t - Duration::minutes(10), &m(10)).await;
    inventory(&stores, ALICE, t - Duration::minutes(50), &m(50)).await;

    let now = current(&stores, account, &[ALICE]).await;

    assert_eq!(
        (now.wallet_cents, now.inventory_cents),
        (1_000_000_000, 5_000_000_000)
    );
    // Every later history point includes both the wallet and the inventory figure.
    let points = wealth_store(&stores)
        .history(account, all(&[ALICE]).ids(), Range::Day, t)
        .await
        .unwrap();
    for point in points.iter().filter(|p| p.at >= t - Duration::minutes(10)) {
        assert_eq!(point.wallet_cents, 1_000_000_000, "{}", point.at);
        assert_eq!(point.inventory_cents, 5_000_000_000, "{}", point.at);
    }
    let before_wallet = points
        .iter()
        .rev()
        .find(|p| p.at < t - Duration::minutes(10) && p.at >= t - Duration::minutes(50))
        .unwrap();
    assert_eq!(
        before_wallet.wallet_cents, 100_000_000,
        "the older wallet figure"
    );
    assert_eq!(before_wallet.inventory_cents, 5_000_000_000);
}

#[tokio::test]
async fn the_last_point_equals_the_current_figures() {
    let stores = common::stores();
    let t = now();
    let account = alice_with_everything(&stores, t - Duration::minutes(10)).await;

    let points = wealth_store(&stores)
        .history(account, all(&[ALICE]).ids(), Range::Week, t)
        .await
        .unwrap();
    let last = points.last().unwrap();
    let current = current(&stores, account, &[ALICE]).await;

    assert_eq!(last.at, t);
    assert_eq!(
        (
            last.wallet_cents,
            last.sell_cents,
            last.escrow_cents,
            last.inventory_cents
        ),
        (
            current.wallet_cents,
            current.sell_cents,
            current.escrow_cents,
            current.inventory_cents
        )
    );
    assert_eq!(last.total_cents(), current.total_cents());
}

#[tokio::test]
async fn before_any_recording_the_points_are_empty_and_say_so() {
    let stores = common::stores();
    let t = now();
    let account = account(&stores, &[ALICE]).await;
    wallet(&stores, ALICE, t + Duration::hours(1), &m(5)).await; // in the future: not history

    let points = wealth_store(&stores)
        .history(account, all(&[ALICE]).ids(), Range::Day, t)
        .await
        .unwrap();

    assert!(!wealth::has_history(&points));
    assert!(
        points
            .iter()
            .all(|p| p.total_cents() == 0 && p.contributors == 0)
    );
    assert_eq!(
        current(&stores, account, &[ALICE]).await.total_cents(),
        500_000_000
    );
}

#[tokio::test]
async fn the_history_starts_with_the_first_recording() {
    let stores = common::stores();
    let t = now();
    let account = account(&stores, &[ALICE]).await;
    wallet(&stores, ALICE, t - Duration::hours(2), &m(10)).await;
    wallet(&stores, ALICE, t - Duration::hours(1), &m(15)).await;

    let points = wealth_store(&stores)
        .history(account, all(&[ALICE]).ids(), Range::Day, t)
        .await
        .unwrap();

    let at = |ago: Duration| points.iter().find(|p| p.at >= t - ago).unwrap();
    assert_eq!(
        points
            .iter()
            .rev()
            .find(|p| p.at < t - Duration::hours(2))
            .unwrap()
            .total_cents(),
        0
    );
    assert_eq!(at(Duration::minutes(110)).wallet_cents, 1_000_000_000);
    assert_eq!(points.last().unwrap().wallet_cents, 1_500_000_000);
    assert!(wealth::has_history(&points));
}

#[tokio::test]
async fn a_character_that_stopped_being_polled_keeps_contributing() {
    let stores = common::stores();
    let t = now();
    let account = account(&stores, &[ALICE, BOB]).await;
    wallet(&stores, ALICE, t - Duration::days(3), &m(10)).await;
    wallet(&stores, BOB, t - Duration::minutes(5), &m(20)).await;

    let last = current(&stores, account, &[ALICE, BOB]).await;
    assert_eq!(
        last.wallet_cents, 3_000_000_000,
        "Alice's last known balance still counts"
    );
    let points = wealth_store(&stores)
        .history(account, all(&[ALICE, BOB]).ids(), Range::Day, t)
        .await
        .unwrap();
    assert_eq!(points[0].wallet_cents, 1_000_000_000);
    assert_eq!(points.last().unwrap().wallet_cents, 3_000_000_000);
}

#[tokio::test]
async fn only_the_selected_characters_of_the_account_count() {
    let stores = common::stores();
    let t = now() - Duration::minutes(10);
    let mine = account(&stores, &[ALICE, BOB]).await;
    let theirs = account(&stores, &[CAROL]).await;
    wallet(&stores, ALICE, t, &m(10)).await;
    wallet(&stores, BOB, t, &m(20)).await;
    wallet(&stores, CAROL, t, &m(999)).await;
    inventory(&stores, CAROL, t, &m(999)).await;

    // Narrowed to Alice.
    let narrowed = CharacterSelection::resolve(&[ALICE, BOB], Some(&ALICE.to_string()));
    assert_eq!(
        wealth_store(&stores)
            .current(mine, narrowed.ids())
            .await
            .unwrap()
            .total_cents(),
        1_000_000_000
    );
    // Carol's ID in the selection does not leak her data to another account.
    let sneaky = CharacterSelection::resolve(&[ALICE, BOB, CAROL], None);
    assert_eq!(
        wealth_store(&stores)
            .current(mine, sneaky.ids())
            .await
            .unwrap()
            .total_cents(),
        3_000_000_000
    );
    let points = wealth_store(&stores)
        .history(mine, sneaky.ids(), Range::Day, now())
        .await
        .unwrap();
    assert_eq!(points.last().unwrap().total_cents(), 3_000_000_000);
    assert_eq!(
        wealth_store(&stores)
            .current(theirs, all(&[CAROL]).ids())
            .await
            .unwrap()
            .total_cents(),
        199_800_000_000
    );
}

// ---- the inventory list ----

use sekondo::{
    orders::Dir,
    wealth::{Inventory, InventorySort, InventorySortKey},
};

const STATION: i64 = 60003760;
const OTHER_STATION: i64 = 60008494;

async fn name(stores: &Stores, id: i64, name: &str) {
    stores
        .market
        .store_names(
            &[NameRow {
                id,
                category: "x".to_owned(),
                name: Some(name.to_owned()),
            }],
            OffsetDateTime::now_utc(),
            None,
        )
        .await
        .unwrap();
}

async fn price(stores: &Stores, type_id: i64, price: Option<&str>) {
    stores
        .market
        .seed_price(type_id, price.map(cents))
        .await
        .unwrap();
}

struct Held {
    item_id: i64,
    character: i64,
    type_id: i64,
    quantity: i64,
    top: Option<i64>,
    copy: bool,
}

fn held(item_id: i64, character: i64, type_id: i64, quantity: i64, top: i64) -> Held {
    Held {
        item_id,
        character,
        type_id,
        quantity,
        top: Some(top),
        copy: false,
    }
}

async fn holds(stores: &Stores, h: Held) {
    stores
        .wealth
        .seed_asset(
            h.character,
            &EsiAsset {
                item_id: h.item_id,
                type_id: h.type_id,
                location_id: h.top.unwrap_or(99),
                location_type: "station".to_owned(),
                location_flag: "Hangar".to_owned(),
                quantity: h.quantity,
                is_singleton: false,
                is_blueprint_copy: h.copy,
            },
            h.top,
        )
        .await
        .unwrap();
}

async fn inventory_of(
    stores: &Stores,
    account: Uuid,
    characters: &[i64],
    sort: InventorySort,
) -> Inventory {
    wealth::inventory(&*wealth_store(stores), account, &all(characters), sort)
        .await
        .unwrap()
}

fn by(key: InventorySortKey, dir: Dir) -> InventorySort {
    InventorySort { key, dir }
}

fn items(inventory: &Inventory) -> Vec<(i64, i64, i64)> {
    inventory
        .rows
        .iter()
        .map(|r| (r.character_id, r.type_id, r.quantity))
        .collect()
}

#[tokio::test]
async fn the_same_item_in_two_containers_of_one_station_is_one_row() {
    let stores = common::stores();
    let account = account(&stores, &[ALICE]).await;
    price(&stores, 34, Some("5.00")).await;
    name(&stores, 34, "Tritanium").await;
    name(&stores, STATION, "Jita IV").await;
    holds(&stores, held(1, ALICE, 34, 100, STATION)).await; // in a container
    holds(&stores, held(2, ALICE, 34, 50, STATION)).await; // loose in the hangar

    let list = inventory_of(&stores, account, &[ALICE], InventorySort::default()).await;

    assert_eq!(items(&list), [(ALICE, 34, 150)]);
    let row = &list.rows[0];
    assert_eq!(row.type_name.as_deref(), Some("Tritanium"));
    assert_eq!(row.location_name.as_deref(), Some("Jita IV"));
    assert_eq!(row.unit_price_cents, Some(500));
    assert_eq!(row.value_cents, Some(75_000));
}

#[tokio::test]
async fn two_characters_with_the_same_item_at_the_same_station_have_a_row_each() {
    let stores = common::stores();
    let account = account(&stores, &[ALICE, BOB]).await;
    price(&stores, 34, Some("5.00")).await;
    holds(&stores, held(1, ALICE, 34, 10, STATION)).await;
    holds(&stores, held(2, BOB, 34, 20, STATION)).await;

    let list = inventory_of(&stores, account, &[ALICE, BOB], InventorySort::default()).await;

    assert_eq!(
        items(&list),
        [(BOB, 34, 20), (ALICE, 34, 10)],
        "by value, highest first"
    );
}

#[tokio::test]
async fn the_same_item_at_two_stations_has_a_row_each() {
    let stores = common::stores();
    let account = account(&stores, &[ALICE]).await;
    price(&stores, 34, Some("5.00")).await;
    holds(&stores, held(1, ALICE, 34, 10, STATION)).await;
    holds(&stores, held(2, ALICE, 34, 5, OTHER_STATION)).await;

    let list = inventory_of(&stores, account, &[ALICE], InventorySort::default()).await;

    assert_eq!(list.rows.len(), 2);
}

#[tokio::test]
async fn by_default_the_most_valuable_row_comes_first_and_unpriced_rows_last() {
    let stores = common::stores();
    let account = account(&stores, &[ALICE]).await;
    price(&stores, 34, Some("5.00")).await;
    price(&stores, 35, Some("100.00")).await;
    price(&stores, 36, None).await;
    holds(&stores, held(1, ALICE, 34, 10, STATION)).await; // 50
    holds(&stores, held(2, ALICE, 35, 10, STATION)).await; // 1000
    holds(&stores, held(3, ALICE, 36, 99, STATION)).await; // no price
    holds(
        &stores,
        Held {
            copy: true,
            ..held(4, ALICE, 35, 1, STATION)
        },
    )
    .await; // a copy

    let list = inventory_of(&stores, account, &[ALICE], InventorySort::default()).await;

    let order: Vec<(i64, Option<i64>)> = list
        .rows
        .iter()
        .map(|r| (r.type_id, r.value_cents))
        .collect();
    assert_eq!(order[0], (35, Some(100_000)));
    assert_eq!(order[1], (34, Some(5_000)));
    assert!(order[2..].iter().all(|(_, value)| value.is_none()));
    assert_eq!(
        list.unpriced_rows, 2,
        "the unpriced item and the blueprint copy"
    );
    assert_eq!(list.total_cents, 105_000);
}

#[tokio::test]
async fn a_blueprint_copy_has_a_row_of_its_own_without_price_or_value() {
    let stores = common::stores();
    let account = account(&stores, &[ALICE]).await;
    price(&stores, 35, Some("100.00")).await;
    holds(&stores, held(1, ALICE, 35, 1, STATION)).await;
    holds(
        &stores,
        Held {
            copy: true,
            ..held(2, ALICE, 35, 3, STATION)
        },
    )
    .await;

    let list = inventory_of(&stores, account, &[ALICE], InventorySort::default()).await;

    assert_eq!(list.rows.len(), 2);
    let copy = list.rows.iter().find(|r| r.quantity == 3).unwrap();
    assert_eq!((copy.unit_price_cents, copy.value_cents), (None, None));
    assert_eq!(list.total_cents, 10_000);
}

#[tokio::test]
async fn sorting_by_item_name_follows_the_name_and_the_direction() {
    let stores = common::stores();
    let account = account(&stores, &[ALICE]).await;
    for (id, label) in [(34, "Tritanium"), (35, "Pyerite"), (36, "Mexallon")] {
        name(&stores, id, label).await;
        price(&stores, id, Some("1.00")).await;
        holds(&stores, held(id, ALICE, id, 1, STATION)).await;
    }
    // No name: sorts by its placeholder.
    price(&stores, 37, Some("1.00")).await;
    holds(&stores, held(37, ALICE, 37, 1, STATION)).await;

    let asc = inventory_of(
        &stores,
        account,
        &[ALICE],
        by(InventorySortKey::Item, Dir::Asc),
    )
    .await;
    let desc = inventory_of(
        &stores,
        account,
        &[ALICE],
        by(InventorySortKey::Item, Dir::Desc),
    )
    .await;

    let types = |l: &Inventory| l.rows.iter().map(|r| r.type_id).collect::<Vec<_>>();
    assert_eq!(
        types(&asc),
        [37, 36, 35, 34],
        "#37 sorts before the letters"
    );
    assert_eq!(types(&desc), [34, 35, 36, 37]);
}

#[tokio::test]
async fn unpriced_rows_stay_last_in_both_directions_of_price_and_value() {
    let stores = common::stores();
    let account = account(&stores, &[ALICE]).await;
    price(&stores, 34, Some("5.00")).await;
    price(&stores, 35, Some("100.00")).await;
    price(&stores, 36, None).await;
    holds(&stores, held(1, ALICE, 34, 10, STATION)).await;
    holds(&stores, held(2, ALICE, 35, 10, STATION)).await;
    holds(&stores, held(3, ALICE, 36, 10, STATION)).await;

    for key in [InventorySortKey::Price, InventorySortKey::Value] {
        for dir in [Dir::Asc, Dir::Desc] {
            let list = inventory_of(&stores, account, &[ALICE], by(key, dir)).await;
            let types: Vec<i64> = list.rows.iter().map(|r| r.type_id).collect();
            let priced = if dir == Dir::Asc { [34, 35] } else { [35, 34] };
            assert_eq!(types, [priced[0], priced[1], 36], "{key:?} {dir:?}");
        }
    }
}

#[tokio::test]
async fn every_sort_key_is_usable_and_parsed_by_name() {
    let stores = common::stores();
    let account = account(&stores, &[ALICE, BOB]).await;
    price(&stores, 34, Some("5.00")).await;
    holds(&stores, held(1, ALICE, 34, 10, STATION)).await;
    holds(&stores, held(2, BOB, 34, 10, OTHER_STATION)).await;

    for name in [
        "character",
        "item",
        "quantity",
        "price",
        "value",
        "location",
    ] {
        let sort = InventorySort::parse(Some(name), Some("desc"));
        assert_eq!(sort.key.name(), name);
        let list = inventory_of(&stores, account, &[ALICE, BOB], sort).await;
        assert_eq!(list.rows.len(), 2, "{name}");
    }
    assert_eq!(
        InventorySort::parse(Some("; DROP"), None),
        InventorySort::default()
    );
    assert_eq!(InventorySort::default().key, InventorySortKey::Value);
    assert_eq!(InventorySort::default().dir, Dir::Desc);
}

#[tokio::test]
async fn another_accounts_assets_and_unselected_characters_are_not_listed() {
    let stores = common::stores();
    let mine = account(&stores, &[ALICE, BOB]).await;
    let _theirs = account(&stores, &[CAROL]).await;
    price(&stores, 34, Some("5.00")).await;
    holds(&stores, held(1, ALICE, 34, 10, STATION)).await;
    holds(&stores, held(2, BOB, 34, 20, STATION)).await;
    holds(&stores, held(3, CAROL, 34, 999, STATION)).await;

    let everyone = CharacterSelection::resolve(&[ALICE, BOB, CAROL], None);
    let list = wealth::inventory(
        &*wealth_store(&stores),
        mine,
        &everyone,
        InventorySort::default(),
    )
    .await
    .unwrap();
    assert_eq!(items(&list), [(BOB, 34, 20), (ALICE, 34, 10)]);

    let only_alice = CharacterSelection::resolve(&[ALICE, BOB], Some(&ALICE.to_string()));
    let list = wealth::inventory(
        &*wealth_store(&stores),
        mine,
        &only_alice,
        InventorySort::default(),
    )
    .await
    .unwrap();
    assert_eq!(items(&list), [(ALICE, 34, 10)]);
}

#[tokio::test]
async fn locations_are_named_unresolved_or_unknown() {
    let stores = common::stores();
    let account = account(&stores, &[ALICE]).await;
    price(&stores, 34, Some("1.00")).await;
    // A structure the cache knows but ESI would not describe, and an unresolvable location.
    let structure = 1_000_000_000_002_i64;
    stores
        .market
        .store_names(
            &[NameRow {
                id: structure,
                category: "structure".to_owned(),
                name: None,
            }],
            OffsetDateTime::now_utc(),
            Some(OffsetDateTime::now_utc() + Duration::hours(24)),
        )
        .await
        .unwrap();
    holds(&stores, held(1, ALICE, 34, 1, structure)).await;
    holds(&stores, held(2, ALICE, 34, 2, STATION)).await; // no name cached
    holds(
        &stores,
        Held {
            top: None,
            ..held(3, ALICE, 34, 4, 0)
        },
    )
    .await;

    let list = inventory_of(
        &stores,
        account,
        &[ALICE],
        by(InventorySortKey::Quantity, Dir::Asc),
    )
    .await;

    let rows: Vec<_> = list
        .rows
        .iter()
        .map(|r| {
            (
                r.quantity,
                r.location_id,
                r.location_name.clone(),
                r.location_known,
                r.top_known,
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (1, structure, None, true, true),
            (2, STATION, None, false, true),
            (4, 99, None, false, false),
        ]
    );
}
