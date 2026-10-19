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
async fn a_wallet_poll_sends_the_wealth_updated_signal_to_its_account_only(pool: PgPool) {
    let app = app(pool).await;
    let alice = signed_in(&app, ALICE).await;
    let bob = signed_in(&app, BOB).await;
    let mut alice_stream = open_stream(&app, &alice).await;
    let mut bob_stream = open_stream(&app, &bob).await;

    app.state
        .wealth
        .poll(ALICE, sekondo::wealth_poller::Kind::Wallet)
        .await;

    let (alice_text, _) = read_for(&mut alice_stream, Duration::from_millis(600)).await;
    let (bob_text, _) = read_for(&mut bob_stream, Duration::from_millis(600)).await;
    assert!(
        alice_text.contains("event: wealth-updated"),
        "{alice_text:?}"
    );
    assert!(!bob_text.contains("wealth-updated"), "{bob_text:?}");
}

#[sqlx::test]
async fn an_assets_poll_sends_the_wealth_updated_signal(pool: PgPool) {
    let app = app(pool).await;
    let alice = signed_in(&app, ALICE).await;
    let mut stream = open_stream(&app, &alice).await;

    app.state
        .wealth
        .poll(ALICE, sekondo::wealth_poller::Kind::Assets)
        .await;

    let (text, _) = read_for(&mut stream, Duration::from_millis(600)).await;
    assert!(text.contains("event: wealth-updated"), "{text:?}");
}

#[sqlx::test]
async fn a_price_refresh_signals_each_account_with_assets_once(pool: PgPool) {
    let app = app(pool).await;
    let alice = signed_in(&app, ALICE).await;
    let bob = signed_in(&app, BOB).await;
    let carol = signed_in(&app, 90000003).await;
    for character in [ALICE, BOB] {
        app.state
            .wealth
            .poll(character, sekondo::wealth_poller::Kind::Assets)
            .await;
    }
    let mut streams = vec![
        open_stream(&app, &alice).await,
        open_stream(&app, &bob).await,
        open_stream(&app, &carol).await,
    ];

    app.state.prices.refresh().await;

    let mut counts = Vec::new();
    for stream in &mut streams {
        let (text, _) = read_for(stream, Duration::from_millis(600)).await;
        counts.push(text.matches("event: wealth-updated").count());
    }
    assert_eq!(counts, [1, 1, 0], "Carol has no assets");
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

#[sqlx::test]
async fn a_skills_poll_that_changes_a_fee_sends_fees_updated_without_any_data(pool: PgPool) {
    let app = app(pool).await;
    let alice = signed_in(&app, ALICE).await;
    app.state.poller.poll_character(ALICE).await;
    let mut stream = open_stream(&app, &alice).await;

    app.state
        .fees
        .poll(ALICE, sekondo::fee_poller::Kind::Skills)
        .await;

    let (text, _) = read_for(&mut stream, Duration::from_millis(600)).await;
    assert!(
        text.contains("event: fees-updated\ndata: fees-updated\n"),
        "{text:?}"
    );
    for secret in ["16622", "accounting", "3.375", "standing"] {
        assert!(
            !text.to_lowercase().contains(secret),
            "{secret} in {text:?}"
        );
    }
}

#[sqlx::test]
async fn a_skills_poll_that_changes_nothing_sends_no_fees_updated(pool: PgPool) {
    let app = app(pool).await;
    let alice = signed_in(&app, ALICE).await;
    app.state.poller.poll_character(ALICE).await;
    app.state
        .fees
        .poll(ALICE, sekondo::fee_poller::Kind::Skills)
        .await;
    let mut stream = open_stream(&app, &alice).await;

    app.state
        .fees
        .poll(ALICE, sekondo::fee_poller::Kind::Skills)
        .await;

    let (text, _) = read_for(&mut stream, Duration::from_millis(600)).await;
    assert!(!text.contains("fees-updated"), "{text:?}");
}

#[sqlx::test]
async fn fees_updated_stays_within_the_account(pool: PgPool) {
    let app = app(pool).await;
    let alice = signed_in(&app, ALICE).await;
    let bob = signed_in(&app, BOB).await;
    app.state.poller.poll_character(ALICE).await;
    app.state.poller.poll_character(BOB).await;
    let mut alice_stream = open_stream(&app, &alice).await;
    let mut bob_stream = open_stream(&app, &bob).await;

    app.state
        .fees
        .poll(ALICE, sekondo::fee_poller::Kind::Skills)
        .await;

    let (alice_text, _) = read_for(&mut alice_stream, Duration::from_millis(600)).await;
    let (bob_text, _) = read_for(&mut bob_stream, Duration::from_millis(600)).await;
    assert!(alice_text.contains("event: fees-updated"), "{alice_text:?}");
    assert!(!bob_text.contains("fees-updated"), "{bob_text:?}");
}

fn one_purchase() -> serde_json::Value {
    serde_json::json!({
        "character_id": ALICE,
        "transactions": [{
            "transaction_id": 1, "date": "2026-10-01T12:00:00Z", "type_id": 34,
            "location_id": 60003760, "quantity": 10, "unit_price": 100.25,
            "is_buy": true, "is_personal": true,
        }],
    })
}

#[sqlx::test]
async fn a_transactions_poll_that_stores_a_purchase_sends_orders_updated_and_not_wealth(
    pool: PgPool,
) {
    let app = app(pool).await;
    let alice = signed_in(&app, ALICE).await;
    app.stub_control("transactions", one_purchase()).await;
    let mut stream = open_stream(&app, &alice).await;

    app.state
        .wealth
        .poll(ALICE, sekondo::wealth_poller::Kind::Transactions)
        .await;

    let (text, _) = read_for(&mut stream, Duration::from_millis(600)).await;
    assert!(text.contains("event: orders-updated"), "{text:?}");
    assert!(!text.contains("wealth-updated"), "{text:?}");
}

fn one_sale() -> serde_json::Value {
    serde_json::json!({
        "character_id": ALICE,
        "transactions": [{
            "transaction_id": 2, "date": "2026-10-01T12:00:00Z", "type_id": 34,
            "location_id": 60003760, "quantity": 10, "unit_price": 150.0,
            "is_buy": false, "is_personal": true,
        }],
    })
}

#[sqlx::test]
async fn a_new_sale_sends_trades_updated_and_no_orders_updated(pool: PgPool) {
    let app = app(pool).await;
    let alice = signed_in(&app, ALICE).await;
    app.stub_control("transactions", one_sale()).await;
    let mut stream = open_stream(&app, &alice).await;

    app.state
        .wealth
        .poll(ALICE, sekondo::wealth_poller::Kind::Transactions)
        .await;

    let (text, _) = read_for(&mut stream, Duration::from_millis(600)).await;
    assert!(text.contains("event: trades-updated"), "{text:?}");
    assert!(!text.contains("orders-updated"), "{text:?}");
    assert!(!text.contains("wealth-updated"), "{text:?}");
}

#[sqlx::test]
async fn a_new_purchase_sends_trades_updated_and_orders_updated(pool: PgPool) {
    let app = app(pool).await;
    let alice = signed_in(&app, ALICE).await;
    app.stub_control("transactions", one_purchase()).await;
    let mut stream = open_stream(&app, &alice).await;

    app.state
        .wealth
        .poll(ALICE, sekondo::wealth_poller::Kind::Transactions)
        .await;

    let (text, _) = read_for(&mut stream, Duration::from_millis(600)).await;
    assert!(text.contains("event: trades-updated"), "{text:?}");
    assert!(text.contains("event: orders-updated"), "{text:?}");
}

#[sqlx::test]
async fn trades_updated_stays_within_the_account(pool: PgPool) {
    let app = app(pool).await;
    let alice = signed_in(&app, ALICE).await;
    let bob = signed_in(&app, BOB).await;
    app.stub_control("transactions", one_sale()).await;
    let mut alice_stream = open_stream(&app, &alice).await;
    let mut bob_stream = open_stream(&app, &bob).await;

    app.state
        .wealth
        .poll(ALICE, sekondo::wealth_poller::Kind::Transactions)
        .await;

    let (alice_text, _) = read_for(&mut alice_stream, Duration::from_millis(600)).await;
    let (bob_text, _) = read_for(&mut bob_stream, Duration::from_millis(600)).await;
    assert!(alice_text.contains("trades-updated"), "{alice_text:?}");
    assert!(!bob_text.contains("trades-updated"), "{bob_text:?}");
}

#[sqlx::test]
async fn a_transactions_poll_that_stores_nothing_new_sends_no_signal(pool: PgPool) {
    let app = app(pool).await;
    let alice = signed_in(&app, ALICE).await;
    app.stub_control("transactions", one_purchase()).await;
    app.state
        .wealth
        .poll(ALICE, sekondo::wealth_poller::Kind::Transactions)
        .await;
    let mut stream = open_stream(&app, &alice).await;

    // The same purchase again, and an empty answer.
    app.state
        .wealth
        .poll(ALICE, sekondo::wealth_poller::Kind::Transactions)
        .await;

    let (text, _) = read_for(&mut stream, Duration::from_millis(600)).await;
    assert!(!text.contains("event:"), "{text:?}");
}

async fn tradable_book(app: &TestApp) {
    let order = |id: i64, buy: bool, price: f64| {
        serde_json::json!({
            "order_id": id, "type_id": 34, "location_id": 60003760, "system_id": 30000142,
            "is_buy_order": buy, "price": price, "range": "region", "duration": 90,
            "issued": "2026-10-11T12:00:00Z", "min_volume": 1, "volume_remain": 5,
            "volume_total": 5,
        })
    };
    let res = app
        .stub_control(
            "market-orders",
            serde_json::json!({"region_id": 10000002, "orders": [order(1, true, 9.0), order(2, false, 10.0)]}),
        )
        .await;
    assert_eq!(res.status(), 204);
}

#[sqlx::test]
async fn a_book_refresh_signals_every_account(pool: PgPool) {
    let app = app(pool).await;
    let alice = signed_in(&app, ALICE).await;
    let bob = signed_in(&app, BOB).await;
    let mut alice_stream = open_stream(&app, &alice).await;
    let mut bob_stream = open_stream(&app, &bob).await;
    tradable_book(&app).await;

    app.state.station_book.refresh().await;

    for stream in [&mut alice_stream, &mut bob_stream] {
        let (text, _) = read_for(stream, Duration::from_millis(600)).await;
        assert_eq!(
            text.matches("event: opportunities-updated").count(),
            1,
            "{text:?}"
        );
    }
}

#[sqlx::test]
async fn a_failed_book_refresh_signals_nobody(pool: PgPool) {
    let app = app(pool).await;
    let alice = signed_in(&app, ALICE).await;
    let mut stream = open_stream(&app, &alice).await;
    tradable_book(&app).await;
    app.stub_control(
        "esi-fail-next",
        serde_json::json!({"route": "market", "status": 503}),
    )
    .await;

    app.state.station_book.refresh().await;

    let (text, _) = read_for(&mut stream, Duration::from_millis(600)).await;
    assert!(!text.contains("opportunities-updated"), "{text:?}");
}

async fn tradable_in_the_database(app: &TestApp) {
    for (id, buy) in [(1, true), (2, false)] {
        sqlx::query(
            "INSERT INTO station_orders (region_id, station_id, order_id, type_id, is_buy_order, price, volume_remain)
             VALUES (10000002, 60003760, $1, 34, $2, 10, 5)",
        )
        .bind(id)
        .bind(buy)
        .execute(&app.state.pool)
        .await
        .unwrap();
    }
}

#[sqlx::test]
async fn a_history_pass_that_stored_rows_signals_once_and_an_empty_one_does_not(pool: PgPool) {
    let app = app(pool).await;
    let alice = signed_in(&app, ALICE).await;
    let mut stream = open_stream(&app, &alice).await;
    tradable_in_the_database(&app).await;

    // No history for item 34: the pass stores nothing.
    app.state.station_history.tick().await;
    let (text, _) = read_for(&mut stream, Duration::from_millis(400)).await;
    assert!(!text.contains("opportunities-updated"), "{text:?}");

    // The next day ESI has history for it.
    let today = time::OffsetDateTime::now_utc().date();
    let day = |back: i64| {
        serde_json::json!({"date": (today - time::Duration::days(back)).to_string(),
            "average": 5.0, "highest": 6.0, "lowest": 4.0, "order_count": 3, "volume": 100})
    };
    app.stub_control(
        "market-history",
        serde_json::json!({"region_id": 10000002, "type_id": 34, "days": [day(2), day(1)]}),
    )
    .await;
    sqlx::query("UPDATE history_sync SET next_poll_at = now() - interval '1 minute'")
        .execute(&app.state.pool)
        .await
        .unwrap();

    app.state.station_history.tick().await;

    let (text, _) = read_for(&mut stream, Duration::from_millis(600)).await;
    assert_eq!(
        text.matches("event: opportunities-updated").count(),
        1,
        "{text:?}"
    );
}
