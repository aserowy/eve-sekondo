mod common;

use common::PollEnv;
use sekondo::{clock::Clock, poller::PollOutcome, wealth_poller::Kind};
use serde_json::json;
use time::{Duration, OffsetDateTime};

const ALICE: i64 = 90000001;

async fn linked() -> PollEnv {
    let env = PollEnv::new().await;
    let account = env.new_account().await;
    env.link(account, ALICE).await;
    env
}

async fn balances(env: &PollEnv, character: i64) -> Vec<i64> {
    let rows = env.stores.wealth.wallet_balances(character).await.unwrap();
    rows.into_iter().map(|(_, cents)| cents).collect()
}

fn kind_named(kind: &str) -> Kind {
    match kind {
        "wallet" => Kind::Wallet,
        "assets" => Kind::Assets,
        _ => Kind::Transactions,
    }
}

async fn sync_row(
    env: &PollEnv,
    character: i64,
    kind: &str,
) -> (Option<OffsetDateTime>, OffsetDateTime, Option<String>, i32) {
    let state = env
        .stores
        .wealth
        .wealth_sync(character, kind_named(kind))
        .await
        .unwrap()
        .expect("a sync row");
    (
        state.last_success_at,
        state.next_poll_at,
        state.last_error,
        state.failures,
    )
}

#[tokio::test]
async fn a_wallet_poll_records_the_balance_and_when_it_happened() {
    let env = linked().await;
    env.control("wallet", json!({"character_id": ALICE, "balance": 1234.56}))
        .await;
    let polled_at = env.clock.now();

    let outcome = env.wealth.poll(ALICE, Kind::Wallet).await;

    assert_eq!(outcome, PollOutcome::Updated);
    assert_eq!(balances(&env, ALICE).await, [123_456]);
    let (success, next, error, failures) = sync_row(&env, ALICE, "wallet").await;
    assert_eq!(success, Some(polled_at));
    assert!(next >= polled_at + env.interval, "{next} vs {polled_at}");
    assert_eq!((error, failures), (None, 0));
}

#[tokio::test]
async fn every_poll_adds_a_row_so_the_balance_has_a_history() {
    let env = linked().await;
    env.wealth.poll(ALICE, Kind::Wallet).await;
    env.control("wallet", json!({"character_id": ALICE, "balance": 10.0}))
        .await;
    env.clock.advance(Duration::minutes(21));
    env.wealth.poll(ALICE, Kind::Wallet).await;

    assert_eq!(balances(&env, ALICE).await, [250_000_000, 1000]);
}

#[tokio::test]
async fn the_next_poll_waits_for_esis_expiry_when_that_is_later() {
    let env = linked().await;
    env.control("esi-expires", json!({"seconds": 3 * 3600}))
        .await;
    let polled_at = env.clock.now();

    env.wealth.poll(ALICE, Kind::Wallet).await;

    let (_, next, ..) = sync_row(&env, ALICE, "wallet").await;
    assert!(next >= polled_at + Duration::hours(3), "{next}");
}

#[tokio::test]
async fn a_character_needing_relogin_is_not_requested_and_keeps_its_balance() {
    let env = linked().await;
    env.wealth.poll(ALICE, Kind::Wallet).await;
    env.repo.mark_needs_relogin(ALICE).await.unwrap();
    let calls = env.stub.esi_calls("wallet");
    let tokens = env.stub.token_calls();

    let outcome = env.wealth.poll(ALICE, Kind::Wallet).await;

    assert_eq!(outcome, PollOutcome::NeedsRelogin);
    assert_eq!(env.stub.esi_calls("wallet"), calls);
    assert_eq!(env.stub.token_calls(), tokens);
    assert_eq!(balances(&env, ALICE).await.len(), 1);
}

#[tokio::test]
async fn the_scheduler_skips_characters_needing_relogin() {
    let env = linked().await;
    env.repo.mark_needs_relogin(ALICE).await.unwrap();
    env.wealth.tick().await; // schedules nothing for it
    env.clock.advance(Duration::hours(2));

    assert_eq!(env.wealth.tick().await, 0);
    assert_eq!(env.stub.esi_calls("wallet"), 0);
}

#[tokio::test]
async fn the_scheduler_polls_a_new_character_within_one_interval() {
    let env = linked().await;
    assert_eq!(env.wealth.tick().await, 0, "first polls lie ahead");
    let scheduled = env
        .stores
        .wealth
        .wealth_sync(ALICE, Kind::Wallet)
        .await
        .unwrap()
        .into_iter()
        .count();
    assert_eq!(scheduled, 1);

    env.clock.advance(env.interval + Duration::minutes(1));
    assert_eq!(env.wealth.tick().await, 1);
    assert_eq!(balances(&env, ALICE).await.len(), 1);
    assert_eq!(env.wealth.tick().await, 0, "not due again right away");
}

#[tokio::test]
async fn a_failed_poll_keeps_the_balance_and_records_the_failure() {
    let env = linked().await;
    env.wealth.poll(ALICE, Kind::Wallet).await;
    let (success_before, ..) = sync_row(&env, ALICE, "wallet").await;
    env.control("wallet", json!({"character_id": ALICE, "balance": 1.0}))
        .await;
    env.clock.advance(Duration::minutes(21));
    env.control("esi-fail-next", json!({"route": "wallet", "status": 503}))
        .await;
    let failed_at = env.clock.now();

    let outcome = env.wealth.poll(ALICE, Kind::Wallet).await;

    assert_eq!(outcome, PollOutcome::Failed);
    assert_eq!(balances(&env, ALICE).await, [250_000_000]);
    let (success, next, error, failures) = sync_row(&env, ALICE, "wallet").await;
    assert_eq!(success, success_before);
    assert_eq!(failures, 1);
    assert!(error.unwrap().contains("503"));
    assert_eq!(next, failed_at + Duration::minutes(1));
}

#[tokio::test]
async fn an_esi_pause_defers_the_poll_without_counting_a_failure() {
    let env = linked().await;
    env.wealth.poll(ALICE, Kind::Wallet).await;
    env.control(
        "esi-fail-next",
        json!({"route": "wallet", "status": 429,
               "headers": {"retry-after": "120", "x-ratelimit-group": "char-wallet"}}),
    )
    .await;
    env.clock.advance(Duration::minutes(21));
    let at = env.clock.now();

    assert_eq!(
        env.wealth.poll(ALICE, Kind::Wallet).await,
        PollOutcome::Deferred
    );

    let (_, next, _, failures) = sync_row(&env, ALICE, "wallet").await;
    assert_eq!(failures, 0);
    assert!(next >= at + Duration::seconds(120));
}

#[tokio::test]
async fn a_wallet_failure_does_not_touch_the_order_poll_and_the_other_way_round() {
    let env = linked().await;
    env.poller.poll_character(ALICE).await;
    env.wealth.poll(ALICE, Kind::Wallet).await;

    env.control("esi-fail-next", json!({"route": "wallet", "status": 500}))
        .await;
    env.clock.advance(Duration::minutes(21));
    assert_eq!(
        env.wealth.poll(ALICE, Kind::Wallet).await,
        PollOutcome::Failed
    );
    assert_eq!(env.poller.poll_character(ALICE).await, PollOutcome::Updated);
    let order_failures = env
        .stores
        .orders
        .order_sync(ALICE)
        .await
        .unwrap()
        .unwrap()
        .failures;
    assert_eq!(order_failures, 0);

    env.control("esi-fail-next", json!({"route": "orders", "status": 500}))
        .await;
    env.clock.advance(Duration::minutes(21));
    assert_eq!(env.poller.poll_character(ALICE).await, PollOutcome::Failed);
    assert_eq!(
        env.wealth.poll(ALICE, Kind::Wallet).await,
        PollOutcome::Updated
    );
    let (_, _, _, wallet_failures) = sync_row(&env, ALICE, "wallet").await;
    assert_eq!(wallet_failures, 0);
}

#[tokio::test]
async fn a_character_unlinked_during_its_poll_is_dropped() {
    let env = PollEnv::new().await;
    // Never linked: the token service does not know it.
    assert_eq!(
        env.wealth.poll(ALICE, Kind::Wallet).await,
        PollOutcome::Dropped
    );
}

// ---- assets ----

const BOB: i64 = 90000002;

fn asset(id: i64, type_id: i64, quantity: i64) -> serde_json::Value {
    json!({
        "item_id": id, "type_id": type_id, "location_id": 60003760, "location_type": "station",
        "location_flag": "Hangar", "quantity": quantity, "is_singleton": false,
    })
}

async fn set_assets(
    env: &PollEnv,
    character: i64,
    assets: Vec<serde_json::Value>,
    page_size: usize,
) {
    let res = env
        .control(
            "assets",
            json!({"character_id": character, "assets": assets, "page_size": page_size}),
        )
        .await;
    assert_eq!(res.status(), 204);
}

async fn asset_ids(env: &PollEnv, character: i64) -> Vec<i64> {
    env.stores.wealth.asset_ids(character).await.unwrap()
}

async fn inventory_values(env: &PollEnv, character: i64) -> Vec<i64> {
    let rows = env.stores.wealth.inventory_values(character).await.unwrap();
    rows.into_iter().map(|(_, cents)| cents).collect()
}

async fn set_price(env: &PollEnv, type_id: i64, price: Option<&str>) {
    let cents = price.map(|p| (p.parse::<f64>().unwrap() * 100.0).round() as i64);
    env.stores.market.seed_price(type_id, cents).await.unwrap();
}

#[tokio::test]
async fn the_assets_of_all_pages_become_the_current_assets() {
    let env = linked().await;
    let items: Vec<_> = (1..=5).map(|id| asset(id, 34, 1)).collect();
    set_assets(&env, ALICE, items, 2).await;

    assert_eq!(
        env.wealth.poll(ALICE, Kind::Assets).await,
        PollOutcome::Updated
    );

    assert_eq!(asset_ids(&env, ALICE).await, [1, 2, 3, 4, 5]);
    assert_eq!(env.stub.esi_calls("assets"), 3);
}

#[tokio::test]
async fn an_item_that_is_gone_stops_being_an_asset() {
    let env = linked().await;
    set_assets(&env, ALICE, vec![asset(1, 34, 1), asset(2, 34, 1)], 1000).await;
    env.wealth.poll(ALICE, Kind::Assets).await;

    set_assets(&env, ALICE, vec![asset(2, 34, 1)], 1000).await;
    env.clock.advance(Duration::hours(2));
    env.wealth.poll(ALICE, Kind::Assets).await;

    assert_eq!(asset_ids(&env, ALICE).await, [2]);
}

#[tokio::test]
async fn a_failure_on_the_second_page_keeps_the_assets_and_retries_later() {
    let env = linked().await;
    set_assets(&env, ALICE, vec![asset(1, 34, 1), asset(2, 34, 1)], 1).await;
    env.wealth.poll(ALICE, Kind::Assets).await;
    // The second page cannot be read: its only item lacks the item id.
    let mut broken = asset(4, 34, 1);
    broken.as_object_mut().unwrap().remove("item_id");
    set_assets(&env, ALICE, vec![asset(3, 34, 1), broken], 1).await;
    env.clock.advance(Duration::hours(2));
    let failed_at = env.clock.now();

    let outcome = env.wealth.poll(ALICE, Kind::Assets).await;

    assert_eq!(outcome, PollOutcome::Failed);
    assert_eq!(asset_ids(&env, ALICE).await, [1, 2]);
    assert_eq!(inventory_values(&env, ALICE).await.len(), 1, "no entry");
    let (_, next, error, failures) = sync_row(&env, ALICE, "assets").await;
    assert!(error.is_some());
    assert_eq!(failures, 1);
    assert_eq!(next, failed_at + Duration::minutes(1));
}

#[tokio::test]
async fn assets_are_not_polled_again_within_the_hour_whatever_the_order_interval() {
    let env = linked().await;
    let polled_at = env.clock.now();

    env.wealth.poll(ALICE, Kind::Assets).await;

    let (success, next, ..) = sync_row(&env, ALICE, "assets").await;
    assert_eq!(success, Some(polled_at));
    assert!(next >= polled_at + Duration::hours(1), "{next}");
}

#[tokio::test]
async fn the_scheduler_polls_wallet_assets_and_transactions_independently() {
    let env = linked().await;
    env.wealth.tick().await;
    let mut kinds = Vec::new();
    for (name, kind) in [
        ("assets", Kind::Assets),
        ("transactions", Kind::Transactions),
        ("wallet", Kind::Wallet),
    ] {
        if env
            .stores
            .wealth
            .wealth_sync(ALICE, kind)
            .await
            .unwrap()
            .is_some()
        {
            kinds.push(name);
        }
    }
    assert_eq!(kinds, ["assets", "transactions", "wallet"]);

    // After 70 minutes all are due: the wallet (20 minute interval), the assets and the
    // transactions (an hour).
    env.clock.advance(Duration::minutes(70));
    assert_eq!(env.wealth.tick().await, 3);
    assert_eq!(env.stub.esi_calls("wallet"), 1);
    assert_eq!(env.stub.esi_calls("assets"), 1);
    assert!(env.stub.esi_calls("transactions") >= 1);

    // A failing wallet leaves the assets alone.
    env.control("esi-fail-next", json!({"route": "wallet", "status": 500}))
        .await;
    env.wealth.poll(ALICE, Kind::Wallet).await;
    let (_, _, _, assets_failures) = sync_row(&env, ALICE, "assets").await;
    assert_eq!(assets_failures, 0);
}

#[tokio::test]
async fn every_assets_poll_records_the_inventory_value_from_the_stored_prices() {
    let env = linked().await;
    // Alice holds 1000 of type 34 and 50 of type 35. Add a blueprint copy and an unpriced item.
    let mut copy = asset(3, 34, 1);
    copy["is_blueprint_copy"] = json!(true);
    set_assets(
        &env,
        ALICE,
        vec![asset(1, 34, 1000), asset(2, 35, 50), copy, asset(4, 36, 9)],
        1000,
    )
    .await;
    set_price(&env, 34, Some("5.50")).await;
    set_price(&env, 35, Some("10.25")).await;
    set_price(&env, 36, None).await;

    env.wealth.poll(ALICE, Kind::Assets).await;

    // 1000 * 5.50 + 50 * 10.25 = 6012.50; the copy and the unpriced item add nothing.
    assert_eq!(inventory_values(&env, ALICE).await, [601_250]);
}

#[tokio::test]
async fn without_any_price_the_inventory_is_worth_nothing_but_still_recorded() {
    let env = linked().await;
    env.wealth.poll(ALICE, Kind::Assets).await;
    assert_eq!(inventory_values(&env, ALICE).await, [0]);
}

#[tokio::test]
async fn an_item_that_changed_hands_moves_to_its_new_owner() {
    let env = linked().await;
    let account = env.new_account().await;
    env.link(account, BOB).await;
    set_assets(&env, ALICE, vec![asset(1, 34, 1)], 1000).await;
    set_assets(&env, BOB, vec![], 1000).await;
    env.wealth.poll(ALICE, Kind::Assets).await;

    // Bob is polled before Alice learns that the item is gone.
    set_assets(&env, BOB, vec![asset(1, 34, 1)], 1000).await;
    assert_eq!(
        env.wealth.poll(BOB, Kind::Assets).await,
        PollOutcome::Updated
    );

    assert_eq!(asset_ids(&env, BOB).await, [1]);
    assert!(asset_ids(&env, ALICE).await.is_empty());
}

// ---- names ----

async fn name_of(env: &PollEnv, id: i64) -> Option<Option<String>> {
    let rows = env.stores.market.names_of(&[id]).await.unwrap();
    rows.into_iter().next().map(|(_, name)| name)
}

fn asset_at(id: i64, type_id: i64, location: i64, location_type: &str) -> serde_json::Value {
    json!({
        "item_id": id, "type_id": type_id, "location_id": location, "location_type": location_type,
        "location_flag": "Hangar", "quantity": 1, "is_singleton": false,
    })
}

#[tokio::test]
async fn the_names_of_asset_types_stations_and_systems_are_resolved() {
    let env = linked().await;
    // The stub knows no solar system; the lookup is the same for any public ID, so a known
    // region ID stands in for the solar system.
    set_assets(
        &env,
        ALICE,
        vec![
            asset_at(1, 34, 60003760, "station"),
            asset_at(2, 35, 10000043, "solar_system"),
        ],
        1000,
    )
    .await;

    env.wealth.poll(ALICE, Kind::Assets).await;

    assert_eq!(name_of(&env, 34).await, Some(Some("Tritanium".to_owned())));
    assert_eq!(name_of(&env, 35).await, Some(Some("Pyerite".to_owned())));
    assert!(
        name_of(&env, 60003760)
            .await
            .flatten()
            .unwrap()
            .starts_with("Jita IV")
    );
    assert_eq!(
        name_of(&env, 10000043).await,
        Some(Some("Domain".to_owned()))
    );
}

#[tokio::test]
async fn a_structure_is_named_with_the_owners_token_and_a_refused_one_stays_unnamed() {
    let env = linked().await;
    env.control(
        "structure-access",
        json!({"structure_id": 1000000000002_i64, "allowed": false}),
    )
    .await;
    set_assets(
        &env,
        ALICE,
        vec![
            asset_at(1, 34, 1000000000001, "item"),
            asset_at(2, 34, 1000000000002, "other"),
        ],
        1000,
    )
    .await;

    env.wealth.poll(ALICE, Kind::Assets).await;

    assert_eq!(
        name_of(&env, 1000000000001).await,
        Some(Some("Alpha Keepstar".to_owned()))
    );
    assert_eq!(
        name_of(&env, 1000000000002).await,
        Some(None),
        "retried daily"
    );
    let retry = env
        .stores
        .market
        .names_table()
        .await
        .unwrap()
        .into_iter()
        .find(|name| name.id == 1000000000002)
        .unwrap()
        .retry_after;
    assert!(retry.unwrap() > env.clock.now() + Duration::hours(23));
}

#[tokio::test]
async fn a_name_that_cannot_be_resolved_never_fails_the_assets_poll() {
    let env = linked().await;
    env.control("esi-fail-next", json!({"route": "names", "status": 503}))
        .await;

    assert_eq!(
        env.wealth.poll(ALICE, Kind::Assets).await,
        PollOutcome::Updated
    );

    assert_eq!(asset_ids(&env, ALICE).await, [2001, 2002]);
    assert_eq!(
        name_of(&env, 34).await,
        None,
        "asked again on the next poll"
    );
}

// ---- transactions ----

fn transaction(id: i64, buy: bool, personal: bool) -> serde_json::Value {
    json!({
        "transaction_id": id, "date": "2026-10-01T12:00:00Z", "type_id": 34,
        "location_id": 60003760, "quantity": 10, "unit_price": 100.25,
        "is_buy": buy, "is_personal": personal, "client_id": 9, "journal_ref_id": id,
    })
}

fn purchase(id: i64) -> serde_json::Value {
    transaction(id, true, true)
}

async fn set_transactions(
    env: &PollEnv,
    character: i64,
    transactions: Vec<serde_json::Value>,
    limit: Option<usize>,
) {
    let mut body = json!({"character_id": character, "transactions": transactions});
    if let Some(limit) = limit {
        body["limit"] = json!(limit);
    }
    let res = env.control("transactions", body).await;
    assert_eq!(res.status(), 204);
}

fn sale(id: i64) -> serde_json::Value {
    transaction(id, false, true)
}

/// Every purchase and sale stored of the character, as the trades page lists them.
async fn trades_of(env: &PollEnv, character: i64) -> Vec<sekondo::trades::TradeRow> {
    let account = env.account_of(character).await;
    let mut rows = env
        .stores
        .trades
        .list_trades(
            account,
            &[character],
            time::macros::datetime!(2000-01-01 0:00 UTC),
            time::macros::datetime!(2100-01-01 0:00 UTC),
        )
        .await
        .unwrap();
    rows.sort_by_key(|row| row.transaction_id);
    rows
}

async fn sale_ids(env: &PollEnv, character: i64) -> Vec<i64> {
    let rows = trades_of(env, character).await;
    rows.iter()
        .filter(|r| r.is_sale)
        .map(|r| r.transaction_id)
        .collect()
}

async fn purchase_ids(env: &PollEnv, character: i64) -> Vec<i64> {
    let rows = trades_of(env, character).await;
    rows.iter()
        .filter(|r| !r.is_sale)
        .map(|r| r.transaction_id)
        .collect()
}

/// A personal sell order of 10 units that was seen before the purchases it is costed from.
async fn remember_order_11(env: &PollEnv) {
    let issued = time::macros::datetime!(2026-10-02 12:00 UTC);
    let order = sekondo::esi::EsiOrder {
        order_id: 11,
        type_id: 34,
        region_id: 10000002,
        location_id: 60003760,
        is_buy_order: false,
        is_corporation: false,
        price_cents: 20_000,
        volume_remain: 10,
        volume_total: 10,
        min_volume: 1,
        range: "station".to_owned(),
        escrow_cents: None,
        issued,
        duration: 90,
    };
    env.stores
        .orders
        .replace_orders(ALICE, issued, &[order], issued + Duration::hours(1), None)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_first_poll_reads_back_over_several_requests_and_stores_every_purchase() {
    let env = linked().await;
    set_transactions(&env, ALICE, (1..=5).map(purchase).collect(), Some(2)).await;

    let outcome = env.wealth.poll(ALICE, Kind::Transactions).await;

    assert_eq!(outcome, PollOutcome::Updated);
    assert_eq!(purchase_ids(&env, ALICE).await, [1, 2, 3, 4, 5]);
    // [5, 4], [3, 2], [1], and the empty answer past the oldest.
    assert_eq!(env.stub.esi_calls("transactions"), 4);
    let stored = trades_of(&env, ALICE)
        .await
        .into_iter()
        .find(|row| row.transaction_id == 5)
        .map(|row| {
            (
                row.type_id,
                row.quantity,
                row.unit_price_cents,
                row.location_id,
            )
        })
        .unwrap();
    assert_eq!(stored, (34, 10, 10_025, 60003760));
}

#[tokio::test]
async fn a_later_poll_stops_at_a_transaction_it_already_stored() {
    let env = linked().await;
    set_transactions(&env, ALICE, (1..=5).map(purchase).collect(), Some(2)).await;
    env.wealth.poll(ALICE, Kind::Transactions).await;
    let calls = env.stub.esi_calls("transactions");

    // One new purchase: the newest answer [6, 5] already holds a stored one.
    set_transactions(&env, ALICE, (1..=6).map(purchase).collect(), Some(2)).await;
    env.clock.advance(Duration::hours(2));
    env.wealth.poll(ALICE, Kind::Transactions).await;

    assert_eq!(env.stub.esi_calls("transactions"), calls + 1);
    assert_eq!(purchase_ids(&env, ALICE).await, [1, 2, 3, 4, 5, 6]);
}

#[tokio::test]
async fn a_poll_reads_on_past_answers_without_a_stored_transaction() {
    let env = linked().await;
    set_transactions(&env, ALICE, vec![purchase(1)], Some(2)).await;
    env.wealth.poll(ALICE, Kind::Transactions).await;
    let calls = env.stub.esi_calls("transactions");

    // Three new ones: [4, 3] is all new, so it reads on to [2, 1], which holds a stored one.
    set_transactions(&env, ALICE, (1..=4).map(purchase).collect(), Some(2)).await;
    env.clock.advance(Duration::hours(2));
    env.wealth.poll(ALICE, Kind::Transactions).await;

    assert_eq!(env.stub.esi_calls("transactions"), calls + 2);
    assert_eq!(purchase_ids(&env, ALICE).await, [1, 2, 3, 4]);
}

#[tokio::test]
async fn a_poll_asks_for_at_most_fifty_answers() {
    let env = linked().await;
    // Corporation transactions are never stored, so nothing ever tells the poll to stop.
    let others = (1..=60)
        .map(|id| transaction(id, id % 2 == 0, false))
        .collect();
    set_transactions(&env, ALICE, others, Some(1)).await;

    let outcome = env.wealth.poll(ALICE, Kind::Transactions).await;

    assert_eq!(outcome, PollOutcome::Updated);
    assert_eq!(env.stub.esi_calls("transactions"), 50);
}

#[tokio::test]
async fn a_first_poll_stores_the_personal_purchases_and_sales() {
    let env = linked().await;
    set_transactions(&env, ALICE, vec![sale(1), purchase(2), sale(3)], None).await;

    env.wealth.poll(ALICE, Kind::Transactions).await;

    assert_eq!(purchase_ids(&env, ALICE).await, [2]);
    assert_eq!(sale_ids(&env, ALICE).await, [1, 3]);
    let stored = trades_of(&env, ALICE)
        .await
        .into_iter()
        .find(|row| row.transaction_id == 3)
        .map(|row| {
            (
                row.type_id,
                row.quantity,
                row.unit_price_cents,
                row.location_id,
                row.at,
            )
        })
        .unwrap();
    assert_eq!(
        stored,
        (
            34,
            10,
            10_025,
            60003760,
            time::macros::datetime!(2026-10-01 12:00 UTC)
        )
    );
}

#[tokio::test]
async fn corporation_transactions_are_not_stored() {
    let env = linked().await;
    set_transactions(
        &env,
        ALICE,
        vec![
            transaction(1, false, false),
            transaction(2, true, false),
            purchase(3),
            sale(4),
        ],
        None,
    )
    .await;

    env.wealth.poll(ALICE, Kind::Transactions).await;

    assert_eq!(purchase_ids(&env, ALICE).await, [3]);
    assert_eq!(sale_ids(&env, ALICE).await, [4]);
}

#[tokio::test]
async fn a_later_poll_stops_at_a_sale_it_already_stored() {
    let env = linked().await;
    set_transactions(&env, ALICE, (1..=5).map(sale).collect(), Some(2)).await;
    env.wealth.poll(ALICE, Kind::Transactions).await;
    let calls = env.stub.esi_calls("transactions");

    // One new sale: the newest answer [6, 5] already holds a stored one.
    set_transactions(&env, ALICE, (1..=6).map(sale).collect(), Some(2)).await;
    env.clock.advance(Duration::hours(2));
    env.wealth.poll(ALICE, Kind::Transactions).await;

    assert_eq!(env.stub.esi_calls("transactions"), calls + 1);
    assert_eq!(sale_ids(&env, ALICE).await, [1, 2, 3, 4, 5, 6]);
}

#[tokio::test]
async fn a_sale_is_not_a_purchase_lot() {
    let env = linked().await;
    remember_order_11(&env).await;
    set_transactions(&env, ALICE, vec![sale(1)], None).await;

    env.wealth.poll(ALICE, Kind::Transactions).await;

    assert_eq!(purchase_ids(&env, ALICE).await, Vec::<i64>::new());
    assert_eq!(sale_ids(&env, ALICE).await, [1]);
    let covered = env
        .orders_of(ALICE)
        .await
        .into_iter()
        .find(|row| row.order_id == 11)
        .and_then(|row| row.covered_units);
    assert_eq!(covered.unwrap_or(0), 0, "the sale gave the order no lot");
}

#[tokio::test]
async fn a_purchase_is_stored_once_however_often_it_is_returned() {
    let env = linked().await;
    set_transactions(&env, ALICE, vec![purchase(1)], None).await;
    env.wealth.poll(ALICE, Kind::Transactions).await;
    env.clock.advance(Duration::hours(2));

    env.wealth.poll(ALICE, Kind::Transactions).await;

    assert_eq!(purchase_ids(&env, ALICE).await, [1]);
}

#[tokio::test]
async fn purchases_outlive_the_window_of_esi() {
    let env = linked().await;
    set_transactions(&env, ALICE, vec![purchase(1)], None).await;
    env.wealth.poll(ALICE, Kind::Transactions).await;

    set_transactions(&env, ALICE, vec![], None).await;
    env.clock.advance(Duration::days(60));
    env.wealth.poll(ALICE, Kind::Transactions).await;

    assert_eq!(purchase_ids(&env, ALICE).await, [1]);
}

#[tokio::test]
async fn a_character_needing_relogin_is_skipped_and_keeps_its_purchases() {
    let env = linked().await;
    set_transactions(&env, ALICE, vec![purchase(1)], None).await;
    env.wealth.poll(ALICE, Kind::Transactions).await;
    env.repo.mark_needs_relogin(ALICE).await.unwrap();
    let calls = env.stub.esi_calls("transactions");

    let outcome = env.wealth.poll(ALICE, Kind::Transactions).await;

    assert_eq!(outcome, PollOutcome::NeedsRelogin);
    assert_eq!(env.stub.esi_calls("transactions"), calls);
    assert_eq!(purchase_ids(&env, ALICE).await, [1]);
}

#[tokio::test]
async fn re_login_skip_and_failure_keep_the_sales() {
    let env = linked().await;
    set_transactions(&env, ALICE, vec![sale(1)], None).await;
    env.wealth.poll(ALICE, Kind::Transactions).await;
    env.control(
        "esi-fail-next",
        json!({"route": "transactions", "status": 503}),
    )
    .await;
    env.clock.advance(Duration::hours(2));
    assert_eq!(
        env.wealth.poll(ALICE, Kind::Transactions).await,
        PollOutcome::Failed
    );
    assert_eq!(sale_ids(&env, ALICE).await, [1]);

    env.repo.mark_needs_relogin(ALICE).await.unwrap();
    assert_eq!(
        env.wealth.poll(ALICE, Kind::Transactions).await,
        PollOutcome::NeedsRelogin
    );
    assert_eq!(sale_ids(&env, ALICE).await, [1]);
}

#[tokio::test]
async fn a_failed_poll_keeps_the_purchases_records_the_failure_and_retries_later() {
    let env = linked().await;
    set_transactions(&env, ALICE, vec![purchase(1)], None).await;
    env.wealth.poll(ALICE, Kind::Transactions).await;
    env.control(
        "esi-fail-next",
        json!({"route": "transactions", "status": 503}),
    )
    .await;
    env.clock.advance(Duration::hours(2));
    let failed_at = env.clock.now();

    let outcome = env.wealth.poll(ALICE, Kind::Transactions).await;

    assert_eq!(outcome, PollOutcome::Failed);
    assert_eq!(purchase_ids(&env, ALICE).await, [1]);
    let (_, next, error, failures) = sync_row(&env, ALICE, "transactions").await;
    assert_eq!(failures, 1);
    assert!(error.is_some());
    assert!(
        next > failed_at && next < failed_at + Duration::hours(1),
        "{next}"
    );
    // The other polls of the character are untouched.
    assert_eq!(
        env.wealth.poll(ALICE, Kind::Wallet).await,
        PollOutcome::Updated
    );
}

#[tokio::test]
async fn transactions_are_polled_hourly_whatever_the_order_interval() {
    let env = linked().await;
    let polled_at = env.clock.now();

    env.wealth.poll(ALICE, Kind::Transactions).await;

    let (success, next, ..) = sync_row(&env, ALICE, "transactions").await;
    assert_eq!(success, Some(polled_at));
    assert!(next >= polled_at + Duration::hours(1), "{next}");
}

#[tokio::test]
async fn a_purchase_stored_late_feeds_an_order_remembered_before_it() {
    let env = linked().await;
    remember_order_11(&env).await;
    set_transactions(&env, ALICE, vec![purchase(1)], None).await;

    env.wealth.poll(ALICE, Kind::Transactions).await;

    let order = env
        .orders_of(ALICE)
        .await
        .into_iter()
        .find(|row| row.order_id == 11)
        .unwrap();
    let cost = (order.covered_units.unwrap(), order.cost_cents.unwrap());
    assert_eq!(cost, (10, 100_250));
}

#[tokio::test]
async fn sales_are_filled_in_once_after_the_upgrade() {
    let env = linked().await;
    // Before the upgrade only the purchase was stored, and the poll had stopped at it.
    set_transactions(&env, ALICE, vec![purchase(5)], Some(2)).await;
    env.wealth.poll(ALICE, Kind::Transactions).await;
    env.stores.wealth.seed_full_read(ALICE, true).await.unwrap();
    let mut history: Vec<_> = (1..=4).map(sale).collect();
    history.push(purchase(5));
    set_transactions(&env, ALICE, history, Some(2)).await;
    let calls = env.stub.esi_calls("transactions");

    // The newest answer [5, 4] holds the stored purchase, and the poll reads on regardless:
    // [5, 4], [3, 2], [1], and the empty answer past the oldest.
    env.clock.advance(Duration::hours(2));
    env.wealth.poll(ALICE, Kind::Transactions).await;

    assert_eq!(env.stub.esi_calls("transactions"), calls + 4);
    assert_eq!(sale_ids(&env, ALICE).await, [1, 2, 3, 4]);
    let flag = env.stores.wealth.full_read(ALICE).await.unwrap();
    assert!(!flag, "the flag is cleared by the successful poll");

    // The second poll stops at the stored purchase again.
    let calls = env.stub.esi_calls("transactions");
    env.clock.advance(Duration::hours(2));
    env.wealth.poll(ALICE, Kind::Transactions).await;
    assert_eq!(env.stub.esi_calls("transactions"), calls + 1);
}

#[tokio::test]
async fn a_failed_full_read_keeps_the_flag_for_the_retry() {
    let env = linked().await;
    set_transactions(&env, ALICE, vec![purchase(1)], None).await;
    env.wealth.poll(ALICE, Kind::Transactions).await;
    env.stores.wealth.seed_full_read(ALICE, true).await.unwrap();
    env.control(
        "esi-fail-next",
        json!({"route": "transactions", "status": 503}),
    )
    .await;
    env.clock.advance(Duration::hours(2));

    let outcome = env.wealth.poll(ALICE, Kind::Transactions).await;

    assert_eq!(outcome, PollOutcome::Failed);
    let flag = env.stores.wealth.full_read(ALICE).await.unwrap();
    assert!(flag);
}

#[tokio::test]
async fn a_new_sale_gets_its_cost_from_the_purchases_in_the_same_poll() {
    let env = linked().await;
    let mut bought = purchase(1);
    bought["date"] = json!("2026-09-30T12:00:00Z");
    set_transactions(&env, ALICE, vec![bought, sale(2)], None).await;

    env.wealth.poll(ALICE, Kind::Transactions).await;

    let sold = trades_of(&env, ALICE)
        .await
        .into_iter()
        .find(|row| row.transaction_id == 2)
        .unwrap();
    let cost = (
        sold.covered_units.unwrap(),
        sold.cost_per_unit_cents.unwrap() * 10,
    );
    assert_eq!(cost, (10, 100_250));
}

#[tokio::test]
async fn the_names_of_the_traded_items_and_stations_are_resolved() {
    let env = linked().await;
    set_transactions(&env, ALICE, vec![purchase(1), sale(2)], None).await;

    env.wealth.poll(ALICE, Kind::Transactions).await;

    assert_eq!(name_of(&env, 34).await, Some(Some("Tritanium".to_owned())));
    assert_eq!(
        name_of(&env, 60003760).await,
        Some(Some(
            "Jita IV - Moon 4 - Caldari Navy Assembly Plant".to_owned()
        ))
    );
}
