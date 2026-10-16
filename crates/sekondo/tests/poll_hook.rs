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
