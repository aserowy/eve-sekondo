mod common;

use common::Fixture;
use eve_stub::TokenOptions;
use serde_json::{Value, json};

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const ORDERS_SCOPE: &str = "esi-markets.read_character_orders.v1";

async fn control(f: &Fixture, body: Value) {
    let res = f
        .client
        .post(f.url("/_stub/transactions"))
        .bearer_auth(common::CONTROL_TOKEN)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);
}

async fn get(f: &Fixture, query: &str, token: Option<&str>) -> reqwest::Response {
    let mut request = f
        .client
        .get(f.url(&format!("/characters/{ALICE}/wallet/transactions{query}")));
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    request.send().await.unwrap()
}

fn transaction(id: i64) -> Value {
    json!({
        "transaction_id": id, "date": "2026-10-11T12:00:00Z", "type_id": 34,
        "location_id": 60003760, "quantity": 10, "unit_price": 100.25,
        "is_buy": true, "is_personal": true, "client_id": 9, "journal_ref_id": id,
    })
}

fn ids(transactions: &[Value]) -> Vec<i64> {
    transactions
        .iter()
        .map(|t| t["transaction_id"].as_i64().unwrap())
        .collect()
}

#[tokio::test]
async fn without_a_fixture_the_list_is_empty() {
    let f = common::start().await;
    let token = f.stub.issue_access_token(ALICE).unwrap();

    let res = get(&f, "", Some(&token)).await;

    assert_eq!(res.status(), 200);
    assert!(res.headers().contains_key("expires"));
    assert_eq!(res.json::<Vec<Value>>().await.unwrap(), Vec::<Value>::new());
}

#[tokio::test]
async fn the_newest_come_first_up_to_the_limit_and_from_id_includes_the_one_it_names() {
    let f = common::start().await;
    // Handed over in any order: the stub answers newest (highest ID) first.
    let transactions: Vec<Value> = [3, 1, 5, 2, 4].into_iter().map(transaction).collect();
    control(
        &f,
        json!({"character_id": ALICE, "transactions": transactions, "limit": 2}),
    )
    .await;
    let token = f.stub.issue_access_token(ALICE).unwrap();

    let first: Vec<Value> = get(&f, "", Some(&token)).await.json().await.unwrap();
    assert_eq!(ids(&first), [5, 4]);
    let second: Vec<Value> = get(&f, "?from_id=3", Some(&token))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(ids(&second), [3, 2]);
    let third: Vec<Value> = get(&f, "?from_id=1", Some(&token))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(ids(&third), [1]);
    let past: Vec<Value> = get(&f, "?from_id=0", Some(&token))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(past, Vec::<Value>::new());
}

#[tokio::test]
async fn transactions_are_forbidden_without_the_wallet_scope_or_for_another_character() {
    let f = common::start().await;
    let narrow = f
        .stub
        .issue_token(
            ALICE,
            &TokenOptions {
                scopes: Some(vec![ORDERS_SCOPE.to_owned()]),
                ..TokenOptions::default()
            },
        )
        .unwrap();
    assert_eq!(get(&f, "", Some(&narrow)).await.status(), 403);
    let bob = f.stub.issue_access_token(BOB).unwrap();
    assert_eq!(get(&f, "", Some(&bob)).await.status(), 403);
    assert_eq!(get(&f, "", None).await.status(), 401);
}

#[tokio::test]
async fn an_injected_failure_hits_the_route_and_is_counted() {
    let f = common::start().await;
    let token = f.stub.issue_access_token(ALICE).unwrap();
    let res = f
        .client
        .post(f.url("/_stub/esi-fail-next"))
        .bearer_auth(common::CONTROL_TOKEN)
        .json(&json!({"route": "transactions", "status": 503}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);

    assert_eq!(get(&f, "", Some(&token)).await.status(), 503);
    assert_eq!(get(&f, "", Some(&token)).await.status(), 200);
    let stats: Value = f
        .client
        .get(f.url("/_stub/stats"))
        .bearer_auth(common::CONTROL_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(stats["transactions_calls"], 2);
}
