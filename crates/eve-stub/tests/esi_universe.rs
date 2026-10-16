mod common;

use common::Fixture;
use eve_stub::TokenOptions;
use serde_json::{Value, json};

const STRUCTURE: i64 = 1000000000001;

async fn control(f: &Fixture, path: &str, body: Value) -> reqwest::Response {
    f.client
        .post(f.url(&format!("/_stub/{path}")))
        .bearer_auth(common::CONTROL_TOKEN)
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn names(f: &Fixture, ids: &[i64]) -> reqwest::Response {
    f.client
        .post(f.url("/universe/names"))
        .json(&ids)
        .send()
        .await
        .unwrap()
}

async fn structure(f: &Fixture, id: i64, token: Option<&str>) -> reqwest::Response {
    let mut request = f.client.get(f.url(&format!("/universe/structures/{id}")));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    request.send().await.unwrap()
}

#[tokio::test]
async fn names_are_answered_from_the_fixture_table() {
    let f = common::start().await;

    let res = names(&f, &[34, 10000002, 60003760]).await;

    assert_eq!(res.status(), 200);
    let body: Vec<Value> = res.json().await.unwrap();
    let find = |id: i64| body.iter().find(|n| n["id"] == id).unwrap();
    assert_eq!(find(34)["name"], "Tritanium");
    assert_eq!(find(34)["category"], "inventory_type");
    assert_eq!(find(10000002)["name"], "The Forge");
    assert_eq!(find(10000002)["category"], "region");
    assert_eq!(find(60003760)["category"], "station");
    assert!(find(60003760)["name"].as_str().unwrap().starts_with("Jita"));
}

#[tokio::test]
async fn one_unknown_id_fails_the_whole_names_request() {
    let f = common::start().await;
    let res = names(&f, &[34, 999999999]).await;
    assert_eq!(res.status(), 404);
}

#[tokio::test]
async fn names_requests_are_recorded() {
    let f = common::start().await;
    names(&f, &[34, 35]).await;
    names(&f, &[34, 999999999]).await;
    assert_eq!(f.stub.esi_calls("names"), 2);
    assert_eq!(
        f.stub.names_requests(),
        vec![vec![34, 35], vec![34, 999999999]]
    );
}

#[tokio::test]
async fn structures_need_a_token_with_the_structures_scope() {
    let f = common::start().await;
    let token = f.stub.issue_access_token(90000001).unwrap();

    let res = structure(&f, STRUCTURE, Some(&token)).await;
    assert_eq!(res.status(), 200);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["name"], "Alpha Keepstar");

    assert_eq!(structure(&f, STRUCTURE, None).await.status(), 401);
    let orders_only = f
        .stub
        .issue_token(
            90000001,
            &TokenOptions {
                scopes: Some(vec!["esi-markets.read_character_orders.v1".to_owned()]),
                ..TokenOptions::default()
            },
        )
        .unwrap();
    assert_eq!(
        structure(&f, STRUCTURE, Some(&orders_only)).await.status(),
        403
    );
    assert_eq!(f.stub.esi_calls("structures"), 3);
}

#[tokio::test]
async fn an_unknown_structure_is_not_found() {
    let f = common::start().await;
    let token = f.stub.issue_access_token(90000001).unwrap();
    assert_eq!(
        structure(&f, 1000000999999, Some(&token)).await.status(),
        404
    );
}

#[tokio::test]
async fn structure_access_can_be_denied_and_allowed_again() {
    let f = common::start().await;
    let token = f.stub.issue_access_token(90000001).unwrap();

    let res = control(
        &f,
        "structure-access",
        json!({"structure_id": STRUCTURE, "allowed": false}),
    )
    .await;
    assert_eq!(res.status(), 204);
    assert_eq!(structure(&f, STRUCTURE, Some(&token)).await.status(), 403);

    control(
        &f,
        "structure-access",
        json!({"structure_id": STRUCTURE, "allowed": true}),
    )
    .await;
    assert_eq!(structure(&f, STRUCTURE, Some(&token)).await.status(), 200);
}

#[tokio::test]
async fn the_next_esi_calls_can_be_made_to_fail() {
    let f = common::start().await;
    let token = f.stub.issue_access_token(90000001).unwrap();

    let res = control(&f, "esi-fail-next", json!({"count": 2, "status": 503})).await;
    assert_eq!(res.status(), 204);

    // Any ESI route fails, and failures are counted as calls.
    assert_eq!(names(&f, &[34]).await.status(), 503);
    assert_eq!(structure(&f, STRUCTURE, Some(&token)).await.status(), 503);
    assert_eq!(names(&f, &[34]).await.status(), 200);
    assert_eq!(f.stub.esi_calls("names"), 2);
}

#[tokio::test]
async fn failures_can_carry_rate_limit_headers() {
    let f = common::start().await;
    control(
        &f,
        "esi-fail-next",
        json!({"status": 429, "headers": {"retry-after": "60", "x-ratelimit-group": "char-market"}}),
    )
    .await;
    let res = names(&f, &[34]).await;
    assert_eq!(res.status(), 429);
    assert_eq!(res.headers()["retry-after"], "60");
    assert_eq!(res.headers()["x-ratelimit-group"], "char-market");

    control(
        &f,
        "esi-fail-next",
        json!({"status": 420, "headers": {"x-esi-error-limit-remain": "0", "x-esi-error-limit-reset": "30"}}),
    )
    .await;
    let res = names(&f, &[34]).await;
    assert_eq!(res.status(), 420);
    assert_eq!(res.headers()["x-esi-error-limit-reset"], "30");

    // Only the one call failed.
    assert_eq!(names(&f, &[34]).await.status(), 200);
}

#[tokio::test]
async fn reset_clears_failures_denials_and_records() {
    let f = common::start().await;
    let token = f.stub.issue_access_token(90000001).unwrap();
    control(&f, "esi-fail-next", json!({"count": 5, "status": 500})).await;
    control(
        &f,
        "structure-access",
        json!({"structure_id": STRUCTURE, "allowed": false}),
    )
    .await;
    names(&f, &[34]).await;

    control(&f, "reset", json!({})).await;

    assert_eq!(f.stub.esi_calls("names"), 0);
    assert!(f.stub.names_requests().is_empty());
    assert_eq!(names(&f, &[34]).await.status(), 200);
    assert_eq!(structure(&f, STRUCTURE, Some(&token)).await.status(), 200);
}
