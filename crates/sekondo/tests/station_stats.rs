mod common;

use common::PollEnv;
use sekondo::{
    clock::Clock,
    opportunities::{
        analysis::{Direction, WindowKind},
        stats,
    },
    store::WindowRow,
};
use serde_json::{Value, json};
use time::{Date, Duration, OffsetDateTime};

const FORGE: i64 = 10000002;
const JITA_44: i64 = 60003760;

fn order(id: i64, type_id: i64, buy: bool, price: f64, remain: i64) -> Value {
    json!({
        "order_id": id, "type_id": type_id, "location_id": JITA_44, "system_id": 30000142,
        "is_buy_order": buy, "price": price, "range": "region",
        "duration": 90, "issued": "2026-10-11T12:00:00Z", "min_volume": 1,
        "volume_remain": remain, "volume_total": remain,
    })
}

/// Item 34 with two buy orders within 1% of its best and one sell order.
async fn tradable_34(env: &PollEnv) {
    let orders = vec![
        order(1, 34, true, 100.0, 500),
        order(2, 34, true, 99.5, 300),
        order(3, 34, false, 110.0, 200),
    ];
    env.control(
        "market-orders",
        json!({"region_id": FORGE, "orders": orders}),
    )
    .await;
    env.control("esi-expires", json!({"seconds": 300})).await;
}

fn day(env: &PollEnv, back: i64) -> Date {
    env.clock.now().date() - Duration::days(back)
}

async fn history_row(
    env: &PollEnv,
    type_id: i64,
    back: i64,
    avg: f64,
    high: f64,
    low: f64,
    volume: i64,
) {
    env.stores
        .station
        .seed_history(
            FORGE,
            &[sekondo::store::SeedHistoryRow {
                type_id,
                day: day(env, back),
                average: avg,
                highest: high,
                lowest: low,
                volume,
                order_count: 10,
            }],
        )
        .await
        .unwrap();
}

async fn snapshot_at(env: &PollEnv, at: OffsetDateTime, type_id: i64, taken: (i64, i64)) {
    env.stores
        .station
        .seed_snapshots(
            FORGE,
            JITA_44,
            &[sekondo::store::SeedSnapshotRow {
                recorded_at: at,
                type_id,
                best_buy: 100.0,
                best_sell: 110.0,
                buy_near_units: 10,
                buy_near_orders: 2,
                sell_near_units: 20,
                sell_near_orders: 3,
                buy_units: 100,
                sell_units: 200,
                buy_taken: taken.0,
                sell_taken: taken.1,
            }],
        )
        .await
        .unwrap();
}

async fn roll_up(env: &PollEnv) {
    let from = env.clock.now() - Duration::days(40);
    let to = env.clock.now() + Duration::hours(2);
    env.stores.station.refresh_rollups(from, to).await.unwrap();
}

type Stats = sekondo::store::ItemStatsRecord;

async fn stats_of(env: &PollEnv, type_id: i64) -> Option<Stats> {
    env.stores
        .station
        .item_stats_row(FORGE, JITA_44, type_id)
        .await
        .unwrap()
}

fn close(actual: Option<f64>, expected: f64, tolerance: f64) {
    let actual = actual.expect("a value");
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} is not within {tolerance} of {expected}"
    );
}

// ---- book fields: after a book refresh ----

#[tokio::test]
async fn a_book_refresh_fills_the_book_fields_and_nothing_else() {
    let env = PollEnv::new().await;
    tradable_34(&env).await;

    env.station.refresh().await;

    let stats = stats_of(&env, 34).await.expect("a row");
    assert_eq!(stats.book_at, Some(env.clock.now()));
    assert_eq!(stats.best_buy.as_deref(), Some("100.00"));
    assert_eq!(stats.best_sell.as_deref(), Some("110.00"));
    assert_eq!(
        (stats.buy_near_orders, stats.sell_near_orders),
        (Some(2), Some(1))
    );
    assert_eq!(stats.rho, Some(0.5), "nothing taken yet");
    assert_eq!(stats.indicators_at, None);
    assert_eq!(stats.sigma, None);
    assert_eq!(stats.expansion_at, None);
}

#[tokio::test]
async fn the_buy_side_ratio_is_taken_from_thirty_days_of_snapshots() {
    let env = PollEnv::new().await;
    tradable_34(&env).await;
    // Two earlier hours: 30 units taken from buy orders and 70 from sell orders.
    snapshot_at(&env, env.clock.now() - Duration::hours(5), 34, (10, 20)).await;
    snapshot_at(&env, env.clock.now() - Duration::hours(3), 34, (20, 50)).await;
    roll_up(&env).await;

    env.station.refresh().await;

    close(stats_of(&env, 34).await.unwrap().rho, 0.3, 1e-12);
}

#[tokio::test]
async fn an_item_that_is_no_longer_tradable_loses_its_row() {
    let env = PollEnv::new().await;
    tradable_34(&env).await;
    env.station.refresh().await;
    assert!(stats_of(&env, 34).await.is_some());
    env.clock.advance(Duration::minutes(6));
    env.control(
        "market-orders",
        json!({"region_id": FORGE, "orders": [order(3, 34, false, 110.0, 200)]}),
    )
    .await;

    env.station.refresh().await;

    assert!(stats_of(&env, 34).await.is_none());
}

// ---- indicator fields: after a history pass ----

/// 30 days of history, alternating 100 and 110, the oldest day 100 and yesterday 110, with the
/// volume 1000 + the days back.
async fn alternating_history(env: &PollEnv, type_id: i64) {
    for back in 1..=30 {
        let average = if (30 - back) % 2 == 0 { 100.0 } else { 110.0 };
        history_row(
            env,
            type_id,
            back,
            average,
            average + 1.0,
            average - 1.0,
            1000 + back,
        )
        .await;
    }
}

#[tokio::test]
async fn a_history_pass_fills_the_indicator_fields() {
    let env = PollEnv::new().await;
    tradable_34(&env).await;
    env.station.refresh().await;
    alternating_history(&env, 34).await;

    stats::refresh_indicators(&*env.stores.station, env.clock.now())
        .await
        .unwrap();

    let stats = stats_of(&env, 34).await.unwrap();
    assert_eq!(stats.indicators_at, Some(env.clock.now()));
    // Days 1..=7 back: (1001 + ... + 1007) / 7, and days 1..=30 back: (30000 + 465) / 30.
    close(stats.volume_7d, 1004.0, 1e-9);
    close(stats.volume_30d, 1015.5, 1e-9);
    // 29 returns of +-ln(1.1): s = ln(1.1) * sqrt((29 - 1/29) / 28).
    assert_eq!(stats.returns, Some(29));
    close(stats.sigma, 0.09694, 1e-4);
    // Last five (110, 100, 110, 100, 110) average 106; last twenty average 105.
    close(stats.sma5, 106.0, 1e-9);
    close(stats.sma20, 105.0, 1e-9);
    // Bands 95 and 115 around 105; yesterday's 110 sits at 0.75.
    close(stats.percent_b, 0.75, 1e-9);
    // 106 is above 105 now, but the day before the SMA 5 was 104: it crossed today (yesterday's
    // history day is the latest).
    assert_eq!(stats.cross_day, Some(day(&env, 1)));
    // The older days traded more, so the last five days are below the last twenty.
    assert_eq!(stats.volume_rising, Some(false));
    let rsi = stats.rsi14.expect("an RSI");
    assert!(rsi > 40.0 && rsi < 60.0, "{rsi}");
}

#[tokio::test]
async fn a_history_pass_leaves_the_book_fields_and_a_book_refresh_the_indicators() {
    let env = PollEnv::new().await;
    tradable_34(&env).await;
    env.station.refresh().await;
    let book_at = env.clock.now();
    alternating_history(&env, 34).await;
    env.clock.advance(Duration::minutes(1));
    stats::refresh_indicators(&*env.stores.station, env.clock.now())
        .await
        .unwrap();
    let indicators_at = env.clock.now();

    let after_history = stats_of(&env, 34).await.unwrap();
    assert_eq!(after_history.book_at, Some(book_at), "the book fields stay");
    assert_eq!(after_history.indicators_at, Some(indicators_at));

    env.clock.advance(Duration::minutes(6));
    env.station.refresh().await;

    let after_book = stats_of(&env, 34).await.unwrap();
    assert_eq!(after_book.book_at, Some(env.clock.now()));
    assert_eq!(
        after_book.indicators_at,
        Some(indicators_at),
        "the indicators stay"
    );
    close(after_book.sigma, 0.09694, 1e-4);
}

#[tokio::test]
async fn too_little_history_leaves_the_indicators_empty() {
    let env = PollEnv::new().await;
    tradable_34(&env).await;
    env.station.refresh().await;
    // 12 days: 11 returns, but no 20-day figures.
    for back in 1..=12 {
        let average = if back % 2 == 0 { 100.0 } else { 110.0 };
        history_row(&env, 34, back, average, average + 1.0, average - 1.0, 500).await;
    }

    stats::refresh_indicators(&*env.stores.station, env.clock.now())
        .await
        .unwrap();

    let stats = stats_of(&env, 34).await.unwrap();
    assert_eq!(stats.returns, Some(11));
    assert!(stats.sigma.is_some());
    assert_eq!(
        (stats.sma20, stats.percent_b, stats.cross_day),
        (None, None, None)
    );
    assert!(stats.sma5.is_some());
    close(stats.volume_7d, 500.0, 1e-9);
    close(stats.volume_30d, 500.0 * 12.0 / 30.0, 1e-9);
}

#[tokio::test]
async fn an_item_with_five_days_of_history_has_no_volatility() {
    let env = PollEnv::new().await;
    tradable_34(&env).await;
    env.station.refresh().await;
    for back in 1..=5 {
        history_row(&env, 34, back, 100.0, 101.0, 99.0, 500).await;
    }

    stats::refresh_indicators(&*env.stores.station, env.clock.now())
        .await
        .unwrap();

    let stats = stats_of(&env, 34).await.unwrap();
    assert_eq!((stats.returns, stats.sigma), (Some(4), None));
}

// ---- flagging completed days ----

/// Midnight UTC of the day `back` days before today.
fn midnight(env: &PollEnv, back: i64) -> OffsetDateTime {
    day(env, back).midnight().assume_utc()
}

/// One snapshot of item 34 at `hour`:10 of the day `back` days ago, with these best prices.
async fn hour_snapshot(
    env: &PollEnv,
    back: i64,
    hour: i64,
    buy: f64,
    sell: f64,
    taken: (i64, i64),
) {
    env.stores
        .station
        .seed_snapshots(
            FORGE,
            JITA_44,
            &[sekondo::store::SeedSnapshotRow {
                recorded_at: midnight(env, back) + Duration::hours(hour) + Duration::minutes(10),
                type_id: 34,
                best_buy: buy,
                best_sell: sell,
                buy_near_units: 10,
                buy_near_orders: 2,
                sell_near_units: 20,
                sell_near_orders: 3,
                buy_units: 100,
                sell_units: 200,
                buy_taken: taken.0,
                sell_taken: taken.1,
            }],
        )
        .await
        .unwrap();
}

/// The hours `0..hours` of a day with a spread of 10%.
async fn quiet_day(env: &PollEnv, back: i64, hours: i64) {
    for hour in 0..hours {
        hour_snapshot(env, back, hour, 100.0, 110.0, (0, 0)).await;
    }
}

async fn flag(env: &PollEnv) -> usize {
    stats::flag_completed_days(&*env.stores.station, env.clock.now())
        .await
        .unwrap()
}

async fn flagged(env: &PollEnv) -> Vec<(Date, i32, i32)> {
    env.stores
        .station
        .flagged_days(FORGE, JITA_44, day(env, 400))
        .await
        .unwrap()
        .into_iter()
        .map(|f| (f.day, f.hours, f.expanded_hours))
        .collect()
}

#[tokio::test]
async fn a_completed_day_is_flagged_once_and_a_running_day_is_not() {
    let env = PollEnv::new().await;
    // Three days ago: a sell jump at 18:00. Two days ago: quiet, but the rollup ends at 23:00
    // that day, so it is not complete yet.
    quiet_day(&env, 3, 18).await;
    hour_snapshot(&env, 3, 18, 100.0, 140.0, (0, 50)).await;
    for hour in 19..24 {
        hour_snapshot(&env, 3, hour, 100.0, 110.0, (0, 0)).await;
    }
    quiet_day(&env, 2, 24).await;
    roll_up(&env).await;

    assert_eq!(flag(&env).await, 1, "only the third day is complete");

    assert_eq!(flagged(&env).await, [(day(&env, 3), 24, 1)]);
    let hours = env
        .stores
        .station
        .expanded_hours(FORGE, JITA_44, midnight(&env, 400))
        .await
        .unwrap();
    assert_eq!(hours.len(), 1);
    let hour = &hours[0].hour;
    assert_eq!(hour.start, midnight(&env, 3) + Duration::hours(18));
    assert_eq!(
        hour.direction,
        sekondo::opportunities::analysis::Direction::Spike
    );
    assert!((hour.ratio - 140.0 / 110.0).abs() < 1e-9, "{}", hour.ratio);
    assert_eq!((hour.extreme_cents, hour.taken), (14_000, 50));

    // An hour on the next day after its first hour completes the second day.
    hour_snapshot(&env, 1, 1, 100.0, 110.0, (0, 0)).await;
    roll_up(&env).await;
    assert_eq!(flag(&env).await, 1);
    assert_eq!(
        flagged(&env).await,
        [(day(&env, 3), 24, 1), (day(&env, 2), 24, 0)]
    );

    // Nothing new: nothing is flagged again.
    assert_eq!(flag(&env).await, 0);
    assert_eq!(flagged(&env).await.len(), 2);
}

#[tokio::test]
async fn the_first_run_flags_every_retained_day() {
    let env = PollEnv::new().await;
    for back in 2..=5 {
        quiet_day(&env, back, 24).await;
    }
    hour_snapshot(&env, 1, 2, 100.0, 110.0, (0, 0)).await;
    roll_up(&env).await;

    assert_eq!(flag(&env).await, 4);
    assert_eq!(flagged(&env).await.len(), 4);
    assert_eq!(flag(&env).await, 0);
}

#[tokio::test]
async fn a_day_with_nineteen_hours_is_flagged_but_has_no_expanded_hours() {
    let env = PollEnv::new().await;
    quiet_day(&env, 2, 18).await;
    hour_snapshot(&env, 2, 20, 100.0, 300.0, (0, 9)).await;
    hour_snapshot(&env, 1, 2, 100.0, 110.0, (0, 0)).await;
    roll_up(&env).await;

    assert_eq!(flag(&env).await, 1);

    assert_eq!(flagged(&env).await, [(day(&env, 2), 19, 0)]);
    assert!(
        env.stores
            .station
            .expanded_hours(FORGE, JITA_44, midnight(&env, 400))
            .await
            .unwrap()
            .is_empty()
    );
}

// ---- recurring windows: when the hourly rollup advanced ----

async fn refresh_expansions(env: &PollEnv) -> bool {
    stats::refresh_expansions_if_advanced(&*env.stores.station, env.clock.now())
        .await
        .unwrap()
}

/// An item row for item 34, so that there is something to compute the windows for.
async fn item_row(env: &PollEnv) {
    env.stores
        .station
        .seed_item_stats(
            FORGE,
            JITA_44,
            34,
            &Stats {
                book_at: Some(env.clock.now()),
                best_buy: Some("100.00".to_owned()),
                best_sell: Some("110.00".to_owned()),
                buy_near_orders: Some(1),
                sell_near_orders: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
}

async fn windows(env: &PollEnv) -> Vec<WindowRow> {
    env.stores.station.windows(FORGE, JITA_44).await.unwrap()
}

/// Item 34 on `days_back` days: a spike at 18:00 (the best sell jumps to 160.00, 100 units taken
/// from sell orders) and a dip at 11:00 (the best buy falls to 60.00, 40 units taken from buy
/// orders), the rest at 100.00 / 110.00.
async fn spike_and_dip_days(env: &PollEnv, days_back: std::ops::RangeInclusive<i64>) {
    for back in days_back {
        for hour in 0..24 {
            let (buy, sell, taken) = match hour {
                11 => (60.0, 110.0, (40, 0)),
                18 => (100.0, 160.0, (0, 100)),
                _ => (100.0, 110.0, (0, 0)),
            };
            hour_snapshot(env, back, hour, buy, sell, taken).await;
        }
    }
}

fn daily(hour: u8, direction: Direction, price_cents: i64, v_win: f64) -> WindowRow {
    WindowRow {
        type_id: 34,
        kind: WindowKind::HourOfDay { hour },
        direction,
        price_cents,
        occurrences: 8,
        covered: 8,
        v_win,
    }
}

#[tokio::test]
async fn an_item_with_a_spike_and_a_dip_window_stores_both() {
    let env = PollEnv::new().await;
    item_row(&env).await;
    spike_and_dip_days(&env, 2..=9).await;
    hour_snapshot(&env, 1, 1, 100.0, 110.0, (0, 0)).await;
    roll_up(&env).await;

    assert!(refresh_expansions(&env).await);

    // The dip: one tick above 0.6 x the median best buy of 100.00 (60.00 -> 60.01). The spike: one
    // tick below 160.00 / 110.00 x the median best sell of 110.00 (160.00 -> 159.90).
    assert_eq!(
        windows(&env).await,
        [
            daily(11, Direction::Dip, 6_001, 40.0),
            daily(18, Direction::Spike, 15_990, 100.0)
        ]
    );
}

#[tokio::test]
async fn an_item_that_turns_jumpy_loses_its_windows() {
    let env = PollEnv::new().await;
    item_row(&env).await;
    spike_and_dip_days(&env, 5..=12).await;
    hour_snapshot(&env, 4, 1, 100.0, 110.0, (0, 0)).await;
    roll_up(&env).await;
    assert!(refresh_expansions(&env).await);
    assert_eq!(windows(&env).await.len(), 2);

    // Three days on which 11 of 24 hours jump: 49 of 264 hours (19%) have expanded.
    for back in 2..=4 {
        for hour in 0..24 {
            let sell = if hour < 11 { 160.0 } else { 110.0 };
            hour_snapshot(&env, back, hour, 100.0, sell, (0, 5)).await;
        }
    }
    hour_snapshot(&env, 1, 1, 100.0, 110.0, (0, 0)).await;
    roll_up(&env).await;
    assert!(refresh_expansions(&env).await);

    assert_eq!(windows(&env).await, []);
}

#[tokio::test]
async fn windows_follow_the_hourly_rollup_and_nothing_else() {
    let env = PollEnv::new().await;
    tradable_34(&env).await;
    env.station.refresh().await;
    // Without a rollup there is nothing to advance.
    assert!(!refresh_expansions(&env).await);
    assert_eq!(stats_of(&env, 34).await.unwrap().expansion_at, None);

    // Own snapshots of a single hour, rolled up: computed, with no window, but with a marker.
    snapshot_at(&env, env.clock.now() - Duration::hours(3), 34, (0, 0)).await;
    roll_up(&env).await;
    assert!(refresh_expansions(&env).await);
    assert_eq!(windows(&env).await, []);
    let marker = stats_of(&env, 34)
        .await
        .unwrap()
        .expansion_at
        .expect("the rollup it was computed for");

    // Nothing advanced: a book refresh leaves the marker alone, and no recompute runs.
    env.clock.advance(Duration::minutes(6));
    env.station.refresh().await;
    assert!(!refresh_expansions(&env).await);
    assert_eq!(stats_of(&env, 34).await.unwrap().expansion_at, Some(marker));

    // A newer hour rolls up: computed again.
    env.clock.advance(Duration::hours(2));
    snapshot_at(&env, env.clock.now(), 34, (0, 0)).await;
    roll_up(&env).await;
    assert!(refresh_expansions(&env).await);
    assert!(stats_of(&env, 34).await.unwrap().expansion_at > Some(marker));
}
