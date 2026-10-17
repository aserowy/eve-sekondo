mod common;

use common::Fixture;
use eve_stub::TokenOptions;
use serde_json::{Value, json};

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const ORDERS_SCOPE: &str = "esi-markets.read_character_orders.v1";

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

async fn get(f: &Fixture, path: &str, token: Option<&str>) -> reqwest::Response {
    let mut request = f.client.get(f.url(path));
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

fn asset(id: i64) -> Value {
    json!({
        "item_id": id, "type_id": 34, "location_id": 60003760, "location_type": "station",
        "location_flag": "Hangar", "quantity": 1, "is_singleton": false,
    })
}

fn narrow_token(f: &Fixture, character: i64) -> String {
    f.stub
        .issue_token(
            character,
            &TokenOptions {
                scopes: Some(vec![ORDERS_SCOPE.to_owned()]),
                ..TokenOptions::default()
            },
        )
        .unwrap()
}

#[tokio::test]
async fn the_wallet_is_a_bare_number_that_a_control_can_replace() {
    let f = common::start().await;
    let token = f.stub.issue_access_token(ALICE).unwrap();
    let res = get(&f, &format!("/characters/{ALICE}/wallet"), Some(&token)).await;
    assert_eq!(res.status(), 200);
    assert!(res.headers().contains_key("expires"));
    assert_eq!(res.json::<f64>().await.unwrap(), 2_500_000.0);

    control(
        &f,
        "wallet",
        json!({"character_id": ALICE, "balance": 12.34}),
    )
    .await;
    let res = get(&f, &format!("/characters/{ALICE}/wallet"), Some(&token)).await;
    assert_eq!(res.json::<f64>().await.unwrap(), 12.34);
}

#[tokio::test]
async fn the_wallet_is_forbidden_without_the_scope_or_for_another_character() {
    let f = common::start().await;
    let path = format!("/characters/{ALICE}/wallet");
    assert_eq!(
        get(&f, &path, Some(&narrow_token(&f, ALICE)))
            .await
            .status(),
        403
    );
    let bob = f.stub.issue_access_token(BOB).unwrap();
    assert_eq!(get(&f, &path, Some(&bob)).await.status(), 403);
    assert_eq!(get(&f, &path, None).await.status(), 401);
}

#[tokio::test]
async fn assets_come_in_pages_of_a_thousand_with_the_page_count_in_a_header() {
    let f = common::start().await;
    let assets: Vec<Value> = (1..=1001).map(asset).collect();
    control(
        &f,
        "assets",
        json!({"character_id": ALICE, "assets": assets}),
    )
    .await;
    let token = f.stub.issue_access_token(ALICE).unwrap();

    let first = get(&f, &format!("/characters/{ALICE}/assets"), Some(&token)).await;
    assert_eq!(first.headers()["x-pages"], "2");
    assert_eq!(first.json::<Vec<Value>>().await.unwrap().len(), 1000);
    let second = get(
        &f,
        &format!("/characters/{ALICE}/assets?page=2"),
        Some(&token),
    )
    .await;
    assert_eq!(second.json::<Vec<Value>>().await.unwrap().len(), 1);
}

#[tokio::test]
async fn assets_are_forbidden_without_the_scope() {
    let f = common::start().await;
    let res = get(
        &f,
        &format!("/characters/{ALICE}/assets"),
        Some(&narrow_token(&f, ALICE)),
    )
    .await;
    assert_eq!(res.status(), 403);
}

#[tokio::test]
async fn prices_need_no_token_and_a_control_replaces_them() {
    let f = common::start().await;
    let res = get(&f, "/markets/prices", None).await;
    assert_eq!(res.status(), 200);
    assert!(!res.json::<Vec<Value>>().await.unwrap().is_empty());

    control(
        &f,
        "prices",
        json!({"prices": [{"type_id": 34, "average_price": 11.0}]}),
    )
    .await;
    let body: Vec<Value> = get(&f, "/markets/prices", None).await.json().await.unwrap();
    assert_eq!(body, [json!({"type_id": 34, "average_price": 11.0})]);
}

#[tokio::test]
async fn an_injected_failure_hits_the_new_routes_and_is_counted() {
    let f = common::start().await;
    let token = f.stub.issue_access_token(ALICE).unwrap();
    for (route, path) in [
        ("wallet", format!("/characters/{ALICE}/wallet")),
        ("assets", format!("/characters/{ALICE}/assets")),
        ("prices", "/markets/prices".to_owned()),
    ] {
        control(&f, "esi-fail-next", json!({"route": route, "status": 503})).await;
        assert_eq!(get(&f, &path, Some(&token)).await.status(), 503, "{route}");
        assert_eq!(get(&f, &path, Some(&token)).await.status(), 200, "{route}");
        assert_eq!(stats(&f).await[format!("{route}_calls")], 2, "{route}");
    }
}

#[tokio::test]
async fn esi_expires_applies_to_the_new_routes() {
    let f = common::start().await;
    control(&f, "esi-expires", json!({"seconds": 3600})).await;
    let res = get(&f, "/markets/prices", None).await;
    let expires = httpdate::parse_http_date(res.headers()["expires"].to_str().unwrap()).unwrap();
    let ahead = expires
        .duration_since(std::time::SystemTime::now())
        .unwrap();
    assert!(ahead.as_secs() > 3500, "{ahead:?}");
}
