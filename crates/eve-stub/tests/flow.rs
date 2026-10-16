mod common;

use serde_json::Value;

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
async fn authorize_page_lists_the_fixture_characters() {
    let f = common::start().await;
    let res = f
        .client
        .get(f.url("/v2/oauth/authorize"))
        .query(&f.authorize_query("s1"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();
    for name in ["Alice Alpha", "Bob Bravo", "Carol Charlie"] {
        assert!(html.contains(name), "{name} missing");
    }
    assert!(html.contains("/v2/oauth/authorize/select?character_id=90000001"));
    assert!(!html.contains("<script"), "the picker needs no scripts");
}

#[tokio::test]
async fn authorize_rejects_invalid_requests() {
    let f = common::start().await;
    for (drop, why) in [
        ("state", "no state"),
        ("code_challenge", "no PKCE"),
        ("redirect_uri", "no redirect"),
    ] {
        let query: Vec<_> = f
            .authorize_query("s1")
            .into_iter()
            .filter(|(k, _)| *k != drop)
            .collect();
        let res = f
            .client
            .get(f.url("/v2/oauth/authorize"))
            .query(&query)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 400, "{why}");
    }
    let mut query = f.authorize_query("s1");
    query.iter_mut().find(|(k, _)| *k == "client_id").unwrap().1 = "someone-else".into();
    let res = f
        .client
        .get(f.url("/v2/oauth/authorize"))
        .query(&query)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400, "unknown client");
}

#[tokio::test]
async fn authorization_code_grant_issues_tokens_for_the_picked_character() {
    let f = common::start().await;
    let (code, state) = f.pick(90000002, "state-123").await;
    assert_eq!(state, "state-123");

    let body = f.exchange(&code).await;
    assert_eq!(body["token_type"], "Bearer");
    assert_eq!(body["expires_in"], 1199);
    assert!(!body["refresh_token"].as_str().unwrap().is_empty());
    let c = claims(body["access_token"].as_str().unwrap());
    assert_eq!(c["sub"], "CHARACTER:EVE:90000002");
    assert_eq!(c["name"], "Bob Bravo");
}

#[tokio::test]
async fn wrong_pkce_verifier_is_rejected() {
    let f = common::start().await;
    let (code, _) = f.pick(90000001, "s").await;
    let res = f
        .token_request(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            (
                "code_verifier",
                "not-the-verifier-not-the-verifier-not-the-verifier",
            ),
            ("redirect_uri", common::REDIRECT_URI),
        ])
        .await;
    assert_eq!(res.status(), 400);
    assert_eq!(res.json::<Value>().await.unwrap()["error"], "invalid_grant");
}

#[tokio::test]
async fn codes_are_single_use() {
    let f = common::start().await;
    let (code, _) = f.pick(90000001, "s").await;
    f.exchange(&code).await;
    let again = f
        .token_request(&[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("code_verifier", common::VERIFIER),
            ("redirect_uri", common::REDIRECT_URI),
        ])
        .await;
    assert_eq!(again.status(), 400);
}

#[tokio::test]
async fn refresh_grant_rotates_the_refresh_token() {
    let f = common::start().await;
    let (code, _) = f.pick(90000001, "s").await;
    let first = f.exchange(&code).await;
    let old = first["refresh_token"].as_str().unwrap();

    let res = f
        .token_request(&[("grant_type", "refresh_token"), ("refresh_token", old)])
        .await;
    assert_eq!(res.status(), 200);
    let second: Value = res.json().await.unwrap();
    let new = second["refresh_token"].as_str().unwrap();
    assert_ne!(old, new);
    assert_eq!(
        claims(second["access_token"].as_str().unwrap())["sub"],
        "CHARACTER:EVE:90000001"
    );

    // The old token is dead, the new one works.
    let replay = f
        .token_request(&[("grant_type", "refresh_token"), ("refresh_token", old)])
        .await;
    assert_eq!(replay.status(), 400);
    assert_eq!(
        replay.json::<Value>().await.unwrap()["error"],
        "invalid_grant"
    );
    let ok = f
        .token_request(&[("grant_type", "refresh_token"), ("refresh_token", new)])
        .await;
    assert_eq!(ok.status(), 200);
}

#[tokio::test]
async fn unknown_refresh_token_and_bad_client_credentials_are_rejected() {
    let f = common::start().await;
    let res = f
        .token_request(&[("grant_type", "refresh_token"), ("refresh_token", "nope")])
        .await;
    assert_eq!(res.status(), 400);

    let body = "grant_type=refresh_token&refresh_token=nope";
    let res = f
        .client
        .post(f.url("/v2/oauth/token"))
        .basic_auth(common::CLIENT_ID, Some("wrong-secret"))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 401);

    let res = f.token_request(&[("grant_type", "password")]).await;
    assert_eq!(res.status(), 400);
}

#[tokio::test]
async fn granted_scopes_are_the_intersection_of_requested_and_available() {
    let mut config = eve_stub::StubConfig::new(
        common::CLIENT_ID,
        common::CLIENT_SECRET,
        common::CONTROL_TOKEN,
    );
    config.characters[0].scopes = vec!["esi-universe.read_structures.v1".to_owned()];
    let f = common::start_with(config).await;
    let (code, _) = f.pick(90000001, "s").await;
    let body = f.exchange(&code).await;
    assert_eq!(
        claims(body["access_token"].as_str().unwrap())["scp"],
        "esi-universe.read_structures.v1"
    );
}
