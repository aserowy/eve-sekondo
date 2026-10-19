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
    let start = html.find(r#"id="freshness""#).expect("a freshness section");
    let rest = &html[start..];
    &rest[..rest.find("</section>").unwrap()]
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
        "/opportunities/expansion",
        "/opportunities/classic",
        "/opportunities/reversion",
        "/opportunities/trend",
        "/opportunities/trend/content",
        "/opportunities/freshness",
    ] {
        let res = app.browser().get(&app.url(path)).await;
        assert!(res.status().is_redirection(), "{path}");
        assert_eq!(Browser::location(&res), "/", "{path}");
    }
}

#[sqlx::test]
async fn the_entry_redirects_to_the_first_table_and_keeps_the_query(pool: PgPool) {
    let (app, browser) = setup(pool).await;

    let plain = browser.get(&app.url("/opportunities")).await;
    assert_eq!(plain.status(), 303);
    assert_eq!(Browser::location(&plain), "/opportunities/expansion");

    let query = format!("chars={ALICE}&cs=item&cd=asc&ts=sigma&td=desc&page=2");
    let with_query = browser
        .get(&app.url(&format!("/opportunities?{query}")))
        .await;
    assert_eq!(with_query.status(), 303);
    assert_eq!(
        Browser::location(&with_query),
        format!("/opportunities/expansion?{query}")
    );
}

#[sqlx::test]
async fn an_unknown_table_is_not_found(pool: PgPool) {
    let (app, browser) = setup(pool).await;

    for path in [
        "/opportunities/arbitrage",
        "/opportunities/arbitrage/content",
    ] {
        let res = browser.get(&app.url(path)).await;
        assert_eq!(res.status(), 404, "{path}");
    }
}

#[sqlx::test]
async fn each_page_shows_one_table_below_the_sub_menu_and_the_filters_with_the_freshness_last(
    pool: PgPool,
) {
    let (app, browser) = setup(pool).await;

    for (table, title) in [
        ("expansion", "Recurring spread expansion"),
        ("classic", "Classic spread trade"),
        ("reversion", "Mean reversion"),
        ("trend", "Trend"),
    ] {
        let html = get(&app, &browser, &format!("/opportunities/{table}")).await;

        assert!(html.contains(STATION_NAME), "{table}");
        assert!(!html.contains("<h1"), "no page heading");
        assert!(!html.contains("panel"), "no self-made panels");
        let at = |needle: &str| {
            html.find(needle)
                .unwrap_or_else(|| panic!("{needle} missing in {table}"))
        };
        let positions = [
            at(r#"<nav class="sub-menu""#),
            at(r#"id="filter-box""#),
            at(&format!(r#"<section id="{table}""#)),
            at(r#"id="freshness""#),
        ];
        assert!(
            positions.windows(2).all(|w| w[0] < w[1]),
            "{table}: {positions:?}"
        );
        assert!(html.contains(&format!("<h2>{title}</h2>")), "{table}");
        for other in ["expansion", "classic", "reversion", "trend"] {
            if other != table {
                assert!(
                    !html.contains(&format!(r#"<section id="{other}""#)),
                    "{table} page shows {other}"
                );
            }
        }
    }
}

/// The links of the page sub-menu: text, href and whether it is the current page.
fn sub_menu(html: &str) -> Vec<(String, String, bool)> {
    let menu = html.split_once(r#"<nav class="sub-menu""#).unwrap().1;
    let menu = &menu[..menu.find("</nav>").unwrap()];
    menu.split("<a ")
        .skip(1)
        .map(|a| {
            let href = a
                .split(r#"href=""#)
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap();
            let text = a.split_once('>').unwrap().1.split("</a>").next().unwrap();
            (
                text.to_owned(),
                href.replace("&#38;", "&"),
                a.contains(r#"aria-current="page""#),
            )
        })
        .collect()
}

#[sqlx::test]
async fn the_sub_menu_lists_the_four_tables_as_text_without_counts_and_marks_the_current_one(
    pool: PgPool,
) {
    let (app, browser) = setup(pool.clone()).await;
    // A classic row that would have made a number.
    sqlx::query(
        "INSERT INTO station_item_stats
             (region_id, station_id, type_id, book_at, best_buy, best_sell, buy_near_orders,
              sell_near_orders, rho, indicators_at, volume_7d, volume_30d, sigma, returns)
         VALUES (10000002, 60003760, 34, now(), 100, 130, 0, 0, 0.5, now(), 100000000, 100000000, 0.02, 29)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let html = get(&app, &browser, "/opportunities/trend").await;

    let links = sub_menu(&html);
    let texts: Vec<_> = links.iter().map(|l| l.0.as_str()).collect();
    assert_eq!(
        texts,
        [
            "Recurring spread expansion",
            "Classic spread trade",
            "Mean reversion",
            "Trend"
        ]
    );
    let current: Vec<_> = links.iter().filter(|l| l.2).map(|l| l.0.as_str()).collect();
    assert_eq!(current, ["Trend"]);
    assert!(!html.contains(r#"class="count""#), "no row counts");
}

#[sqlx::test]
async fn the_sub_menu_carries_the_selection_and_every_sorting_but_no_paging(pool: PgPool) {
    let (app, browser) = setup(pool).await;
    browser.sso_round_trip(&app, "/auth/add", BOB).await;

    let html = get(
        &app,
        &browser,
        &format!(
            "/opportunities/classic?chars={BOB}&cs=item&cd=asc&ts=sigma&td=desc&page=2&per_page=25"
        ),
    )
    .await;

    let query = format!("?chars={BOB}&cs=item&cd=asc&ts=sigma&td=desc");
    for (label, href, _) in sub_menu(&html) {
        let table = match label.as_str() {
            "Recurring spread expansion" => "expansion",
            "Classic spread trade" => "classic",
            "Mean reversion" => "reversion",
            _ => "trend",
        };
        assert_eq!(href, format!("/opportunities/{table}{query}"), "{label}");
    }
}

#[sqlx::test]
async fn a_recent_book_refresh_shows_its_time_and_is_not_stale(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    let last = OffsetDateTime::now_utc() - Duration::minutes(5);
    book_sync(&pool, Some(last), None).await;

    let html = get(&app, &browser, "/opportunities/expansion").await;
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

    let html = get(&app, &browser, "/opportunities/expansion").await;
    let strip = freshness(&html);

    assert!(strip.contains(&sekondo::format::eve_time(last)), "{strip}");
    assert!(strip.contains("Stale"), "{strip}");
}

#[sqlx::test]
async fn a_book_that_was_never_refreshed_is_stale_and_waiting(pool: PgPool) {
    let (app, browser) = setup(pool).await;

    let html = get(&app, &browser, "/opportunities/expansion").await;
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
async fn the_navigation_links_to_the_first_table_and_marks_opportunities_current_on_every_table(
    pool: PgPool,
) {
    let (app, browser) = setup(pool).await;

    for table in ["expansion", "classic", "reversion", "trend"] {
        let html = get(&app, &browser, &format!("/opportunities/{table}")).await;

        let nav = html.split_once("</aside>").unwrap().0;
        assert!(
            nav.contains(r#"href="/opportunities/expansion" aria-label="Opportunities""#)
                || nav.contains(r#"href="/opportunities/expansion""#),
            "{nav}"
        );
        let current: Vec<_> = nav
            .split("<a ")
            .skip(1)
            .filter(|a| a.contains(r#"aria-current="page""#))
            .map(|a| {
                a.split(r#"aria-label=""#)
                    .nth(1)
                    .unwrap()
                    .split('"')
                    .next()
                    .unwrap()
            })
            .collect();
        assert_eq!(current, ["Opportunities"], "{table}");
    }
}

#[sqlx::test]
async fn the_navigation_carries_a_narrowed_selection_to_and_from_the_page(pool: PgPool) {
    let (app, browser) = setup(pool).await;
    browser.sso_round_trip(&app, "/auth/add", BOB).await;
    let unescaped = |html: String| html.replace("&#38;", "&").replace("&amp;", "&");

    for path in [
        format!("/orders/sell?chars={BOB}"),
        format!("/dashboard?chars={BOB}"),
        format!("/wealth?chars={BOB}"),
        format!("/opportunities/classic?chars={BOB}"),
    ] {
        let html = unescaped(get(&app, &browser, &path).await);
        assert!(
            html.contains(&format!(r#"href="/opportunities/expansion?chars={BOB}""#)),
            "{path} does not carry the selection to the page"
        );
        assert!(
            html.contains(&format!(r#"href="/orders/sell?chars={BOB}""#)),
            "{path}"
        );
    }
}
