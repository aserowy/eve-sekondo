mod common;

use common::PollEnv;
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const FORGE: i64 = 10000002;
const STATION: i64 = 60003760;
const SYSTEM: i64 = 30000142;

fn own_order(id: i64, type_id: i64, region: i64) -> Value {
    json!({
        "order_id": id, "type_id": type_id, "region_id": region, "location_id": STATION,
        "is_buy_order": false, "is_corporation": false, "price": 10.0,
        "volume_remain": 5, "volume_total": 20, "min_volume": 1, "range": "station",
        "issued": "2026-10-01T12:00:00Z", "duration": 90
    })
}

fn market_order(id: i64, type_id: i64, price: f64) -> Value {
    json!({
        "order_id": id, "type_id": type_id, "location_id": STATION, "system_id": SYSTEM,
        "is_buy_order": false, "price": price, "range": "station",
        "duration": 90, "issued": "2026-10-11T12:00:00Z", "min_volume": 1,
        "volume_remain": 3, "volume_total": 5,
    })
}

async fn ok(res: reqwest::Response) {
    assert_eq!(res.status(), 204);
}

async fn set_orders(env: &PollEnv, character: i64, orders: Vec<Value>) {
    ok(env
        .control(
            "orders",
            json!({"character_id": character, "orders": orders}),
        )
        .await)
    .await;
}

async fn set_market(env: &PollEnv, region: i64, orders: Vec<Value>, page_size: Option<usize>) {
    let mut body = json!({"region_id": region, "orders": orders});
    if let Some(size) = page_size {
        body["page_size"] = json!(size);
    }
    ok(env.control("market-orders", body).await).await;
}

/// Alice has a sell order for item 34 in The Forge, polled and stored.
async fn alice_with_an_order() -> PollEnv {
    let env = PollEnv::new().await;
    let account = env.new_account().await;
    env.link(account, ALICE).await;
    set_orders(&env, ALICE, vec![own_order(1, 34, FORGE)]).await;
    env.poller.poll_character(ALICE).await;
    // ESI's market answers are cached for 5 minutes, as the real ones are.
    ok(env.control("esi-expires", json!({"seconds": 300})).await).await;
    env
}

async fn pairs(env: &PollEnv) -> Vec<(i64, i64)> {
    env.stores.market.tracked_pairs().await.unwrap()
}

async fn market_ids(env: &PollEnv) -> Vec<i64> {
    env.stores
        .market
        .public_orders()
        .await
        .unwrap()
        .iter()
        .map(|o| o.order_id)
        .collect()
}

async fn market_volumes(env: &PollEnv) -> Vec<(i64, i64)> {
    env.stores
        .market
        .public_orders()
        .await
        .unwrap()
        .iter()
        .map(|o| (o.order_id, o.volume_remain))
        .collect()
}

async fn sync_of(env: &PollEnv, region: i64, type_id: i64) -> sekondo::store::SyncState {
    env.stores
        .market
        .pair_sync(region, type_id)
        .await
        .unwrap()
        .unwrap()
}

async fn next_poll_at(env: &PollEnv, region: i64, type_id: i64) -> OffsetDateTime {
    sync_of(env, region, type_id).await.next_poll_at
}

/// The account of the one linked character.
async fn the_account(env: &PollEnv) -> uuid::Uuid {
    env.account_of(ALICE).await
}

/// Tracks the pairs, moves on to when they are due and refreshes them.
async fn refresh_all(env: &PollEnv) {
    env.market.tick().await;
    env.clock.advance(Duration::minutes(5));
    env.market.tick().await;
}

// ---- tracking ----

#[tokio::test]
async fn a_pair_is_tracked_after_an_order_poll_and_refreshed_within_one_interval() {
    use sekondo::clock::Clock;
    let env = alice_with_an_order().await;
    let tracked_at = env.clock.now();
    set_market(&env, FORGE, vec![market_order(100, 34, 9.0)], None).await;

    env.market.tick().await;

    assert_eq!(pairs(&env).await, [(FORGE, 34)]);
    let next = next_poll_at(&env, FORGE, 34).await;
    assert!(
        next >= tracked_at && next < tracked_at + Duration::minutes(5),
        "{next}"
    );
    assert_eq!(env.stub.esi_calls("market"), 0, "spread, not at once");

    env.clock.advance(Duration::minutes(5));
    env.market.tick().await;

    assert_eq!(env.stub.esi_calls("market"), 1);
    assert_eq!(market_ids(&env).await, [100]);
}

#[tokio::test]
async fn stored_volumes_follow_a_refresh() {
    let env = alice_with_an_order().await;
    let mut order = market_order(100, 34, 9.0);
    order["volume_remain"] = json!(20);
    set_market(&env, FORGE, vec![order.clone()], None).await;
    refresh_all(&env).await;
    assert_eq!(market_volumes(&env).await, [(100, 20)]);

    order["volume_remain"] = json!(8);
    set_market(&env, FORGE, vec![order], None).await;
    env.clock.advance(Duration::minutes(21));
    env.market.tick().await;

    assert_eq!(market_volumes(&env).await, [(100, 8)]);
}

#[tokio::test]
async fn two_accounts_share_a_pair_and_it_is_requested_once_per_refresh() {
    let env = alice_with_an_order().await;
    let other = env.new_account().await;
    env.link(other, BOB).await;
    set_orders(&env, BOB, vec![own_order(2, 34, FORGE)]).await;
    env.poller.poll_character(BOB).await;
    set_market(&env, FORGE, vec![market_order(100, 34, 9.0)], None).await;

    refresh_all(&env).await;

    assert_eq!(pairs(&env).await, [(FORGE, 34)]);
    assert_eq!(env.stub.esi_calls("market"), 1);
}

#[tokio::test]
async fn a_pair_stops_being_used_and_its_copy_is_removed() {
    let env = alice_with_an_order().await;
    set_market(&env, FORGE, vec![market_order(100, 34, 9.0)], None).await;
    refresh_all(&env).await;
    assert_eq!(market_ids(&env).await, [100]);

    set_orders(&env, ALICE, vec![]).await;
    env.poller.poll_character(ALICE).await;
    env.market.tick().await;

    assert_eq!(pairs(&env).await, Vec::<(i64, i64)>::new());
    assert_eq!(market_ids(&env).await, Vec::<i64>::new());
    let calls = env.stub.esi_calls("market");
    env.clock.advance(Duration::minutes(30));
    env.market.tick().await;
    assert_eq!(env.stub.esi_calls("market"), calls, "no longer refreshed");
}

// ---- a successful refresh ----

#[tokio::test]
async fn a_three_page_answer_is_stored_whole_and_replaces_the_old_copy() {
    let env = alice_with_an_order().await;
    let orders: Vec<Value> = (100..105).map(|id| market_order(id, 34, 9.0)).collect();
    set_market(&env, FORGE, orders, Some(2)).await;

    refresh_all(&env).await;

    assert_eq!(market_ids(&env).await, [100, 101, 102, 103, 104]);
    assert_eq!(env.stub.esi_calls("market"), 3);

    set_market(&env, FORGE, vec![market_order(103, 34, 8.0)], None).await;
    env.clock.advance(Duration::minutes(6));
    env.market.tick().await;
    assert_eq!(market_ids(&env).await, [103]);
    let cents = env.stores.market.public_orders().await.unwrap()[0].price_cents;
    assert_eq!(cents, 800);
}

#[tokio::test]
async fn a_refresh_waits_for_the_cache_of_esi() {
    use sekondo::clock::Clock;
    let env = alice_with_an_order().await;
    ok(env.control("esi-expires", json!({"seconds": 480})).await).await;
    env.market.tick().await;
    env.clock.advance(Duration::minutes(5));
    env.market.tick().await;
    let refreshed_at = env.clock.now();
    assert_eq!(env.stub.esi_calls("market"), 1);

    // ESI's Expires is 8 minutes ahead, later than the 5 minute interval, plus a little jitter.
    let next = next_poll_at(&env, FORGE, 34).await;
    assert!(
        next >= refreshed_at + Duration::minutes(8)
            && next <= refreshed_at + Duration::minutes(8) + Duration::seconds(30),
        "{next}"
    );
    env.clock.advance(Duration::minutes(7));
    env.market.tick().await;
    assert_eq!(env.stub.esi_calls("market"), 1, "still cached at ESI");

    env.clock.advance(Duration::minutes(2));
    env.market.tick().await;
    assert_eq!(env.stub.esi_calls("market"), 2);
}

#[tokio::test]
async fn a_short_cache_still_waits_for_the_interval() {
    use sekondo::clock::Clock;
    let env = alice_with_an_order().await;
    ok(env.control("esi-expires", json!({"seconds": 60})).await).await;
    refresh_all(&env).await;
    let refreshed_at = env.clock.now();

    let next = next_poll_at(&env, FORGE, 34).await;
    assert!(next >= refreshed_at + Duration::minutes(5), "{next}");
}

// ---- failures ----

#[tokio::test]
async fn a_failed_refresh_keeps_the_old_copy_and_is_retried_after_the_backoff() {
    use sekondo::clock::Clock;
    let env = alice_with_an_order().await;
    set_market(&env, FORGE, vec![market_order(100, 34, 9.0)], None).await;
    refresh_all(&env).await;
    assert_eq!(market_ids(&env).await, [100]);

    set_market(&env, FORGE, vec![market_order(200, 34, 9.0)], None).await;
    ok(env
        .control("esi-fail-next", json!({"route": "market", "status": 503}))
        .await)
    .await;
    env.clock.advance(Duration::minutes(6));
    let failed_at = env.clock.now();
    env.market.tick().await;

    assert_eq!(market_ids(&env).await, [100], "the old copy is kept");
    let state = sync_of(&env, FORGE, 34).await;
    assert_eq!(state.failures, 1);
    assert!(state.last_error.is_some());
    assert_eq!(
        next_poll_at(&env, FORGE, 34).await,
        failed_at + Duration::minutes(1)
    );

    let calls = env.stub.esi_calls("market");
    env.clock.advance(Duration::seconds(59));
    env.market.tick().await;
    assert_eq!(
        env.stub.esi_calls("market"),
        calls,
        "not before the backoff"
    );
    env.clock.advance(Duration::seconds(2));
    env.market.tick().await;
    assert_eq!(market_ids(&env).await, [200]);
    assert_eq!(
        sync_of(&env, FORGE, 34).await.failures,
        0,
        "a success resets the count"
    );
}

#[tokio::test]
async fn consecutive_failures_back_off_exponentially() {
    use sekondo::clock::Clock;
    let env = alice_with_an_order().await;
    env.market.tick().await;
    ok(env
        .control(
            "esi-fail-next",
            json!({"route": "market", "status": 503, "count": 2}),
        )
        .await)
    .await;
    env.clock.advance(Duration::minutes(5));
    env.market.tick().await;
    env.clock.advance(Duration::minutes(1));
    let second_failure_at = env.clock.now();
    env.market.tick().await;

    assert_eq!(
        next_poll_at(&env, FORGE, 34).await,
        second_failure_at + Duration::minutes(2)
    );
}

#[tokio::test]
async fn a_deferred_answer_reschedules_without_counting_a_failure() {
    use sekondo::clock::Clock;
    let env = alice_with_an_order().await;
    env.market.tick().await;
    ok(env
        .control(
            "esi-fail-next",
            json!({"route": "market", "status": 429,
                   "headers": {"retry-after": "120", "x-ratelimit-group": "market-order"}}),
        )
        .await)
    .await;
    env.clock.advance(Duration::minutes(5));
    let limited_at = env.clock.now();
    env.market.tick().await;

    assert_eq!(sync_of(&env, FORGE, 34).await.failures, 0);
    let next = next_poll_at(&env, FORGE, 34).await;
    assert!(
        next >= limited_at + Duration::seconds(120) && next <= limited_at + Duration::seconds(150),
        "{next}"
    );
    // While the gate pauses the route, nothing is requested.
    let calls = env.stub.esi_calls("market");
    env.clock.advance(Duration::seconds(60));
    env.market.tick().await;
    assert_eq!(env.stub.esi_calls("market"), calls);
}

// ---- standing ----

const OTHER_STATION: i64 = 60008494;

fn own(id: i64, type_id: i64, buy: bool, price: f64, location: i64) -> Value {
    let mut order = own_order(id, type_id, FORGE);
    order["is_buy_order"] = json!(buy);
    order["price"] = json!(price);
    order["location_id"] = json!(location);
    order
}

fn public(id: i64, buy: bool, price: f64, location: i64, system: i64, range: &str) -> Value {
    let mut order = market_order(id, 34, price);
    order["is_buy_order"] = json!(buy);
    order["location_id"] = json!(location);
    order["system_id"] = json!(system);
    order["range"] = json!(range);
    order
}

fn sell_at(id: i64, price: f64) -> Value {
    public(id, false, price, STATION, SYSTEM, "station")
}

struct Standing {
    undercut: Option<i64>,
    best_cents: Option<i64>,
    gap_cents: Option<i64>,
    undercut_volume: Option<i64>,
}

/// Alice's orders and the market of The Forge are in place and the pair was refreshed.
async fn market_with(alices: Vec<Value>, market: Vec<Value>) -> (PollEnv, uuid::Uuid) {
    let env = PollEnv::new().await;
    let account = env.new_account().await;
    env.link(account, ALICE).await;
    set_orders(&env, ALICE, alices).await;
    env.poller.poll_character(ALICE).await;
    set_market(&env, FORGE, market, None).await;
    refresh_all(&env).await;
    (env, account)
}

async fn standings(
    env: &PollEnv,
    account: uuid::Uuid,
    side: sekondo::orders::Side,
) -> Vec<Standing> {
    use sekondo::{orders::Sort, selection::CharacterSelection};
    let selection = CharacterSelection::resolve(&[ALICE, BOB], None);
    env.stores
        .orders
        .list_orders(account, selection.ids(), side, Sort::default())
        .await
        .unwrap()
        .into_iter()
        .map(|r| Standing {
            undercut: r.undercut,
            best_cents: r.best_price_cents,
            gap_cents: r.gap_cents,
            undercut_volume: r.undercut_volume,
        })
        .collect()
}

async fn sell_standing(env: &PollEnv, account: uuid::Uuid) -> Standing {
    standings(env, account, sekondo::orders::Side::Sell)
        .await
        .remove(0)
}

#[tokio::test]
async fn the_lowest_sell_price_at_its_station_is_top() {
    let (env, account) = market_with(
        vec![own(1, 34, false, 100.0, STATION)],
        vec![
            sell_at(1, 100.0),
            sell_at(2, 101.0),
            public(3, false, 99.0, OTHER_STATION, SYSTEM, "station"),
        ],
    )
    .await;

    let standing = sell_standing(&env, account).await;

    assert_eq!(standing.undercut, Some(0));
    assert_eq!(standing.best_cents, None);
}

#[tokio::test]
async fn a_sell_order_is_undercut_by_cheaper_orders_at_its_station_only() {
    let (env, account) = market_with(
        vec![own(1, 34, false, 100.0, STATION)],
        vec![
            sell_at(1, 100.0),
            sell_at(2, 99.0),
            sell_at(3, 95.0),
            public(4, false, 98.0, OTHER_STATION, SYSTEM, "station"),
        ],
    )
    .await;

    let standing = sell_standing(&env, account).await;

    assert_eq!(standing.undercut, Some(2));
    assert_eq!(standing.best_cents, Some(9500));
}

#[tokio::test]
async fn a_tie_for_the_lowest_sell_price_is_top_for_both() {
    let (env, account) = market_with(
        vec![own(1, 34, false, 100.0, STATION)],
        vec![sell_at(1, 100.0), sell_at(2, 100.0)],
    )
    .await;

    assert_eq!(sell_standing(&env, account).await.undercut, Some(0));
}

#[tokio::test]
async fn a_cheaper_order_of_another_linked_character_counts() {
    let env = PollEnv::new().await;
    let account = env.new_account().await;
    env.link(account, ALICE).await;
    env.link(account, BOB).await;
    set_orders(&env, ALICE, vec![own(1, 34, false, 100.0, STATION)]).await;
    set_orders(&env, BOB, vec![own(2, 34, false, 90.0, STATION)]).await;
    env.poller.poll_character(ALICE).await;
    env.poller.poll_character(BOB).await;
    set_market(&env, FORGE, vec![sell_at(1, 100.0), sell_at(2, 90.0)], None).await;
    refresh_all(&env).await;

    let mut rows = standings(&env, account, sekondo::orders::Side::Sell).await;

    // Default order is by item and then order id: Alice's order first.
    let (alice, bob) = (rows.remove(0), rows.remove(0));
    assert_eq!((alice.undercut, alice.best_cents), (Some(1), Some(9000)));
    assert_eq!(bob.undercut, Some(0));
}

// ---- standing of a buy order ----

const NEIGHBOUR: i64 = 30000144;

fn own_buy() -> Value {
    own(1, 34, true, 50.0, STATION)
}

fn own_buy_row() -> Value {
    public(1, true, 50.0, STATION, SYSTEM, "station")
}

fn buy_at(id: i64, price: f64, location: i64, system: i64, range: &str) -> Value {
    public(id, true, price, location, system, range)
}

async fn buy_standing(alices: Vec<Value>, market: Vec<Value>) -> Standing {
    let (env, account) = market_with(alices, market).await;
    standings(&env, account, sekondo::orders::Side::Buy)
        .await
        .remove(0)
}

#[tokio::test]
async fn a_buy_order_is_top_when_only_unreaching_orders_are_better() {
    let standing = buy_standing(
        vec![own_buy()],
        vec![
            own_buy_row(),
            buy_at(2, 60.0, OTHER_STATION, SYSTEM, "station"),
            buy_at(3, 40.0, OTHER_STATION, SYSTEM, "region"),
        ],
    )
    .await;

    assert_eq!(standing.undercut, Some(0));
}

#[tokio::test]
async fn a_region_range_order_reaches_everywhere() {
    let standing = buy_standing(
        vec![own_buy()],
        vec![
            own_buy_row(),
            buy_at(2, 55.0, OTHER_STATION, NEIGHBOUR, "region"),
        ],
    )
    .await;

    assert_eq!(standing.undercut, Some(1));
}

#[tokio::test]
async fn any_range_at_the_same_station_reaches() {
    let standing = buy_standing(
        vec![own_buy()],
        vec![
            own_buy_row(),
            buy_at(2, 52.0, STATION, SYSTEM, "station"),
            buy_at(3, 53.0, STATION, SYSTEM, "3"),
        ],
    )
    .await;

    assert_eq!(standing.undercut, Some(2));
    assert_eq!(standing.best_cents, Some(5300));
}

#[tokio::test]
async fn a_solar_system_range_order_reaches_only_from_the_same_system() {
    let standing = buy_standing(
        vec![own_buy()],
        vec![
            own_buy_row(),
            buy_at(2, 55.0, OTHER_STATION, SYSTEM, "solarsystem"),
            buy_at(3, 56.0, OTHER_STATION, NEIGHBOUR, "solarsystem"),
        ],
    )
    .await;

    assert_eq!(standing.undercut, Some(1));
    assert_eq!(standing.best_cents, Some(5500));
}

#[tokio::test]
async fn a_jump_range_order_in_the_same_system_reaches() {
    let standing = buy_standing(
        vec![own_buy()],
        vec![own_buy_row(), buy_at(2, 55.0, OTHER_STATION, SYSTEM, "5")],
    )
    .await;

    assert_eq!(standing.undercut, Some(1));
}

#[tokio::test]
async fn a_jump_range_order_from_another_system_is_not_counted() {
    let standing = buy_standing(
        vec![own_buy()],
        vec![
            own_buy_row(),
            buy_at(2, 55.0, OTHER_STATION, NEIGHBOUR, "5"),
        ],
    )
    .await;

    assert_eq!(standing.undercut, Some(0));
}

#[tokio::test]
async fn a_station_range_order_at_another_station_does_not_reach() {
    let standing = buy_standing(
        vec![own_buy()],
        vec![
            own_buy_row(),
            buy_at(2, 70.0, OTHER_STATION, SYSTEM, "station"),
        ],
    )
    .await;

    assert_eq!(standing.undercut, Some(0));
}

#[tokio::test]
async fn the_own_range_of_a_buy_order_is_ignored() {
    let mut ranged = own_buy();
    ranged["range"] = json!("region");
    let mut row = own_buy_row();
    row["range"] = json!("region");

    let standing = buy_standing(
        vec![ranged],
        vec![row, buy_at(2, 60.0, OTHER_STATION, SYSTEM, "station")],
    )
    .await;

    assert_eq!(standing.undercut, Some(0));
}

#[tokio::test]
async fn a_buy_order_not_in_the_market_data_yet_has_no_standing() {
    let standing = buy_standing(
        vec![own_buy()],
        vec![buy_at(2, 60.0, STATION, SYSTEM, "region")],
    )
    .await;

    assert_eq!(standing.undercut, None);
    assert_eq!(standing.best_cents, None);
}

#[tokio::test]
async fn sell_orders_ignore_the_buy_side_of_the_market() {
    let (env, account) = market_with(
        vec![own(1, 34, false, 100.0, STATION)],
        vec![
            sell_at(1, 100.0),
            buy_at(2, 200.0, STATION, SYSTEM, "region"),
        ],
    )
    .await;

    assert_eq!(sell_standing(&env, account).await.undercut, Some(0));
}

// ---- gap and no market data ----

#[tokio::test]
async fn a_sell_order_behind_has_the_gap_to_the_best_price() {
    let (env, account) = market_with(
        vec![own(1, 34, false, 1_012_500.0, STATION)],
        vec![sell_at(1, 1_012_500.0), sell_at(2, 1_000_000.0)],
    )
    .await;

    let standing = sell_standing(&env, account).await;

    assert_eq!(standing.gap_cents, Some(1_250_000));
    assert_eq!(standing.best_cents, Some(100_000_000));
}

#[tokio::test]
async fn a_buy_order_behind_has_the_gap_to_the_best_reaching_price() {
    let mut own_row = own_buy_row();
    own_row["price"] = json!(95.0);
    let mut own = own_buy();
    own["price"] = json!(95.0);

    let standing = buy_standing(
        vec![own],
        vec![own_row, buy_at(2, 100.0, STATION, SYSTEM, "station")],
    )
    .await;

    assert_eq!(standing.gap_cents, Some(500));
    assert_eq!(standing.best_cents, Some(10_000));
}

#[tokio::test]
async fn a_top_order_has_no_gap() {
    let (env, account) = market_with(
        vec![own(1, 34, false, 100.0, STATION)],
        vec![sell_at(1, 100.0)],
    )
    .await;

    assert_eq!(sell_standing(&env, account).await.gap_cents, None);
}

#[tokio::test]
async fn a_pair_that_was_not_fetched_yet_has_no_standing_and_no_gap() {
    let env = alice_with_an_order().await;
    // The stub knows cheaper orders, but the pair is not refreshed yet.
    set_market(&env, FORGE, vec![sell_at(2, 1.0)], None).await;
    env.market.tick().await;
    let account = the_account(&env).await;

    let standing = sell_standing(&env, account).await;

    assert_eq!(standing.undercut, None);
    assert_eq!(standing.gap_cents, None);
}

#[tokio::test]
async fn an_order_in_a_player_structure_has_no_standing_however_fresh_the_data() {
    const STRUCTURE: i64 = 1000000000001;
    let (env, account) = market_with(
        vec![own(1, 34, false, 100.0, STRUCTURE)],
        vec![public(2, false, 50.0, STRUCTURE, SYSTEM, "station")],
    )
    .await;

    let standing = sell_standing(&env, account).await;

    assert_eq!(standing.undercut, None);
    assert_eq!(standing.gap_cents, None);
}

#[tokio::test]
async fn stale_market_data_is_still_used_as_last_known() {
    let (env, account) = market_with(
        vec![own(1, 34, false, 100.0, STATION)],
        vec![sell_at(1, 100.0), sell_at(2, 90.0)],
    )
    .await;

    env.clock.advance(Duration::hours(1));

    assert_eq!(sell_standing(&env, account).await.undercut, Some(1));
}

// ---- snapshots of a pair ----

use sekondo::market::snapshot;

#[tokio::test]
async fn identical_snapshots_change_no_account() {
    let (env, _) = market_with(
        vec![own(1, 34, false, 100.0, STATION)],
        vec![sell_at(1, 100.0), sell_at(2, 90.0)],
    )
    .await;

    let before = snapshot(&*env.stores.market, FORGE, 34).await.unwrap();
    let after = snapshot(&*env.stores.market, FORGE, 34).await.unwrap();

    assert_eq!(before.changed_accounts(&after), Vec::<uuid::Uuid>::new());
}

#[tokio::test]
async fn a_changed_undercut_or_gap_changes_its_owner_only() {
    let env = PollEnv::new().await;
    let (mine, theirs) = (env.new_account().await, env.new_account().await);
    env.link(mine, ALICE).await;
    env.link(theirs, BOB).await;
    set_orders(&env, ALICE, vec![own(1, 34, false, 100.0, STATION)]).await;
    set_orders(&env, BOB, vec![own(2, 34, false, 50.0, OTHER_STATION)]).await;
    env.poller.poll_character(ALICE).await;
    env.poller.poll_character(BOB).await;
    set_market(
        &env,
        FORGE,
        vec![
            sell_at(1, 100.0),
            public(2, false, 50.0, OTHER_STATION, SYSTEM, "station"),
        ],
        None,
    )
    .await;
    ok(env.control("esi-expires", json!({"seconds": 300})).await).await;
    refresh_all(&env).await;
    let before = snapshot(&*env.stores.market, FORGE, 34).await.unwrap();

    // Another order at 90 undercuts Alice only.
    set_market(
        &env,
        FORGE,
        vec![
            sell_at(1, 100.0),
            public(2, false, 50.0, OTHER_STATION, SYSTEM, "station"),
            sell_at(3, 90.0),
        ],
        None,
    )
    .await;
    env.clock.advance(Duration::minutes(6));
    env.market.tick().await;
    let undercut = snapshot(&*env.stores.market, FORGE, 34).await.unwrap();
    assert_eq!(before.changed_accounts(&undercut), [mine]);

    // The best price moves from 90 to 80: still undercut once, but the gap changed.
    set_market(
        &env,
        FORGE,
        vec![
            sell_at(1, 100.0),
            public(2, false, 50.0, OTHER_STATION, SYSTEM, "station"),
            sell_at(3, 80.0),
        ],
        None,
    )
    .await;
    env.clock.advance(Duration::minutes(6));
    env.market.tick().await;
    let wider = snapshot(&*env.stores.market, FORGE, 34).await.unwrap();
    assert_eq!(undercut.changed_accounts(&wider), [mine]);
}

#[tokio::test]
async fn a_standing_that_appears_changes_its_owner() {
    let env = alice_with_an_order().await;
    let account = the_account(&env).await;
    let before = snapshot(&*env.stores.market, FORGE, 34).await.unwrap();

    set_market(&env, FORGE, vec![sell_at(1, 10.0)], None).await;
    refresh_all(&env).await;
    let after = snapshot(&*env.stores.market, FORGE, 34).await.unwrap();

    assert_eq!(before.changed_accounts(&after), [account]);
}

// ---- signals ----

use futures_util::{StreamExt, stream::BoxStream};
use sekondo::events::{Event, EventBus};

async fn signals(stream: &mut BoxStream<'static, Event>) -> Vec<Event> {
    let mut seen = Vec::new();
    while let Ok(Some(event)) =
        tokio::time::timeout(std::time::Duration::from_millis(100), stream.next()).await
    {
        seen.push(event);
    }
    seen
}

/// Alice (account `mine`) sells at 100 and has a refreshed market where she is top.
async fn top_seller() -> (PollEnv, uuid::Uuid) {
    let (env, account) = market_with(
        vec![own(1, 34, false, 100.0, STATION)],
        vec![sell_at(1, 100.0)],
    )
    .await;
    ok(env.control("esi-expires", json!({"seconds": 300})).await).await;
    (env, account)
}

async fn refresh_again(env: &PollEnv) {
    env.clock.advance(Duration::minutes(21));
    env.market.tick().await;
}

#[tokio::test]
async fn a_refresh_that_changes_a_standing_signals_the_owner() {
    let (env, account) = top_seller().await;
    let mut stream = env.events.subscribe(account);

    set_market(&env, FORGE, vec![sell_at(1, 100.0), sell_at(2, 90.0)], None).await;
    refresh_again(&env).await;

    assert_eq!(signals(&mut stream).await, [Event::MarketUpdated]);
}

#[tokio::test]
async fn a_refresh_that_only_changes_an_undercutting_orders_volume_signals_the_owner() {
    let (env, account) = market_with(
        vec![own(1, 34, false, 100.0, STATION)],
        vec![sell_at(1, 100.0), remaining(sell_at(2, 90.0), 20)],
    )
    .await;
    ok(env.control("esi-expires", json!({"seconds": 300})).await).await;
    let mut stream = env.events.subscribe(account);

    // Same standing, same gap: only the 20 units in front drop to 8.
    set_market(
        &env,
        FORGE,
        vec![sell_at(1, 100.0), remaining(sell_at(2, 90.0), 8)],
        None,
    )
    .await;
    refresh_again(&env).await;

    assert_eq!(signals(&mut stream).await, [Event::MarketUpdated]);
}

#[tokio::test]
async fn a_refresh_that_changes_nothing_signals_nobody() {
    let (env, account) = top_seller().await;
    let mut stream = env.events.subscribe(account);

    // Other orders, but none that changes the standing or the gap.
    set_market(
        &env,
        FORGE,
        vec![
            sell_at(1, 100.0),
            sell_at(5, 150.0),
            public(6, false, 1.0, OTHER_STATION, SYSTEM, "station"),
        ],
        None,
    )
    .await;
    refresh_again(&env).await;

    assert_eq!(env.stub.esi_calls("market"), 2, "it did refresh");
    assert_eq!(signals(&mut stream).await, Vec::<Event>::new());
}

#[tokio::test]
async fn a_signal_stays_within_the_account_of_the_changed_order() {
    let (env, account) = top_seller().await;
    let other = env.new_account().await;
    let mut mine = env.events.subscribe(account);
    let mut theirs = env.events.subscribe(other);

    set_market(&env, FORGE, vec![sell_at(1, 100.0), sell_at(2, 90.0)], None).await;
    refresh_again(&env).await;

    assert_eq!(signals(&mut mine).await, [Event::MarketUpdated]);
    assert_eq!(signals(&mut theirs).await, Vec::<Event>::new());
}

#[tokio::test]
async fn a_standing_that_appears_on_the_first_fetch_is_signalled() {
    let env = alice_with_an_order().await;
    let account = the_account(&env).await;
    let mut stream = env.events.subscribe(account);
    set_market(&env, FORGE, vec![sell_at(1, 10.0)], None).await;

    refresh_all(&env).await;

    assert_eq!(signals(&mut stream).await, [Event::MarketUpdated]);
}

#[tokio::test]
async fn a_failed_refresh_signals_nobody() {
    let (env, account) = top_seller().await;
    let mut stream = env.events.subscribe(account);
    set_market(&env, FORGE, vec![sell_at(1, 100.0), sell_at(2, 90.0)], None).await;
    ok(env
        .control("esi-fail-next", json!({"route": "market", "status": 503}))
        .await)
    .await;

    refresh_again(&env).await;

    assert_eq!(signals(&mut stream).await, Vec::<Event>::new());
}

// ---- undercut volume ----

fn remaining(mut order: Value, units: i64) -> Value {
    order["volume_remain"] = json!(units);
    order
}

#[tokio::test]
async fn a_sell_order_behind_two_orders_has_their_remaining_volume_as_undercut_volume() {
    let (env, account) = market_with(
        vec![own(1, 34, false, 100.0, STATION)],
        vec![
            sell_at(1, 100.0),
            remaining(sell_at(2, 99.0), 5),
            remaining(sell_at(3, 95.0), 20),
            remaining(public(4, false, 90.0, OTHER_STATION, SYSTEM, "station"), 50),
        ],
    )
    .await;

    assert_eq!(sell_standing(&env, account).await.undercut_volume, Some(25));
}

#[tokio::test]
async fn a_buy_order_behind_reaching_orders_sums_their_volume_whatever_the_minimum() {
    let mut big_minimum = remaining(buy_at(2, 55.0, OTHER_STATION, NEIGHBOUR, "region"), 1000);
    big_minimum["min_volume"] = json!(500);
    let standing = buy_standing(
        vec![own_buy()],
        vec![
            own_buy_row(),
            big_minimum,
            remaining(buy_at(3, 52.0, STATION, SYSTEM, "station"), 300),
            remaining(buy_at(4, 60.0, OTHER_STATION, NEIGHBOUR, "5"), 700),
        ],
    )
    .await;

    assert_eq!(standing.undercut, Some(2));
    assert_eq!(standing.undercut_volume, Some(1300));
}

#[tokio::test]
async fn a_top_order_has_no_undercut_volume() {
    let (env, account) = market_with(
        vec![own(1, 34, false, 100.0, STATION)],
        vec![sell_at(1, 100.0), remaining(sell_at(2, 101.0), 40)],
    )
    .await;

    assert_eq!(sell_standing(&env, account).await.undercut_volume, None);
}

#[tokio::test]
async fn an_order_without_market_data_has_no_undercut_volume() {
    let env = alice_with_an_order().await;
    let account = the_account(&env).await;

    assert_eq!(sell_standing(&env, account).await.undercut_volume, None);
}
