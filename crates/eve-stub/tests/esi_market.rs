mod common;

use common::Fixture;
use serde_json::{Value, json};

const REGION: i64 = 10000002;

async fn control(f: &Fixture, path: &str, body: Value) {
    let res = f
        .client
        .post(f.url(&format!("/_stub/{path}")))
        .bearer_auth(common::CONTROL_TOKEN)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204, "control {path}");
}

async fn market(f: &Fixture, region: i64, query: &str) -> reqwest::Response {
    f.client
        .get(f.url(&format!("/markets/{region}/orders?{query}")))
        .send()
        .await
        .unwrap()
}

fn order(id: i64, type_id: i64, buy: bool) -> Value {
    json!({
        "order_id": id, "type_id": type_id, "location_id": 60003760, "system_id": 30000142,
        "is_buy_order": buy, "price": 10.0 + id as f64, "range": "station",
        "duration": 90, "issued": "2026-10-11T12:00:00Z", "min_volume": 1,
        "volume_remain": 3, "volume_total": 5,
    })
}

fn ids(orders: &[Value]) -> Vec<i64> {
    orders
        .iter()
        .map(|o| o["order_id"].as_i64().unwrap())
        .collect()
}

#[tokio::test]
async fn a_region_without_fixture_orders_has_one_empty_page() {
    let f = common::start().await;

    let res = market(&f, REGION, "order_type=all&type_id=34&page=1").await;

    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["x-pages"], "1");
    assert_eq!(res.json::<Vec<Value>>().await.unwrap(), Vec::<Value>::new());
}

#[tokio::test]
async fn the_answer_needs_no_token_and_carries_the_esi_headers() {
    let f = common::start().await;
    control(
        &f,
        "market-orders",
        json!({"region_id": REGION, "orders": [order(1, 34, false)]}),
    )
    .await;

    let res = market(&f, REGION, "order_type=all&type_id=34").await;

    assert_eq!(res.status(), 200);
    for name in [
        "expires",
        "date",
        "x-pages",
        "x-ratelimit-group",
        "x-ratelimit-remaining",
    ] {
        assert!(res.headers().contains_key(name), "missing {name}");
    }
    // Real ESI limits this route by rate and sends no error-limit headers.
    assert!(!res.headers().contains_key("x-esi-error-limit-remain"));
    assert!(!res.headers().contains_key("x-esi-error-limit-reset"));
    assert_eq!(ids(&res.json::<Vec<Value>>().await.unwrap()), [1]);
}

#[tokio::test]
async fn orders_are_filtered_by_region_item_and_side() {
    let f = common::start().await;
    control(
        &f,
        "market-orders",
        json!({"region_id": REGION, "orders": [
            order(1, 34, false), order(2, 34, true), order(3, 35, false),
        ]}),
    )
    .await;
    control(
        &f,
        "market-orders",
        json!({"region_id": 10000043, "orders": [order(4, 34, false)]}),
    )
    .await;

    let all = market(&f, REGION, "order_type=all&type_id=34").await;
    let sell = market(&f, REGION, "order_type=sell&type_id=34").await;
    let buy = market(&f, REGION, "order_type=buy&type_id=34").await;
    let other = market(&f, 10000043, "order_type=all&type_id=34").await;

    assert_eq!(ids(&all.json::<Vec<Value>>().await.unwrap()), [1, 2]);
    assert_eq!(ids(&sell.json::<Vec<Value>>().await.unwrap()), [1]);
    assert_eq!(ids(&buy.json::<Vec<Value>>().await.unwrap()), [2]);
    assert_eq!(ids(&other.json::<Vec<Value>>().await.unwrap()), [4]);
}

#[tokio::test]
async fn a_multi_page_answer_is_paged_with_x_pages() {
    let f = common::start().await;
    let orders: Vec<Value> = (1..=5).map(|id| order(id, 34, false)).collect();
    control(
        &f,
        "market-orders",
        json!({"region_id": REGION, "orders": orders, "page_size": 2}),
    )
    .await;

    let mut pages = Vec::new();
    for page in 1..=3 {
        let res = market(
            &f,
            REGION,
            &format!("order_type=all&type_id=34&page={page}"),
        )
        .await;
        assert_eq!(res.headers()["x-pages"], "3");
        pages.push(ids(&res.json::<Vec<Value>>().await.unwrap()));
    }
    // No page given means the first one.
    let first = market(&f, REGION, "order_type=all&type_id=34").await;

    assert_eq!(pages, [vec![1, 2], vec![3, 4], vec![5]]);
    assert_eq!(ids(&first.json::<Vec<Value>>().await.unwrap()), [1, 2]);
}

#[tokio::test]
async fn an_injected_failure_hits_the_market_route_and_calls_are_counted() {
    let f = common::start().await;
    control(
        &f,
        "esi-fail-next",
        json!({"route": "market", "status": 503}),
    )
    .await;

    let failed = market(&f, REGION, "order_type=all&type_id=34").await;
    let recovered = market(&f, REGION, "order_type=all&type_id=34").await;

    assert_eq!(failed.status(), 503);
    assert_eq!(recovered.status(), 200);
    assert_eq!(f.stub.esi_calls("market"), 2);
}

#[tokio::test]
async fn reset_clears_the_market_orders() {
    let f = common::start().await;
    control(
        &f,
        "market-orders",
        json!({"region_id": REGION, "orders": [order(1, 34, false)]}),
    )
    .await;
    control(&f, "reset", json!({})).await;

    let res = market(&f, REGION, "order_type=all&type_id=34").await;

    assert_eq!(res.json::<Vec<Value>>().await.unwrap(), Vec::<Value>::new());
}
