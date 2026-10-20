mod common;

use common::PollEnv;
use serde_json::{Value, json};
use time::{Date, Duration, OffsetDateTime};

use sekondo::clock::Clock;

const FORGE: i64 = 10000002;
const JITA_44: i64 = 60003760;

/// A station book order, added to the book as it is.
async fn book_order(env: &PollEnv, id: i64, type_id: i64, buy: bool) {
    let mut book = env.stores.station.book(FORGE, JITA_44).await.unwrap();
    book.push(sekondo::store::StationOrder {
        order_id: id,
        type_id,
        is_buy_order: buy,
        price_cents: 1_000,
        volume_remain: 5,
    });
    env.stores
        .station
        .replace_station_book(
            FORGE,
            JITA_44,
            env.clock.now(),
            &book,
            env.clock.now(),
            None,
        )
        .await
        .unwrap();
}

/// A tradable item: one order of each side.
async fn tradable(env: &PollEnv, type_id: i64) {
    book_order(env, type_id * 10, type_id, true).await;
    book_order(env, type_id * 10 + 1, type_id, false).await;
}

fn day(env: &PollEnv, back: i64) -> Date {
    env.clock.now().date() - Duration::days(back)
}

fn history_day(date: Date, average: f64, volume: i64) -> Value {
    json!({"date": date.to_string(), "average": average, "highest": average + 1.0,
           "lowest": average - 1.0, "order_count": 12, "volume": volume})
}

/// The history of an item for the `days` days up to yesterday, oldest first.
async fn set_history(env: &PollEnv, type_id: i64, days: i64, average: f64) {
    let days: Vec<Value> = (1..=days)
        .rev()
        .map(|back| history_day(day(env, back), average, 1000 + back))
        .collect();
    let res = env
        .control(
            "market-history",
            json!({"region_id": FORGE, "type_id": type_id, "days": days}),
        )
        .await;
    assert_eq!(res.status(), 204);
}

async fn stored_days(env: &PollEnv, type_id: i64) -> Vec<Date> {
    env.stores
        .station
        .history_table(FORGE, type_id)
        .await
        .unwrap()
        .iter()
        .map(|h| h.day)
        .collect()
}

struct Sync {
    next_poll_at: OffsetDateTime,
    last_error: Option<String>,
    failures: i32,
}

async fn item_sync(env: &PollEnv, type_id: i64) -> Sync {
    let state = env
        .stores
        .station
        .history_sync(FORGE, type_id)
        .await
        .unwrap()
        .unwrap();
    Sync {
        next_poll_at: state.next_poll_at,
        last_error: state.last_error,
        failures: state.failures,
    }
}

/// The history run's row of the station sync.
async fn pass_state(env: &PollEnv) -> Option<sekondo::store::SyncState> {
    env.stores
        .station
        .station_sync_state(FORGE, JITA_44, "history")
        .await
        .unwrap()
}

// ---- who is fetched ----

#[tokio::test]
async fn a_new_tradable_item_gets_its_last_thirty_days() {
    let env = PollEnv::new().await;
    tradable(&env, 34).await;
    set_history(&env, 34, 40, 5.5).await;

    let fetched = env.history.tick().await;

    assert_eq!(fetched, 1);
    let days = stored_days(&env, 34).await;
    assert_eq!(days.len(), 30);
    assert_eq!(days[0], day(&env, 30), "the oldest kept day");
    assert_eq!(days[29], day(&env, 1), "yesterday");
    let row = env
        .stores
        .station
        .history_table(FORGE, 34)
        .await
        .unwrap()
        .into_iter()
        .find(|h| h.day == day(&env, 1))
        .unwrap();
    assert_eq!(
        (
            row.average,
            row.highest,
            row.lowest,
            row.volume,
            row.order_count
        ),
        ("5.50".into(), "6.50".into(), "4.50".into(), 1001, 12)
    );
}

#[tokio::test]
async fn an_item_with_only_one_side_is_not_fetched() {
    let env = PollEnv::new().await;
    book_order(&env, 1, 34, false).await;
    tradable(&env, 35).await;

    env.history.tick().await;

    assert_eq!(env.stub.history_requests(), [(FORGE, 35)]);
}

#[tokio::test]
async fn an_item_that_stops_being_tradable_is_no_longer_fetched() {
    let env = PollEnv::new().await;
    tradable(&env, 34).await;
    env.history.tick().await;
    let book: Vec<_> = env
        .stores
        .station
        .book(FORGE, JITA_44)
        .await
        .unwrap()
        .into_iter()
        .filter(|o| !(o.type_id == 34 && o.is_buy_order))
        .collect();
    env.stores
        .station
        .replace_station_book(
            FORGE,
            JITA_44,
            env.clock.now(),
            &book,
            env.clock.now(),
            None,
        )
        .await
        .unwrap();
    env.clock.advance(Duration::days(2));

    env.history.tick().await;

    assert_eq!(env.stub.history_requests().len(), 1);
}

#[tokio::test]
async fn a_fetched_item_is_not_fetched_again_before_esis_expiry() {
    let env = PollEnv::new().await;
    tradable(&env, 34).await;
    set_history(&env, 34, 5, 5.0).await;
    env.control("esi-expires", json!({"seconds": 6 * 3600}))
        .await;

    env.history.tick().await;
    env.clock.advance(Duration::hours(5));
    env.history.tick().await;
    assert_eq!(env.stub.history_requests().len(), 1);

    env.clock.advance(Duration::hours(2));
    env.history.tick().await;
    assert_eq!(env.stub.history_requests().len(), 2);
}

// ---- what is kept ----

#[tokio::test]
async fn the_pass_upserts_the_new_days_and_deletes_the_old_ones() {
    let env = PollEnv::new().await;
    tradable(&env, 34).await;
    // An old row of an item that is not even tradable any more, and a stale row of this one.
    for (type_id, back) in [(99, 45), (34, 31), (34, 1)] {
        env.stores
            .station
            .seed_history(
                FORGE,
                &[sekondo::store::SeedHistoryRow {
                    type_id,
                    day: day(&env, back),
                    average: 1.0,
                    highest: 1.0,
                    lowest: 1.0,
                    volume: 1,
                    order_count: 1,
                }],
            )
            .await
            .unwrap();
    }
    set_history(&env, 34, 3, 7.25).await;

    env.history.tick().await;

    assert_eq!(
        stored_days(&env, 99).await,
        Vec::<Date>::new(),
        "older than 30 days"
    );
    let kept = stored_days(&env, 34).await;
    assert_eq!(
        kept,
        [day(&env, 3), day(&env, 2), day(&env, 1)],
        "31 days back is gone"
    );
    let average = env
        .stores
        .station
        .history_table(FORGE, 34)
        .await
        .unwrap()
        .into_iter()
        .find(|h| h.day == day(&env, 1))
        .unwrap()
        .average;
    assert_eq!(average, "7.25", "the stale row was overwritten");
}

#[tokio::test]
async fn an_item_without_history_waits_until_the_next_day() {
    let env = PollEnv::new().await;
    tradable(&env, 36).await;

    env.history.tick().await;

    assert_eq!(stored_days(&env, 36).await, Vec::<Date>::new());
    let sync = item_sync(&env, 36).await;
    assert_eq!(sync.failures, 0);
    assert_eq!(sync.last_error, None);
    assert!(sync.next_poll_at >= env.clock.now() + Duration::hours(23));
    env.clock.advance(Duration::hours(12));
    env.history.tick().await;
    assert_eq!(env.stub.history_requests().len(), 1, "not within the day");
}

#[tokio::test]
async fn an_item_esi_does_not_know_waits_until_the_next_day_too() {
    let env = PollEnv::new().await;
    tradable(&env, 36).await;
    env.control(
        "market-fail",
        json!({"region_id": FORGE, "type_id": 36, "status": 404}),
    )
    .await;

    env.history.tick().await;

    let sync = item_sync(&env, 36).await;
    assert_eq!(sync.failures, 0);
    assert!(sync.next_poll_at >= env.clock.now() + Duration::hours(23));
}

// ---- failure ----

#[tokio::test]
async fn a_failing_item_is_retried_while_the_others_are_updated() {
    use sekondo::poller::BACKOFF_START;
    let env = PollEnv::new().await;
    tradable(&env, 34).await;
    tradable(&env, 35).await;
    set_history(&env, 34, 3, 5.0).await;
    set_history(&env, 35, 3, 6.0).await;
    env.control(
        "market-fail",
        json!({"region_id": FORGE, "type_id": 35, "status": 503}),
    )
    .await;

    env.history.tick().await;

    assert_eq!(
        stored_days(&env, 34).await.len(),
        3,
        "the other item is updated"
    );
    assert_eq!(stored_days(&env, 35).await, Vec::<Date>::new());
    let failed = item_sync(&env, 35).await;
    assert_eq!(failed.failures, 1);
    assert!(failed.last_error.unwrap().contains("503"));
    let wait = failed.next_poll_at - env.clock.now();
    assert!(
        wait >= BACKOFF_START && wait <= BACKOFF_START + Duration::minutes(1),
        "{wait}"
    );

    env.control("market-fail", json!({"clear": true})).await;
    env.clock.advance(Duration::minutes(3));
    env.history.tick().await;

    assert_eq!(stored_days(&env, 35).await.len(), 3, "retried and stored");
    assert_eq!(item_sync(&env, 35).await.failures, 0);
    let asked: Vec<i64> = env.stub.history_requests().iter().map(|r| r.1).collect();
    assert_eq!(
        asked.iter().filter(|t| **t == 34).count(),
        1,
        "34 is not asked again"
    );
}

#[tokio::test]
async fn a_rate_limited_pass_stops_and_defers_without_counting_a_failure() {
    let env = PollEnv::new().await;
    for type_id in [34, 35, 36, 37] {
        tradable(&env, type_id).await;
    }
    env.control(
        "esi-fail-next",
        json!({"route": "history", "status": 429, "headers": {"retry-after": "120"}}),
    )
    .await;

    env.history.tick().await;

    assert!(
        env.stub.history_requests().len() <= 2,
        "the gate stops the rest: {:?}",
        env.stub.history_requests()
    );
    let mut failures = 0;
    for type_id in [34, 35, 36, 37] {
        if let Some(state) = env
            .stores
            .station
            .history_sync(FORGE, type_id)
            .await
            .unwrap()
        {
            failures += state.failures;
        }
    }
    assert_eq!(failures, 0);
}

// ---- the sync state shown on the page ----

#[tokio::test]
async fn a_pass_records_when_the_history_run_finished_and_its_last_error() {
    let env = PollEnv::new().await;
    tradable(&env, 34).await;
    tradable(&env, 35).await;
    set_history(&env, 34, 3, 5.0).await;
    env.control(
        "market-fail",
        json!({"region_id": FORGE, "type_id": 35, "status": 503}),
    )
    .await;

    env.history.tick().await;

    let state = pass_state(&env).await.unwrap();
    assert_eq!(state.last_success_at, Some(env.clock.now()));
    assert!(state.last_error.unwrap().contains("503"));

    env.control("market-fail", json!({"clear": true})).await;
    env.clock.advance(Duration::minutes(3));
    env.history.tick().await;
    assert_eq!(pass_state(&env).await.unwrap().last_error, None);
}

#[tokio::test]
async fn a_tick_with_nothing_due_changes_nothing() {
    let env = PollEnv::new().await;

    assert_eq!(env.history.tick().await, 0);

    assert!(pass_state(&env).await.is_none());
}
