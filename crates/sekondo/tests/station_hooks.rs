//! The `/_test/station-*` hooks exist only in builds with the `test-hooks` feature.
#![cfg(feature = "test-hooks")]

mod common;

use common::{AppBuilder, Browser, TestApp};
use serde_json::{Value, json};
use sqlx::PgPool;

const ALICE: i64 = 90000001;

async fn signed_in(app: &TestApp) -> (Browser, String) {
    let browser = app.browser();
    browser.sso_round_trip(app, "/auth/login", ALICE).await;
    let html = browser
        .get(&app.url("/characters"))
        .await
        .text()
        .await
        .unwrap();
    let token = html
        .split(r#"<meta name="csrf-token" content=""#)
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_owned();
    (browser, token)
}

async fn hook(
    app: &TestApp,
    browser: &Browser,
    token: &str,
    name: &str,
    body: Option<Value>,
) -> reqwest::Response {
    let mut request = browser
        .post(&app.url(&format!("/_test/{name}")))
        .header("origin", &app.base_url)
        .header("hx-request", "true")
        .header("x-csrf-token", token);
    if let Some(body) = body {
        request = request.json(&body);
    }
    browser.send(request).await
}

async fn count(pool: &PgPool, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
        .fetch_one(pool)
        .await
        .unwrap()
}

fn order(id: i64, buy: bool, price: f64) -> Value {
    json!({
        "order_id": id, "type_id": 34, "location_id": 60003760, "system_id": 30000142,
        "is_buy_order": buy, "price": price, "range": "region", "duration": 90,
        "issued": "2026-10-11T12:00:00Z", "min_volume": 1, "volume_remain": 5,
        "volume_total": 5,
    })
}

#[sqlx::test]
async fn the_book_hook_runs_one_refresh(pool: PgPool) {
    let app = AppBuilder::new(pool.clone()).with_stub().spawn().await;
    let (browser, token) = signed_in(&app).await;
    app.stub_control(
        "market-orders",
        json!({"region_id": 10000002, "orders": [order(1, true, 9.0), order(2, false, 10.0)]}),
    )
    .await;

    let res = hook(&app, &browser, &token, "station-book", None).await;

    assert_eq!(res.status(), 200);
    assert_eq!(res.text().await.unwrap(), "updated");
    assert_eq!(count(&pool, "station_orders").await, 2);
    assert_eq!(count(&pool, "station_snapshots").await, 1);
}

#[sqlx::test]
async fn the_history_hook_runs_until_nothing_is_due(pool: PgPool) {
    let app = AppBuilder::new(pool.clone()).with_stub().spawn().await;
    let (browser, token) = signed_in(&app).await;
    for (id, buy) in [(1, true), (2, false)] {
        sqlx::query(
            "INSERT INTO station_orders (region_id, station_id, order_id, type_id, is_buy_order, price, volume_remain)
             VALUES (10000002, 60003760, $1, 34, $2, 10, 5)",
        )
        .bind(id)
        .bind(buy)
        .execute(&pool)
        .await
        .unwrap();
    }

    let res = hook(&app, &browser, &token, "station-history", None).await;

    assert_eq!(res.status(), 200);
    assert_eq!(res.text().await.unwrap(), "1", "one item asked");
    assert_eq!(app.stub().esi_calls("history"), 1);
}

#[sqlx::test]
async fn the_seed_hook_inserts_snapshots_and_history_for_past_times(pool: PgPool) {
    let app = AppBuilder::new(pool.clone()).with_stub().spawn().await;
    let (browser, token) = signed_in(&app).await;

    let res = hook(
        &app,
        &browser,
        &token,
        "station-seed",
        Some(json!({
            "snapshots": [
                {"type_id": 34, "minutes_back": 300, "best_buy": 100.0, "best_sell": 110.0},
                {"type_id": 34, "minutes_back": 240, "best_buy": 100.0, "best_sell": 130.0,
                 "buy_taken": 4, "sell_taken": 6},
            ],
            "history": [
                {"type_id": 34, "days_back": 2, "average": 105.0, "highest": 110.0, "lowest": 100.0, "volume": 900},
                {"type_id": 34, "days_back": 1, "average": 106.0, "highest": 111.0, "lowest": 101.0, "volume": 800},
            ],
        })),
    )
    .await;

    assert_eq!(res.status(), 200);
    assert_eq!(res.text().await.unwrap(), "2 snapshots, 2 history days");
    assert_eq!(count(&pool, "station_snapshots").await, 2);
    assert_eq!(count(&pool, "market_history").await, 2);
    let (best_sell, taken): (String, i64) = sqlx::query_as(
        "SELECT best_sell::text, buy_taken + sell_taken FROM station_snapshots ORDER BY recorded_at DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((best_sell.as_str(), taken), ("130.00", 10));
    // Defaults fill what the body leaves out.
    let near: i64 = sqlx::query_scalar("SELECT min(buy_near_units) FROM station_snapshots")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(near > 0);
}

#[sqlx::test]
async fn the_rollup_hook_materializes_both_aggregates(pool: PgPool) {
    let app = AppBuilder::new(pool.clone()).with_stub().spawn().await;
    let (browser, token) = signed_in(&app).await;
    hook(
        &app,
        &browser,
        &token,
        "station-seed",
        Some(json!({"snapshots": [
            {"type_id": 34, "minutes_back": 3000, "best_buy": 100.0, "best_sell": 110.0},
        ]})),
    )
    .await;
    assert_eq!(count(&pool, "station_hourly").await, 0, "not rolled up yet");

    let res = hook(&app, &browser, &token, "station-rollups", None).await;

    assert_eq!(res.status(), 200);
    assert_eq!(count(&pool, "station_hourly").await, 1);
    assert_eq!(count(&pool, "station_daily").await, 1);
}
