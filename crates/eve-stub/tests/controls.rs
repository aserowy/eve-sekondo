mod common;

use common::Fixture;
use serde_json::{Value, json};

async fn control(f: &Fixture, path: &str, body: Value) -> reqwest::Response {
    f.client
        .post(f.url(&format!("/_stub/{path}")))
        .bearer_auth(common::CONTROL_TOKEN)
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn login(f: &Fixture, character_id: i64) -> Value {
    let (code, _) = f.pick(character_id, "s").await;
    f.exchange(&code).await
}

async fn refresh(f: &Fixture, token: &str) -> reqwest::Response {
    f.token_request(&[("grant_type", "refresh_token"), ("refresh_token", token)])
        .await
}

fn claims(token: &str) -> Value {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    serde_json::from_slice(
        &URL_SAFE_NO_PAD
            .decode(token.split('.').nth(1).unwrap())
            .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn controls_require_the_control_token() {
    let f = common::start().await;
    let none = f
        .client
        .post(f.url("/_stub/revoke"))
        .json(&json!({"character_id": 90000001}))
        .send()
        .await
        .unwrap();
    assert_eq!(none.status(), 401);
    let wrong = f
        .client
        .post(f.url("/_stub/revoke"))
        .bearer_auth("wrong")
        .json(&json!({"character_id": 90000001}))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 401);
}

#[tokio::test]
async fn revoke_makes_the_refresh_token_invalid_grant() {
    let f = common::start().await;
    let tokens = login(&f, 90000001).await;
    let other = login(&f, 90000002).await;

    assert_eq!(
        control(&f, "revoke", json!({"character_id": 90000001}))
            .await
            .status(),
        204
    );

    let res = refresh(&f, tokens["refresh_token"].as_str().unwrap()).await;
    assert_eq!(res.status(), 400);
    assert_eq!(res.json::<Value>().await.unwrap()["error"], "invalid_grant");
    // Only the revoked character is affected.
    assert_eq!(
        refresh(&f, other["refresh_token"].as_str().unwrap())
            .await
            .status(),
        200
    );
}

#[tokio::test]
async fn owner_hash_change_shows_up_in_new_tokens() {
    let f = common::start().await;
    let tokens = login(&f, 90000001).await;
    assert_eq!(
        claims(tokens["access_token"].as_str().unwrap())["owner"],
        "owner-hash-90000001"
    );

    let res = control(
        &f,
        "owner-hash",
        json!({"character_id": 90000001, "owner_hash": "sold-to-someone-else"}),
    )
    .await;
    assert_eq!(res.status(), 204);

    let refreshed: Value = refresh(&f, tokens["refresh_token"].as_str().unwrap())
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        claims(refreshed["access_token"].as_str().unwrap())["owner"],
        "sold-to-someone-else"
    );
    let fresh = login(&f, 90000001).await;
    assert_eq!(
        claims(fresh["access_token"].as_str().unwrap())["owner"],
        "sold-to-someone-else"
    );

    assert_eq!(
        control(
            &f,
            "owner-hash",
            json!({"character_id": 1, "owner_hash": "x"})
        )
        .await
        .status(),
        404
    );
}

#[tokio::test]
async fn reduced_scopes_apply_to_existing_refresh_tokens() {
    let f = common::start().await;
    let tokens = login(&f, 90000001).await;
    assert_eq!(
        claims(tokens["access_token"].as_str().unwrap())["scp"]
            .as_array()
            .unwrap()
            .len(),
        6
    );

    let res = control(
        &f,
        "scopes",
        json!({"character_id": 90000001, "scopes": ["esi-universe.read_structures.v1"]}),
    )
    .await;
    assert_eq!(res.status(), 204);

    let refreshed: Value = refresh(&f, tokens["refresh_token"].as_str().unwrap())
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        claims(refreshed["access_token"].as_str().unwrap())["scp"],
        "esi-universe.read_structures.v1"
    );
}

#[tokio::test]
async fn fail_next_token_returns_503_once_and_counts_calls() {
    let f = common::start().await;
    let tokens = login(&f, 90000001).await;
    let calls_before = f.stub.token_calls();

    assert_eq!(
        control(&f, "fail-next-token", json!({})).await.status(),
        204
    );
    let failed = refresh(&f, tokens["refresh_token"].as_str().unwrap()).await;
    assert_eq!(failed.status(), 503);

    // The failed call consumed nothing: the same refresh token still works.
    let ok = refresh(&f, tokens["refresh_token"].as_str().unwrap()).await;
    assert_eq!(ok.status(), 200);
    assert_eq!(f.stub.token_calls(), calls_before + 2);

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
    assert_eq!(stats["token_calls"].as_u64().unwrap(), calls_before + 2);
}

#[tokio::test]
async fn fail_next_token_can_fail_several_calls() {
    let f = common::start().await;
    control(&f, "fail-next-token", json!({"count": 2})).await;
    for _ in 0..2 {
        assert_eq!(refresh(&f, "anything").await.status(), 503);
    }
    assert_eq!(refresh(&f, "anything").await.status(), 400);
}

#[tokio::test]
async fn reset_restores_the_fixture() {
    let f = common::start().await;
    let tokens = login(&f, 90000001).await;
    control(
        &f,
        "owner-hash",
        json!({"character_id": 90000001, "owner_hash": "changed"}),
    )
    .await;
    control(
        &f,
        "scopes",
        json!({"character_id": 90000002, "scopes": []}),
    )
    .await;
    control(&f, "fail-next-token", json!({"count": 5})).await;

    assert_eq!(control(&f, "reset", json!({})).await.status(), 204);

    // Refresh tokens are gone, failures cleared, characters back to normal.
    let res = refresh(&f, tokens["refresh_token"].as_str().unwrap()).await;
    assert_eq!(res.status(), 400, "the token is invalid, not a 503");
    let fresh = login(&f, 90000001).await;
    let c = claims(fresh["access_token"].as_str().unwrap());
    assert_eq!(c["owner"], "owner-hash-90000001");
    let bob = login(&f, 90000002).await;
    assert_eq!(
        claims(bob["access_token"].as_str().unwrap())["scp"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
}
