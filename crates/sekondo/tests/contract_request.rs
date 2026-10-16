//! The authorization URL the app builds satisfies the contract's request variant.

use esi_contract::{check, contract};
use sekondo::oauth::{Intent, PendingFlow};
use serde_json::{Map, Value};
use url::Url;

#[test]
fn the_authorization_request_matches_its_contract() {
    let url = PendingFlow::new(Intent::Login).authorize_url(
        &Url::parse("https://login.example/").unwrap(),
        "client",
        "https://localhost:8443/auth/callback",
    );
    let query: Map<String, Value> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), Value::String(v.into_owned())))
        .collect();

    let violations = check(
        &contract("authorization"),
        "request",
        None,
        &http::HeaderMap::new(),
        Some(&Value::Object(query)),
    );

    assert!(violations.is_empty(), "{violations:?}");
}
