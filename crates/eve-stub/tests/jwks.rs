mod common;

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde_json::Value;

#[tokio::test]
async fn issued_token_verifies_against_the_jwks_served_over_tls() {
    let f = common::start().await;

    let jwks: Value = f
        .client
        .get(f.url("/oauth/jwks"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let key = &jwks["keys"][0];
    assert_eq!(key["kty"], "RSA");
    assert_eq!(key["alg"], "RS256");
    let kid = key["kid"].as_str().unwrap();

    let token = f.stub.issue_access_token(90000001).unwrap();
    let header = jsonwebtoken::decode_header(&token).unwrap();
    assert_eq!(header.kid.as_deref(), Some(kid));

    let decoding =
        DecodingKey::from_rsa_components(key["n"].as_str().unwrap(), key["e"].as_str().unwrap())
            .unwrap();
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[common::CLIENT_ID]);
    validation.set_issuer(&[f.stub.base_url.as_str()]);
    let claims = decode::<Value>(&token, &decoding, &validation)
        .unwrap()
        .claims;

    assert_eq!(claims["sub"], "CHARACTER:EVE:90000001");
    assert_eq!(claims["name"], "Alice Alpha");
    assert_eq!(claims["owner"], "owner-hash-90000001");
    assert_eq!(
        claims["aud"],
        serde_json::json!([common::CLIENT_ID, "EVE Online"])
    );
    assert!(claims["exp"].as_u64().unwrap() > claims["iat"].as_u64().unwrap());
    assert_eq!(claims["scp"][0], "esi-markets.read_character_orders.v1");
    assert_eq!(claims["scp"][1], "esi-universe.read_structures.v1");
}

#[tokio::test]
async fn a_token_signed_by_another_key_does_not_verify() {
    let a = common::start().await;
    let b = common::start().await;
    let jwks_b: Value = b
        .client
        .get(b.url("/oauth/jwks"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let key = &jwks_b["keys"][0];
    let token = a.stub.issue_access_token(90000001).unwrap();
    let decoding =
        DecodingKey::from_rsa_components(key["n"].as_str().unwrap(), key["e"].as_str().unwrap())
            .unwrap();
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[common::CLIENT_ID]);
    assert!(decode::<Value>(&token, &decoding, &validation).is_err());
}

#[tokio::test]
async fn single_scope_is_a_string_and_unknown_character_has_no_token() {
    let mut config = eve_stub::StubConfig::new(
        common::CLIENT_ID,
        common::CLIENT_SECRET,
        common::CONTROL_TOKEN,
    );
    config.characters[0].scopes = vec!["esi-universe.read_structures.v1".to_owned()];
    let f = common::start_with(config).await;
    let token = f.stub.issue_access_token(90000001).unwrap();
    let payload = token.split('.').nth(1).unwrap();
    let json: Value = serde_json::from_slice(
        &base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, payload)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(json["scp"], "esi-universe.read_structures.v1");
    assert!(f.stub.issue_access_token(1).is_none());
}

#[tokio::test]
async fn plain_http_is_not_served() {
    let f = common::start().await;
    let plain = format!("http://127.0.0.1:{}/oauth/jwks", f.stub.addr.port());
    assert!(reqwest::Client::new().get(plain).send().await.is_err());
}
