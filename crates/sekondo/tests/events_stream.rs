mod common;

use std::time::Duration;

use common::{AppBuilder, Browser, ManualClock, TestApp};
use sqlx::PgPool;

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const TICK: Duration = Duration::from_millis(150);

async fn app(pool: PgPool) -> TestApp {
    AppBuilder::new(pool)
        .with_stub()
        .sse_tick(TICK)
        .spawn()
        .await
}

async fn signed_in(app: &TestApp, character: i64) -> Browser {
    let browser = app.browser();
    browser.sso_round_trip(app, "/auth/login", character).await;
    browser
}

async fn open_stream(app: &TestApp, browser: &Browser) -> reqwest::Response {
    let res = browser.get(&app.url("/events")).await;
    assert_eq!(res.status(), 200);
    assert_eq!(res.headers()["content-type"], "text/event-stream");
    res
}

/// Reads the stream for up to `window` and returns what arrived and whether it ended.
async fn read_for(res: &mut reqwest::Response, window: Duration) -> (String, bool) {
    let mut text = String::new();
    let deadline = tokio::time::Instant::now() + window;
    loop {
        match tokio::time::timeout_at(deadline, res.chunk()).await {
            Err(_) => return (text, false),
            Ok(Ok(Some(chunk))) => text.push_str(&String::from_utf8_lossy(&chunk)),
            Ok(Ok(None)) | Ok(Err(_)) => return (text, true),
        }
    }
}

#[sqlx::test]
async fn a_poll_sends_the_orders_updated_signal(pool: PgPool) {
    let app = app(pool).await;
    let browser = signed_in(&app, ALICE).await;
    let mut stream = open_stream(&app, &browser).await;

    app.state.poller.poll_character(ALICE).await;

    let (text, _) = read_for(&mut stream, Duration::from_millis(600)).await;
    assert!(text.contains("event: orders-updated"), "{text:?}");
}

#[sqlx::test]
async fn signals_stay_within_the_account(pool: PgPool) {
    let app = app(pool).await;
    let alice = signed_in(&app, ALICE).await;
    let bob = signed_in(&app, BOB).await;
    let mut alice_stream = open_stream(&app, &alice).await;
    let mut bob_stream = open_stream(&app, &bob).await;

    app.state.poller.poll_character(ALICE).await;

    let (alice_text, _) = read_for(&mut alice_stream, Duration::from_millis(600)).await;
    let (bob_text, ended) = read_for(&mut bob_stream, Duration::from_millis(600)).await;
    assert!(alice_text.contains("orders-updated"), "{alice_text:?}");
    assert!(!bob_text.contains("orders-updated"), "{bob_text:?}");
    assert!(!ended);
}

#[sqlx::test]
async fn an_unauthenticated_request_opens_no_stream(pool: PgPool) {
    let app = app(pool).await;

    let res = app.browser().get(&app.url("/events")).await;

    assert_eq!(res.status(), 401);
    assert_ne!(
        res.headers()
            .get("content-type")
            .map(|v| v.to_str().unwrap()),
        Some("text/event-stream")
    );
}

#[sqlx::test]
async fn signals_carry_nothing_but_their_name(pool: PgPool) {
    let app = app(pool).await;
    let browser = signed_in(&app, ALICE).await;
    let mut stream = open_stream(&app, &browser).await;

    app.state.poller.poll_character(ALICE).await;

    let (text, _) = read_for(&mut stream, Duration::from_millis(600)).await;
    for line in text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with(':'))
    {
        assert!(
            line == "event: orders-updated" || line == "data: orders-updated",
            "unexpected line {line:?}"
        );
    }
    for secret in ["Alice", "90000001", "price", "5001", "Tritanium"] {
        assert!(!text.contains(secret), "{secret} leaked: {text:?}");
    }
}

#[sqlx::test]
async fn the_stream_sends_keep_alives(pool: PgPool) {
    let app = app(pool).await;
    let browser = signed_in(&app, ALICE).await;
    let mut stream = open_stream(&app, &browser).await;

    let (text, ended) = read_for(&mut stream, Duration::from_millis(600)).await;

    assert!(!ended);
    assert!(text.matches(": keep-alive").count() >= 2, "{text:?}");
}

#[sqlx::test]
async fn signing_out_elsewhere_closes_the_stream_and_reconnecting_fails(pool: PgPool) {
    let app = app(pool).await;
    let browser = signed_in(&app, ALICE).await;
    let mut stream = open_stream(&app, &browser).await;
    let cookie = browser.cookie().unwrap();

    // The other tab signs out.
    let page = browser
        .get(&app.url("/characters"))
        .await
        .text()
        .await
        .unwrap();
    let token = page
        .split(r#"<meta name="csrf-token" content=""#)
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_owned();
    let out = browser
        .send(
            browser
                .post(&app.url("/auth/logout"))
                .header("origin", &app.base_url)
                .header("hx-request", "true")
                .header("x-csrf-token", token),
        )
        .await;
    assert_eq!(out.status(), 200);

    let (_, ended) = read_for(&mut stream, Duration::from_secs(3)).await;
    assert!(ended, "the stream closes");

    let replay = app.browser();
    replay.set_cookie(Some(cookie));
    assert_eq!(replay.get(&app.url("/events")).await.status(), 401);
}

#[sqlx::test]
async fn the_stream_closes_when_the_session_outlives_its_lifetime(pool: PgPool) {
    let clock = ManualClock::new();
    let app = AppBuilder::new(pool)
        .with_stub()
        .clock(clock.clone())
        .sse_tick(TICK)
        .spawn()
        .await;
    let browser = signed_in(&app, ALICE).await;
    let mut stream = open_stream(&app, &browser).await;

    let (_, ended) = read_for(&mut stream, Duration::from_millis(400)).await;
    assert!(!ended);

    clock.advance(time::Duration::days(31));

    let (_, ended) = read_for(&mut stream, Duration::from_secs(3)).await;
    assert!(ended);
}
