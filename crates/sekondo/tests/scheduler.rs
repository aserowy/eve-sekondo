mod common;

use common::PollEnv;
use serde_json::json;
use time::{Duration, OffsetDateTime};

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const CAROL: i64 = 90000003;

async fn linked(interval: Duration, characters: &[i64]) -> PollEnv {
    let env = PollEnv::with_interval(interval).await;
    let account = env.new_account().await;
    for id in characters {
        env.link(account, *id).await;
    }
    env
}

async fn next_poll_at(env: &PollEnv, id: i64) -> OffsetDateTime {
    env.stores
        .orders
        .order_sync(id)
        .await
        .unwrap()
        .unwrap()
        .next_poll_at
}

#[tokio::test]
async fn a_new_character_is_scheduled_within_the_interval_not_polled_at_once() {
    use sekondo::clock::Clock;
    let env = linked(Duration::minutes(20), &[ALICE]).await;
    let linked_at = env.clock.now();

    env.poller.tick().await;

    let next = next_poll_at(&env, ALICE).await;
    assert!(
        next >= linked_at && next < linked_at + Duration::minutes(20),
        "{next}"
    );
    assert_eq!(env.stub.esi_calls("orders"), 0);

    env.clock.advance(Duration::minutes(20));
    env.poller.tick().await;
    assert_eq!(env.stub.esi_calls("orders"), 1);
}

#[tokio::test]
async fn the_next_poll_waits_for_the_interval() {
    let env = linked(Duration::minutes(20), &[ALICE]).await;
    // ESI would allow a poll after 5 minutes.
    env.control("esi-expires", json!({"seconds": 300})).await;
    env.poller.tick().await;
    env.clock.advance(Duration::minutes(20));
    env.poller.tick().await;
    assert_eq!(env.stub.esi_calls("orders"), 1);

    env.clock.advance(Duration::minutes(19));
    env.poller.tick().await;
    assert_eq!(env.stub.esi_calls("orders"), 1, "19 minutes after the poll");

    // 20 minutes plus the most jitter there is.
    env.clock
        .advance(Duration::minutes(1) + Duration::seconds(30));
    env.poller.tick().await;
    assert_eq!(env.stub.esi_calls("orders"), 2);
}

#[tokio::test]
async fn the_next_poll_waits_for_esis_cache_expiry() {
    let env = linked(Duration::minutes(5), &[ALICE]).await;
    env.control("esi-expires", json!({"seconds": 1200})).await;
    env.poller.tick().await;
    env.clock.advance(Duration::minutes(5));
    env.poller.tick().await;
    assert_eq!(env.stub.esi_calls("orders"), 1);

    env.clock.advance(Duration::minutes(10));
    env.poller.tick().await;
    assert_eq!(
        env.stub.esi_calls("orders"),
        1,
        "after the interval, before Expires"
    );

    env.clock
        .advance(Duration::minutes(10) + Duration::seconds(31));
    env.poller.tick().await;
    assert_eq!(env.stub.esi_calls("orders"), 2);
}

#[tokio::test]
async fn characters_linked_together_are_scheduled_at_different_times() {
    let env = linked(Duration::minutes(20), &[ALICE, BOB, CAROL]).await;

    env.poller.tick().await;

    let mut seconds = Vec::new();
    for id in [ALICE, BOB, CAROL] {
        seconds.push(next_poll_at(&env, id).await.unix_timestamp());
    }
    seconds.sort_unstable();
    seconds.dedup();
    assert_eq!(seconds.len(), 3, "all three scheduled in different seconds");
}

#[tokio::test]
async fn a_character_needing_relogin_is_skipped_and_picked_up_again_after_login() {
    let env = linked(Duration::minutes(20), &[ALICE]).await;
    env.poller.tick().await;
    env.clock.advance(Duration::minutes(20));
    env.poller.tick().await;
    assert_eq!(env.stub.esi_calls("orders"), 1);

    env.repo.mark_needs_relogin(ALICE).await.unwrap();
    let tokens = env.stub.token_calls();
    env.clock.advance(Duration::minutes(40));
    env.poller.tick().await;
    assert_eq!(env.stub.esi_calls("orders"), 1);
    assert_eq!(env.stub.token_calls(), tokens, "no SSO call either");

    // Logging in again clears the flag; the overdue character is polled on the next tick.
    let account = env
        .repo
        .find_character(ALICE)
        .await
        .unwrap()
        .unwrap()
        .account_id;
    env.link(account, ALICE).await;
    env.poller.tick().await;
    assert_eq!(env.stub.esi_calls("orders"), 2);
}

#[tokio::test]
async fn a_character_that_needs_relogin_gets_no_schedule() {
    let env = linked(Duration::minutes(20), &[ALICE]).await;
    env.repo.mark_needs_relogin(ALICE).await.unwrap();

    env.poller.tick().await;

    assert!(env.stores.orders.order_sync(ALICE).await.unwrap().is_none());
}

#[tokio::test]
async fn a_character_is_not_polled_twice_while_its_poll_is_running() {
    let env = linked(Duration::minutes(20), &[ALICE]).await;
    env.poller.tick().await;
    env.clock.advance(Duration::minutes(20));
    env.control("esi-delay", json!({"millis": 300})).await;

    let (first, second) = tokio::join!(env.poller.tick(), async {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        env.poller.tick().await
    });

    assert_eq!((first, second), (1, 0));
    assert_eq!(env.stub.esi_calls("orders"), 1);
}
