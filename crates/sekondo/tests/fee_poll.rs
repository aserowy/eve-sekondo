mod common;

use common::PollEnv;
use futures_util::{StreamExt, stream::BoxStream};
use sekondo::{
    clock::Clock,
    events::{Event, EventBus},
    fee_poller::Kind,
    poller::PollOutcome,
};
use serde_json::json;
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const JITA: i64 = 60003760;

async fn linked(pool: PgPool) -> PollEnv {
    let env = PollEnv::new(pool).await;
    let account = env.new_account().await;
    env.link(account, ALICE).await;
    env
}

/// Alice's order poll has run: she sells in Jita 4-4 and buys in a structure.
async fn with_orders(pool: PgPool) -> PollEnv {
    let env = linked(pool).await;
    env.poller.poll_character(ALICE).await;
    env
}

async fn account_of(env: &PollEnv, character: i64) -> uuid::Uuid {
    sqlx::query_scalar("SELECT account_id FROM characters WHERE character_id = $1")
        .bind(character)
        .fetch_one(&env.pool)
        .await
        .unwrap()
}

async fn signals(stream: &mut BoxStream<'static, Event>) -> Vec<Event> {
    let mut seen = Vec::new();
    while let Ok(Some(event)) =
        tokio::time::timeout(std::time::Duration::from_millis(100), stream.next()).await
    {
        seen.push(event);
    }
    seen
}

async fn skills(env: &PollEnv, character: i64) -> Vec<(i64, i16)> {
    sqlx::query_as(
        "SELECT skill_id, active_level FROM character_skills WHERE character_id = $1 ORDER BY skill_id",
    )
    .bind(character)
    .fetch_all(&env.pool)
    .await
    .unwrap()
}

async fn standings(env: &PollEnv, character: i64) -> Vec<(i64, String, String)> {
    sqlx::query_as(
        "SELECT from_id, from_type, standing::text FROM character_standings \
         WHERE character_id = $1 ORDER BY from_id",
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
        "SELECT last_success_at, next_poll_at, last_error, failures FROM fee_sync \
         WHERE character_id = $1 AND kind = $2",
    )
    .bind(character)
    .bind(kind)
    .fetch_one(&env.pool)
    .await
    .unwrap()
}

// ---- polling skills and standings ----

#[sqlx::test]
async fn the_active_level_is_stored_and_only_the_two_skills_that_matter(pool: PgPool) {
    let env = linked(pool).await;
    let polled_at = env.clock.now();

    let outcome = env.fees.poll(ALICE, Kind::Skills).await;

    assert_eq!(outcome, PollOutcome::Updated);
    // Accounting 16622: active 4 of trained 5. Broker Relations 3446: 5. Connections is dropped.
    assert_eq!(skills(&env, ALICE).await, [(3446, 5), (16622, 4)]);
    let (success, next, error, failures) = sync_row(&env, ALICE, "skills").await;
    assert_eq!(success, Some(polled_at));
    assert!(next >= polled_at + Duration::hours(1), "{next}");
    assert_eq!((error, failures), (None, 0));
}

#[sqlx::test]
async fn standings_drop_agents_and_keep_what_esi_reported(pool: PgPool) {
    let env = linked(pool).await;
    env.control(
        "standings",
        json!({"character_id": ALICE, "standings": [
            {"from_id": 3009841, "from_type": "agent", "standing": 3.2},
            {"from_id": 1000035, "from_type": "npc_corp", "standing": 0.07999999821186066},
            {"from_id": 500001, "from_type": "faction", "standing": -5.0},
        ]}),
    )
    .await;

    assert_eq!(
        env.fees.poll(ALICE, Kind::Standings).await,
        PollOutcome::Updated
    );

    assert_eq!(
        standings(&env, ALICE).await,
        [
            (500001, "faction".to_owned(), "-5".to_owned()),
            (
                1000035,
                "npc_corp".to_owned(),
                "0.07999999821186066".to_owned()
            ),
        ]
    );
}

#[sqlx::test]
async fn a_poll_replaces_what_was_stored(pool: PgPool) {
    let env = linked(pool).await;
    env.fees.poll(ALICE, Kind::Skills).await;
    env.control(
        "skills",
        json!({"character_id": ALICE, "skills": [
            {"skill_id": 16622, "active_skill_level": 5, "trained_skill_level": 5},
        ]}),
    )
    .await;

    env.fees.poll(ALICE, Kind::Skills).await;

    assert_eq!(
        skills(&env, ALICE).await,
        [(16622, 5)],
        "Broker Relations is gone"
    );
}

#[sqlx::test]
async fn the_next_poll_waits_for_the_hour_and_for_esis_expiry(pool: PgPool) {
    let env = linked(pool).await;
    let polled_at = env.clock.now();
    env.fees.poll(ALICE, Kind::Standings).await;
    let (_, next, ..) = sync_row(&env, ALICE, "standings").await;
    assert!(next >= polled_at + Duration::hours(1));

    env.control("esi-expires", json!({"seconds": 3 * 3600}))
        .await;
    env.fees.poll(ALICE, Kind::Standings).await;
    let (_, next, ..) = sync_row(&env, ALICE, "standings").await;
    assert!(next >= polled_at + Duration::hours(3), "{next}");
}

#[sqlx::test]
async fn the_scheduler_polls_a_new_character_within_one_hour_and_not_again_before_the_next(
    pool: PgPool,
) {
    let env = linked(pool).await;
    assert_eq!(env.fees.tick().await, 0, "first polls lie ahead");
    let scheduled: i64 = sqlx::query_scalar("SELECT count(*) FROM fee_sync")
        .fetch_one(&env.pool)
        .await
        .unwrap();
    assert_eq!(scheduled, 2, "skills and standings");

    env.clock.advance(Duration::hours(1) + Duration::minutes(1));
    assert_eq!(env.fees.tick().await, 2);
    assert_eq!(skills(&env, ALICE).await.len(), 2);
    assert_eq!(env.fees.tick().await, 0, "not due again right away");

    env.clock.advance(Duration::minutes(20));
    assert_eq!(
        env.fees.tick().await,
        0,
        "an hour has not passed since the poll"
    );
    assert_eq!(env.stub.esi_calls("skills"), 1);
    assert_eq!(env.stub.esi_calls("standings"), 1);
}

#[sqlx::test]
async fn a_character_needing_relogin_is_not_requested_and_keeps_its_values(pool: PgPool) {
    let env = linked(pool).await;
    env.fees.poll(ALICE, Kind::Skills).await;
    env.repo.mark_needs_relogin(ALICE).await.unwrap();
    let calls = env.stub.esi_calls("skills");
    let tokens = env.stub.token_calls();

    let outcome = env.fees.poll(ALICE, Kind::Skills).await;

    assert_eq!(outcome, PollOutcome::NeedsRelogin);
    assert_eq!(env.stub.esi_calls("skills"), calls);
    assert_eq!(env.stub.token_calls(), tokens);
    assert_eq!(skills(&env, ALICE).await.len(), 2);
}

#[sqlx::test]
async fn the_scheduler_skips_characters_needing_relogin(pool: PgPool) {
    let env = linked(pool).await;
    env.repo.mark_needs_relogin(ALICE).await.unwrap();
    env.fees.tick().await;
    env.clock.advance(Duration::hours(3));

    assert_eq!(env.fees.tick().await, 0);
    assert_eq!(env.stub.esi_calls("skills"), 0);
    assert_eq!(env.stub.esi_calls("standings"), 0);
}

#[sqlx::test]
async fn a_failed_poll_keeps_the_last_values_and_is_retried_later(pool: PgPool) {
    let env = linked(pool).await;
    env.fees.poll(ALICE, Kind::Standings).await;
    let before = standings(&env, ALICE).await;
    assert!(!before.is_empty());
    env.control("standings", json!({"character_id": ALICE, "standings": []}))
        .await;
    env.clock.advance(Duration::hours(1) + Duration::minutes(1));
    env.control(
        "esi-fail-next",
        json!({"route": "standings", "status": 503}),
    )
    .await;
    let failed_at = env.clock.now();

    let outcome = env.fees.poll(ALICE, Kind::Standings).await;

    assert_eq!(outcome, PollOutcome::Failed);
    assert_eq!(standings(&env, ALICE).await, before);
    let (_, next, error, failures) = sync_row(&env, ALICE, "standings").await;
    assert_eq!(failures, 1);
    assert!(error.unwrap().contains("503"));
    assert_eq!(next, failed_at + Duration::minutes(1));

    env.clock.advance(Duration::minutes(2));
    assert_eq!(env.fees.tick().await, 1, "retried");
    assert!(
        standings(&env, ALICE).await.is_empty(),
        "now the empty list is known"
    );
}

#[sqlx::test]
async fn an_esi_pause_defers_the_poll_without_counting_a_failure(pool: PgPool) {
    let env = linked(pool).await;
    env.fees.poll(ALICE, Kind::Skills).await;
    env.control(
        "esi-fail-next",
        json!({"route": "skills", "status": 429,
               "headers": {"retry-after": "120", "x-ratelimit-group": "char-detail"}}),
    )
    .await;
    env.clock.advance(Duration::hours(1) + Duration::minutes(1));
    let at = env.clock.now();

    assert_eq!(
        env.fees.poll(ALICE, Kind::Skills).await,
        PollOutcome::Deferred
    );

    let (_, next, _, failures) = sync_row(&env, ALICE, "skills").await;
    assert_eq!(failures, 0);
    assert!(next >= at + Duration::seconds(120));
}

#[sqlx::test]
async fn a_fee_failure_does_not_touch_the_order_or_the_wealth_poll_and_the_other_way_round(
    pool: PgPool,
) {
    let env = linked(pool).await;
    env.poller.poll_character(ALICE).await;
    env.wealth
        .poll(ALICE, sekondo::wealth_poller::Kind::Wallet)
        .await;
    env.fees.poll(ALICE, Kind::Skills).await;

    env.control("esi-fail-next", json!({"route": "skills", "status": 500}))
        .await;
    env.clock.advance(Duration::hours(1) + Duration::minutes(1));
    assert_eq!(
        env.fees.poll(ALICE, Kind::Skills).await,
        PollOutcome::Failed
    );
    assert_eq!(env.poller.poll_character(ALICE).await, PollOutcome::Updated);
    assert_eq!(
        env.wealth
            .poll(ALICE, sekondo::wealth_poller::Kind::Wallet)
            .await,
        PollOutcome::Updated
    );
    let failures: Vec<i32> = sqlx::query_scalar(
        "SELECT failures FROM order_sync WHERE character_id = $1 UNION ALL \
         SELECT failures FROM wealth_sync WHERE character_id = $1",
    )
    .bind(ALICE)
    .fetch_all(&env.pool)
    .await
    .unwrap();
    assert!(failures.iter().all(|f| *f == 0), "{failures:?}");

    env.control("esi-fail-next", json!({"route": "orders", "status": 500}))
        .await;
    env.clock.advance(Duration::minutes(21));
    assert_eq!(env.poller.poll_character(ALICE).await, PollOutcome::Failed);
    assert_eq!(
        env.fees.poll(ALICE, Kind::Skills).await,
        PollOutcome::Updated
    );
    let (_, _, _, skill_failures) = sync_row(&env, ALICE, "skills").await;
    assert_eq!(skill_failures, 0);
}

#[sqlx::test]
async fn a_character_unlinked_during_its_poll_is_dropped(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    // Never linked: the token service does not know it.
    assert_eq!(
        env.fees.poll(ALICE, Kind::Skills).await,
        PollOutcome::Dropped
    );
}

// ---- station owners ----

#[sqlx::test]
async fn a_station_shared_by_two_accounts_is_requested_once(pool: PgPool) {
    let env = with_orders(pool).await;
    let other = env.new_account().await;
    env.link(other, BOB).await;
    env.poller.poll_character(BOB).await;

    let learned = env.fees.resolve_stations().await;

    assert_eq!(learned, 1);
    assert_eq!(
        env.stub.station_requests(),
        [JITA],
        "once, and not the structure"
    );
    let owner: i64 =
        sqlx::query_scalar("SELECT owner_id FROM station_owners WHERE station_id = $1")
            .bind(JITA)
            .fetch_one(&env.pool)
            .await
            .unwrap();
    assert_eq!(owner, 1000035);

    assert_eq!(env.fees.resolve_stations().await, 0);
    assert_eq!(
        env.stub.station_requests(),
        [JITA],
        "known owners are not asked again"
    );
}

#[sqlx::test]
async fn a_station_without_orders_is_not_looked_up(pool: PgPool) {
    let env = linked(pool).await;

    assert_eq!(env.fees.resolve_stations().await, 0);
    assert!(env.stub.station_requests().is_empty());
}

#[sqlx::test]
async fn a_failed_station_lookup_is_retried_on_a_later_tick(pool: PgPool) {
    let env = with_orders(pool).await;
    env.control("esi-fail-next", json!({"route": "stations", "status": 503}))
        .await;

    assert_eq!(env.fees.resolve_stations().await, 0);
    assert_eq!(env.fees.resolve_stations().await, 0, "not at once again");
    assert_eq!(env.stub.esi_calls("stations"), 1);

    env.clock.advance(Duration::minutes(2));
    assert_eq!(env.fees.resolve_stations().await, 1);
    assert_eq!(env.stub.esi_calls("stations"), 2);
}

#[sqlx::test]
async fn the_scheduler_tick_resolves_station_owners(pool: PgPool) {
    let env = with_orders(pool).await;

    env.fees.tick().await;

    assert_eq!(env.stub.station_requests(), [JITA]);
}

// ---- the fees-updated signal ----

#[sqlx::test]
async fn a_poll_that_changes_a_fee_signals_the_owner(pool: PgPool) {
    let env = with_orders(pool).await;
    let mut stream = env.events.subscribe(account_of(&env, ALICE).await);

    env.fees.poll(ALICE, Kind::Skills).await;

    assert_eq!(signals(&mut stream).await, [Event::FeesUpdated]);
}

#[sqlx::test]
async fn a_poll_that_changes_nothing_signals_nobody(pool: PgPool) {
    let env = with_orders(pool).await;
    env.fees.poll(ALICE, Kind::Skills).await;
    let mut stream = env.events.subscribe(account_of(&env, ALICE).await);
    env.clock.advance(Duration::hours(1) + Duration::minutes(1));

    assert_eq!(
        env.fees.poll(ALICE, Kind::Skills).await,
        PollOutcome::Updated
    );

    assert_eq!(signals(&mut stream).await, Vec::<Event>::new());
}

#[sqlx::test]
async fn a_changed_skill_signals_again(pool: PgPool) {
    let env = with_orders(pool).await;
    env.fees.poll(ALICE, Kind::Skills).await;
    let mut stream = env.events.subscribe(account_of(&env, ALICE).await);
    env.control(
        "skills",
        json!({"character_id": ALICE, "skills": [
            {"skill_id": 16622, "active_skill_level": 5, "trained_skill_level": 5},
        ]}),
    )
    .await;
    env.clock.advance(Duration::hours(1) + Duration::minutes(1));

    env.fees.poll(ALICE, Kind::Skills).await;

    assert_eq!(signals(&mut stream).await, [Event::FeesUpdated]);
}

#[sqlx::test]
async fn a_skill_the_page_does_not_use_signals_nobody(pool: PgPool) {
    let env = with_orders(pool).await;
    env.fees.poll(ALICE, Kind::Skills).await;
    let mut stream = env.events.subscribe(account_of(&env, ALICE).await);
    env.control(
        "skills",
        json!({"character_id": ALICE, "skills": [
            {"skill_id": 16622, "active_skill_level": 4, "trained_skill_level": 5},
            {"skill_id": 3446, "active_skill_level": 5, "trained_skill_level": 5},
            {"skill_id": 3359, "active_skill_level": 5, "trained_skill_level": 5},
        ]}),
    )
    .await;
    env.clock.advance(Duration::hours(1) + Duration::minutes(1));

    env.fees.poll(ALICE, Kind::Skills).await;

    assert_eq!(signals(&mut stream).await, Vec::<Event>::new());
}

#[sqlx::test]
async fn a_standing_that_changes_a_fee_signals_and_a_signal_stays_within_the_account(pool: PgPool) {
    let env = with_orders(pool).await;
    let other = env.new_account().await;
    env.link(other, BOB).await;
    env.fees.poll(ALICE, Kind::Skills).await;
    env.fees.poll(ALICE, Kind::Standings).await;
    env.fees.resolve_stations().await;
    let mut mine = env.events.subscribe(account_of(&env, ALICE).await);
    let mut theirs = env.events.subscribe(other);
    env.control(
        "standings",
        json!({"character_id": ALICE, "standings": [
            {"from_id": 500001, "from_type": "faction", "standing": 9.0},
        ]}),
    )
    .await;
    env.clock.advance(Duration::hours(1) + Duration::minutes(1));

    env.fees.poll(ALICE, Kind::Standings).await;

    assert_eq!(signals(&mut mine).await, [Event::FeesUpdated]);
    assert_eq!(signals(&mut theirs).await, Vec::<Event>::new());
}

#[sqlx::test]
async fn a_station_owner_that_completes_a_fee_signals_only_the_affected_accounts(pool: PgPool) {
    let env = with_orders(pool).await;
    let other = env.new_account().await;
    env.link(other, BOB).await;
    env.poller.poll_character(BOB).await;
    // Alice has both polls; Bob has none, so the owner changes nothing he can see.
    env.fees.poll(ALICE, Kind::Skills).await;
    env.fees.poll(ALICE, Kind::Standings).await;
    let mut mine = env.events.subscribe(account_of(&env, ALICE).await);
    let mut theirs = env.events.subscribe(other);

    env.fees.resolve_stations().await;

    assert_eq!(signals(&mut mine).await, [Event::FeesUpdated]);
    assert_eq!(signals(&mut theirs).await, Vec::<Event>::new());
}

#[sqlx::test]
async fn a_sale_at_a_new_npc_station_gets_its_owner_looked_up_and_signals_the_account(
    pool: PgPool,
) {
    let env = linked(pool).await;
    env.fees.poll(ALICE, Kind::Skills).await;
    env.fees.poll(ALICE, Kind::Standings).await;
    // No order anywhere: the only reason to look at Jita 4-4 is Alice's sale there.
    sqlx::query(
        "INSERT INTO sales (transaction_id, character_id, type_id, location_id, quantity, \
         unit_price, sold_at) VALUES (7, $1, 34, $2, 10, 150, now())",
    )
    .bind(ALICE)
    .bind(JITA)
    .execute(&env.pool)
    .await
    .unwrap();
    let mut mine = env.events.subscribe(account_of(&env, ALICE).await);

    let learned = env.fees.resolve_stations().await;

    assert_eq!(learned, 1);
    assert_eq!(env.stub.station_requests(), [JITA]);
    assert_eq!(signals(&mut mine).await, [Event::FeesUpdated]);
}

#[sqlx::test]
async fn a_skills_poll_that_changes_the_fees_of_a_sale_signals_the_account_without_any_order(
    pool: PgPool,
) {
    let env = linked(pool).await;
    sqlx::query(
        "INSERT INTO sales (transaction_id, character_id, type_id, location_id, quantity, \
         unit_price, sold_at) VALUES (7, $1, 34, $2, 10, 150, now())",
    )
    .bind(ALICE)
    .bind(JITA)
    .execute(&env.pool)
    .await
    .unwrap();
    let mut stream = env.events.subscribe(account_of(&env, ALICE).await);

    env.fees.poll(ALICE, Kind::Skills).await;

    assert_eq!(signals(&mut stream).await, [Event::FeesUpdated]);
}

#[sqlx::test]
async fn a_skills_poll_changes_nothing_for_a_character_without_orders_or_sales(pool: PgPool) {
    let env = linked(pool).await;
    let mut stream = env.events.subscribe(account_of(&env, ALICE).await);

    env.fees.poll(ALICE, Kind::Skills).await;

    assert_eq!(signals(&mut stream).await, Vec::<Event>::new());
}

// ---- the built-in station ----

#[sqlx::test]
async fn the_owner_of_jita_44_is_looked_up_once_without_any_order_there(pool: PgPool) {
    let env = linked(pool).await;

    env.fees.tick().await;

    assert_eq!(env.stub.station_requests(), [JITA]);
    let owner: i64 =
        sqlx::query_scalar("SELECT owner_id FROM station_owners WHERE station_id = $1")
            .bind(JITA)
            .fetch_one(&env.pool)
            .await
            .unwrap();
    assert_eq!(owner, 1000035);

    env.clock.advance(Duration::hours(3));
    env.fees.tick().await;
    assert_eq!(
        env.stub.station_requests(),
        [JITA],
        "known, so not asked again"
    );
}

#[sqlx::test]
async fn an_owner_learned_from_orders_is_not_asked_for_again_as_the_built_in_station(pool: PgPool) {
    let env = with_orders(pool).await;

    env.fees.tick().await;

    assert_eq!(
        env.stub.station_requests(),
        [JITA],
        "Jita 4-4 once, and not the structure"
    );
}

#[sqlx::test]
async fn a_failed_built_in_lookup_is_retried_on_a_later_tick(pool: PgPool) {
    let env = linked(pool).await;
    env.control("esi-fail-next", json!({"route": "stations", "status": 503}))
        .await;

    env.fees.tick().await;
    env.fees.tick().await;
    assert_eq!(env.stub.esi_calls("stations"), 1, "not at once again");

    env.clock.advance(Duration::minutes(2));
    env.fees.tick().await;
    assert_eq!(env.stub.esi_calls("stations"), 2);
    let known: i64 = sqlx::query_scalar("SELECT count(*) FROM station_owners")
        .fetch_one(&env.pool)
        .await
        .unwrap();
    assert_eq!(known, 1);
}

#[sqlx::test]
async fn learning_the_built_in_owner_signals_every_account(pool: PgPool) {
    let env = linked(pool).await;
    let other = env.new_account().await;
    env.link(other, BOB).await;
    let mut alice = env.events.subscribe(account_of(&env, ALICE).await);
    let mut bob = env.events.subscribe(account_of(&env, BOB).await);

    env.fees.tick().await;

    assert!(signals(&mut alice).await.contains(&Event::FeesUpdated));
    assert!(signals(&mut bob).await.contains(&Event::FeesUpdated));
}
