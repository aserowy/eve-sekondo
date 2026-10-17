mod common;

use common::{AppBuilder, Browser, TestApp};
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;

async fn app(pool: PgPool) -> TestApp {
    AppBuilder::new(pool).with_stub().spawn().await
}

async fn signed_in(app: &TestApp) -> Browser {
    let browser = app.browser();
    browser.sso_round_trip(app, "/auth/login", ALICE).await;
    browser
}

async fn get(app: &TestApp, browser: &Browser, path: &str) -> String {
    let res = browser.get(&app.url(path)).await;
    assert_eq!(res.status(), 200, "{path}");
    res.text().await.unwrap()
}

/// The part of the freshness strip about one kind (`wallet` or `assets`) of the first character.
fn kind<'a>(html: &'a str, kind: &str) -> &'a str {
    let marker = format!(r#"data-kind="{kind}""#);
    let start = html
        .find(&marker)
        .unwrap_or_else(|| panic!("no {kind} in {html}"));
    let rest = &html[start..];
    &rest[..rest.find("</div>").unwrap()]
}

async fn sync(pool: &PgPool, character: i64, kind: &str, last: Option<OffsetDateTime>) {
    sqlx::query(
        "INSERT INTO wealth_sync (character_id, kind, next_poll_at, last_success_at) VALUES ($1, $2, now(), $3)",
    )
    .bind(character)
    .bind(kind)
    .bind(last)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test]
async fn unauthenticated_requests_are_redirected_to_the_landing_page(pool: PgPool) {
    let app = app(pool).await;
    for path in ["/wealth", "/wealth/freshness"] {
        let res = app.browser().get(&app.url(path)).await;
        assert!(res.status().is_redirection(), "{path}");
        assert_eq!(Browser::location(&res), "/", "{path}");
    }
}

#[sqlx::test]
async fn the_page_shows_the_filter_and_the_wealth_link_as_current(pool: PgPool) {
    let app = app(pool).await;
    let browser = signed_in(&app).await;

    let html = get(&app, &browser, "/wealth").await;

    assert!(!html.contains("<h1"), "the navigation names the page");
    assert!(html.contains(r#"data-character-filter"#));
    assert!(html.contains(r#"hx-get="/wealth""#));
    assert!(html.contains("Alice Alpha"));
    let current: Vec<_> = html
        .split("<a ")
        .skip(1)
        .filter(|tag| {
            tag.split_once('>')
                .is_some_and(|(a, _)| a.contains(r#"aria-current="page""#))
        })
        .map(|tag| {
            tag.split_once('>')
                .unwrap()
                .1
                .split('<')
                .next()
                .unwrap()
                .trim()
                .to_owned()
        })
        .collect();
    assert_eq!(current, ["Wealth"]);
}

#[sqlx::test]
async fn the_filter_keeps_the_sorting_and_range_unless_they_are_the_defaults(pool: PgPool) {
    let app = app(pool).await;
    let browser = signed_in(&app).await;

    let plain = get(&app, &browser, "/wealth").await;
    assert!(plain.contains(r#"data-default-sort="value""#));
    assert!(plain.contains(r#"data-default-dir="desc""#));
    assert!(plain.contains(r#"name="sort" value="value""#));
    assert!(plain.contains(r#"name="range" value="7d""#));

    let custom = get(&app, &browser, "/wealth?sort=item&dir=asc&range=30d").await;
    assert!(custom.contains(r#"name="sort" value="item""#));
    assert!(custom.contains(r#"name="dir" value="asc""#));
    assert!(custom.contains(r#"name="range" value="30d""#));
    // The freshness strip only depends on the selection.
    assert!(custom.contains(r#"hx-get="/wealth/freshness""#));
}

#[sqlx::test]
async fn the_wealth_link_carries_the_selection_and_never_the_range(pool: PgPool) {
    let app = app(pool).await;
    let browser = signed_in(&app).await;
    browser.sso_round_trip(&app, "/auth/add", BOB).await;

    let dashboard = get(
        &app,
        &browser,
        &format!("/dashboard?chars={ALICE}&range=30d"),
    )
    .await;
    assert!(
        dashboard.contains(&format!(r#"<a href="/wealth?chars={ALICE}""#)),
        "{dashboard}"
    );
    assert!(!dashboard.contains("/wealth?chars=90000001&amp;range"));

    let all = get(&app, &browser, "/dashboard").await;
    assert!(all.contains(r#"<a href="/wealth""#));
}

#[sqlx::test]
async fn the_freshness_strip_shows_wallet_and_assets_times_per_character(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    let now = OffsetDateTime::now_utc();
    let wallet = now - Duration::minutes(5);
    let assets = now - Duration::minutes(30);
    sync(&pool, ALICE, "wallet", Some(wallet)).await;
    sync(&pool, ALICE, "assets", Some(assets)).await;

    let html = get(&app, &browser, "/wealth/freshness").await;

    assert!(html.contains("Alice Alpha"));
    assert!(kind(&html, "wallet").contains(&sekondo::format::eve_time(wallet)));
    assert!(kind(&html, "assets").contains(&sekondo::format::eve_time(assets)));
    assert!(!html.contains("Stale"), "{html}");
}

#[sqlx::test]
async fn assets_that_are_three_hours_old_are_stale_but_a_fresh_wallet_is_not(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    let now = OffsetDateTime::now_utc();
    sync(&pool, ALICE, "wallet", Some(now - Duration::minutes(5))).await;
    sync(&pool, ALICE, "assets", Some(now - Duration::hours(3))).await;

    let html = get(&app, &browser, "/wealth/freshness").await;

    assert!(kind(&html, "assets").contains("Stale"));
    assert!(!kind(&html, "wallet").contains("Stale"));
}

#[sqlx::test]
async fn assets_under_two_hours_old_are_not_stale(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    let now = OffsetDateTime::now_utc();
    sync(&pool, ALICE, "wallet", Some(now)).await;
    sync(&pool, ALICE, "assets", Some(now - Duration::minutes(110))).await;

    let html = get(&app, &browser, "/wealth/freshness").await;

    assert!(!kind(&html, "assets").contains("Stale"));
}

#[sqlx::test]
async fn a_character_needing_relogin_is_stale_in_both_and_links_to_login(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    let now = OffsetDateTime::now_utc();
    sync(&pool, ALICE, "wallet", Some(now)).await;
    sync(&pool, ALICE, "assets", Some(now)).await;
    app.state.repo.mark_needs_relogin(ALICE).await.unwrap();

    let html = get(&app, &browser, "/wealth/freshness").await;

    assert!(kind(&html, "wallet").contains("Stale"));
    assert!(kind(&html, "assets").contains("Stale"));
    assert!(html.contains(r#"href="/auth/add""#) && html.contains("Log in again"));
}

#[sqlx::test]
async fn a_character_never_polled_waits_for_its_first_update(pool: PgPool) {
    let app = app(pool).await;
    let browser = signed_in(&app).await;

    let html = get(&app, &browser, "/wealth/freshness").await;

    assert!(kind(&html, "wallet").contains("Waiting for first update"));
    assert!(kind(&html, "wallet").contains("never"));
}

#[sqlx::test]
async fn only_the_selected_characters_of_the_account_are_shown(pool: PgPool) {
    let app = app(pool).await;
    let alice = signed_in(&app).await;
    alice.sso_round_trip(&app, "/auth/add", BOB).await;
    let stranger = app.browser();
    stranger.sso_round_trip(&app, "/auth/login", 90000003).await;

    let both = get(&app, &alice, "/wealth/freshness").await;
    assert!(both.contains("Alice Alpha") && both.contains("Bob Bravo"));
    assert!(!both.contains("Carol"));

    let narrowed = get(&app, &alice, &format!("/wealth/freshness?chars={BOB}")).await;
    assert!(!narrowed.contains("Alice Alpha") && narrowed.contains("Bob Bravo"));
    // A foreign ID in the URL is ignored.
    let sneaky = get(&app, &alice, "/wealth/freshness?chars=90000003").await;
    assert!(!sneaky.contains("Carol"));
}

// ---- the inventory list ----

async fn price(pool: &PgPool, type_id: i64, price: Option<&str>) {
    sqlx::query("INSERT INTO item_prices (type_id, average_price) VALUES ($1, $2::numeric)")
        .bind(type_id)
        .bind(price)
        .execute(pool)
        .await
        .unwrap();
}

async fn name(pool: &PgPool, id: i64, name: &str) {
    sqlx::query("INSERT INTO names (id, category, name, resolved_at) VALUES ($1, 'x', $2, now())")
        .bind(id)
        .bind(name)
        .execute(pool)
        .await
        .unwrap();
}

async fn holds(
    pool: &PgPool,
    item_id: i64,
    character: i64,
    type_id: i64,
    quantity: i64,
    copy: bool,
) {
    sqlx::query(
        "INSERT INTO current_assets (item_id, character_id, type_id, location_id, location_type, \
         location_flag, quantity, is_singleton, is_blueprint_copy, top_location_id) \
         VALUES ($1, $2, $3, 60003760, 'station', 'Hangar', $4, false, $5, 60003760)",
    )
    .bind(item_id)
    .bind(character)
    .bind(type_id)
    .bind(quantity)
    .bind(copy)
    .execute(pool)
    .await
    .unwrap();
}

/// The text of each row's cells, row by row.
fn rows(html: &str) -> Vec<Vec<String>> {
    let body = html.split("<tbody>").nth(1).unwrap_or_default();
    body.split("<tr")
        .skip(1)
        .map(|row| {
            row.split("<td")
                .skip(1)
                .map(|cell| {
                    let text = cell
                        .split_once('>')
                        .unwrap()
                        .1
                        .split("</td>")
                        .next()
                        .unwrap();
                    text.trim().to_owned()
                })
                .collect()
        })
        .collect()
}

#[sqlx::test]
async fn unauthenticated_requests_for_the_list_are_redirected(pool: PgPool) {
    let app = app(pool).await;
    let res = app.browser().get(&app.url("/wealth/items")).await;
    assert!(res.status().is_redirection());
    assert_eq!(Browser::location(&res), "/");
}

#[sqlx::test]
async fn the_list_is_sorted_by_value_with_unpriced_rows_last_and_shows_the_total(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    price(&pool, 34, Some("5.00")).await;
    price(&pool, 35, Some("1000000.00")).await;
    name(&pool, 34, "Tritanium").await;
    name(&pool, 35, "Pyerite").await;
    name(&pool, 60003760, "Jita IV").await;
    holds(&pool, 1, ALICE, 34, 10, false).await; // 50.00
    holds(&pool, 2, ALICE, 35, 1234, false).await; // 1,234,000,000.00
    holds(&pool, 3, ALICE, 35, 2, true).await; // a blueprint copy: unpriced
    holds(&pool, 4, ALICE, 99, 1, false).await; // no name, no price

    let html = get(&app, &browser, "/wealth/items").await;
    let rows = rows(&html);

    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0][..2], ["Alice Alpha", "Pyerite"]);
    assert_eq!(rows[1][..2], ["Alice Alpha", "Tritanium"]);
    let unpriced: Vec<&str> = rows[2..].iter().map(|r| r[1].as_str()).collect();
    assert!(
        unpriced.contains(&"Pyerite") && unpriced.contains(&"#99"),
        "{unpriced:?}"
    );
    for row in &rows[2..] {
        assert_eq!((row[3].as_str(), row[4].as_str()), ("\u{2014}", "\u{2014}"));
    }
    assert_eq!(rows[0][2], "1234");
    assert_eq!(rows[0][5], "Jita IV");
    assert!(html.contains("2 unpriced"), "{html}");
    assert!(html.contains("Total value"), "{html}");
}

#[sqlx::test]
async fn amounts_are_shortened_with_the_exact_value_on_hover(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    price(&pool, 35, Some("1234567890.12")).await;
    holds(&pool, 1, ALICE, 35, 1, false).await;

    let html = get(&app, &browser, "/wealth/items").await;

    assert!(html.contains(">1.235b<"), "{html}");
    assert!(html.contains(r#"title="1,234,567,890.12 ISK""#), "{html}");
}

#[sqlx::test]
async fn the_headers_sort_and_the_sorted_one_reverses(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    holds(&pool, 1, ALICE, 34, 10, false).await;

    let html = get(&app, &browser, "/wealth/items").await;
    assert!(
        html.contains(r#"aria-sort="descending""#),
        "value is sorted by default"
    );
    assert!(
        html.contains(r#"href="/wealth?sort=value&#38;dir=asc""#),
        "{html}"
    );
    assert!(
        html.contains(r#"href="/wealth?sort=item&#38;dir=asc""#),
        "{html}"
    );

    let by_item = get(
        &app,
        &browser,
        "/wealth/items?sort=item&dir=asc&range=30d&chars=90000001",
    )
    .await;
    assert!(by_item.contains(r#"aria-sort="ascending""#));
    assert!(
        by_item.contains(r#"href="/wealth?sort=item&#38;dir=desc&#38;range=30d""#),
        "{by_item}"
    );
}

#[sqlx::test]
async fn an_empty_list_says_so(pool: PgPool) {
    let app = app(pool).await;
    let browser = signed_in(&app).await;

    let html = get(&app, &browser, "/wealth/items").await;

    assert!(html.contains("No items."), "{html}");
    assert!(!html.contains("<table"));
}

#[sqlx::test]
async fn the_list_covers_only_the_selected_characters_of_the_account(pool: PgPool) {
    let app = app(pool.clone()).await;
    let alice = signed_in(&app).await;
    alice.sso_round_trip(&app, "/auth/add", BOB).await;
    let stranger = app.browser();
    stranger.sso_round_trip(&app, "/auth/login", 90000003).await;
    price(&pool, 34, Some("1.00")).await;
    holds(&pool, 1, ALICE, 34, 1, false).await;
    holds(&pool, 2, BOB, 34, 2, false).await;
    holds(&pool, 3, 90000003, 34, 999, false).await;

    let all = rows(&get(&app, &alice, "/wealth/items").await);
    assert_eq!(all.len(), 2);
    let narrowed = rows(&get(&app, &alice, &format!("/wealth/items?chars={BOB}")).await);
    assert_eq!(narrowed.len(), 1);
    assert_eq!(narrowed[0][0], "Bob Bravo");
    let sneaky = rows(&get(&app, &alice, "/wealth/items?chars=90000003").await);
    assert_eq!(
        sneaky.len(),
        2,
        "a foreign ID is ignored, so everything of the account shows"
    );
}

#[sqlx::test]
async fn the_page_embeds_the_list_with_its_live_refresh(pool: PgPool) {
    let app = app(pool).await;
    let browser = signed_in(&app).await;

    let html = get(&app, &browser, "/wealth?sort=item&dir=asc").await;

    assert!(html.contains(r#"id="wealth-items""#));
    assert!(html.contains(r#"hx-get="/wealth/items?sort=item&#38;dir=asc""#));
    assert!(html.contains("sse:wealth-updated, sse:orders-updated, htmx:sseOpen from:body"));
    assert!(!html.contains("sse:market-updated"));
}

// ---- the graph ----

#[sqlx::test]
async fn unauthenticated_requests_for_the_graph_are_redirected(pool: PgPool) {
    let app = app(pool).await;
    let res = app.browser().get(&app.url("/wealth/graph")).await;
    assert!(res.status().is_redirection());
    assert_eq!(Browser::location(&res), "/");
}

#[sqlx::test]
async fn without_history_the_graph_says_so_and_draws_nothing(pool: PgPool) {
    let app = app(pool).await;
    let browser = signed_in(&app).await;

    let html = get(&app, &browser, "/wealth/graph").await;

    assert!(html.contains("No history yet."));
    assert!(!html.contains("<canvas"));
}

#[sqlx::test]
async fn the_graph_carries_four_series_and_the_range_links_push_the_page_url(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    sqlx::query("INSERT INTO wallet_balances (character_id, polled_at, balance) VALUES ($1, now() - interval '1 hour', 5)")
        .bind(ALICE)
        .execute(&pool)
        .await
        .unwrap();

    let html = get(&app, &browser, "/wealth/graph?range=30d&chars=90000001").await;

    assert!(html.contains(r#"data-chart="wealth-stack""#));
    for name in ["Sell orders", "Buy escrow", "Wallet", "Inventory"] {
        assert!(
            html.contains(&format!("&#34;name&#34;:&#34;{name}&#34;")),
            "{name}"
        );
    }
    // Alice is the only character, so `chars` is dropped; the page URL pushed keeps range.
    assert!(
        html.contains(r#"hx-push-url="/wealth?range=90d""#),
        "{html}"
    );
    assert!(html.contains(r#"hx-get="/wealth/graph?range=90d""#));
    // A switch cancels a refresh still in flight, which would put the old range back.
    assert!(html.contains(r##"hx-sync="#wealth-graph:replace""##));
    // The default range links to the bare page.
    assert!(html.contains(r#"hx-push-url="/wealth""#));
    // The live refresh keeps the range, and the filter's hidden input follows the switch.
    assert!(html.contains(r#"hx-get="/wealth/graph?range=30d""#));
    assert!(html.contains(r#"id="wealth-range" name="range" value="30d" hx-swap-oob="true""#));
    assert!(html.contains("sse:wealth-updated, sse:orders-updated, htmx:sseOpen from:body"));
}

#[sqlx::test]
async fn an_unknown_range_selects_seven_days(pool: PgPool) {
    let app = app(pool).await;
    let browser = signed_in(&app).await;

    let html = get(&app, &browser, "/wealth/graph?range=1y").await;

    assert!(html.contains(r#"value="7d""#));
    assert_eq!(html.matches(r#"aria-current="true""#).count(), 1);
}

const WARNINGS_BADGE: &str = r#"class="badge stale warnings">"#;

#[sqlx::test]
async fn a_character_needing_relogin_with_stale_wallet_and_assets_counts_once(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    let now = OffsetDateTime::now_utc();
    sync(&pool, ALICE, "wallet", Some(now - Duration::hours(5))).await;
    sync(&pool, ALICE, "assets", Some(now - Duration::hours(5))).await;
    app.state.repo.mark_needs_relogin(ALICE).await.unwrap();

    let html = get(&app, &browser, "/wealth/freshness").await;

    assert!(
        html.contains(&format!("{WARNINGS_BADGE}1</mark>")),
        "{html}"
    );
}

#[sqlx::test]
async fn stale_assets_alone_count_as_a_warning(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    let now = OffsetDateTime::now_utc();
    sync(&pool, ALICE, "wallet", Some(now)).await;
    sync(&pool, ALICE, "assets", Some(now - Duration::hours(3))).await;

    let html = get(&app, &browser, "/wealth/freshness").await;

    assert!(
        html.contains(&format!("{WARNINGS_BADGE}1</mark>")),
        "{html}"
    );
}

#[sqlx::test]
async fn fresh_wallet_and_assets_show_no_warning_count(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    let now = OffsetDateTime::now_utc();
    sync(&pool, ALICE, "wallet", Some(now)).await;
    sync(&pool, ALICE, "assets", Some(now)).await;

    let html = get(&app, &browser, "/wealth/freshness").await;

    assert!(!html.contains("warnings"), "{html}");
}
