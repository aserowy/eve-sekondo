#![allow(dead_code)]

use eve_stub::{Stub, StubConfig, pki::TestPki};

pub const CLIENT_ID: &str = "test-client";
pub const CLIENT_SECRET: &str = "test-secret";
pub const CONTROL_TOKEN: &str = "test-control-token";
pub const REDIRECT_URI: &str = "https://localhost:9/auth/callback";

pub struct Fixture {
    pub stub: Stub,
    pub client: reqwest::Client,
}

pub async fn start() -> Fixture {
    start_with(StubConfig::new(CLIENT_ID, CLIENT_SECRET, CONTROL_TOKEN)).await
}

pub async fn start_with(config: StubConfig) -> Fixture {
    esi_contract::gate::require_verified_stub();
    start_unverified_with(config).await
}

/// Starts the stub without asking whether it was verified: for `tests/contract.rs`, which the
/// verification itself needs to run on a stub that is not verified yet.
pub async fn start_unverified() -> Fixture {
    start_unverified_with(StubConfig::new(CLIENT_ID, CLIENT_SECRET, CONTROL_TOKEN)).await
}

async fn start_unverified_with(config: StubConfig) -> Fixture {
    let pki = TestPki::generate();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let stub = Stub::start(
        config,
        listener,
        pki.cert_pem.clone().into_bytes(),
        pki.key_pem.clone().into_bytes(),
    )
    .await
    .unwrap();
    Fixture {
        client: pki.client(),
        stub,
    }
}

impl Fixture {
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.stub.base_url)
    }
}

/// PKCE S256 challenge for a verifier.
pub fn challenge(verifier: &str) -> String {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use sha2::{Digest, Sha256};
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub const SCOPES: &str = "esi-markets.read_character_orders.v1 esi-universe.read_structures.v1 esi-wallet.read_character_wallet.v1 esi-assets.read_assets.v1 esi-skills.read_skills.v1 esi-characters.read_standings.v1";
pub const VERIFIER: &str = "a-pkce-verifier-that-is-long-enough-0123456789abcdef";

impl Fixture {
    pub fn authorize_query(&self, state: &str) -> Vec<(&'static str, String)> {
        vec![
            ("response_type", "code".to_owned()),
            ("client_id", CLIENT_ID.to_owned()),
            ("redirect_uri", REDIRECT_URI.to_owned()),
            ("scope", SCOPES.to_owned()),
            ("state", state.to_owned()),
            ("code_challenge", challenge(VERIFIER)),
            ("code_challenge_method", "S256".to_owned()),
        ]
    }

    /// Picks a character on the authorize page and returns the (code, state) from the redirect.
    pub async fn pick(&self, character_id: i64, state: &str) -> (String, String) {
        let mut query = self.authorize_query(state);
        query.push(("character_id", character_id.to_string()));
        let res = self
            .client
            .get(self.url("/v2/oauth/authorize/select"))
            .query(&query)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 302);
        let location = res.headers()["location"].to_str().unwrap().to_owned();
        let url = url::Url::parse(&location).unwrap();
        assert!(location.starts_with(REDIRECT_URI), "{location}");
        let get = |k: &str| {
            url.query_pairs()
                .find(|(n, _)| n == k)
                .unwrap()
                .1
                .into_owned()
        };
        (get("code"), get("state"))
    }

    pub async fn token_request(&self, params: &[(&str, &str)]) -> reqwest::Response {
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(params)
            .finish();
        self.client
            .post(self.url("/v2/oauth/token"))
            .basic_auth(CLIENT_ID, Some(CLIENT_SECRET))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .unwrap()
    }

    pub async fn exchange(&self, code: &str) -> serde_json::Value {
        let res = self
            .token_request(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("code_verifier", VERIFIER),
                ("redirect_uri", REDIRECT_URI),
            ])
            .await;
        assert_eq!(res.status(), 200);
        res.json().await.unwrap()
    }
}
