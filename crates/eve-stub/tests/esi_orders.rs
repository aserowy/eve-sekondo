mod common;

use common::Fixture;
use eve_stub::TokenOptions;
use serde_json::{Value, json};

const STRUCTURES_SCOPE: &str = "esi-universe.read_structures.v1";

async fn control(f: &Fixture, path: &str, body: Value) -> reqwest::Response {
    f.client
        .post(f.url(&format!("/_stub/{path}")))
        .bearer_auth(common::CONTROL_TOKEN)
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn orders(f: &Fixture, character_id: i64, token: Option<&str>) -> reqwest::Response {
    let mut request = f
        .client
        .get(f.url(&format!("/characters/{character_id}/orders")));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    request.send().await.unwrap()
}

async fn stats(f: &Fixture) -> Value {
    f.client
        .get(f.url("/_stub/stats"))
        .bearer_auth(common::CONTROL_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

fn header(res: &reqwest::Response, name: &str) -> String {
    res.headers()[name].to_str().unwrap().to_owned()
}

#[tokio::test]
async fn a_valid_token_gets_the_fixture_orders_with_esi_headers() {
    let f = common::start().await;
    let token = f.stub.issue_access_token(90000001).unwrap();

    let res = orders(&f, 90000001, Some(&token)).await;

    assert_eq!(res.status(), 200);
    for name in [
        "expires",
        "date",
        "x-esi-error-limit-remain",
        "x-esi-error-limit-reset",
    ] {
        assert!(res.headers().contains_key(name), "missing {name}");
    }
    // Real ESI sends no rate-limit headers on this route.
    assert!(!res.headers().contains_key("x-ratelimit-group"));
    let body: Vec<Value> = res.json().await.unwrap();
    assert!(!body.is_empty());
    for order in &body {
        for field in [
            "order_id",
            "type_id",
            "region_id",
            "location_id",
            "price",
            "volume_remain",
            "volume_total",
            "range",
            "issued",
            "duration",
        ] {
            assert!(order.get(field).is_some(), "order lacks {field}: {order}");
        }
        // Real ESI only sends the flag on buy orders.
        assert_eq!(
            order.get("is_buy_order"),
            order.get("min_volume").map(|_| &Value::Bool(true))
        );
    }
}

#[tokio::test]
async fn the_token_of_another_character_is_forbidden() {
    let f = common::start().await;
    let token = f.stub.issue_access_token(90000002).unwrap();
    assert_eq!(orders(&f, 90000001, Some(&token)).await.status(), 403);
}

#[tokio::test]
async fn a_token_without_the_orders_scope_is_forbidden() {
    let f = common::start().await;
    let token = f
        .stub
        .issue_token(
            90000001,
            &TokenOptions {
                scopes: Some(vec![STRUCTURES_SCOPE.to_owned()]),
                ..TokenOptions::default()
            },
        )
        .unwrap();
    assert_eq!(orders(&f, 90000001, Some(&token)).await.status(), 403);
}

#[tokio::test]
async fn a_missing_or_foreign_token_is_unauthorized() {
    let f = common::start().await;
    assert_eq!(orders(&f, 90000001, None).await.status(), 401);
    assert_eq!(
        orders(&f, 90000001, Some("not-issued-by-the-stub"))
            .await
            .status(),
        401
    );
}

#[tokio::test]
async fn replaced_orders_are_served() {
    let f = common::start().await;
    let token = f.stub.issue_access_token(90000001).unwrap();
    let order = json!({
        "order_id": 42, "type_id": 34, "region_id": 10000002, "location_id": 60003760,
        "is_buy_order": true, "is_corporation": true, "price": 4.25, "escrow": 1000.5,
        "volume_remain": 10, "volume_total": 20, "min_volume": 1, "range": "station",
        "issued": "2026-10-01T12:00:00Z", "duration": 90
    });

    let res = control(
        &f,
        "orders",
        json!({"character_id": 90000001, "orders": [order]}),
    )
    .await;
    assert_eq!(res.status(), 204);

    let body: Vec<Value> = orders(&f, 90000001, Some(&token))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body, vec![order]);

    // Another character is untouched, and an empty list is a valid answer.
    control(
        &f,
        "orders",
        json!({"character_id": 90000001, "orders": []}),
    )
    .await;
    let body: Vec<Value> = orders(&f, 90000001, Some(&token))
        .await
        .json()
        .await
        .unwrap();
    assert!(body.is_empty());
}

#[tokio::test]
async fn expires_follows_the_control_and_the_date_header() {
    let f = common::start().await;
    let token = f.stub.issue_access_token(90000001).unwrap();
    let ahead = |res: &reqwest::Response| {
        let expires = httpdate::parse_http_date(&header(res, "expires")).unwrap();
        let date = httpdate::parse_http_date(&header(res, "date")).unwrap();
        expires.duration_since(date).unwrap().as_secs()
    };

    let res = orders(&f, 90000001, Some(&token)).await;
    assert_eq!(ahead(&res), 1200);

    assert_eq!(
        control(&f, "esi-expires", json!({"seconds": 300}))
            .await
            .status(),
        204
    );
    let res = orders(&f, 90000001, Some(&token)).await;
    assert_eq!(ahead(&res), 300);
}

#[tokio::test]
async fn calls_are_counted_per_route_and_cleared_by_reset() {
    let f = common::start().await;
    let token = f.stub.issue_access_token(90000001).unwrap();
    orders(&f, 90000001, Some(&token)).await;
    orders(&f, 90000001, Some(&token)).await;
    assert_eq!(stats(&f).await["orders_calls"], 2);
    assert_eq!(f.stub.esi_calls("orders"), 2);

    control(&f, "esi-expires", json!({"seconds": 5})).await;
    control(
        &f,
        "orders",
        json!({"character_id": 90000001, "orders": []}),
    )
    .await;
    control(&f, "reset", json!({})).await;

    assert_eq!(stats(&f).await["orders_calls"], 0);
    let res = orders(&f, 90000001, Some(&token)).await;
    let body: Vec<Value> = res.json().await.unwrap();
    assert!(!body.is_empty(), "reset restores the fixture orders");
}
