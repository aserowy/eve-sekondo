mod common;

use common::{AppBuilder, Browser, TestApp};

const ALICE: i64 = 90000001;

async fn app() -> TestApp {
    AppBuilder::new().with_stub().spawn().await
}

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

fn logout(app: &TestApp, browser: &Browser, token: Option<&str>) -> reqwest::RequestBuilder {
    let mut request = browser
        .post(&app.url("/auth/logout"))
        .header("origin", &app.base_url)
        .header("hx-request", "true");
    if let Some(token) = token {
        request = request.header("x-csrf-token", token);
    }
    request
}

/// The stored record of the session the cookie names.
async fn session_record(app: &TestApp, cookie: &str) -> Option<tower_sessions::session::Record> {
    let id: tower_sessions::session::Id = cookie.split_once('=').unwrap().1.parse().unwrap();
    app.stores.sessions.load(&id).await.unwrap()
}

#[tokio::test]
async fn signing_out_deletes_the_server_side_session_and_the_old_cookie_stops_working() {
    let app = app().await;
    let browser = app.browser();
    browser.sso_round_trip(&app, "/auth/login", ALICE).await;
    let cookie = browser.cookie().unwrap();
    let token = token(&app, &browser).await;
    assert!(session_record(&app, &cookie).await.is_some());

    let res = browser.send(logout(&app, &browser, Some(&token))).await;
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["hx-redirect"], "/");
    assert!(
        session_record(&app, &cookie).await.is_none(),
        "the session record is deleted, not just the cookie"
    );

    // Replaying the pre-logout cookie does not authenticate.
    let replay = app.browser();
    replay.set_cookie(Some(cookie));
    let res = replay.get(&app.url("/characters")).await;
    assert!(res.status().is_redirection());
    assert_eq!(Browser::location(&res), "/");

    // And the signing-out browser itself is signed out.
    let res = browser.get(&app.url("/characters")).await;
    assert_eq!(Browser::location(&res), "/");
}

#[tokio::test]
async fn logging_in_again_after_sign_out_returns_to_the_same_account() {
    let app = app().await;
    let browser = app.browser();
    browser.sso_round_trip(&app, "/auth/login", ALICE).await;
    browser.sso_round_trip(&app, "/auth/add", 90000002).await;
    let token = token(&app, &browser).await;
    browser.send(logout(&app, &browser, Some(&token))).await;

    let again = app.browser();
    again.sso_round_trip(&app, "/auth/login", 90000002).await;
    let html = again
        .get(&app.url("/characters"))
        .await
        .text()
        .await
        .unwrap();
    assert!(
        html.contains("Alice Alpha") && html.contains("Bob Bravo"),
        "the same account, both characters"
    );
}

#[tokio::test]
async fn without_htmx_sign_out_redirects_to_the_landing_page() {
    let app = app().await;
    let browser = app.browser();
    browser.sso_round_trip(&app, "/auth/login", ALICE).await;
    let token = token(&app, &browser).await;
    let res = browser
        .send(
            browser
                .post(&app.url("/auth/logout"))
                .header("origin", &app.base_url)
                .header("x-csrf-token", &token),
        )
        .await;
    assert!(res.status().is_redirection());
    assert_eq!(Browser::location(&res), "/");
}

#[tokio::test]
async fn sign_out_without_a_csrf_token_is_forbidden_and_keeps_the_session() {
    let app = app().await;
    let browser = app.browser();
    browser.sso_round_trip(&app, "/auth/login", ALICE).await;

    let res = browser.send(logout(&app, &browser, None)).await;
    assert_eq!(res.status(), 403);
    assert_eq!(
        browser.get(&app.url("/characters")).await.status(),
        200,
        "still signed in"
    );
}

#[tokio::test]
async fn sign_out_from_a_foreign_origin_is_forbidden() {
    let app = app().await;
    let browser = app.browser();
    browser.sso_round_trip(&app, "/auth/login", ALICE).await;
    let token = token(&app, &browser).await;
    let res = browser
        .send(
            browser
                .post(&app.url("/auth/logout"))
                .header("origin", "https://evil.example")
                .header("x-csrf-token", &token),
        )
        .await;
    assert_eq!(res.status(), 403);
    assert_eq!(browser.get(&app.url("/characters")).await.status(), 200);
}

#[tokio::test]
async fn logging_out_is_not_possible_with_a_get_request() {
    let app = app().await;
    let browser = app.browser();
    browser.sso_round_trip(&app, "/auth/login", ALICE).await;
    let res = browser.get(&app.url("/auth/logout")).await;
    assert_eq!(res.status(), 405, "GET must not change state");
    assert_eq!(browser.get(&app.url("/characters")).await.status(), 200);
}
