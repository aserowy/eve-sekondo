mod common;

use common::Fixture;
use eve_stub::TokenOptions;
use serde_json::{Value, json};

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const JITA: i64 = 60003760;
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
async fn skills_carry_active_and_trained_levels_and_a_control_replaces_them() {
    let f = common::start().await;
    let token = f.stub.issue_access_token(ALICE).unwrap();
    let path = format!("/characters/{ALICE}/skills");
    let res = get(&f, &path, Some(&token)).await;
    assert_eq!(res.status(), 200);
    assert!(res.headers().contains_key("expires"));
    let body: Value = res.json().await.unwrap();
    let accounting = body["skills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["skill_id"] == 16622)
        .unwrap();
    assert_eq!(accounting["active_skill_level"], 4);
    assert_eq!(accounting["trained_skill_level"], 5);

    control(
        &f,
        "skills",
        json!({"character_id": ALICE, "skills": [
            {"skill_id": 16622, "active_skill_level": 5, "trained_skill_level": 5},
        ]}),
    )
    .await;
    let body: Value = get(&f, &path, Some(&token)).await.json().await.unwrap();
    assert_eq!(body["skills"][0]["active_skill_level"], 5);
    assert_eq!(body["skills"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn standings_are_a_list_that_a_control_replaces() {
    let f = common::start().await;
    let token = f.stub.issue_access_token(ALICE).unwrap();
    let path = format!("/characters/{ALICE}/standings");
    let body: Vec<Value> = get(&f, &path, Some(&token)).await.json().await.unwrap();
    assert!(body.iter().any(|s| s["from_type"] == "npc_corp"));
    assert!(body.iter().any(|s| s["from_type"] == "faction"));

    control(
        &f,
        "standings",
        json!({"character_id": ALICE, "standings": [
            {"from_id": 1000035, "from_type": "npc_corp", "standing": 0.12345},
        ]}),
    )
    .await;
    let body: Vec<Value> = get(&f, &path, Some(&token)).await.json().await.unwrap();
    assert_eq!(
        body,
        [json!({"from_id": 1000035, "from_type": "npc_corp", "standing": 0.12345})]
    );
}

#[tokio::test]
async fn skills_and_standings_are_forbidden_without_the_scope_or_for_another_character() {
    let f = common::start().await;
    for what in ["skills", "standings"] {
        let path = format!("/characters/{ALICE}/{what}");
        let narrow = narrow_token(&f, ALICE);
        assert_eq!(get(&f, &path, Some(&narrow)).await.status(), 403, "{what}");
        let bob = f.stub.issue_access_token(BOB).unwrap();
        assert_eq!(get(&f, &path, Some(&bob)).await.status(), 403, "{what}");
        assert_eq!(get(&f, &path, None).await.status(), 401, "{what}");
    }
}

#[tokio::test]
async fn a_station_needs_no_token_and_names_its_owner() {
    let f = common::start().await;
    let res = get(&f, &format!("/universe/stations/{JITA}"), None).await;
    assert_eq!(res.status(), 200);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["owner"], 1000035);

    control(
        &f,
        "stations",
        json!({"station_id": 60099999, "owner": 1000002}),
    )
    .await;
    let body: Value = get(&f, "/universe/stations/60099999", None)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body["owner"], 1000002);

    assert_eq!(
        get(&f, "/universe/stations/60000001", None).await.status(),
        404
    );
}

#[tokio::test]
async fn an_injected_failure_hits_the_fee_routes_and_they_are_counted() {
    let f = common::start().await;
    let token = f.stub.issue_access_token(ALICE).unwrap();
    for (route, path) in [
        ("skills", format!("/characters/{ALICE}/skills")),
        ("standings", format!("/characters/{ALICE}/standings")),
        ("stations", format!("/universe/stations/{JITA}")),
    ] {
        control(&f, "esi-fail-next", json!({"route": route, "status": 503})).await;
        assert_eq!(get(&f, &path, Some(&token)).await.status(), 503, "{route}");
        assert_eq!(get(&f, &path, Some(&token)).await.status(), 200, "{route}");
        assert_eq!(f.stub.esi_calls(route), 2, "{route}");
    }
}
