mod common;

use common::PollEnv;
use sekondo::{clock::Clock, opportunities::stats};
use serde_json::{Value, json};
use sqlx::PgPool;
use time::{Date, Duration, OffsetDateTime, Weekday};

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
    sqlx::query(
        "INSERT INTO market_history (region_id, type_id, day, average, highest, lowest, volume, order_count)
         VALUES ($1, $2, $3, $4::numeric, $5::numeric, $6::numeric, $7, 10)",
    )
    .bind(FORGE)
    .bind(type_id)
    .bind(day(env, back))
    .bind(avg)
    .bind(high)
    .bind(low)
    .bind(volume)
    .execute(&env.pool)
    .await
    .unwrap();
}

async fn snapshot_at(env: &PollEnv, at: OffsetDateTime, type_id: i64, taken: (i64, i64)) {
    sqlx::query(
        "INSERT INTO station_snapshots (recorded_at, region_id, station_id, type_id, best_buy, best_sell,
             spread, buy_near_units, buy_near_orders, sell_near_units, sell_near_orders,
             buy_units, sell_units, buy_taken, sell_taken)
         VALUES ($1, $2, $3, $4, 100, 110, 0.1, 10, 2, 20, 3, 100, 200, $5, $6)",
    )
    .bind(at)
    .bind(FORGE)
    .bind(JITA_44)
    .bind(type_id)
    .bind(taken.0)
    .bind(taken.1)
    .execute(&env.pool)
    .await
    .unwrap();
}

async fn roll_up(env: &PollEnv) {
    let from = env.clock.now() - Duration::days(40);
    let to = env.clock.now() + Duration::hours(2);
    for view in ["station_hourly", "station_daily"] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "CALL refresh_continuous_aggregate('{view}', '{from}'::timestamptz, '{to}'::timestamptz)"
        )))
        .execute(&env.pool)
        .await
        .unwrap();
    }
}

#[derive(Debug, sqlx::FromRow)]
struct Stats {
    book_at: Option<OffsetDateTime>,
    best_buy: Option<String>,
    best_sell: Option<String>,
    buy_near_orders: Option<i32>,
    sell_near_orders: Option<i32>,
    rho: Option<f64>,
    indicators_at: Option<OffsetDateTime>,
    volume_7d: Option<f64>,
    volume_30d: Option<f64>,
    sigma: Option<f64>,
    returns: Option<i32>,
    sma5: Option<f64>,
    sma20: Option<f64>,
    cross_day: Option<Date>,
    volume_rising: Option<bool>,
    percent_b: Option<f64>,
    rsi14: Option<f64>,
    expansion_at: Option<OffsetDateTime>,
    expansion_kind: Option<String>,
    expansion_weekday: Option<i16>,
    expansion_hour: Option<i16>,
    expansion_sell: Option<String>,
}

async fn stats_of(env: &PollEnv, type_id: i64) -> Option<Stats> {
    sqlx::query_as(
        "SELECT book_at, best_buy::text, best_sell::text, buy_near_orders, sell_near_orders, rho,
                indicators_at, volume_7d, volume_30d, sigma, returns, sma5, sma20, cross_day,
                volume_rising, percent_b, rsi14, expansion_at, expansion_kind, expansion_weekday,
                expansion_hour, expansion_sell::text
         FROM station_item_stats WHERE type_id = $1",
    )
    .bind(type_id)
    .fetch_optional(&env.pool)
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

#[sqlx::test]
async fn a_book_refresh_fills_the_book_fields_and_nothing_else(pool: PgPool) {
    let env = PollEnv::new(pool).await;
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

#[sqlx::test]
async fn the_buy_side_ratio_is_taken_from_thirty_days_of_snapshots(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    tradable_34(&env).await;
    // Two earlier hours: 30 units taken from buy orders and 70 from sell orders.
    snapshot_at(&env, env.clock.now() - Duration::hours(5), 34, (10, 20)).await;
    snapshot_at(&env, env.clock.now() - Duration::hours(3), 34, (20, 50)).await;
    roll_up(&env).await;

    env.station.refresh().await;

    close(stats_of(&env, 34).await.unwrap().rho, 0.3, 1e-12);
}

#[sqlx::test]
async fn an_item_that_is_no_longer_tradable_loses_its_row(pool: PgPool) {
    let env = PollEnv::new(pool).await;
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

#[sqlx::test]
async fn a_history_pass_fills_the_indicator_fields(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    tradable_34(&env).await;
    env.station.refresh().await;
    alternating_history(&env, 34).await;

    stats::refresh_indicators(&env.pool, env.clock.now())
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

#[sqlx::test]
async fn a_history_pass_leaves_the_book_fields_and_a_book_refresh_the_indicators(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    tradable_34(&env).await;
    env.station.refresh().await;
    let book_at = env.clock.now();
    alternating_history(&env, 34).await;
    env.clock.advance(Duration::minutes(1));
    stats::refresh_indicators(&env.pool, env.clock.now())
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

#[sqlx::test]
async fn too_little_history_leaves_the_indicators_empty(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    tradable_34(&env).await;
    env.station.refresh().await;
    // 12 days: 11 returns, but no 20-day figures.
    for back in 1..=12 {
        let average = if back % 2 == 0 { 100.0 } else { 110.0 };
        history_row(&env, 34, back, average, average + 1.0, average - 1.0, 500).await;
    }

    stats::refresh_indicators(&env.pool, env.clock.now())
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

#[sqlx::test]
async fn an_item_with_five_days_of_history_has_no_volatility(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    tradable_34(&env).await;
    env.station.refresh().await;
    for back in 1..=5 {
        history_row(&env, 34, back, 100.0, 101.0, 99.0, 500).await;
    }

    stats::refresh_indicators(&env.pool, env.clock.now())
        .await
        .unwrap();

    let stats = stats_of(&env, 34).await.unwrap();
    assert_eq!((stats.returns, stats.sigma), (Some(4), None));
}

// ---- expansion fields: when the hourly rollup advanced ----

/// The most recent dates before today on this weekday, nearest first.
fn recent(env: &PollEnv, weekday: Weekday, count: usize) -> Vec<i64> {
    (1..=28)
        .filter(|back| day(env, *back).weekday() == weekday)
        .take(count)
        .collect()
}

#[sqlx::test]
async fn expansion_fields_follow_the_hourly_rollup_and_nothing_else(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    tradable_34(&env).await;
    env.station.refresh().await;
    // 28 days of history with a spread of 5%, except three of the last four Tuesdays.
    let tuesdays = recent(&env, Weekday::Tuesday, 4);
    let peaks = [122.5, 121.8, 120.4];
    for back in 1..=28 {
        let expanded = tuesdays[..3].iter().position(|t| *t == back);
        let high = expanded.map_or(105.0, |i| peaks[i]);
        history_row(&env, 34, back, 102.0, high, 100.0, 500).await;
    }
    // Without a rollup there is nothing to advance.
    assert!(
        !stats::refresh_expansions_if_advanced(&env.pool, env.clock.now())
            .await
            .unwrap()
    );
    assert_eq!(stats_of(&env, 34).await.unwrap().expansion_at, None);

    // Own snapshots of a single hour, rolled up.
    let first_hour = env.clock.now() - Duration::hours(3);
    snapshot_at(&env, first_hour, 34, (0, 0)).await;
    roll_up(&env).await;
    assert!(
        stats::refresh_expansions_if_advanced(&env.pool, env.clock.now())
            .await
            .unwrap()
    );

    let stats = stats_of(&env, 34).await.unwrap();
    // Less than 7 days of own snapshots: a weekday window from the daily figures. P25 of
    // 120.40, 121.80 and 122.50 is 121.10; one tick below is 121.00.
    assert_eq!(stats.expansion_kind.as_deref(), Some("weekday"));
    assert_eq!(
        stats.expansion_weekday,
        Some(1),
        "Tuesday, counted from Monday"
    );
    assert_eq!(stats.expansion_hour, None);
    assert_eq!(stats.expansion_sell.as_deref(), Some("121.00"));
    let marker = stats.expansion_at.expect("the rollup it was computed for");

    // Nothing advanced: a book refresh leaves the expansion fields alone, and no recompute runs.
    env.clock.advance(Duration::minutes(6));
    env.station.refresh().await;
    assert!(
        !stats::refresh_expansions_if_advanced(&env.pool, env.clock.now())
            .await
            .unwrap()
    );
    assert_eq!(stats_of(&env, 34).await.unwrap().expansion_at, Some(marker));

    // A newer hour rolls up: the fields are computed again.
    env.clock.advance(Duration::hours(2));
    snapshot_at(&env, env.clock.now(), 34, (0, 0)).await;
    roll_up(&env).await;
    assert!(
        stats::refresh_expansions_if_advanced(&env.pool, env.clock.now())
            .await
            .unwrap()
    );
    assert!(stats_of(&env, 34).await.unwrap().expansion_at > Some(marker));
}

#[sqlx::test]
async fn an_item_without_a_recurring_window_has_empty_expansion_fields_but_a_marker(pool: PgPool) {
    let env = PollEnv::new(pool).await;
    tradable_34(&env).await;
    env.station.refresh().await;
    for back in 1..=28 {
        history_row(&env, 34, back, 102.0, 105.0, 100.0, 500).await;
    }
    snapshot_at(&env, env.clock.now() - Duration::hours(3), 34, (0, 0)).await;
    roll_up(&env).await;

    assert!(
        stats::refresh_expansions_if_advanced(&env.pool, env.clock.now())
            .await
            .unwrap()
    );

    let stats = stats_of(&env, 34).await.unwrap();
    assert_eq!(stats.expansion_kind, None);
    assert_eq!(stats.expansion_sell, None);
    assert!(stats.expansion_at.is_some());
    assert!(
        !stats::refresh_expansions_if_advanced(&env.pool, env.clock.now())
            .await
            .unwrap()
    );
}
