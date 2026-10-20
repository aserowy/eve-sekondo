//! Without the `test-hooks` feature the test routes do not exist.
#![cfg(not(feature = "test-hooks"))]

mod common;

use common::AppBuilder;

const ALICE: i64 = 90000001;

#[tokio::test]
async fn the_market_poll_hook_is_absent() {
    let app = AppBuilder::new().with_stub().spawn().await;
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

#[tokio::test]
async fn the_wealth_and_fee_hooks_are_absent() {
    let app = AppBuilder::new().with_stub().spawn().await;
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

#[tokio::test]
async fn the_station_hooks_are_absent() {
    let app = AppBuilder::new().with_stub().spawn().await;
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

    for hook in [
        "station-book",
        "station-history",
        "station-rollups",
        "station-seed",
    ] {
        let res = browser
            .send(
                browser
                    .post(&app.url(&format!("/_test/{hook}")))
                    .header("origin", &app.base_url)
                    .header("hx-request", "true")
                    .header("x-csrf-token", token),
            )
            .await;
        assert_eq!(res.status(), 404, "{hook}");
    }
}
