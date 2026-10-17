use std::time::Duration;

use eve_stub::{Stub, StubConfig, TokenOptions, pki::TestPki};
use secrecy::SecretString;
use sekondo::sso::{SsoClient, TokenError};
use url::Url;

const CLIENT_ID: &str = "test-client";
const ALICE: i64 = 90000001;

struct Env {
    stub: Stub,
    sso: SsoClient,
}

async fn start_stub(pki: &TestPki, config: StubConfig) -> Stub {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    Stub::start(
        config,
        listener,
        pki.cert_pem.clone().into_bytes(),
        pki.key_pem.clone().into_bytes(),
    )
    .await
    .unwrap()
}

fn client_for(pki: &TestPki, stub: &Stub, min_refetch: Duration) -> SsoClient {
    sekondo::tls::install_crypto_provider();
    SsoClient::new(
        pki.client(),
        Url::parse(&stub.base_url).unwrap(),
        CLIENT_ID.to_owned(),
        SecretString::from("test-secret".to_owned()),
        min_refetch,
    )
}

async fn env() -> Env {
    env_with(
        StubConfig::new(CLIENT_ID, "test-secret", "ctl"),
        Duration::ZERO,
    )
    .await
}

async fn env_with(config: StubConfig, min_refetch: Duration) -> Env {
    let pki = TestPki::generate();
    let stub = start_stub(&pki, config).await;
    let sso = client_for(&pki, &stub, min_refetch);
    Env { stub, sso }
}

#[tokio::test]
async fn a_valid_token_yields_the_character() {
    let e = env().await;
    let token = e.stub.issue_access_token(ALICE).unwrap();
    let verified = e.sso.validate(&token).await.unwrap();
    assert_eq!(verified.character_id, ALICE);
    assert_eq!(verified.name, "Alice Alpha");
    assert_eq!(verified.owner_hash, "owner-hash-90000001");
    assert_eq!(
        verified.scopes,
        [
            "esi-markets.read_character_orders.v1",
            "esi-universe.read_structures.v1",
            "esi-wallet.read_character_wallet.v1",
            "esi-assets.read_assets.v1"
        ]
    );
    assert!(verified.missing_scopes().is_empty());
    let remaining = verified.expires_at - time::OffsetDateTime::now_utc();
    assert!(remaining.whole_seconds() > 1000 && remaining.whole_seconds() <= 1200);
}

#[tokio::test]
async fn scp_is_read_as_a_string_an_array_or_absent() {
    let e = env().await;
    let one = TokenOptions {
        scopes: Some(vec!["esi-universe.read_structures.v1".into()]),
        ..Default::default()
    };
    let verified = e
        .sso
        .validate(&e.stub.issue_token(ALICE, &one).unwrap())
        .await
        .unwrap();
    assert_eq!(verified.scopes, ["esi-universe.read_structures.v1"]);
    assert_eq!(
        verified.missing_scopes(),
        [
            "esi-markets.read_character_orders.v1",
            "esi-wallet.read_character_wallet.v1",
            "esi-assets.read_assets.v1"
        ]
    );

    let none = TokenOptions {
        omit_scopes: true,
        ..Default::default()
    };
    let verified = e
        .sso
        .validate(&e.stub.issue_token(ALICE, &none).unwrap())
        .await
        .unwrap();
    assert!(verified.scopes.is_empty());
    assert_eq!(verified.missing_scopes().len(), 4);
}

#[tokio::test]
async fn a_token_signed_with_another_key_is_rejected() {
    let e = env().await;
    let other = env().await; // same kid string, different key
    let forged = other.stub.issue_access_token(ALICE).unwrap();
    assert!(matches!(
        e.sso.validate(&forged).await,
        Err(TokenError::Invalid(_))
    ));
}

#[tokio::test]
async fn a_tampered_payload_is_rejected() {
    let e = env().await;
    let token = e.stub.issue_access_token(ALICE).unwrap();
    let mut parts: Vec<&str> = token.split('.').collect();
    let evil = {
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        let mut json: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        json["sub"] = "CHARACTER:EVE:1".into();
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json).unwrap())
    };
    parts[1] = &evil;
    assert!(e.sso.validate(&parts.join(".")).await.is_err());
}

#[tokio::test]
async fn wrong_audience_is_rejected() {
    let e = env().await;
    for audience in [
        vec!["someone-else".to_owned(), "EVE Online".to_owned()],
        vec![CLIENT_ID.to_owned()],
    ] {
        let options = TokenOptions {
            audience: Some(audience.clone()),
            ..Default::default()
        };
        let token = e.stub.issue_token(ALICE, &options).unwrap();
        assert!(e.sso.validate(&token).await.is_err(), "{audience:?}");
    }
}

#[tokio::test]
async fn an_expired_token_is_rejected() {
    let e = env().await;
    let options = TokenOptions {
        expires_in_secs: Some(-3600),
        ..Default::default()
    };
    let token = e.stub.issue_token(ALICE, &options).unwrap();
    assert!(matches!(
        e.sso.validate(&token).await,
        Err(TokenError::Invalid(_))
    ));
}

#[tokio::test]
async fn the_issuer_is_accepted_with_and_without_the_scheme_but_nothing_else() {
    let with = env().await;
    assert!(
        with.sso
            .validate(&with.stub.issue_access_token(ALICE).unwrap())
            .await
            .is_ok()
    );

    let mut config = StubConfig::new(CLIENT_ID, "test-secret", "ctl");
    config.issuer_with_scheme = false;
    let without = env_with(config, Duration::ZERO).await;
    assert!(
        without
            .sso
            .validate(&without.stub.issue_access_token(ALICE).unwrap())
            .await
            .is_ok()
    );

    let options = TokenOptions {
        issuer: Some("https://evil.example".into()),
        ..Default::default()
    };
    let token = with.stub.issue_token(ALICE, &options).unwrap();
    assert!(with.sso.validate(&token).await.is_err());
}

#[tokio::test]
async fn garbage_is_rejected() {
    let e = env().await;
    for token in ["", "abc", "a.b.c", "eyJhbGciOiJub25lIn0.e30."] {
        assert!(e.sso.validate(token).await.is_err(), "{token:?}");
    }
}

#[tokio::test]
async fn the_jwks_are_cached_and_refetched_for_an_unknown_kid() {
    let e = env().await;
    e.sso
        .validate(&e.stub.issue_access_token(ALICE).unwrap())
        .await
        .unwrap();
    e.sso
        .validate(&e.stub.issue_access_token(ALICE).unwrap())
        .await
        .unwrap();
    assert_eq!(
        e.stub.jwks_calls(),
        1,
        "the second validation uses the cache"
    );

    e.stub.rotate_key();
    e.sso
        .validate(&e.stub.issue_access_token(ALICE).unwrap())
        .await
        .unwrap();
    assert_eq!(e.stub.jwks_calls(), 2, "an unknown kid triggers a refetch");
}

#[tokio::test]
async fn refetching_for_unknown_kids_is_throttled() {
    let e = env_with(
        StubConfig::new(CLIENT_ID, "test-secret", "ctl"),
        Duration::from_secs(3600),
    )
    .await;
    e.sso
        .validate(&e.stub.issue_access_token(ALICE).unwrap())
        .await
        .unwrap();
    e.stub.rotate_key();
    let result = e
        .sso
        .validate(&e.stub.issue_access_token(ALICE).unwrap())
        .await;
    assert!(matches!(result, Err(TokenError::UnknownKey)));
    assert_eq!(
        e.stub.jwks_calls(),
        1,
        "no second fetch within the throttle window"
    );
}

#[tokio::test]
async fn an_unreachable_sso_is_reported_as_a_jwks_error() {
    let pki = TestPki::generate();
    let stub = start_stub(&pki, StubConfig::new(CLIENT_ID, "test-secret", "ctl")).await;
    let token = stub.issue_access_token(ALICE).unwrap();
    let sso = client_for(&pki, &stub, Duration::ZERO);
    drop(stub);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(matches!(
        sso.validate(&token).await,
        Err(TokenError::Jwks(_))
    ));
}

#[tokio::test]
async fn a_rotated_key_under_the_same_kid_is_picked_up_after_a_signature_failure() {
    let e = env().await;
    e.sso
        .validate(&e.stub.issue_access_token(ALICE).unwrap())
        .await
        .unwrap();
    assert_eq!(e.stub.jwks_calls(), 1);

    e.stub.rotate_key_keeping_kid(); // the cached key no longer matches, the kid does
    let token = e.stub.issue_access_token(ALICE).unwrap();
    e.sso
        .validate(&token)
        .await
        .expect("one refetch and retry fixes it");
    assert_eq!(e.stub.jwks_calls(), 2);
}

#[tokio::test]
async fn a_forged_signature_does_not_loop_and_is_throttled() {
    let e = env_with(
        StubConfig::new(CLIENT_ID, "test-secret", "ctl"),
        Duration::from_secs(3600),
    )
    .await;
    e.sso
        .validate(&e.stub.issue_access_token(ALICE).unwrap())
        .await
        .unwrap();
    let other = env().await; // same kid, other key: a forgery
    let forged = other.stub.issue_access_token(ALICE).unwrap();

    for _ in 0..3 {
        assert!(matches!(
            e.sso.validate(&forged).await,
            Err(TokenError::Invalid(_))
        ));
    }
    assert_eq!(
        e.stub.jwks_calls(),
        1,
        "forged tokens cannot make the app hammer SSO"
    );
}

#[tokio::test]
async fn a_forged_signature_costs_at_most_one_refetch_when_not_throttled() {
    let e = env().await;
    e.sso
        .validate(&e.stub.issue_access_token(ALICE).unwrap())
        .await
        .unwrap();
    let other = env().await;
    let forged = other.stub.issue_access_token(ALICE).unwrap();
    assert!(matches!(
        e.sso.validate(&forged).await,
        Err(TokenError::Invalid(_))
    ));
    assert_eq!(
        e.stub.jwks_calls(),
        2,
        "exactly one refetch per failed validation"
    );
}
