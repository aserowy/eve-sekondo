mod common;

use common::PollEnv;
use sekondo::poller::PollOutcome;
use serde_json::{Value, json};
use sqlx::PgPool;
use time::Duration;

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;

fn order(id: i64, price: f64, remain: i64) -> Value {
    json!({
        "order_id": id, "type_id": 34, "region_id": 10000002, "location_id": 60003760,
        "is_buy_order": false, "is_corporation": false, "price": price,
        "volume_remain": remain, "volume_total": 20, "min_volume": 1, "range": "station",
        "issued": "2026-10-01T12:00:00Z", "duration": 90
    })
}

async fn set_orders(env: &PollEnv, character: i64, orders: Vec<Value>) {
    let res = env
        .control(
            "orders",
            json!({"character_id": character, "orders": orders}),
        )
        .await;
    assert_eq!(res.status(), 204);
}

async fn current_ids(pool: &PgPool, character: i64) -> Vec<i64> {
    sqlx::query_scalar(
        "SELECT order_id FROM current_orders WHERE character_id = $1 ORDER BY order_id",
    )
    .bind(character)
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn linked(pool: PgPool) -> PollEnv {
    let env = PollEnv::new(pool).await;
    let account = env.new_account().await;
    env.link(account, ALICE).await;
    env
}

#[sqlx::test]
async fn the_orders_of_a_linked_character_become_its_current_orders(pool: PgPool) {
    let env = linked(pool).await;
    set_orders(&env, ALICE, vec![order(1, 10.0, 5), order(2, 20.0, 6)]).await;

    assert_eq!(env.poller.poll_character(ALICE).await, PollOutcome::Updated);

    assert_eq!(current_ids(&env.pool, ALICE).await, vec![1, 2]);
}

#[sqlx::test]
async fn an_order_that_closes_stops_being_current(pool: PgPool) {
    let env = linked(pool).await;
    set_orders(&env, ALICE, vec![order(1, 10.0, 5), order(2, 20.0, 6)]).await;
    env.poller.poll_character(ALICE).await;

    set_orders(&env, ALICE, vec![order(2, 20.0, 6)]).await;
    env.clock.advance(Duration::minutes(21));
    env.poller.poll_character(ALICE).await;

    assert_eq!(current_ids(&env.pool, ALICE).await, vec![2]);
}

#[sqlx::test]
async fn a_changed_order_shows_its_new_price_and_volume(pool: PgPool) {
    let env = linked(pool).await;
    set_orders(&env, ALICE, vec![order(1, 10.0, 5)]).await;
    env.poller.poll_character(ALICE).await;

    set_orders(&env, ALICE, vec![order(1, 12.34, 3)]).await;
    env.poller.poll_character(ALICE).await;

    let (cents, remain): (i64, i64) = sqlx::query_as(
        "SELECT (price * 100)::bigint, volume_remain FROM current_orders WHERE order_id = 1",
    )
    .fetch_one(&env.pool)
    .await
    .unwrap();
    assert_eq!((cents, remain), (1234, 3));
}

#[sqlx::test]
async fn an_order_placed_for_the_corporation_is_kept_and_flagged(pool: PgPool) {
    let env = linked(pool).await;
    let mut corp = order(1, 10.0, 5);
    corp["is_corporation"] = json!(true);
    set_orders(&env, ALICE, vec![corp, order(2, 20.0, 6)]).await;

    env.poller.poll_character(ALICE).await;

    let flags: Vec<(i64, bool)> =
        sqlx::query_as("SELECT order_id, is_corporation FROM current_orders ORDER BY order_id")
            .fetch_all(&env.pool)
            .await
            .unwrap();
    assert_eq!(flags, vec![(1, true), (2, false)]);
}

#[sqlx::test]
async fn every_poll_records_one_snapshot_row_per_order(pool: PgPool) {
    let env = linked(pool).await;
    set_orders(
        &env,
        ALICE,
        vec![order(1, 10.0, 5), order(2, 20.0, 6), order(3, 30.0, 7)],
    )
    .await;

    env.poller.poll_character(ALICE).await;
    env.clock.advance(Duration::minutes(21));
    env.poller.poll_character(ALICE).await;

    let per_poll: Vec<(i64,)> = sqlx::query_as(
        "SELECT count(*) FROM order_snapshots WHERE character_id = $1 GROUP BY polled_at ORDER BY polled_at",
    )
    .bind(ALICE)
    .fetch_all(&env.pool)
    .await
    .unwrap();
    assert_eq!(per_poll, vec![(3,), (3,)]);
}

#[sqlx::test]
async fn a_poll_without_orders_records_the_poll_but_no_snapshots(pool: PgPool) {
    let env = linked(pool).await;
    set_orders(&env, ALICE, vec![]).await;

    assert_eq!(env.poller.poll_character(ALICE).await, PollOutcome::Updated);

    let snapshots: i64 = sqlx::query_scalar("SELECT count(*) FROM order_snapshots")
        .fetch_one(&env.pool)
        .await
        .unwrap();
    let synced: Option<time::OffsetDateTime> =
        sqlx::query_scalar("SELECT last_success_at FROM order_sync WHERE character_id = $1")
            .bind(ALICE)
            .fetch_one(&env.pool)
            .await
            .unwrap();
    assert_eq!(snapshots, 0);
    assert!(synced.is_some());
}

#[sqlx::test]
async fn unlinking_a_character_removes_its_snapshots(pool: PgPool) {
    let env = linked(pool).await;
    let account = env
        .repo
        .find_character(ALICE)
        .await
        .unwrap()
        .unwrap()
        .account_id;
    env.link(account, BOB).await;
    set_orders(&env, ALICE, vec![order(1, 10.0, 5)]).await;
    set_orders(&env, BOB, vec![order(2, 10.0, 5)]).await;
    env.poller.poll_character(ALICE).await;
    env.poller.poll_character(BOB).await;

    env.repo.unlink_character(account, ALICE).await.unwrap();

    let remaining: Vec<i64> =
        sqlx::query_scalar("SELECT DISTINCT character_id FROM order_snapshots")
            .fetch_all(&env.pool)
            .await
            .unwrap();
    assert_eq!(remaining, vec![BOB]);
}

#[sqlx::test]
async fn a_successful_poll_records_when_it_happened_and_when_the_next_one_is_due(pool: PgPool) {
    use sekondo::clock::Clock;
    let env = linked(pool).await;
    let polled_at = env.clock.now();

    env.poller.poll_character(ALICE).await;

    let (last_success, next, error, expires): (
        time::OffsetDateTime,
        time::OffsetDateTime,
        Option<String>,
        Option<time::OffsetDateTime>,
    ) = sqlx::query_as(
        "SELECT last_success_at, next_poll_at, last_error, esi_expires_at FROM order_sync WHERE character_id = $1",
    )
    .bind(ALICE)
    .fetch_one(&env.pool)
    .await
    .unwrap();
    assert_eq!(last_success, polled_at);
    assert!(next >= polled_at + env.interval, "{next} vs {polled_at}");
    assert!(error.is_none());
    assert!(expires.is_some());
}

// ---- failure paths (4.2) ----

async fn sync_row(
    pool: &PgPool,
    character: i64,
) -> (
    Option<time::OffsetDateTime>,
    time::OffsetDateTime,
    Option<String>,
    i32,
) {
    sqlx::query_as(
        "SELECT last_success_at, next_poll_at, last_error, failures FROM order_sync WHERE character_id = $1",
    )
    .bind(character)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Makes the stored access token look expired, so the next poll goes to SSO.
async fn expire_token(pool: &PgPool, id: i64) {
    sqlx::query("UPDATE character_tokens SET access_expires_at = now() - interval '1 hour' WHERE character_id = $1")
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
}

async fn needs_relogin(env: &PollEnv, id: i64) -> bool {
    env.repo
        .find_character(id)
        .await
        .unwrap()
        .unwrap()
        .needs_relogin
}

#[sqlx::test]
async fn a_character_needing_relogin_is_not_requested_and_keeps_its_orders(pool: PgPool) {
    let env = linked(pool).await;
    set_orders(&env, ALICE, vec![order(1, 10.0, 5)]).await;
    env.poller.poll_character(ALICE).await;
    env.repo.mark_needs_relogin(ALICE).await.unwrap();
    let orders_calls = env.stub.esi_calls("orders");
    let token_calls = env.stub.token_calls();

    let outcome = env.poller.poll_character(ALICE).await;

    assert_eq!(outcome, PollOutcome::NeedsRelogin);
    assert_eq!(env.stub.esi_calls("orders"), orders_calls);
    assert_eq!(env.stub.token_calls(), token_calls);
    assert_eq!(current_ids(&env.pool, ALICE).await, vec![1]);
}

#[sqlx::test]
async fn a_rejected_refresh_token_marks_the_character_and_requests_no_orders(pool: PgPool) {
    let env = linked(pool).await;
    set_orders(&env, ALICE, vec![order(1, 10.0, 5)]).await;
    env.poller.poll_character(ALICE).await;
    expire_token(&env.pool, ALICE).await;
    env.control("revoke", json!({"character_id": ALICE})).await;
    let orders_calls = env.stub.esi_calls("orders");

    let outcome = env.poller.poll_character(ALICE).await;

    assert_eq!(outcome, PollOutcome::NeedsRelogin);
    assert!(needs_relogin(&env, ALICE).await);
    assert_eq!(env.stub.esi_calls("orders"), orders_calls);
    assert_eq!(current_ids(&env.pool, ALICE).await, vec![1]);
}

#[sqlx::test]
async fn a_temporary_token_failure_is_recorded_and_keeps_the_orders(pool: PgPool) {
    let env = linked(pool).await;
    set_orders(&env, ALICE, vec![order(1, 10.0, 5)]).await;
    env.poller.poll_character(ALICE).await;
    expire_token(&env.pool, ALICE).await;
    env.control("fail-next-token", json!({"count": 1})).await;

    let outcome = env.poller.poll_character(ALICE).await;

    assert_eq!(outcome, PollOutcome::Failed);
    assert!(!needs_relogin(&env, ALICE).await);
    assert_eq!(current_ids(&env.pool, ALICE).await, vec![1]);
    let (_, _, error, failures) = sync_row(&env.pool, ALICE).await;
    assert!(error.is_some());
    assert_eq!(failures, 1);
}

#[sqlx::test]
async fn an_esi_server_error_keeps_the_orders_and_records_the_failure(pool: PgPool) {
    use sekondo::clock::Clock;
    let env = linked(pool).await;
    set_orders(&env, ALICE, vec![order(1, 10.0, 5)]).await;
    env.poller.poll_character(ALICE).await;
    let (success_before, ..) = sync_row(&env.pool, ALICE).await;
    set_orders(&env, ALICE, vec![order(2, 10.0, 5)]).await;
    env.clock.advance(Duration::minutes(21));
    env.control("esi-fail-next", json!({"status": 503})).await;
    let failed_at = env.clock.now();

    let outcome = env.poller.poll_character(ALICE).await;

    assert_eq!(outcome, PollOutcome::Failed);
    assert!(!needs_relogin(&env, ALICE).await);
    assert_eq!(current_ids(&env.pool, ALICE).await, vec![1]);
    let (success, next, error, failures) = sync_row(&env.pool, ALICE).await;
    assert_eq!(success, success_before, "the last success is unchanged");
    assert_eq!(failures, 1);
    assert!(error.unwrap().contains("503"));
    assert_eq!(next, failed_at + Duration::minutes(1));
}

#[sqlx::test]
async fn retries_back_off_exponentially_up_to_the_interval(pool: PgPool) {
    use sekondo::clock::Clock;
    let env = linked(pool).await;
    env.control("esi-fail-next", json!({"count": 10, "status": 500}))
        .await;

    let mut waits = Vec::new();
    for _ in 0..7 {
        let at = env.clock.now();
        assert_eq!(env.poller.poll_character(ALICE).await, PollOutcome::Failed);
        let (_, next, ..) = sync_row(&env.pool, ALICE).await;
        waits.push((next - at).whole_seconds());
        env.clock.advance(next - at);
    }

    // 1, 2, 4, 8, 16 minutes, then capped at the 20-minute interval.
    assert_eq!(waits, vec![60, 120, 240, 480, 960, 1200, 1200]);
}

#[sqlx::test]
async fn a_poll_deferred_by_the_gate_is_rescheduled_without_a_failure(pool: PgPool) {
    use sekondo::clock::Clock;
    let env = linked(pool).await;
    env.control(
        "esi-fail-next",
        json!({"status": 429, "headers": {"retry-after": "120", "x-ratelimit-group": "char-market"}}),
    )
    .await;
    let at = env.clock.now();

    let outcome = env.poller.poll_character(ALICE).await;

    assert_eq!(outcome, PollOutcome::Deferred);
    let (_, next, error, failures) = sync_row(&env.pool, ALICE).await;
    assert!(next >= at + Duration::seconds(120), "{next}");
    assert!(next <= at + Duration::seconds(150), "{next}");
    assert!(error.is_none());
    assert_eq!(failures, 0);

    // While the gate is blocked nothing more is sent.
    let calls = env.stub.esi_calls("orders");
    assert_eq!(
        env.poller.poll_character(ALICE).await,
        PollOutcome::Deferred
    );
    assert_eq!(env.stub.esi_calls("orders"), calls);
}

#[sqlx::test]
async fn a_character_unlinked_during_its_poll_is_dropped_without_error(pool: PgPool) {
    let env = linked(pool).await;
    set_orders(&env, ALICE, vec![order(1, 10.0, 5)]).await;
    env.control("esi-delay", json!({"millis": 300})).await;

    // The character disappears while ESI is still answering the orders request.
    let unlink = async {
        while env.stub.esi_calls("orders") == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        sqlx::query("DELETE FROM characters WHERE character_id = $1")
            .bind(ALICE)
            .execute(&env.pool)
            .await
            .unwrap();
    };
    let (outcome, ()) = tokio::join!(env.poller.poll_character(ALICE), unlink);

    assert_eq!(outcome, PollOutcome::Dropped);
    for table in ["current_orders", "order_snapshots", "order_sync"] {
        let rows: i64 =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                .fetch_one(&env.pool)
                .await
                .unwrap();
        assert_eq!(rows, 0, "{table}");
    }
}

// ---- name resolution (4.3) ----

fn order_at(id: i64, type_id: i64, location: i64) -> Value {
    let mut o = order(id, 10.0, 5);
    o["type_id"] = json!(type_id);
    o["location_id"] = json!(location);
    o
}

async fn names_table(pool: &PgPool) -> Vec<(i64, Option<String>)> {
    sqlx::query_as("SELECT id, name FROM names ORDER BY id")
        .fetch_all(pool)
        .await
        .unwrap()
}

async fn name_of(pool: &PgPool, id: i64) -> Option<Option<String>> {
    sqlx::query_scalar("SELECT name FROM names WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .unwrap()
}

const STRUCTURE: i64 = 1000000000001;

#[sqlx::test]
async fn names_are_requested_from_esi_only_once(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    let account = env.new_account().await;
    env.link(account, ALICE).await;
    env.link(account, BOB).await;
    // Two characters, the same item in the same station.
    set_orders(&env, ALICE, vec![order_at(1, 34, 60003760)]).await;
    set_orders(&env, BOB, vec![order_at(2, 34, 60003760)]).await;

    env.poller.poll_character(ALICE).await;
    env.poller.poll_character(BOB).await;
    env.clock.advance(Duration::minutes(21));
    env.poller.poll_character(ALICE).await;

    let requested: Vec<i64> = env.stub.names_requests().concat();
    let mut sorted = requested.clone();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        vec![34, 10000002, 60003760],
        "each ID once: {requested:?}"
    );
    assert_eq!(env.stub.esi_calls("names"), 1);
    let names = names_table(&env.pool).await;
    assert_eq!(
        names,
        vec![
            (34, Some("Tritanium".to_owned())),
            (10000002, Some("The Forge".to_owned())),
            (
                60003760,
                Some("Jita IV - Moon 4 - Caldari Navy Assembly Plant".to_owned())
            ),
        ]
    );
}

#[sqlx::test]
async fn a_structure_esi_refuses_to_describe_is_remembered_and_retried_after_a_day(pool: PgPool) {
    use sekondo::clock::Clock;
    let env = linked(pool).await;
    env.control(
        "structure-access",
        json!({"structure_id": STRUCTURE, "allowed": false}),
    )
    .await;
    set_orders(&env, ALICE, vec![order_at(1, 34, STRUCTURE)]).await;
    let polled_at = env.clock.now();

    assert_eq!(env.poller.poll_character(ALICE).await, PollOutcome::Updated);

    assert_eq!(name_of(&env.pool, STRUCTURE).await, Some(None));
    let retry_after: time::OffsetDateTime =
        sqlx::query_scalar("SELECT retry_after FROM names WHERE id = $1")
            .bind(STRUCTURE)
            .fetch_one(&env.pool)
            .await
            .unwrap();
    assert_eq!(retry_after, polled_at + Duration::hours(24));
    assert_eq!(env.stub.esi_calls("structures"), 1);

    // Not asked again within the day, however often the character is polled.
    env.clock.advance(Duration::hours(23));
    env.poller.poll_character(ALICE).await;
    assert_eq!(env.stub.esi_calls("structures"), 1);

    // After the day it is, and access has been granted meanwhile.
    env.control(
        "structure-access",
        json!({"structure_id": STRUCTURE, "allowed": true}),
    )
    .await;
    env.clock.advance(Duration::hours(2));
    env.poller.poll_character(ALICE).await;
    assert_eq!(env.stub.esi_calls("structures"), 2);
    assert_eq!(
        name_of(&env.pool, STRUCTURE).await,
        Some(Some("Alpha Keepstar".to_owned()))
    );
}

#[sqlx::test]
async fn one_invalid_id_does_not_stop_the_other_names(pool: PgPool) {
    let env = linked(pool).await;
    set_orders(
        &env,
        ALICE,
        vec![order_at(1, 34, 60003760), order_at(2, 999999, 60003760)],
    )
    .await;

    assert_eq!(env.poller.poll_character(ALICE).await, PollOutcome::Updated);

    assert_eq!(
        name_of(&env.pool, 34).await,
        Some(Some("Tritanium".to_owned()))
    );
    assert!(name_of(&env.pool, 60003760).await.unwrap().is_some());
    assert!(name_of(&env.pool, 10000002).await.unwrap().is_some());
    // The bad one is remembered as unresolvable instead of being asked for on every poll.
    assert_eq!(name_of(&env.pool, 999999).await, Some(None));
    let calls = env.stub.esi_calls("names");
    env.clock.advance(Duration::minutes(21));
    env.poller.poll_character(ALICE).await;
    assert_eq!(env.stub.esi_calls("names"), calls);
}

#[sqlx::test]
async fn a_temporary_failure_caches_nothing_and_is_retried_on_the_next_poll(pool: PgPool) {
    let env = linked(pool).await;
    set_orders(&env, ALICE, vec![order_at(1, 34, STRUCTURE)]).await;
    // The orders call goes through; the name lookups after it fail.
    env.control("esi-fail-next", json!({"route": "names", "status": 503}))
        .await;

    let outcome = env.poller.poll_character(ALICE).await;

    assert_eq!(
        outcome,
        PollOutcome::Updated,
        "the orders are stored regardless"
    );
    assert_eq!(current_ids(&env.pool, ALICE).await, vec![1]);
    assert!(names_table(&env.pool).await.is_empty());

    env.clock.advance(Duration::minutes(21));
    env.poller.poll_character(ALICE).await;
    assert_eq!(
        name_of(&env.pool, STRUCTURE).await,
        Some(Some("Alpha Keepstar".to_owned()))
    );
    assert_eq!(
        name_of(&env.pool, 34).await,
        Some(Some("Tritanium".to_owned()))
    );
}

// ---- signals (5.1) ----

#[sqlx::test]
async fn a_poll_signals_the_account_of_the_character_and_no_other(pool: PgPool) {
    use futures_util::StreamExt;
    use sekondo::events::{Event, EventBus};
    let env = PollEnv::new(pool).await;
    let alice_account = env.new_account().await;
    let bob_account = env.new_account().await;
    env.link(alice_account, ALICE).await;
    env.link(bob_account, BOB).await;
    let mut alice_stream = env.events.subscribe(alice_account);
    let mut bob_stream = env.events.subscribe(bob_account);

    env.poller.poll_character(ALICE).await;

    let wait = std::time::Duration::from_millis(200);
    assert_eq!(
        tokio::time::timeout(wait, alice_stream.next())
            .await
            .unwrap(),
        Some(Event::OrdersUpdated)
    );
    assert!(tokio::time::timeout(wait, bob_stream.next()).await.is_err());
}

#[sqlx::test]
async fn a_failed_poll_and_a_needed_relogin_signal_too_so_freshness_updates(pool: PgPool) {
    use futures_util::StreamExt;
    use sekondo::events::EventBus;
    let env = PollEnv::new(pool).await;
    let account = env.new_account().await;
    env.link(account, ALICE).await;
    let mut stream = env.events.subscribe(account);
    let wait = std::time::Duration::from_millis(200);

    env.control("esi-fail-next", json!({"status": 503})).await;
    assert_eq!(env.poller.poll_character(ALICE).await, PollOutcome::Failed);
    assert!(tokio::time::timeout(wait, stream.next()).await.is_ok());

    env.repo.mark_needs_relogin(ALICE).await.unwrap();
    assert_eq!(
        env.poller.poll_character(ALICE).await,
        PollOutcome::NeedsRelogin
    );
    assert!(tokio::time::timeout(wait, stream.next()).await.is_ok());
}

// ---- poll totals history ----

fn buy_order(id: i64, escrow: f64, is_corporation: bool) -> Value {
    let mut buy = order(id, 4.0, 5);
    buy["is_buy_order"] = json!(true);
    buy["escrow"] = json!(escrow);
    buy["is_corporation"] = json!(is_corporation);
    buy
}

/// Sell value, escrow, both corporation shares, sell and buy count of each entry, oldest first.
type Entry = (String, String, String, String, i32, i32);

async fn poll_totals(pool: &PgPool, character: i64) -> Vec<Entry> {
    sqlx::query_as(
        "SELECT sell_value::text, escrow::text, sell_corp::text, escrow_corp::text, sell_count, \
         buy_count FROM order_poll_totals WHERE character_id = $1 ORDER BY polled_at",
    )
    .bind(character)
    .fetch_all(pool)
    .await
    .unwrap()
}

fn entry(values: [&str; 4], sell_count: i32, buy_count: i32) -> Entry {
    let [sell, escrow, sell_corp, escrow_corp] = values.map(str::to_owned);
    (sell, escrow, sell_corp, escrow_corp, sell_count, buy_count)
}

#[sqlx::test]
async fn a_poll_with_orders_records_its_totals_with_the_corporation_share(pool: PgPool) {
    let env = linked(pool).await;
    let mut corp_sell = order(2, 20.0, 6);
    corp_sell["is_corporation"] = json!(true);
    set_orders(
        &env,
        ALICE,
        vec![
            order(1, 10.0, 5),
            corp_sell,
            buy_order(3, 2.5, false),
            buy_order(4, 7.5, true),
        ],
    )
    .await;

    env.poller.poll_character(ALICE).await;

    assert_eq!(
        poll_totals(&env.pool, ALICE).await,
        vec![entry(["170.00", "10.00", "120.00", "7.50"], 2, 2)]
    );
}

#[sqlx::test]
async fn a_poll_without_orders_records_zeros(pool: PgPool) {
    let env = linked(pool).await;
    set_orders(&env, ALICE, vec![order(1, 10.0, 5)]).await;
    env.poller.poll_character(ALICE).await;
    set_orders(&env, ALICE, vec![]).await;
    env.clock.advance(Duration::minutes(21));

    env.poller.poll_character(ALICE).await;

    assert_eq!(
        poll_totals(&env.pool, ALICE).await,
        vec![
            entry(["50.00", "0.00", "0.00", "0.00"], 1, 0),
            entry(["0.00", "0.00", "0.00", "0.00"], 0, 0),
        ]
    );
}

#[sqlx::test]
async fn a_failed_poll_records_no_entry(pool: PgPool) {
    let env = linked(pool).await;
    set_orders(&env, ALICE, vec![order(1, 10.0, 5)]).await;
    env.poller.poll_character(ALICE).await;
    env.clock.advance(Duration::minutes(21));
    env.control("esi-fail-next", json!({"status": 503})).await;

    assert_eq!(env.poller.poll_character(ALICE).await, PollOutcome::Failed);

    assert_eq!(poll_totals(&env.pool, ALICE).await.len(), 1);
}

#[sqlx::test]
async fn unlinking_a_character_removes_its_poll_totals(pool: PgPool) {
    let env = linked(pool).await;
    let account = env
        .repo
        .find_character(ALICE)
        .await
        .unwrap()
        .unwrap()
        .account_id;
    env.link(account, BOB).await;
    env.poller.poll_character(ALICE).await;
    env.poller.poll_character(BOB).await;

    env.repo.unlink_character(account, ALICE).await.unwrap();

    assert!(poll_totals(&env.pool, ALICE).await.is_empty());
    assert_eq!(poll_totals(&env.pool, BOB).await.len(), 1);
}
