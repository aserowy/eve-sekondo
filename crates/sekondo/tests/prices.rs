mod common;

use common::PollEnv;
use sekondo::{market::RefreshOutcome, wealth_poller::Kind};
use serde_json::{Value, json};
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;

/// Alice and Bob on two accounts, each with the stub's default assets already polled:
/// Alice 1000 of type 34 and 50 of type 35, Bob 20 of type 34.
async fn two_accounts(pool: PgPool) -> PollEnv {
    let env = PollEnv::new(pool).await;
    for character in [ALICE, BOB] {
        let account = env.new_account().await;
        env.link(account, character).await;
        env.wealth.poll(character, Kind::Assets).await;
    }
    // The refresh happens later than the assets polls, so its entries are separate ones.
    env.clock.advance(Duration::minutes(1));
    env
}

async fn prices(env: &PollEnv) -> Vec<(i64, Option<i64>)> {
    sqlx::query_as(
        "SELECT type_id, (average_price * 100)::bigint FROM item_prices ORDER BY type_id",
    )
    .fetch_all(&env.pool)
    .await
    .unwrap()
}

async fn inventory(env: &PollEnv, character: i64) -> Vec<i64> {
    sqlx::query_scalar(
        "SELECT (value * 100)::bigint FROM inventory_values WHERE character_id = $1 ORDER BY recorded_at",
    )
    .bind(character)
    .fetch_all(&env.pool)
    .await
    .unwrap()
}

async fn set_prices(env: &PollEnv, prices: Value) {
    let res = env.control("prices", json!({"prices": prices})).await;
    assert_eq!(res.status(), 204);
}

async fn sync_row(env: &PollEnv) -> (Option<OffsetDateTime>, OffsetDateTime, Option<String>, i32) {
    sqlx::query_as("SELECT last_success_at, next_poll_at, last_error, failures FROM price_sync")
        .fetch_one(&env.pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn one_fetch_prices_the_inventories_of_every_account(pool: PgPool) {
    let env = two_accounts(pool).await;

    assert_eq!(env.prices.refresh().await, RefreshOutcome::Updated);

    assert_eq!(
        env.stub.esi_calls("prices"),
        1,
        "once, not once per account"
    );
    assert_eq!(
        prices(&env).await,
        [(34, Some(550)), (35, Some(1025)), (36, None)]
    );
    // Assets were polled before the prices, so each has a zero entry and the repricing entry.
    assert_eq!(inventory(&env, ALICE).await, [0, 601_250]);
    assert_eq!(inventory(&env, BOB).await, [0, 11_000]);
}

#[sqlx::test]
async fn a_failed_refresh_keeps_the_prices_and_records_no_entry(pool: PgPool) {
    let env = two_accounts(pool).await;
    env.prices.refresh().await;
    let (success_before, ..) = sync_row(&env).await;
    set_prices(&env, json!([{"type_id": 34, "average_price": 99.0}])).await;
    env.control("esi-fail-next", json!({"route": "prices", "status": 503}))
        .await;
    env.clock.advance(Duration::days(2));
    let failed_at = sekondo::clock::Clock::now(&*env.clock);

    assert_eq!(env.prices.refresh().await, RefreshOutcome::Failed);

    assert_eq!(
        prices(&env).await,
        [(34, Some(550)), (35, Some(1025)), (36, None)]
    );
    assert_eq!(inventory(&env, ALICE).await.len(), 2);
    let (success, next, error, failures) = sync_row(&env).await;
    assert_eq!(success, success_before);
    assert!(error.unwrap().contains("503"));
    assert_eq!(failures, 1);
    assert_eq!(next, failed_at + Duration::minutes(1));
}

#[sqlx::test]
async fn doubled_prices_double_the_inventory_entry_at_the_refresh_time(pool: PgPool) {
    let env = two_accounts(pool).await;
    env.prices.refresh().await;
    set_prices(
        &env,
        json!([{"type_id": 34, "average_price": 11.0}, {"type_id": 35, "average_price": 20.5}]),
    )
    .await;
    env.clock.advance(Duration::days(2));
    let refreshed_at = sekondo::clock::Clock::now(&*env.clock);

    assert_eq!(env.prices.refresh().await, RefreshOutcome::Updated);

    assert_eq!(inventory(&env, ALICE).await, [0, 601_250, 1_202_500]);
    let last: OffsetDateTime =
        sqlx::query_scalar("SELECT max(recorded_at) FROM inventory_values WHERE character_id = $1")
            .bind(ALICE)
            .fetch_one(&env.pool)
            .await
            .unwrap();
    assert_eq!(last, refreshed_at);
    // An item without a price any more is dropped from the copy.
    assert_eq!(prices(&env).await, [(34, Some(1100)), (35, Some(2050))]);
}

#[sqlx::test]
async fn blueprint_copies_and_unpriced_items_add_nothing(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = env.new_account().await;
    env.link(account, ALICE).await;
    let item = |id: i64, type_id: i64, copy: bool| {
        let mut asset = json!({
            "item_id": id, "type_id": type_id, "location_id": 60003760,
            "location_type": "station", "location_flag": "Hangar", "quantity": 10,
            "is_singleton": false,
        });
        if copy {
            asset["is_blueprint_copy"] = json!(true);
        }
        asset
    };
    env.control(
        "assets",
        json!({"character_id": ALICE,
               "assets": [item(1, 34, false), item(2, 34, true), item(3, 36, false)]}),
    )
    .await;
    env.wealth.poll(ALICE, Kind::Assets).await;

    env.prices.refresh().await;

    // Only the 10 units of the original of type 34 count: 10 * 5.50.
    assert_eq!(inventory(&env, ALICE).await.last(), Some(&5500));
}

#[sqlx::test]
async fn a_character_without_assets_gets_no_entry_from_a_refresh(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = env.new_account().await;
    env.link(account, 90000003).await;
    env.prices.refresh().await;
    assert!(inventory(&env, 90000003).await.is_empty());
}

#[sqlx::test]
async fn the_refresh_is_daily_and_waits_for_esis_expiry_when_later(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let at = sekondo::clock::Clock::now(&*env.clock);
    env.prices.refresh().await;
    let (_, next, ..) = sync_row(&env).await;
    assert!(next >= at + Duration::days(1), "{next}");

    env.control("esi-expires", json!({"seconds": 3 * 86_400}))
        .await;
    env.clock.advance(Duration::days(2));
    let at = sekondo::clock::Clock::now(&*env.clock);
    env.prices.refresh().await;
    let (_, next, ..) = sync_row(&env).await;
    assert!(next >= at + Duration::days(3), "{next}");
}

#[sqlx::test]
async fn the_scheduler_fetches_the_first_prices_at_once_and_then_once_a_day(pool: PgPool) {
    let env = PollEnv::new(pool).await;

    assert_eq!(env.prices.tick().await, 1);
    assert_eq!(env.prices.tick().await, 0);
    env.clock.advance(Duration::hours(23));
    assert_eq!(env.prices.tick().await, 0);
    env.clock.advance(Duration::hours(2));
    assert_eq!(env.prices.tick().await, 1);
    assert_eq!(env.stub.esi_calls("prices"), 2);
}

#[sqlx::test]
async fn an_esi_pause_defers_the_refresh_without_counting_a_failure(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    env.prices.refresh().await;
    env.control(
        "esi-fail-next",
        json!({"route": "prices", "status": 429, "headers": {"retry-after": "600"}}),
    )
    .await;
    env.clock.advance(Duration::days(2));
    let at = sekondo::clock::Clock::now(&*env.clock);

    assert_eq!(env.prices.refresh().await, RefreshOutcome::Deferred);

    let (_, next, _, failures) = sync_row(&env).await;
    assert_eq!(failures, 0);
    assert!(next >= at + Duration::seconds(600));
}
