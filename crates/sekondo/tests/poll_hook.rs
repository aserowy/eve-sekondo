//! `POST /_test/poll/{id}` exists only in builds with the `test-hooks` feature.
#![cfg(feature = "test-hooks")]

mod common;

use common::{AppBuilder, Browser, TestApp};
use sqlx::PgPool;

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;

async fn token(app: &TestApp, browser: &Browser) -> String {
    let html = browser
        .get(&app.url("/characters"))
        .await
        .text()
        .await
        .unwrap();
    html.split(r#"<meta name="csrf-token" content=""#)
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_owned()
}

async fn poll(app: &TestApp, browser: &Browser, token: &str, id: i64) -> reqwest::Response {
    browser
        .send(
            browser
                .post(&app.url(&format!("/_test/poll/{id}")))
                .header("origin", &app.base_url)
                .header("hx-request", "true")
                .header("x-csrf-token", token),
        )
        .await
}

async fn order_count(pool: &PgPool, character: i64) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM current_orders WHERE character_id = $1")
        .bind(character)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test]
async fn it_polls_a_character_of_the_signed_in_account(pool: PgPool) {
    let app = AppBuilder::new(pool.clone()).with_stub().spawn().await;
    let browser = app.browser();
    browser.sso_round_trip(&app, "/auth/login", ALICE).await;
    let token = token(&app, &browser).await;

    let res = poll(&app, &browser, &token, ALICE).await;

    assert_eq!(res.status(), 200);
    assert_eq!(res.text().await.unwrap(), "updated");
    // The stub's fixture gives Alice three orders.
    assert_eq!(order_count(&pool, ALICE).await, 3);
}

#[sqlx::test]
async fn it_answers_404_for_the_character_of_another_account(pool: PgPool) {
    let app = AppBuilder::new(pool.clone()).with_stub().spawn().await;
    let alice = app.browser();
    alice.sso_round_trip(&app, "/auth/login", ALICE).await;
    let bob = app.browser();
    bob.sso_round_trip(&app, "/auth/login", BOB).await;
    let token = token(&app, &alice).await;

    let res = poll(&app, &alice, &token, BOB).await;

    assert_eq!(res.status(), 404);
    assert_eq!(order_count(&pool, BOB).await, 0);
    assert_eq!(app.stub().esi_calls("orders"), 0);
}

async fn market_poll(app: &TestApp, browser: &Browser, token: Option<&str>) -> reqwest::Response {
    let mut request = browser
        .post(&app.url("/_test/market-poll"))
        .header("origin", &app.base_url)
        .header("hx-request", "true");
    if let Some(token) = token {
        request = request.header("x-csrf-token", token);
    }
    browser.send(request).await
}

#[sqlx::test]
async fn the_market_poll_refreshes_the_tracked_pairs_of_the_account_only_after_a_poll(
    pool: PgPool,
) {
    let app = AppBuilder::new(pool.clone()).with_stub().spawn().await;
    let browser = app.browser();
    browser.sso_round_trip(&app, "/auth/login", ALICE).await;
    let token = token(&app, &browser).await;

    // Nothing is tracked before a poll stored orders.
    let before = market_poll(&app, &browser, Some(&token)).await;
    assert_eq!(before.status(), 200);
    assert_eq!(before.text().await.unwrap(), "0");
    assert_eq!(app.stub().esi_calls("market"), 0);

    poll(&app, &browser, &token, ALICE).await;
    let after = market_poll(&app, &browser, Some(&token)).await;

    // The stub's fixture gives Alice orders for three items in The Forge, due or not.
    assert_eq!(after.text().await.unwrap(), "3");
    assert_eq!(app.stub().esi_calls("market"), 3);
    let synced: i64 =
        sqlx::query_scalar("SELECT count(*) FROM market_sync WHERE last_success_at IS NOT NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(synced, 3);
}

#[sqlx::test]
async fn the_market_poll_leaves_the_pairs_of_other_accounts_alone(pool: PgPool) {
    let app = AppBuilder::new(pool.clone()).with_stub().spawn().await;
    let alice = app.browser();
    alice.sso_round_trip(&app, "/auth/login", ALICE).await;
    let bob = app.browser();
    bob.sso_round_trip(&app, "/auth/login", BOB).await;
    let alice_token = token(&app, &alice).await;
    let bob_token = token(&app, &bob).await;
    poll(&app, &alice, &alice_token, ALICE).await;
    poll(&app, &bob, &bob_token, BOB).await;

    let res = market_poll(&app, &bob, Some(&bob_token)).await;

    // Bob has one order; Alice's two other items are not his business.
    assert_eq!(res.text().await.unwrap(), "1");
    assert_eq!(app.stub().esi_calls("market"), 1);
}

#[sqlx::test]
async fn the_market_poll_needs_the_csrf_token_and_a_session(pool: PgPool) {
    let app = AppBuilder::new(pool.clone()).with_stub().spawn().await;
    let browser = app.browser();
    browser.sso_round_trip(&app, "/auth/login", ALICE).await;
    let token = token(&app, &browser).await;
    poll(&app, &browser, &token, ALICE).await;

    let without_token = market_poll(&app, &browser, None).await;
    assert_eq!(without_token.status(), 403);

    let stranger = app.browser();
    let without_session = market_poll(&app, &stranger, Some(&token)).await;
    assert_ne!(without_session.status(), 200);
    assert_eq!(app.stub().esi_calls("market"), 0);
}
