//! Without the `test-hooks` feature the test routes do not exist.
#![cfg(not(feature = "test-hooks"))]

mod common;

use common::AppBuilder;
use sqlx::PgPool;

const ALICE: i64 = 90000001;

#[sqlx::test]
async fn the_market_poll_hook_is_absent(pool: PgPool) {
    let app = AppBuilder::new(pool).with_stub().spawn().await;
    let browser = app.browser();
    browser.sso_round_trip(&app, "/auth/login", ALICE).await;
    let html = browser
        .get(&app.url("/characters"))
        .await
        .text()
        .await
        .unwrap();
    let token = html
        .split(r#"<meta name="csrf-token" content=""#)
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();

    let res = browser
        .send(
            browser
                .post(&app.url("/_test/market-poll"))
                .header("origin", &app.base_url)
                .header("hx-request", "true")
                .header("x-csrf-token", token),
        )
        .await;

    assert_eq!(res.status(), 404);
}

#[sqlx::test]
async fn the_wealth_and_fee_hooks_are_absent(pool: PgPool) {
    let app = AppBuilder::new(pool).with_stub().spawn().await;
    let browser = app.browser();
    browser.sso_round_trip(&app, "/auth/login", ALICE).await;
    let html = browser
        .get(&app.url("/characters"))
        .await
        .text()
        .await
        .unwrap();
    let token = html
        .split(r#"<meta name="csrf-token" content=""#)
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();

    for path in [
        format!("/_test/wealth-poll/{ALICE}/wallet"),
        format!("/_test/wealth-poll/{ALICE}/transactions"),
        "/_test/price-refresh".to_owned(),
        "/_test/fee-poll".to_owned(),
    ] {
        let res = browser
            .send(
                browser
                    .post(&app.url(&path))
                    .header("origin", &app.base_url)
                    .header("hx-request", "true")
                    .header("x-csrf-token", token),
            )
            .await;
        assert_eq!(res.status(), 404, "{path}");
    }
}
