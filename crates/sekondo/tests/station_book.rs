mod common;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use common::PollEnv;
use sekondo::{
    clock::Clock,
    esi::{EsiBookPage, EsiError, EsiMarketOrder},
    market::RefreshOutcome,
    station_book::{BookPages, StationBookPoller},
};
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime};

const FORGE: i64 = 10000002;
const JITA_44: i64 = 60003760;
const OTHER_JITA_STATION: i64 = 60003761;
const SYSTEM: i64 = 30000142;

fn order(id: i64, type_id: i64, buy: bool, price: f64, remain: i64, location: i64) -> Value {
    json!({
        "order_id": id, "type_id": type_id, "location_id": location, "system_id": SYSTEM,
        "is_buy_order": buy, "price": price, "range": "region",
        "duration": 90, "issued": "2026-10-11T12:00:00Z", "min_volume": 1,
        "volume_remain": remain, "volume_total": remain,
    })
}

fn at_jita(id: i64, type_id: i64, buy: bool, price: f64, remain: i64) -> Value {
    order(id, type_id, buy, price, remain, JITA_44)
}

async fn set_book(env: &PollEnv, orders: Vec<Value>, page_size: Option<usize>) {
    let mut body = json!({"region_id": FORGE, "orders": orders});
    if let Some(size) = page_size {
        body["page_size"] = json!(size);
    }
    assert_eq!(env.control("market-orders", body).await.status(), 204);
    // ESI's region answers are cached for 5 minutes.
    assert_eq!(
        env.control("esi-expires", json!({"seconds": 300}))
            .await
            .status(),
        204
    );
}

async fn book_ids(env: &PollEnv) -> Vec<i64> {
    env.stores
        .station
        .book(FORGE, JITA_44)
        .await
        .unwrap()
        .iter()
        .map(|o| o.order_id)
        .collect()
}

async fn snapshot_count(env: &PollEnv) -> i64 {
    env.stores
        .station
        .snapshots(FORGE, JITA_44)
        .await
        .unwrap()
        .len() as i64
}

/// `(best buy, best sell, spread, buy near units, buy near orders, sell near units, sell near
/// orders, buy units, sell units, buy taken, sell taken)` of the newest snapshot of an item.
type Snapshot = (
    String,
    String,
    String,
    i64,
    i32,
    i64,
    i32,
    i64,
    i64,
    i64,
    i64,
);

async fn latest_snapshot(env: &PollEnv, type_id: i64) -> Snapshot {
    let snapshots = env.stores.station.snapshots(FORGE, JITA_44).await.unwrap();
    let s = snapshots
        .into_iter()
        .rfind(|s| s.type_id == type_id)
        .expect("a snapshot");
    (
        s.best_buy,
        s.best_sell,
        sekondo::fees::round_half_up(&s.spread, 4),
        s.buy_near_units,
        s.buy_near_orders,
        s.sell_near_units,
        s.sell_near_orders,
        s.buy_units,
        s.sell_units,
        s.buy_taken,
        s.sell_taken,
    )
}

struct Sync {
    next_poll_at: OffsetDateTime,
    last_success_at: Option<OffsetDateTime>,
    last_error: Option<String>,
    failures: i32,
}

async fn book_sync(env: &PollEnv) -> Sync {
    let state = env
        .stores
        .station
        .station_sync_state(FORGE, JITA_44, "book")
        .await
        .unwrap()
        .unwrap();
    Sync {
        next_poll_at: state.next_poll_at,
        last_success_at: state.last_success_at,
        last_error: state.last_error,
        failures: state.failures,
    }
}

// ---- what is kept ----

#[tokio::test]
async fn a_refresh_keeps_exactly_the_orders_at_jita_44() {
    let env = PollEnv::new().await;
    set_book(
        &env,
        vec![
            at_jita(1, 34, false, 10.0, 5),
            at_jita(2, 34, true, 9.0, 5),
            // A buy order at another station in Jita whose region range reaches Jita 4-4.
            order(3, 34, true, 9.5, 100, OTHER_JITA_STATION),
            // A station in another system of the region.
            order(4, 35, false, 5.0, 10, 60004588),
        ],
        None,
    )
    .await;

    let outcome = env.station.refresh().await;

    assert_eq!(outcome, RefreshOutcome::Updated);
    assert_eq!(book_ids(&env).await, [1, 2]);
    assert_eq!(env.stub.esi_calls("market"), 1);
}

#[tokio::test]
async fn every_page_of_the_book_is_read_and_merged() {
    let env = PollEnv::new().await;
    let orders: Vec<Value> = (1..=7)
        .map(|id| at_jita(id, 34, id % 2 == 0, 10.0 + id as f64, 5))
        .collect();
    set_book(&env, orders, Some(2)).await;

    env.station.refresh().await;

    assert_eq!(book_ids(&env).await, [1, 2, 3, 4, 5, 6, 7]);
    assert_eq!(env.stub.esi_calls("market"), 4, "four pages of two");
}

#[tokio::test]
async fn the_book_is_replaced_by_the_next_refresh() {
    let env = PollEnv::new().await;
    set_book(
        &env,
        vec![at_jita(1, 34, false, 10.0, 5), at_jita(2, 34, true, 9.0, 5)],
        None,
    )
    .await;
    env.station.refresh().await;
    set_book(
        &env,
        vec![at_jita(2, 34, true, 9.0, 5), at_jita(3, 34, false, 11.0, 5)],
        None,
    )
    .await;

    env.station.refresh().await;

    assert_eq!(book_ids(&env).await, [2, 3]);
}

// ---- the snapshot ----

#[tokio::test]
async fn the_snapshot_holds_the_top_of_book_figures_of_a_tradable_item() {
    let env = PollEnv::new().await;
    set_book(
        &env,
        vec![
            at_jita(1, 34, true, 100.0, 500),
            at_jita(2, 34, true, 99.5, 300),
            at_jita(3, 34, false, 110.0, 200),
            at_jita(4, 34, false, 120.0, 1000),
        ],
        None,
    )
    .await;

    env.station.refresh().await;

    // Best buy 100, best sell 110, spread 10%; 2 buy orders (800 units) and 1 sell order (200
    // units) within 1%; 800 buy and 1200 sell units in total; nothing taken yet.
    assert_eq!(
        latest_snapshot(&env, 34).await,
        (
            "100.00".into(),
            "110.00".into(),
            "0.1000".into(),
            800,
            2,
            200,
            1,
            800,
            1200,
            0,
            0
        )
    );
    let recorded = env.stores.station.snapshots(FORGE, JITA_44).await.unwrap()[0].recorded_at;
    assert_eq!(recorded, env.clock.now());
}

#[tokio::test]
async fn a_bait_buy_order_far_below_an_expensive_sell_is_snapshotted_with_its_huge_spread() {
    let env = PollEnv::new().await;
    // Real Jita books hold 0.01 ISK buy orders on items selling for billions.
    set_book(
        &env,
        vec![
            at_jita(1, 34, true, 0.01, 1),
            at_jita(2, 34, false, 100_000_000_000.0, 1),
        ],
        None,
    )
    .await;

    let outcome = env.station.refresh().await;

    assert_eq!(outcome, RefreshOutcome::Updated);
    let (best_buy, best_sell, spread, ..) = latest_snapshot(&env, 34).await;
    assert_eq!(
        (best_buy.as_str(), best_sell.as_str(), spread.as_str()),
        ("0.01", "100000000000.00", "9999999999999.0000")
    );
}

#[tokio::test]
async fn an_item_with_only_one_side_gets_no_snapshot() {
    let env = PollEnv::new().await;
    set_book(
        &env,
        vec![
            at_jita(1, 34, false, 10.0, 5),
            at_jita(2, 35, false, 10.0, 5),
            at_jita(3, 35, true, 9.0, 5),
        ],
        None,
    )
    .await;

    env.station.refresh().await;

    let items: Vec<i64> = env
        .stores
        .station
        .snapshots(FORGE, JITA_44)
        .await
        .unwrap()
        .iter()
        .map(|s| s.type_id)
        .collect();
    assert_eq!(items, [35]);
    assert_eq!(
        book_ids(&env).await,
        [1, 2, 3],
        "the book keeps every order"
    );
}

#[tokio::test]
async fn units_taken_are_the_drop_in_orders_present_in_both_refreshes() {
    let env = PollEnv::new().await;
    set_book(
        &env,
        vec![
            at_jita(1, 34, true, 100.0, 500),
            at_jita(2, 34, false, 110.0, 200),
            at_jita(3, 34, false, 115.0, 300),
        ],
        None,
    )
    .await;
    env.station.refresh().await;
    env.clock.advance(Duration::minutes(6));
    // Sell order 2 went from 200 to 150 units and sell order 3 disappeared; buy order 1 lost 40
    // units; a new buy order 4 appeared.
    set_book(
        &env,
        vec![
            at_jita(1, 34, true, 100.0, 460),
            at_jita(2, 34, false, 110.0, 150),
            at_jita(4, 34, true, 99.0, 1000),
        ],
        None,
    )
    .await;

    env.station.refresh().await;

    let snapshot = latest_snapshot(&env, 34).await;
    assert_eq!((snapshot.9, snapshot.10), (40, 50), "buy taken, sell taken");
}

#[tokio::test]
async fn the_first_refresh_takes_nothing_and_a_growing_order_takes_nothing() {
    let env = PollEnv::new().await;
    set_book(
        &env,
        vec![
            at_jita(1, 34, true, 100.0, 5),
            at_jita(2, 34, false, 110.0, 5),
        ],
        None,
    )
    .await;
    env.station.refresh().await;
    env.clock.advance(Duration::minutes(6));
    set_book(
        &env,
        vec![
            at_jita(1, 34, true, 100.0, 50),
            at_jita(2, 34, false, 110.0, 5),
        ],
        None,
    )
    .await;

    env.station.refresh().await;

    let snapshot = latest_snapshot(&env, 34).await;
    assert_eq!((snapshot.9, snapshot.10), (0, 0));
}

// ---- failure ----

#[tokio::test]
async fn a_failing_page_keeps_the_book_records_no_snapshot_and_backs_off() {
    use sekondo::poller::BACKOFF_START;
    let env = PollEnv::new().await;
    let book = |first: i64| -> Vec<Value> {
        (first..first + 6)
            .map(|id| at_jita(id, 34, id % 2 == 0, 10.0 + id as f64, 5))
            .collect()
    };
    set_book(&env, book(1), Some(2)).await;
    env.station.refresh().await;
    let before = (book_ids(&env).await, snapshot_count(&env).await);
    assert_eq!(before.0, [1, 2, 3, 4, 5, 6]);
    env.clock.advance(Duration::minutes(6));
    set_book(&env, book(11), Some(2)).await;
    assert_eq!(
        env.control(
            "market-fail",
            json!({"region_id": FORGE, "page": 2, "status": 503})
        )
        .await
        .status(),
        204
    );

    let outcome = env.station.refresh().await;

    assert_eq!(outcome, RefreshOutcome::Failed);
    assert_eq!((book_ids(&env).await, snapshot_count(&env).await), before);
    let sync = book_sync(&env).await;
    assert_eq!(sync.failures, 1);
    assert!(sync.last_error.unwrap().contains("503"));
    let wait = sync.next_poll_at - env.clock.now();
    assert!(
        wait >= BACKOFF_START && wait <= BACKOFF_START + Duration::minutes(1),
        "{wait}"
    );
}

#[tokio::test]
async fn one_failing_page_among_several_fails_the_whole_refresh() {
    let env = PollEnv::new().await;
    let orders: Vec<Value> = (1..=6)
        .map(|id| at_jita(id, 34, id % 2 == 0, 10.0 + id as f64, 5))
        .collect();
    set_book(&env, orders, Some(2)).await;
    env.control(
        "market-fail",
        json!({"region_id": FORGE, "page": 3, "status": 503}),
    )
    .await;

    let outcome = env.station.refresh().await;

    assert_eq!(outcome, RefreshOutcome::Failed);
    assert_eq!(book_ids(&env).await, Vec::<i64>::new());
    assert_eq!(snapshot_count(&env).await, 0);
}

#[tokio::test]
async fn a_rate_limited_refresh_is_deferred_without_counting_a_failure() {
    let env = PollEnv::new().await;
    set_book(&env, vec![at_jita(1, 34, true, 1.0, 1)], None).await;
    env.control(
        "esi-fail-next",
        json!({"route": "market", "status": 429,
               "headers": {"retry-after": "120", "x-ratelimit-group": "market-order"}}),
    )
    .await;

    let outcome = env.station.refresh().await;

    assert_eq!(outcome, RefreshOutcome::Deferred);
    let sync = book_sync(&env).await;
    assert_eq!(sync.failures, 0);
    assert!(sync.next_poll_at >= env.clock.now() + Duration::seconds(120));
}

// ---- schedule ----

#[tokio::test]
async fn the_first_tick_refreshes_at_once_and_the_next_waits_for_the_cache_expiry() {
    let env = PollEnv::new().await;
    set_book(
        &env,
        vec![at_jita(1, 34, true, 1.0, 1), at_jita(2, 34, false, 2.0, 1)],
        None,
    )
    .await;

    assert_eq!(
        env.station.tick().await,
        1,
        "due at once on a fresh database"
    );
    assert_eq!(env.station.tick().await, 0);
    env.clock.advance(Duration::minutes(4));
    assert_eq!(env.station.tick().await, 0, "the cache runs another minute");
    assert_eq!(env.stub.esi_calls("market"), 1);

    env.clock.advance(Duration::minutes(2));
    assert_eq!(env.station.tick().await, 1);
    assert_eq!(env.stub.esi_calls("market"), 2);
    let sync = book_sync(&env).await;
    assert_eq!(sync.last_success_at, Some(env.clock.now()));
    assert_eq!(sync.failures, 0);
}

#[tokio::test]
async fn a_refresh_is_never_earlier_than_five_minutes_after_the_last_even_if_esi_expires_sooner() {
    let env = PollEnv::new().await;
    set_book(&env, vec![at_jita(1, 34, true, 1.0, 1)], None).await;
    env.control("esi-expires", json!({"seconds": 30})).await;
    env.station.tick().await;

    env.clock.advance(Duration::minutes(4));

    assert_eq!(env.station.tick().await, 0);
}

// ---- one cache generation ----

/// A scripted source: read number `n` (counted by requests for page 1) answers page `p` with
/// `reads[n][p - 1]`.
struct Scripted {
    reads: Vec<Vec<EsiBookPage>>,
    state: Mutex<(usize, usize)>,
}

impl Scripted {
    fn page_requests(&self) -> usize {
        self.state.lock().unwrap().1
    }
}

fn page(orders: Vec<EsiMarketOrder>, pages: u32, generation: i64) -> EsiBookPage {
    EsiBookPage {
        orders,
        pages,
        expires_at: None,
        generation: Some(generation),
    }
}

fn market_order(id: i64) -> EsiMarketOrder {
    serde_json::from_value(order(id, 34, id % 2 == 0, 10.0 + id as f64, 5, JITA_44)).unwrap()
}

#[async_trait]
impl BookPages for Scripted {
    async fn page(&self, _region_id: i64, page: u32) -> Result<EsiBookPage, EsiError> {
        let mut state = self.state.lock().unwrap();
        if page == 1 {
            state.0 += 1;
        }
        state.1 += 1;
        Ok(self.reads[state.0 - 1][page as usize - 1].clone())
    }
}

fn scripted(reads: Vec<Vec<EsiBookPage>>, env: &PollEnv) -> (StationBookPoller, Arc<Scripted>) {
    let source = Arc::new(Scripted {
        reads,
        state: Mutex::new((0, 0)),
    });
    let poller = StationBookPoller::with_source(
        &env.stores,
        source.clone(),
        env.events.clone(),
        env.clock.clone(),
        Duration::minutes(5),
    );
    (poller, source)
}

#[tokio::test]
async fn a_read_that_crossed_a_cache_generation_is_retried_at_once() {
    let env = PollEnv::new().await;
    let (poller, source) = scripted(
        vec![
            // The cache turned over between the two pages.
            vec![
                page(vec![market_order(1), market_order(2)], 2, 100),
                page(vec![market_order(3), market_order(4)], 2, 200),
            ],
            vec![
                page(vec![market_order(11), market_order(12)], 2, 200),
                page(vec![market_order(13), market_order(14)], 2, 200),
            ],
        ],
        &env,
    );

    let outcome = poller.refresh().await;

    assert_eq!(outcome, RefreshOutcome::Updated);
    assert_eq!(
        book_ids(&env).await,
        [11, 12, 13, 14],
        "only the consistent read"
    );
    assert_eq!(source.page_requests(), 4);
}

#[tokio::test]
async fn a_book_that_never_settles_is_read_three_times_and_then_fails() {
    let env = PollEnv::new().await;
    let mixed = || {
        vec![
            page(vec![market_order(1), market_order(2)], 2, 100),
            page(vec![market_order(3), market_order(4)], 2, 200),
        ]
    };
    let (poller, source) = scripted(vec![mixed(), mixed(), mixed()], &env);

    let outcome = poller.refresh().await;

    assert_eq!(outcome, RefreshOutcome::Failed);
    assert_eq!(source.page_requests(), 6, "one read and two retries");
    assert_eq!(book_ids(&env).await, Vec::<i64>::new());
    assert_eq!(snapshot_count(&env).await, 0);
    assert!(book_sync(&env).await.last_error.is_some());
}

#[tokio::test]
async fn pages_of_one_generation_may_differ_in_their_local_expiry() {
    // The server's Expires is the same; the local clock reading of it differs by the Date header.
    let env = PollEnv::new().await;
    let mut late = page(vec![market_order(3), market_order(4)], 2, 100);
    late.expires_at = Some(env.clock.now() + Duration::seconds(299));
    let mut early = page(vec![market_order(1), market_order(2)], 2, 100);
    early.expires_at = Some(env.clock.now() + Duration::seconds(300));
    let (poller, source) = scripted(vec![vec![early, late]], &env);

    assert_eq!(poller.refresh().await, RefreshOutcome::Updated);
    assert_eq!(source.page_requests(), 2, "no retry");
}

// ---- names ----

/// `None` when the name was never looked up, `Some(None)` when ESI would not give one.
async fn name_of(env: &PollEnv, id: i64) -> Option<Option<String>> {
    env.stores
        .market
        .names_table()
        .await
        .unwrap()
        .into_iter()
        .find(|row| row.id == id)
        .map(|row| row.name)
}

#[tokio::test]
async fn a_refresh_resolves_the_names_of_the_tradable_items_once() {
    let env = PollEnv::new().await;
    set_book(
        &env,
        vec![
            at_jita(1, 34, true, 9.0, 5),
            at_jita(2, 34, false, 10.0, 5),
            // Not tradable: only a sell order, so its name is not looked up.
            at_jita(3, 35, false, 10.0, 5),
            // Tradable, but ESI does not know the type.
            at_jita(4, 99999, true, 9.0, 5),
            at_jita(5, 99999, false, 10.0, 5),
        ],
        None,
    )
    .await;

    env.station.refresh().await;

    assert_eq!(name_of(&env, 34).await, Some(Some("Tritanium".to_owned())));
    assert_eq!(
        name_of(&env, 99999).await,
        Some(None),
        "cached as unresolvable"
    );
    assert_eq!(name_of(&env, 35).await, None, "not tradable, not asked");

    let asked = env.stub.names_requests().len();
    env.clock.advance(Duration::minutes(6));
    env.station.refresh().await;
    assert_eq!(
        env.stub.names_requests().len(),
        asked,
        "known and cached ids are not asked again"
    );
}

#[tokio::test]
async fn names_that_cannot_be_resolved_never_fail_a_refresh() {
    let env = PollEnv::new().await;
    set_book(
        &env,
        vec![at_jita(1, 34, true, 9.0, 5), at_jita(2, 34, false, 10.0, 5)],
        None,
    )
    .await;
    env.control("esi-fail-next", json!({"route": "names", "status": 503}))
        .await;

    let outcome = env.station.refresh().await;

    assert_eq!(outcome, RefreshOutcome::Updated);
    assert_eq!(book_ids(&env).await, [1, 2]);
    assert_eq!(
        name_of(&env, 34).await,
        None,
        "asked again on a later refresh"
    );
}
