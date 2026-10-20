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
use time::{Duration, OffsetDateTime};

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const JITA: i64 = 60003760;

async fn linked() -> PollEnv {
    let env = PollEnv::new().await;
    let account = env.new_account().await;
    env.link(account, ALICE).await;
    env
}

/// Alice's order poll has run: she sells in Jita 4-4 and buys in a structure.
async fn with_orders() -> PollEnv {
    let env = linked().await;
    env.poller.poll_character(ALICE).await;
    env
}

async fn account_of(env: &PollEnv, character: i64) -> uuid::Uuid {
    env.account_of(character).await
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
    env.stores.fees.skills_of(character).await.unwrap()
}

async fn standings(env: &PollEnv, character: i64) -> Vec<(i64, String, String)> {
    env.stores.fees.standings_of(character).await.unwrap()
}

fn kind_named(kind: &str) -> Kind {
    match kind {
        "skills" => Kind::Skills,
        "standings" => Kind::Standings,
        _ => Kind::Location,
    }
}

async fn sync_row(
    env: &PollEnv,
    character: i64,
    kind: &str,
) -> (Option<OffsetDateTime>, OffsetDateTime, Option<String>, i32) {
    let state = env
        .stores
        .fees
        .fee_sync(character, kind_named(kind))
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

async fn location(env: &PollEnv, character: i64) -> Option<(i64, String)> {
    env.stores.fees.location_of(character).await.unwrap()
}

/// Alice sold 10 of 34 at Jita 4-4 for 150 ISK each.
async fn alice_sold_in_jita(env: &PollEnv) {
    env.stores
        .trades
        .seed_trade(
            ALICE,
            &sekondo::store::TradeSeed {
                transaction_id: 7,
                is_sale: true,
                type_id: 34,
                location_id: JITA,
                quantity: 10,
                unit_price_cents: 15_000,
                at: OffsetDateTime::now_utc(),
            },
        )
        .await
        .unwrap();
}

async fn move_to(env: &PollEnv, character: i64, where_to: serde_json::Value) {
    env.control(
        "location",
        json!({"character_id": character, "location": where_to}),
    )
    .await;
}

// ---- polling the location ----

#[tokio::test]
async fn a_character_docked_in_a_station_is_stored_with_that_station() {
    let env = linked().await;
    let polled_at = env.clock.now();

    assert_eq!(
        env.fees.poll(ALICE, Kind::Location).await,
        PollOutcome::Updated
    );

    assert_eq!(
        location(&env, ALICE).await,
        Some((JITA, "station".to_owned()))
    );
    let (success, ..) = sync_row(&env, ALICE, "location").await;
    assert_eq!(success, Some(polled_at));
}

#[tokio::test]
async fn a_character_docked_in_a_structure_is_stored_with_that_structure() {
    let env = linked().await;
    move_to(
        &env,
        ALICE,
        json!({"solar_system_id": 30000144, "structure_id": 1000000000001u64}),
    )
    .await;

    env.fees.poll(ALICE, Kind::Location).await;

    assert_eq!(
        location(&env, ALICE).await,
        Some((1000000000001, "structure".to_owned()))
    );
}

#[tokio::test]
async fn a_character_in_space_keeps_the_last_location() {
    let env = linked().await;
    env.fees.poll(ALICE, Kind::Location).await;
    move_to(&env, ALICE, json!({"solar_system_id": 30000144})).await;

    assert_eq!(
        env.fees.poll(ALICE, Kind::Location).await,
        PollOutcome::Updated
    );

    assert_eq!(
        location(&env, ALICE).await,
        Some((JITA, "station".to_owned()))
    );
}

#[tokio::test]
async fn the_next_location_poll_waits_for_the_hour() {
    let env = linked().await;
    let polled_at = env.clock.now();

    env.fees.poll(ALICE, Kind::Location).await;

    let (_, next, ..) = sync_row(&env, ALICE, "location").await;
    assert!(next >= polled_at + Duration::hours(1), "{next}");
}

#[tokio::test]
async fn a_failed_location_poll_keeps_the_last_location() {
    let env = linked().await;
    env.fees.poll(ALICE, Kind::Location).await;
    move_to(
        &env,
        ALICE,
        json!({"solar_system_id": 30000144, "structure_id": 1000000000001u64}),
    )
    .await;
    env.clock.advance(Duration::hours(1) + Duration::minutes(1));
    env.control("esi-fail-next", json!({"route": "location", "status": 503}))
        .await;

    assert_eq!(
        env.fees.poll(ALICE, Kind::Location).await,
        PollOutcome::Failed
    );

    assert_eq!(
        location(&env, ALICE).await,
        Some((JITA, "station".to_owned()))
    );
    let (_, _, error, failures) = sync_row(&env, ALICE, "location").await;
    assert_eq!(failures, 1);
    assert!(error.unwrap().contains("503"));
}

#[tokio::test]
async fn a_character_without_the_location_scope_is_never_polled_for_it() {
    let env = PollEnv::new().await;
    let account = env.new_account().await;
    env.control(
        "scopes",
        json!({"character_id": ALICE, "scopes": sekondo::oauth::REQUIRED_SCOPES}),
    )
    .await;
    env.link(account, ALICE).await;

    assert_eq!(
        env.fees.poll(ALICE, Kind::Location).await,
        PollOutcome::Skipped
    );
    env.fees.tick().await;
    env.clock.advance(Duration::hours(3));
    env.fees.tick().await;

    assert_eq!(env.stub.esi_calls("location"), 0);
    assert_eq!(location(&env, ALICE).await, None);
    let rows = env
        .stores
        .fees
        .fee_sync(ALICE, Kind::Location)
        .await
        .unwrap()
        .into_iter()
        .count();
    assert_eq!(rows, 0, "not even scheduled");
}

// ---- polling skills and standings ----

#[tokio::test]
async fn the_active_level_is_stored_and_only_the_two_skills_that_matter() {
    let env = linked().await;
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

#[tokio::test]
async fn standings_drop_agents_and_keep_what_esi_reported() {
    let env = linked().await;
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

#[tokio::test]
async fn a_poll_replaces_what_was_stored() {
    let env = linked().await;
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

#[tokio::test]
async fn the_next_poll_waits_for_the_hour_and_for_esis_expiry() {
    let env = linked().await;
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

#[tokio::test]
async fn the_scheduler_polls_a_new_character_within_one_hour_and_not_again_before_the_next() {
    let env = linked().await;
    assert_eq!(env.fees.tick().await, 0, "first polls lie ahead");
    let mut scheduled = 0;
    for kind in [Kind::Skills, Kind::Standings, Kind::Location] {
        scheduled += env
            .stores
            .fees
            .fee_sync(ALICE, kind)
            .await
            .unwrap()
            .into_iter()
            .count();
    }
    assert_eq!(scheduled, 3, "skills, standings and location");

    env.clock.advance(Duration::hours(1) + Duration::minutes(1));
    assert_eq!(env.fees.tick().await, 3);
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
    assert_eq!(env.stub.esi_calls("location"), 1);
}

#[tokio::test]
async fn a_character_needing_relogin_is_not_requested_and_keeps_its_values() {
    let env = linked().await;
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

#[tokio::test]
async fn the_scheduler_skips_characters_needing_relogin() {
    let env = linked().await;
    env.repo.mark_needs_relogin(ALICE).await.unwrap();
    env.fees.tick().await;
    env.clock.advance(Duration::hours(3));

    assert_eq!(env.fees.tick().await, 0);
    assert_eq!(env.stub.esi_calls("skills"), 0);
    assert_eq!(env.stub.esi_calls("standings"), 0);
}

#[tokio::test]
async fn a_failed_poll_keeps_the_last_values_and_is_retried_later() {
    let env = linked().await;
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

#[tokio::test]
async fn an_esi_pause_defers_the_poll_without_counting_a_failure() {
    let env = linked().await;
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

#[tokio::test]
async fn a_fee_failure_does_not_touch_the_order_or_the_wealth_poll_and_the_other_way_round() {
    let env = linked().await;
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
    let mut failures = vec![
        env.stores
            .orders
            .order_sync(ALICE)
            .await
            .unwrap()
            .unwrap()
            .failures,
    ];
    for kind in [
        sekondo::wealth_poller::Kind::Wallet,
        sekondo::wealth_poller::Kind::Assets,
        sekondo::wealth_poller::Kind::Transactions,
    ] {
        if let Some(state) = env.stores.wealth.wealth_sync(ALICE, kind).await.unwrap() {
            failures.push(state.failures);
        }
    }
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

#[tokio::test]
async fn a_character_unlinked_during_its_poll_is_dropped() {
    let env = PollEnv::new().await;
    // Never linked: the token service does not know it.
    assert_eq!(
        env.fees.poll(ALICE, Kind::Skills).await,
        PollOutcome::Dropped
    );
}

// ---- station owners ----

#[tokio::test]
async fn a_station_shared_by_two_accounts_is_requested_once() {
    let env = with_orders().await;
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
    let owner = env.stores.fees.station_owner(JITA).await.unwrap().unwrap();
    assert_eq!(owner, 1000035);

    assert_eq!(env.fees.resolve_stations().await, 0);
    assert_eq!(
        env.stub.station_requests(),
        [JITA],
        "known owners are not asked again"
    );
}

#[tokio::test]
async fn a_station_without_orders_is_not_looked_up() {
    let env = linked().await;

    assert_eq!(env.fees.resolve_stations().await, 0);
    assert!(env.stub.station_requests().is_empty());
}

#[tokio::test]
async fn a_failed_station_lookup_is_retried_on_a_later_tick() {
    let env = with_orders().await;
    env.control("esi-fail-next", json!({"route": "stations", "status": 503}))
        .await;

    assert_eq!(env.fees.resolve_stations().await, 0);
    assert_eq!(env.fees.resolve_stations().await, 0, "not at once again");
    assert_eq!(env.stub.esi_calls("stations"), 1);

    env.clock.advance(Duration::minutes(2));
    assert_eq!(env.fees.resolve_stations().await, 1);
    assert_eq!(env.stub.esi_calls("stations"), 2);
}

#[tokio::test]
async fn the_scheduler_tick_resolves_station_owners() {
    let env = with_orders().await;

    env.fees.tick().await;

    assert_eq!(env.stub.station_requests(), [JITA]);
}

// ---- the fees-updated signal ----

#[tokio::test]
async fn a_poll_that_changes_a_fee_signals_the_owner() {
    let env = with_orders().await;
    let mut stream = env.events.subscribe(account_of(&env, ALICE).await);

    env.fees.poll(ALICE, Kind::Skills).await;

    assert_eq!(signals(&mut stream).await, [Event::FeesUpdated]);
}

#[tokio::test]
async fn a_poll_that_changes_nothing_signals_nobody() {
    let env = with_orders().await;
    env.fees.poll(ALICE, Kind::Skills).await;
    let mut stream = env.events.subscribe(account_of(&env, ALICE).await);
    env.clock.advance(Duration::hours(1) + Duration::minutes(1));

    assert_eq!(
        env.fees.poll(ALICE, Kind::Skills).await,
        PollOutcome::Updated
    );

    assert_eq!(signals(&mut stream).await, Vec::<Event>::new());
}

#[tokio::test]
async fn a_changed_skill_signals_again() {
    let env = with_orders().await;
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

#[tokio::test]
async fn a_skill_the_page_does_not_use_signals_nobody() {
    let env = with_orders().await;
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

#[tokio::test]
async fn a_standing_that_changes_a_fee_signals_and_a_signal_stays_within_the_account() {
    let env = with_orders().await;
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

#[tokio::test]
async fn a_station_owner_that_completes_a_fee_signals_only_the_affected_accounts() {
    let env = with_orders().await;
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

#[tokio::test]
async fn a_sale_at_a_new_npc_station_gets_its_owner_looked_up_and_signals_the_account() {
    let env = linked().await;
    env.fees.poll(ALICE, Kind::Skills).await;
    env.fees.poll(ALICE, Kind::Standings).await;
    // No order anywhere: the only reason to look at Jita 4-4 is Alice's sale there.
    alice_sold_in_jita(&env).await;
    let mut mine = env.events.subscribe(account_of(&env, ALICE).await);

    let learned = env.fees.resolve_stations().await;

    assert_eq!(learned, 1);
    assert_eq!(env.stub.station_requests(), [JITA]);
    assert_eq!(signals(&mut mine).await, [Event::FeesUpdated]);
}

#[tokio::test]
async fn a_skills_poll_that_changes_the_fees_of_a_sale_signals_the_account_without_any_order() {
    let env = linked().await;
    alice_sold_in_jita(&env).await;
    let mut stream = env.events.subscribe(account_of(&env, ALICE).await);

    env.fees.poll(ALICE, Kind::Skills).await;

    assert_eq!(signals(&mut stream).await, [Event::FeesUpdated]);
}

#[tokio::test]
async fn a_skills_poll_changes_the_fee_card_of_a_character_without_orders_or_sales() {
    let env = linked().await;
    let mut stream = env.events.subscribe(account_of(&env, ALICE).await);

    env.fees.poll(ALICE, Kind::Skills).await;

    // Every character has a fee card at its fee location (Jita 4-4 here), orders or not.
    assert_eq!(signals(&mut stream).await, [Event::FeesUpdated]);
}

// ---- the built-in station ----

#[tokio::test]
async fn the_owner_of_jita_44_is_looked_up_once_without_any_order_there() {
    let env = linked().await;

    env.fees.tick().await;

    assert_eq!(env.stub.station_requests(), [JITA]);
    let owner = env.stores.fees.station_owner(JITA).await.unwrap().unwrap();
    assert_eq!(owner, 1000035);

    env.clock.advance(Duration::hours(3));
    env.fees.tick().await;
    assert_eq!(
        env.stub.station_requests(),
        [JITA],
        "known, so not asked again"
    );
}

#[tokio::test]
async fn an_owner_learned_from_orders_is_not_asked_for_again_as_the_built_in_station() {
    let env = with_orders().await;

    env.fees.tick().await;

    assert_eq!(
        env.stub.station_requests(),
        [JITA],
        "Jita 4-4 once, and not the structure"
    );
}

#[tokio::test]
async fn a_failed_built_in_lookup_is_retried_on_a_later_tick() {
    let env = linked().await;
    env.control("esi-fail-next", json!({"route": "stations", "status": 503}))
        .await;

    env.fees.tick().await;
    env.fees.tick().await;
    assert_eq!(env.stub.esi_calls("stations"), 1, "not at once again");

    env.clock.advance(Duration::minutes(2));
    env.fees.tick().await;
    assert_eq!(env.stub.esi_calls("stations"), 2);
    let known = env.stores.fees.station_owner_count().await.unwrap();
    assert_eq!(known, 1);
}

#[tokio::test]
async fn learning_the_built_in_owner_signals_every_account() {
    let env = linked().await;
    let other = env.new_account().await;
    env.link(other, BOB).await;
    let mut alice = env.events.subscribe(account_of(&env, ALICE).await);
    let mut bob = env.events.subscribe(account_of(&env, BOB).await);

    env.fees.tick().await;

    assert!(signals(&mut alice).await.contains(&Event::FeesUpdated));
    assert!(signals(&mut bob).await.contains(&Event::FeesUpdated));
}

// ---- the fee location ----

const AMARR: i64 = 60008494;

/// Alice with skills and standings polled, docked in Jita 4-4, and both owners known.
async fn alice_docked_in_jita() -> PollEnv {
    let env = linked().await;
    env.fees.poll(ALICE, Kind::Skills).await;
    env.fees.poll(ALICE, Kind::Standings).await;
    env.fees.poll(ALICE, Kind::Location).await;
    env.fees.resolve_stations().await;
    env.fees.tick().await;
    assert!(env.stub.station_requests().contains(&JITA));
    env
}

#[tokio::test]
async fn moving_to_a_station_with_other_fees_signals_the_account() {
    let env = alice_docked_in_jita().await;
    // Amarr's owner is known already, so the move changes the fees right away.
    env.stores
        .fees
        .insert_station_owner(AMARR, 1000086, OffsetDateTime::now_utc())
        .await
        .unwrap();
    let mut stream = env.events.subscribe(account_of(&env, ALICE).await);
    move_to(
        &env,
        ALICE,
        json!({"solar_system_id": 30002187, "station_id": AMARR}),
    )
    .await;
    env.clock.advance(Duration::hours(1) + Duration::minutes(1));

    env.fees.poll(ALICE, Kind::Location).await;

    assert_eq!(signals(&mut stream).await, [Event::FeesUpdated]);
    assert_eq!(
        location(&env, ALICE).await,
        Some((AMARR, "station".to_owned()))
    );
}

#[tokio::test]
async fn the_same_station_signals_nobody() {
    let env = alice_docked_in_jita().await;
    let mut stream = env.events.subscribe(account_of(&env, ALICE).await);
    env.clock.advance(Duration::hours(1) + Duration::minutes(1));

    env.fees.poll(ALICE, Kind::Location).await;

    assert_eq!(signals(&mut stream).await, Vec::<Event>::new());
}

#[tokio::test]
async fn the_owner_of_a_docked_station_is_looked_up_and_completes_the_fee() {
    let env = alice_docked_in_jita().await;
    move_to(
        &env,
        ALICE,
        json!({"solar_system_id": 30002187, "station_id": AMARR}),
    )
    .await;
    env.clock.advance(Duration::hours(1) + Duration::minutes(1));
    env.fees.poll(ALICE, Kind::Location).await;
    let mut stream = env.events.subscribe(account_of(&env, ALICE).await);

    assert_eq!(env.fees.resolve_stations().await, 1);

    assert!(env.stub.station_requests().contains(&AMARR));
    assert_eq!(
        signals(&mut stream).await,
        [Event::FeesUpdated],
        "the broker fee at the docked station is known now"
    );
}

#[tokio::test]
async fn a_docked_station_and_a_docked_structure_are_named_like_order_locations() {
    let env = linked().await;
    let name = |id: i64| {
        let market = env.stores.market.clone();
        async move { market.name_of(id).await.unwrap() }
    };

    move_to(
        &env,
        ALICE,
        json!({"solar_system_id": 30002187, "station_id": AMARR}),
    )
    .await;
    env.fees.poll(ALICE, Kind::Location).await;
    assert_eq!(
        name(AMARR).await.as_deref(),
        Some("Amarr VIII (Oris) - Emperor Family Academy")
    );

    env.clock.advance(Duration::hours(1) + Duration::minutes(1));
    move_to(
        &env,
        ALICE,
        json!({"solar_system_id": 30000144, "structure_id": 1000000000001u64}),
    )
    .await;
    env.fees.poll(ALICE, Kind::Location).await;
    assert_eq!(name(1000000000001).await.as_deref(), Some("Alpha Keepstar"));
}
