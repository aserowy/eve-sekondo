mod common;

use std::collections::{HashMap, HashSet};

use axum::{Router, extract::State, routing::get};
use common::{AppBuilder, Browser, TestApp};
use sekondo::{account, app::AppState};
use sqlx::PgPool;
use tower_sessions::Session;
use url::Url;
use uuid::Uuid;

/// `/test/signin` signs a fresh account in without going through SSO.
fn routes() -> Router<AppState> {
    async fn signin(State(state): State<AppState>, session: Session) -> String {
        let id = Uuid::new_v4();
        account::sign_in(&session, id, state.clock.now())
            .await
            .unwrap();
        id.to_string()
    }
    Router::new().route("/test/signin", get(signin))
}

async fn app(pool: PgPool) -> TestApp {
    AppBuilder::new(pool)
        .routes(routes())
        .with_stub()
        .spawn()
        .await
}

fn params(location: &str) -> (Url, HashMap<String, String>) {
    let url = Url::parse(location).unwrap();
    let map = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    (url, map)
}

#[sqlx::test]
async fn login_redirects_to_the_sso_authorize_url_with_exactly_the_required_scopes(pool: PgPool) {
    let app = app(pool).await;
    let browser = app.browser();
    let res = browser.get(&app.url("/auth/login")).await;
    assert!(res.status().is_redirection());

    let (url, q) = params(&Browser::location(&res));
    assert_eq!(
        format!("{}://localhost:{}", url.scheme(), url.port().unwrap()),
        app.stub().base_url
    );
    assert_eq!(url.path(), "/v2/oauth/authorize");
    assert_eq!(q["response_type"], "code");
    assert_eq!(q["client_id"], common::CLIENT_ID);
    assert_eq!(q["redirect_uri"], format!("{}/auth/callback", app.base_url));
    let scopes: HashSet<_> = q["scope"].split(' ').collect();
    assert_eq!(
        scopes,
        HashSet::from([
            "esi-markets.read_character_orders.v1",
            "esi-universe.read_structures.v1",
            "esi-wallet.read_character_wallet.v1",
            "esi-assets.read_assets.v1"
        ])
    );
    assert_eq!(q["scope"].split(' ').count(), 4, "no duplicates or extras");
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["code_challenge"].len(), 43, "base64url SHA-256");
    assert!(q["state"].len() >= 32);
}

#[sqlx::test]
async fn every_attempt_gets_a_fresh_state_and_challenge(pool: PgPool) {
    let app = app(pool).await;
    let first = params(&Browser::location(
        &app.browser().get(&app.url("/auth/login")).await,
    ))
    .1;
    let second = params(&Browser::location(
        &app.browser().get(&app.url("/auth/login")).await,
    ))
    .1;
    assert_ne!(first["state"], second["state"]);
    assert_ne!(first["code_challenge"], second["code_challenge"]);
}

#[sqlx::test]
async fn add_without_a_session_redirects_to_the_landing_page(pool: PgPool) {
    let app = app(pool).await;
    let res = app.browser().get(&app.url("/auth/add")).await;
    assert!(res.status().is_redirection());
    assert_eq!(Browser::location(&res), "/");
}

#[sqlx::test]
async fn add_when_signed_in_starts_an_sso_flow(pool: PgPool) {
    let app = app(pool).await;
    let browser = app.browser();
    browser.get(&app.url("/test/signin")).await;
    let res = browser.get(&app.url("/auth/add")).await;
    assert!(res.status().is_redirection());
    let (url, q) = params(&Browser::location(&res));
    assert_eq!(url.path(), "/v2/oauth/authorize");
    assert_eq!(q["scope"].split(' ').count(), 4);
}

#[sqlx::test]
async fn login_when_already_signed_in_goes_to_the_dashboard(pool: PgPool) {
    let app = app(pool).await;
    let browser = app.browser();
    browser.get(&app.url("/test/signin")).await;
    let res = browser.get(&app.url("/auth/login")).await;
    assert!(res.status().is_redirection());
    assert_eq!(Browser::location(&res), "/dashboard");
}
