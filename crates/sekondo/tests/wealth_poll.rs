mod common;

use common::PollEnv;
use sekondo::{clock::Clock, poller::PollOutcome, wealth_poller::Kind};
use serde_json::json;
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};

const ALICE: i64 = 90000001;

async fn linked(pool: PgPool) -> PollEnv {
    let env = PollEnv::new(pool).await;
    let account = env.new_account().await;
    env.link(account, ALICE).await;
    env
}

async fn balances(env: &PollEnv, character: i64) -> Vec<i64> {
    sqlx::query_scalar(
        "SELECT (balance * 100)::bigint FROM wallet_balances WHERE character_id = $1 ORDER BY polled_at",
    )
    .bind(character)
    .fetch_all(&env.pool)
    .await
    .unwrap()
}

async fn sync_row(
    env: &PollEnv,
    character: i64,
    kind: &str,
) -> (Option<OffsetDateTime>, OffsetDateTime, Option<String>, i32) {
    sqlx::query_as(
        "SELECT last_success_at, next_poll_at, last_error, failures FROM wealth_sync \
         WHERE character_id = $1 AND kind = $2",
    )
    .bind(character)
    .bind(kind)
    .fetch_one(&env.pool)
    .await
    .unwrap()
}

#[sqlx::test]
async fn a_wallet_poll_records_the_balance_and_when_it_happened(pool: PgPool) {
    let env = linked(pool).await;
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

#[sqlx::test]
async fn every_poll_adds_a_row_so_the_balance_has_a_history(pool: PgPool) {
    let env = linked(pool).await;
    env.wealth.poll(ALICE, Kind::Wallet).await;
    env.control("wallet", json!({"character_id": ALICE, "balance": 10.0}))
        .await;
    env.clock.advance(Duration::minutes(21));
    env.wealth.poll(ALICE, Kind::Wallet).await;

    assert_eq!(balances(&env, ALICE).await, [250_000_000, 1000]);
}

#[sqlx::test]
async fn the_next_poll_waits_for_esis_expiry_when_that_is_later(pool: PgPool) {
    let env = linked(pool).await;
    env.control("esi-expires", json!({"seconds": 3 * 3600}))
        .await;
    let polled_at = env.clock.now();

    env.wealth.poll(ALICE, Kind::Wallet).await;

    let (_, next, ..) = sync_row(&env, ALICE, "wallet").await;
    assert!(next >= polled_at + Duration::hours(3), "{next}");
}

#[sqlx::test]
async fn a_character_needing_relogin_is_not_requested_and_keeps_its_balance(pool: PgPool) {
    let env = linked(pool).await;
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

#[sqlx::test]
async fn the_scheduler_skips_characters_needing_relogin(pool: PgPool) {
    let env = linked(pool).await;
    env.repo.mark_needs_relogin(ALICE).await.unwrap();
    env.wealth.tick().await; // schedules nothing for it
    env.clock.advance(Duration::hours(2));

    assert_eq!(env.wealth.tick().await, 0);
    assert_eq!(env.stub.esi_calls("wallet"), 0);
}

#[sqlx::test]
async fn the_scheduler_polls_a_new_character_within_one_interval(pool: PgPool) {
    let env = linked(pool).await;
    assert_eq!(env.wealth.tick().await, 0, "first polls lie ahead");
    let scheduled: i64 =
        sqlx::query_scalar("SELECT count(*) FROM wealth_sync WHERE kind = 'wallet'")
            .fetch_one(&env.pool)
            .await
            .unwrap();
    assert_eq!(scheduled, 1);

    env.clock.advance(env.interval + Duration::minutes(1));
    assert_eq!(env.wealth.tick().await, 1);
    assert_eq!(balances(&env, ALICE).await.len(), 1);
    assert_eq!(env.wealth.tick().await, 0, "not due again right away");
}

#[sqlx::test]
async fn a_failed_poll_keeps_the_balance_and_records_the_failure(pool: PgPool) {
    let env = linked(pool).await;
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

#[sqlx::test]
async fn an_esi_pause_defers_the_poll_without_counting_a_failure(pool: PgPool) {
    let env = linked(pool).await;
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

#[sqlx::test]
async fn a_wallet_failure_does_not_touch_the_order_poll_and_the_other_way_round(pool: PgPool) {
    let env = linked(pool).await;
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
    let order_failures: i32 =
        sqlx::query_scalar("SELECT failures FROM order_sync WHERE character_id = $1")
            .bind(ALICE)
            .fetch_one(&env.pool)
            .await
            .unwrap();
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

#[sqlx::test]
async fn a_character_unlinked_during_its_poll_is_dropped(pool: PgPool) {
    let env = PollEnv::new(pool).await;
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
    sqlx::query_scalar(
        "SELECT item_id FROM current_assets WHERE character_id = $1 ORDER BY item_id",
    )
    .bind(character)
    .fetch_all(&env.pool)
    .await
    .unwrap()
}

async fn inventory_values(env: &PollEnv, character: i64) -> Vec<i64> {
    sqlx::query_scalar(
        "SELECT (value * 100)::bigint FROM inventory_values WHERE character_id = $1 ORDER BY recorded_at",
    )
    .bind(character)
    .fetch_all(&env.pool)
    .await
    .unwrap()
}

async fn set_price(env: &PollEnv, type_id: i64, price: Option<&str>) {
    sqlx::query(
        "INSERT INTO item_prices (type_id, average_price) VALUES ($1, $2::numeric)
         ON CONFLICT (type_id) DO UPDATE SET average_price = excluded.average_price",
    )
    .bind(type_id)
    .bind(price)
    .execute(&env.pool)
    .await
    .unwrap();
}

#[sqlx::test]
async fn the_assets_of_all_pages_become_the_current_assets(pool: PgPool) {
    let env = linked(pool).await;
    let items: Vec<_> = (1..=5).map(|id| asset(id, 34, 1)).collect();
    set_assets(&env, ALICE, items, 2).await;

    assert_eq!(
        env.wealth.poll(ALICE, Kind::Assets).await,
        PollOutcome::Updated
    );

    assert_eq!(asset_ids(&env, ALICE).await, [1, 2, 3, 4, 5]);
    assert_eq!(env.stub.esi_calls("assets"), 3);
}

#[sqlx::test]
async fn an_item_that_is_gone_stops_being_an_asset(pool: PgPool) {
    let env = linked(pool).await;
    set_assets(&env, ALICE, vec![asset(1, 34, 1), asset(2, 34, 1)], 1000).await;
    env.wealth.poll(ALICE, Kind::Assets).await;

    set_assets(&env, ALICE, vec![asset(2, 34, 1)], 1000).await;
    env.clock.advance(Duration::hours(2));
    env.wealth.poll(ALICE, Kind::Assets).await;

    assert_eq!(asset_ids(&env, ALICE).await, [2]);
}

#[sqlx::test]
async fn a_failure_on_the_second_page_keeps_the_assets_and_retries_later(pool: PgPool) {
    let env = linked(pool).await;
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

#[sqlx::test]
async fn assets_are_not_polled_again_within_the_hour_whatever_the_order_interval(pool: PgPool) {
    let env = linked(pool).await;
    let polled_at = env.clock.now();

    env.wealth.poll(ALICE, Kind::Assets).await;

    let (success, next, ..) = sync_row(&env, ALICE, "assets").await;
    assert_eq!(success, Some(polled_at));
    assert!(next >= polled_at + Duration::hours(1), "{next}");
}

#[sqlx::test]
async fn the_scheduler_polls_assets_and_wallet_independently(pool: PgPool) {
    let env = linked(pool).await;
    env.wealth.tick().await;
    let kinds: Vec<String> = sqlx::query_scalar("SELECT kind FROM wealth_sync ORDER BY kind")
        .fetch_all(&env.pool)
        .await
        .unwrap();
    assert_eq!(kinds, ["assets", "wallet"]);

    // After 70 minutes both are due: the wallet (20 minute interval) and the assets (an hour).
    env.clock.advance(Duration::minutes(70));
    assert_eq!(env.wealth.tick().await, 2);
    assert_eq!(env.stub.esi_calls("wallet"), 1);
    assert_eq!(env.stub.esi_calls("assets"), 1);

    // A failing wallet leaves the assets alone.
    env.control("esi-fail-next", json!({"route": "wallet", "status": 500}))
        .await;
    env.wealth.poll(ALICE, Kind::Wallet).await;
    let (_, _, _, assets_failures) = sync_row(&env, ALICE, "assets").await;
    assert_eq!(assets_failures, 0);
}

#[sqlx::test]
async fn every_assets_poll_records_the_inventory_value_from_the_stored_prices(pool: PgPool) {
    let env = linked(pool).await;
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

#[sqlx::test]
async fn without_any_price_the_inventory_is_worth_nothing_but_still_recorded(pool: PgPool) {
    let env = linked(pool).await;
    env.wealth.poll(ALICE, Kind::Assets).await;
    assert_eq!(inventory_values(&env, ALICE).await, [0]);
}

#[sqlx::test]
async fn an_item_that_changed_hands_moves_to_its_new_owner(pool: PgPool) {
    let env = linked(pool).await;
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
    sqlx::query_scalar("SELECT name FROM names WHERE id = $1")
        .bind(id)
        .fetch_optional(&env.pool)
        .await
        .unwrap()
}

fn asset_at(id: i64, type_id: i64, location: i64, location_type: &str) -> serde_json::Value {
    json!({
        "item_id": id, "type_id": type_id, "location_id": location, "location_type": location_type,
        "location_flag": "Hangar", "quantity": 1, "is_singleton": false,
    })
}

#[sqlx::test]
async fn the_names_of_asset_types_stations_and_systems_are_resolved(pool: PgPool) {
    let env = linked(pool).await;
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

#[sqlx::test]
async fn a_structure_is_named_with_the_owners_token_and_a_refused_one_stays_unnamed(pool: PgPool) {
    let env = linked(pool).await;
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
    let retry: Option<OffsetDateTime> =
        sqlx::query_scalar("SELECT retry_after FROM names WHERE id = 1000000000002")
            .fetch_one(&env.pool)
            .await
            .unwrap();
    assert!(retry.unwrap() > env.clock.now() + Duration::hours(23));
}

#[sqlx::test]
async fn a_name_that_cannot_be_resolved_never_fails_the_assets_poll(pool: PgPool) {
    let env = linked(pool).await;
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
