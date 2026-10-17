mod common;

use common::{AppBuilder, Browser, TestApp};
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const STATION_NAME: &str = "Jita IV - Moon 4 - Caldari Navy Assembly Plant";

async fn setup(pool: PgPool) -> (TestApp, Browser) {
    let app = AppBuilder::new(pool).with_stub().spawn().await;
    let browser = app.browser();
    browser.sso_round_trip(&app, "/auth/login", ALICE).await;
    (app, browser)
}

async fn get(app: &TestApp, browser: &Browser, path: &str) -> String {
    let res = browser.get(&app.url(path)).await;
    assert_eq!(res.status(), 200, "{path}");
    res.text().await.unwrap()
}

/// The freshness section of the page.
fn freshness(html: &str) -> &str {
    let start = html
        .find(r#"id="opportunity-freshness""#)
        .expect("a freshness section");
    let rest = &html[start..];
    &rest[..rest.find(r#"id="opportunity-tables""#).unwrap()]
}

async fn book_sync(pool: &PgPool, last_success: Option<OffsetDateTime>, error: Option<&str>) {
    sqlx::query(
        "INSERT INTO station_sync (region_id, station_id, kind, next_poll_at, last_success_at, last_error)
         VALUES (10000002, 60003760, 'book', now(), $1, $2)",
    )
    .bind(last_success)
    .bind(error)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test]
async fn an_unauthenticated_request_is_redirected_to_the_landing_page(pool: PgPool) {
    let app = AppBuilder::new(pool).with_stub().spawn().await;
    for path in [
        "/opportunities",
        "/opportunities/freshness",
        "/opportunities/tables",
    ] {
        let res = app.browser().get(&app.url(path)).await;
        assert!(res.status().is_redirection(), "{path}");
        assert_eq!(Browser::location(&res), "/", "{path}");
    }
}

#[sqlx::test]
async fn the_page_names_the_station_and_lists_the_freshness_and_the_four_tables_in_order(
    pool: PgPool,
) {
    let (app, browser) = setup(pool).await;

    let html = get(&app, &browser, "/opportunities").await;

    assert!(html.contains(STATION_NAME));
    let at = |needle: &str| {
        html.find(needle)
            .unwrap_or_else(|| panic!("{needle} missing"))
    };
    let positions = [
        at(r#"id="opportunity-freshness""#),
        at("Recurring spread expansion"),
        at("Classic spread trade"),
        at("Mean reversion"),
        at(">Trend<"),
    ];
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{positions:?}");
}

#[sqlx::test]
async fn a_recent_book_refresh_shows_its_time_and_is_not_stale(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    let last = OffsetDateTime::now_utc() - Duration::minutes(5);
    book_sync(&pool, Some(last), None).await;

    let html = get(&app, &browser, "/opportunities").await;
    let strip = freshness(&html);

    assert!(strip.contains(&sekondo::format::eve_time(last)), "{strip}");
    assert!(!strip.contains("Stale"), "{strip}");
    assert!(!strip.contains("Waiting for first refresh"), "{strip}");
}

#[sqlx::test]
async fn a_book_refresh_forty_minutes_ago_is_marked_stale(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    let last = OffsetDateTime::now_utc() - Duration::minutes(40);
    book_sync(&pool, Some(last), None).await;

    let html = get(&app, &browser, "/opportunities").await;
    let strip = freshness(&html);

    assert!(strip.contains(&sekondo::format::eve_time(last)), "{strip}");
    assert!(strip.contains("Stale"), "{strip}");
}

#[sqlx::test]
async fn a_book_that_was_never_refreshed_is_stale_and_waiting(pool: PgPool) {
    let (app, browser) = setup(pool).await;

    let html = get(&app, &browser, "/opportunities").await;
    let strip = freshness(&html);

    assert!(strip.contains("Stale"), "{strip}");
    assert!(strip.contains("Waiting for first refresh"), "{strip}");
}

#[sqlx::test]
async fn the_last_errors_of_the_book_and_the_history_are_shown(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    book_sync(&pool, Some(OffsetDateTime::now_utc()), Some("HTTP 503")).await;
    sqlx::query(
        "INSERT INTO station_sync (region_id, station_id, kind, next_poll_at, last_success_at, last_error)
         VALUES (10000002, 60003760, 'history', now(), now() - interval '2 hours', 'HTTP 502')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let html = get(&app, &browser, "/opportunities/freshness").await;

    assert!(html.contains("HTTP 503"), "{html}");
    assert!(html.contains("HTTP 502"), "{html}");
    assert!(!html.contains("<html"), "a fragment, not a page");
}

#[sqlx::test]
async fn the_days_of_own_snapshots_are_counted(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    for back in [1, 2, 3] {
        sqlx::query(
            "INSERT INTO station_snapshots (recorded_at, region_id, station_id, type_id, best_buy,
                 best_sell, spread, buy_near_units, buy_near_orders, sell_near_units,
                 sell_near_orders, buy_units, sell_units, buy_taken, sell_taken)
             VALUES (now() - make_interval(days => $1) - interval '2 hours', 10000002, 60003760,
                     34, 100, 110, 0.1, 1, 1, 1, 1, 1, 1, 0, 0)",
        )
        .bind(back)
        .execute(&pool)
        .await
        .unwrap();
    }

    let html = get(&app, &browser, "/opportunities/freshness").await;

    assert!(html.contains("3 days of own snapshots"), "{html}");
}

#[sqlx::test]
async fn the_navigation_links_to_the_page_and_marks_it_current(pool: PgPool) {
    let (app, browser) = setup(pool).await;

    let html = get(&app, &browser, "/opportunities").await;

    let nav = html.split_once("</nav>").unwrap().0;
    assert!(
        nav.contains(r#"href="/opportunities" aria-current="page""#),
        "{nav}"
    );
    for link in ["/dashboard", "/orders", "/wealth", "/characters"] {
        assert!(nav.contains(&format!(r#"href="{link}""#)), "{link}");
    }
}

#[sqlx::test]
async fn the_navigation_carries_a_narrowed_selection_to_and_from_the_page(pool: PgPool) {
    let (app, browser) = setup(pool).await;
    browser.sso_round_trip(&app, "/auth/add", BOB).await;
    let unescaped = |html: String| html.replace("&#38;", "&").replace("&amp;", "&");

    for path in [
        format!("/orders?chars={BOB}"),
        format!("/dashboard?chars={BOB}"),
        format!("/wealth?chars={BOB}"),
        format!("/opportunities?chars={BOB}"),
    ] {
        let html = unescaped(get(&app, &browser, &path).await);
        assert!(
            html.contains(&format!(r#"href="/opportunities?chars={BOB}""#)),
            "{path} does not carry the selection to the page"
        );
        assert!(
            html.contains(&format!(r#"href="/orders?chars={BOB}""#)),
            "{path}"
        );
    }
}
