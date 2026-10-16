mod common;

use common::{AppBuilder, Browser, TestApp};
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};

const ALICE: i64 = 90000001;

async fn setup(pool: PgPool) -> (TestApp, Browser) {
    let app = AppBuilder::new(pool).with_stub().spawn().await;
    let browser = app.browser();
    browser.sso_round_trip(&app, "/auth/login", ALICE).await;
    (app, browser)
}

async fn orders_page(app: &TestApp, browser: &Browser) -> String {
    let res = browser.get(&app.url("/orders")).await;
    assert_eq!(res.status(), 200);
    res.text().await.unwrap()
}

/// The freshness strip of the page.
fn strip(html: &str) -> &str {
    let start = html.find(r#"id="freshness""#).expect("a freshness strip");
    let rest = &html[start..];
    &rest[..rest.find(r#"id="order-tables""#).unwrap()]
}

async fn seed_sync(pool: &PgPool, last_success: Option<OffsetDateTime>, next: OffsetDateTime) {
    sqlx::query(
        "INSERT INTO order_sync (character_id, next_poll_at, last_success_at) VALUES ($1, $2, $3)",
    )
    .bind(ALICE)
    .bind(next)
    .bind(last_success)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test]
async fn a_recently_polled_character_shows_its_times_and_no_stale_badge(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    let now = OffsetDateTime::now_utc();
    let last = now - Duration::minutes(5);
    let next = now + Duration::minutes(15);
    seed_sync(&pool, Some(last), next).await;

    let html = orders_page(&app, &browser).await;
    let strip = strip(&html);

    assert!(strip.contains("Alice Alpha"));
    assert!(strip.contains(&sekondo::format::eve_time(last)), "{strip}");
    assert!(strip.contains(&sekondo::format::eve_time(next)), "{strip}");
    assert!(!strip.contains("Stale"), "{strip}");
    assert!(!strip.contains("Log in again"));
    assert!(!strip.contains("Waiting for first update"));
}

#[sqlx::test]
async fn a_character_needing_relogin_is_stale_links_to_login_and_keeps_its_orders(pool: PgPool) {
    let (app, browser) = setup(pool).await;
    app.state.poller.poll_character(ALICE).await;
    app.state.repo.mark_needs_relogin(ALICE).await.unwrap();

    let html = orders_page(&app, &browser).await;
    let strip = strip(&html);

    assert!(strip.contains("Stale"), "{strip}");
    assert!(strip.contains(r#"href="/auth/add""#) && strip.contains("Log in again"));
    // The last known orders are still listed.
    assert!(html.contains(r#"id="order-5001""#));
}

#[sqlx::test]
async fn a_character_whose_last_success_is_too_old_is_stale(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    let now = OffsetDateTime::now_utc();
    // Twice the 20-minute interval is 40 minutes.
    seed_sync(
        &pool,
        Some(now - Duration::minutes(45)),
        now + Duration::minutes(1),
    )
    .await;

    let strip_html = orders_page(&app, &browser).await;
    let strip = strip(&strip_html);

    assert!(strip.contains("Stale"), "{strip}");
    assert!(!strip.contains("Log in again"));
}

#[sqlx::test]
async fn the_threshold_is_twice_the_refresh_interval(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    let now = OffsetDateTime::now_utc();
    seed_sync(
        &pool,
        Some(now - Duration::minutes(39)),
        now + Duration::minutes(1),
    )
    .await;

    let html = orders_page(&app, &browser).await;

    assert!(!strip(&html).contains("Stale"));
}

#[sqlx::test]
async fn a_newly_linked_character_waits_for_its_first_update(pool: PgPool) {
    let (app, browser) = setup(pool).await;

    let html = orders_page(&app, &browser).await;
    let strip = strip(&html);

    assert!(strip.contains("Waiting for first update"), "{strip}");
    assert!(strip.contains("Stale"), "never polled counts as stale");
}

#[sqlx::test]
async fn the_freshness_fragment_is_the_strip_alone(pool: PgPool) {
    let (app, browser) = setup(pool).await;
    app.state.poller.poll_character(ALICE).await;

    let res = browser.get(&app.url("/orders/freshness")).await;
    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();

    assert!(!html.contains("<html") && !html.contains("<nav"));
    assert!(html.contains("Alice Alpha") && html.contains("Last update"));
    assert!(!html.contains(r#"id="sell-orders""#));
}
