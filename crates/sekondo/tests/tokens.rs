mod common;

use std::{sync::Arc, time::Duration};

use common::ManualClock;
use eve_stub::{Stub, StubConfig, pki::TestPki};
use secrecy::{ExposeSecret, SecretString};
use sekondo::{
    config::SecretKey,
    crypto::TokenCipher,
    oauth::{Intent, PendingFlow},
    repo::{CharacterUpsert, Repo},
    sso::SsoClient,
    tokens::{TokenError, TokenService},
};
use serde_json::json;
use time::Duration as TimeDuration;
use url::Url;

const ALICE: i64 = 90000001;
const CLIENT_ID: &str = "test-client";
const CLIENT_SECRET: &str = "test-secret";
const CONTROL: &str = "ctl";
const KEY: [u8; 32] = [5u8; 32];

struct Env {
    stub: Stub,
    pki: &'static TestPki,
    stores: sekondo::store::Stores,
    sso: Arc<SsoClient>,
    repo: Repo,
    clock: Arc<ManualClock>,
    tokens: TokenService,
}

async fn env() -> Env {
    sekondo::tls::install_crypto_provider();
    let pki = common::pki();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let stub = Stub::start(
        StubConfig::new(CLIENT_ID, CLIENT_SECRET, CONTROL),
        listener,
        pki.cert_pem.clone().into_bytes(),
        pki.key_pem.clone().into_bytes(),
    )
    .await
    .unwrap();
    let sso = Arc::new(SsoClient::new(
        pki.client(),
        Url::parse(&stub.base_url).unwrap(),
        CLIENT_ID.to_owned(),
        SecretString::from(CLIENT_SECRET.to_owned()),
        Duration::ZERO,
    ));
    let stores = common::stores();
    let repo = Repo::new(stores.accounts.clone(), TokenCipher::new(&SecretKey(KEY)));
    let clock = ManualClock::new();
    let tokens = TokenService::new(repo.clone(), sso.clone(), clock.clone());
    Env {
        stub,
        pki,
        stores,
        sso,
        repo,
        clock,
        tokens,
    }
}

impl Env {
    /// Links a character through the stub's real authorization code flow.
    async fn link(&self, account: uuid::Uuid, character_id: i64) {
        let flow = PendingFlow::new(Intent::Login);
        let redirect = "https://localhost:9/auth/callback";
        let mut select = flow.authorize_url(
            &Url::parse(&self.stub.base_url).unwrap(),
            CLIENT_ID,
            redirect,
        );
        select.set_path("/v2/oauth/authorize/select");
        select
            .query_pairs_mut()
            .append_pair("character_id", &character_id.to_string());
        let res = self.pki.client().get(select).send().await.unwrap();
        let location = Url::parse(res.headers()["location"].to_str().unwrap()).unwrap();
        let code = location
            .query_pairs()
            .find(|(k, _)| k == "code")
            .unwrap()
            .1
            .into_owned();

        let tokens = self
            .sso
            .exchange_code(&code, &flow.code_verifier, redirect)
            .await
            .unwrap();
        let verified = self
            .sso
            .validate(tokens.access_token.expose_secret())
            .await
            .unwrap();
        self.repo
            .upsert_character(
                account,
                &CharacterUpsert {
                    character_id,
                    name: verified.name,
                    owner_hash: verified.owner_hash,
                    scopes: verified.scopes,
                    refresh_token: tokens.refresh_token.unwrap(),
                    access_token: tokens.access_token,
                    access_expires_at: verified.expires_at,
                },
            )
            .await
            .unwrap();
        // The stored expiry is on the real clock; line the test clock up with it.
    }

    async fn stub_control(&self, path: &str, body: serde_json::Value) {
        let res = self
            .pki
            .client()
            .post(format!("{}/_stub/{path}", self.stub.base_url))
            .bearer_auth(CONTROL)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert!(res.status().is_success());
    }

    async fn needs_relogin(&self, id: i64) -> bool {
        self.repo
            .find_character(id)
            .await
            .unwrap()
            .unwrap()
            .needs_relogin
    }
}

async fn linked() -> Env {
    let e = env().await;
    let account = e.repo.create_account().await.unwrap();
    e.link(account, ALICE).await;
    e
}

// ---- Access token refresh ----

#[tokio::test]
async fn a_valid_access_token_is_returned_without_contacting_sso() {
    let e = linked().await;
    let stored = e
        .repo
        .load_tokens(ALICE)
        .await
        .unwrap()
        .unwrap()
        .access_token
        .unwrap();
    let calls = e.stub.token_calls();

    let token = e.tokens.access_token(ALICE).await.unwrap();
    assert_eq!(token.expose_secret(), stored.expose_secret());
    assert_eq!(e.stub.token_calls(), calls, "no SSO request");

    e.clock.advance(TimeDuration::seconds(1100)); // still ~99 s left
    e.tokens.access_token(ALICE).await.unwrap();
    assert_eq!(
        e.stub.token_calls(),
        calls,
        "still no SSO request with more than 60 s left"
    );
}

#[tokio::test]
async fn an_expiring_access_token_is_refreshed_and_the_rotated_refresh_token_stored() {
    let e = linked().await;
    let before = e.repo.load_tokens(ALICE).await.unwrap().unwrap();
    let calls = e.stub.token_calls();

    e.clock.advance(TimeDuration::seconds(1150)); // within 60 s of expiry
    let token = e.tokens.access_token(ALICE).await.unwrap();

    assert_eq!(e.stub.token_calls(), calls + 1);
    assert_ne!(
        token.expose_secret(),
        before.access_token.unwrap().expose_secret()
    );
    e.sso
        .validate(token.expose_secret())
        .await
        .expect("the new access token is valid");

    let after = e.repo.load_tokens(ALICE).await.unwrap().unwrap();
    assert_ne!(
        after.refresh_token.expose_secret(),
        before.refresh_token.expose_secret(),
        "rotated"
    );
    assert_eq!(
        after.access_token.unwrap().expose_secret(),
        token.expose_secret()
    );
    assert!(!e.needs_relogin(ALICE).await);
}

#[tokio::test]
async fn an_access_token_whose_stored_expiry_has_passed_is_refreshed() {
    let e = linked().await;
    e.clock.advance(TimeDuration::hours(2));
    let calls = e.stub.token_calls();
    e.tokens.access_token(ALICE).await.unwrap();
    assert_eq!(e.stub.token_calls(), calls + 1);
}

#[tokio::test]
async fn simultaneous_requests_refresh_only_once() {
    let e = linked().await;
    // Expire the stored token in the database (not via the clock, which would also expire the new one).
    e.stores
        .accounts
        .seed_access_expiry(
            ALICE,
            time::OffsetDateTime::now_utc() - TimeDuration::hours(1),
        )
        .await
        .unwrap();
    let calls = e.stub.token_calls();

    let (a, b, c) = tokio::join!(
        e.tokens.access_token(ALICE),
        e.tokens.access_token(ALICE),
        e.tokens.access_token(ALICE)
    );
    // Racing refreshes would reuse a rotated-away refresh token and get rejected.
    let (a, b, c) = (a.unwrap(), b.unwrap(), c.unwrap());
    assert_eq!(e.stub.token_calls(), calls + 1, "one refresh for all three");
    assert_eq!(a.expose_secret(), b.expose_secret());
    assert_eq!(b.expose_secret(), c.expose_secret());
    assert!(!e.needs_relogin(ALICE).await);
}

#[tokio::test]
async fn an_unknown_character_is_reported() {
    let e = env().await;
    assert!(matches!(
        e.tokens.access_token(1).await,
        Err(TokenError::UnknownCharacter)
    ));
}

// ---- Needs re-login ----

#[tokio::test]
async fn a_revoked_refresh_token_marks_the_character_and_stops_contacting_sso() {
    let e = linked().await;
    e.stub_control("revoke", json!({"character_id": ALICE}))
        .await;
    e.clock.advance(TimeDuration::hours(2));

    assert!(matches!(
        e.tokens.access_token(ALICE).await,
        Err(TokenError::NeedsRelogin)
    ));
    assert!(e.needs_relogin(ALICE).await);

    // Marked characters never reach SSO again, even with an expired token.
    let calls = e.stub.token_calls();
    for _ in 0..3 {
        assert!(matches!(
            e.tokens.access_token(ALICE).await,
            Err(TokenError::NeedsRelogin)
        ));
    }
    assert_eq!(e.stub.token_calls(), calls);
}

#[tokio::test]
async fn a_marked_character_keeps_its_data() {
    let e = linked().await;
    e.stub_control("revoke", json!({"character_id": ALICE}))
        .await;
    e.clock.advance(TimeDuration::hours(2));
    let _ = e.tokens.access_token(ALICE).await;

    let character = e
        .repo
        .find_character(ALICE)
        .await
        .unwrap()
        .expect("not deleted");
    assert_eq!(character.name, "Alice Alpha");
    assert!(e.repo.load_tokens(ALICE).await.unwrap().is_some());
}

#[tokio::test]
async fn reduced_scopes_on_refresh_mark_the_character() {
    let e = linked().await;
    e.stub_control(
        "scopes",
        json!({"character_id": ALICE, "scopes": ["esi-universe.read_structures.v1"]}),
    )
    .await;
    e.clock.advance(TimeDuration::hours(2));

    assert!(matches!(
        e.tokens.access_token(ALICE).await,
        Err(TokenError::NeedsRelogin)
    ));
    let character = e.repo.find_character(ALICE).await.unwrap().unwrap();
    assert!(character.needs_relogin);
    assert_eq!(
        character.scopes,
        ["esi-universe.read_structures.v1"],
        "the granted scopes are recorded"
    );
}

#[tokio::test]
async fn a_character_linked_with_two_scopes_needs_relogin_until_it_logs_in_with_four() {
    let e = env().await;
    let account = e.repo.create_account().await.unwrap();
    let old = [
        "esi-markets.read_character_orders.v1",
        "esi-universe.read_structures.v1",
    ];
    e.stub_control("scopes", json!({"character_id": ALICE, "scopes": old}))
        .await;
    e.link(account, ALICE).await;
    e.clock.advance(TimeDuration::hours(2));

    assert!(matches!(
        e.tokens.access_token(ALICE).await,
        Err(TokenError::NeedsRelogin)
    ));
    assert!(e.needs_relogin(ALICE).await);

    e.stub_control(
        "scopes",
        json!({"character_id": ALICE, "scopes": sekondo::oauth::REQUIRED_SCOPES}),
    )
    .await;
    e.link(account, ALICE).await;
    assert!(!e.needs_relogin(ALICE).await);
    e.tokens
        .access_token(ALICE)
        .await
        .expect("four scopes pass");
}

#[tokio::test]
async fn a_character_without_the_optional_location_scope_does_not_need_relogin() {
    let e = env().await;
    let account = e.repo.create_account().await.unwrap();
    e.stub_control(
        "scopes",
        json!({"character_id": ALICE, "scopes": sekondo::oauth::REQUIRED_SCOPES}),
    )
    .await;
    e.link(account, ALICE).await;
    e.clock.advance(TimeDuration::hours(2));

    e.tokens
        .access_token(ALICE)
        .await
        .expect("the required scopes are enough");
    assert!(!e.needs_relogin(ALICE).await);
    let character = e.repo.list_characters(account).await.unwrap().remove(0);
    assert!(
        !character
            .scopes
            .iter()
            .any(|s| s == sekondo::oauth::LOCATION_SCOPE)
    );
}

#[tokio::test]
async fn a_character_linked_with_the_four_old_scopes_needs_relogin() {
    let e = env().await;
    let account = e.repo.create_account().await.unwrap();
    let old = [
        "esi-markets.read_character_orders.v1",
        "esi-universe.read_structures.v1",
        "esi-wallet.read_character_wallet.v1",
        "esi-assets.read_assets.v1",
    ];
    e.stub_control("scopes", json!({"character_id": ALICE, "scopes": old}))
        .await;
    e.link(account, ALICE).await;
    e.clock.advance(TimeDuration::hours(2));

    assert!(matches!(
        e.tokens.access_token(ALICE).await,
        Err(TokenError::NeedsRelogin)
    ));
    assert!(e.needs_relogin(ALICE).await);

    e.stub_control(
        "scopes",
        json!({"character_id": ALICE, "scopes": sekondo::oauth::REQUIRED_SCOPES}),
    )
    .await;
    e.link(account, ALICE).await;
    assert!(!e.needs_relogin(ALICE).await);
}

#[tokio::test]
async fn an_sso_server_error_is_temporary_and_does_not_mark_the_character() {
    let e = linked().await;
    e.clock.advance(TimeDuration::hours(2));
    e.stub_control("fail-next-token", json!({})).await;

    assert!(matches!(
        e.tokens.access_token(ALICE).await,
        Err(TokenError::Temporary(_))
    ));
    assert!(!e.needs_relogin(ALICE).await);

    // Retrying later works: the refresh token was not consumed.
    e.tokens
        .access_token(ALICE)
        .await
        .expect("the retry succeeds");
}

#[tokio::test]
async fn a_network_error_is_temporary_and_does_not_mark_the_character() {
    let e = linked().await;
    e.clock.advance(TimeDuration::hours(2));
    e.stub.shutdown();
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert!(matches!(
        e.tokens.access_token(ALICE).await,
        Err(TokenError::Temporary(_))
    ));
    assert!(!e.needs_relogin(ALICE).await);
}

#[tokio::test]
async fn undecryptable_tokens_are_treated_like_a_rejected_refresh() {
    let e = linked().await;
    // The encryption key changed since the tokens were stored.
    let other_key = Repo::new(
        e.stores.accounts.clone(),
        TokenCipher::new(&SecretKey([6u8; 32])),
    );
    let tokens = TokenService::new(other_key, e.sso.clone(), e.clock.clone());
    let calls = e.stub.token_calls();

    assert!(matches!(
        tokens.access_token(ALICE).await,
        Err(TokenError::NeedsRelogin)
    ));
    assert!(e.needs_relogin(ALICE).await);
    assert_eq!(e.stub.token_calls(), calls, "nothing to send to SSO");
}
