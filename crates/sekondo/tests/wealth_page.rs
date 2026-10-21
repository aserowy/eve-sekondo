mod common;

use common::{AppBuilder, Browser, TestApp, kpi};
use sekondo::wealth_poller::Kind;
use time::{Duration, OffsetDateTime};

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;

async fn app() -> TestApp {
    AppBuilder::new().with_stub().spawn().await
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

async fn sync(app: &TestApp, character: i64, kind: &str, last: Option<OffsetDateTime>) {
    let wealth = &app.stores.wealth;
    let next = OffsetDateTime::now_utc();
    match (kind, last) {
        ("wallet", Some(at)) => wealth
            .store_wallet(character, at, 0, next, None)
            .await
            .unwrap(),
        ("assets", Some(at)) => wealth
            .store_assets(character, at, &[], &[], next, None)
            .await
            .unwrap(),
        (_, None) => wealth
            .insert_schedule(
                if kind == "wallet" {
                    Kind::Wallet
                } else {
                    Kind::Assets
                },
                &[character],
                &[next],
            )
            .await
            .unwrap(),
        _ => unreachable!("{kind}"),
    }
}

#[tokio::test]
async fn unauthenticated_requests_are_redirected_to_the_landing_page() {
    let app = app().await;
    for path in ["/wealth", "/wealth/content", "/wealth/freshness"] {
        let res = app.browser().get(&app.url(path)).await;
        assert!(res.status().is_redirection(), "{path}");
        assert_eq!(Browser::location(&res), "/", "{path}");
    }
}

#[tokio::test]
async fn the_page_shows_the_filter_and_the_wealth_link_as_current() {
    let app = app().await;
    let browser = signed_in(&app).await;

    let html = get(&app, &browser, "/wealth").await;

    assert!(html.contains("<h1>Wealth</h1>"), "one page title");
    assert_eq!(html.matches("<h1").count(), 1);
    assert!(html.contains(r#"data-character-filter"#));
    assert!(html.contains(r#"hx-get="/wealth""#));
    let filter = html.split(r#"id="filter-box""#).nth(1).unwrap();
    let filter = &filter[..filter.find(r#"id="content""#).unwrap()];
    assert!(filter.contains("Alice Alpha"), "{filter}");
    assert!(
        filter.contains(r#"id="wealth-range-control""#),
        "the range sits in the filter section"
    );
    assert!(!html.contains("panel"), "no self-made panels");
    assert!(
        !html.contains("<nav class=\"sub-menu\""),
        "no sub-menu on the wealth page"
    );
    let current: Vec<_> = html
        .split("<a ")
        .skip(1)
        .filter_map(|tag| tag.split_once('>'))
        .filter(|(attrs, _)| attrs.contains(r#"aria-current="page""#))
        .map(|(attrs, _)| {
            attrs
                .split(r#"aria-label=""#)
                .nth(1)
                .unwrap()
                .split('"')
                .next()
                .unwrap()
        })
        .collect();
    assert_eq!(current, ["Wealth"]);
}

#[tokio::test]
async fn the_filter_keeps_the_sorting_and_range_unless_they_are_the_defaults() {
    let app = app().await;
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

#[tokio::test]
async fn the_wealth_link_carries_the_selection_and_never_the_range() {
    let app = app().await;
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

#[tokio::test]
async fn the_freshness_strip_shows_wallet_and_assets_times_per_character() {
    let app = app().await;
    let browser = signed_in(&app).await;
    let now = OffsetDateTime::now_utc();
    let wallet = now - Duration::minutes(5);
    let assets = now - Duration::minutes(30);
    sync(&app, ALICE, "wallet", Some(wallet)).await;
    sync(&app, ALICE, "assets", Some(assets)).await;

    let html = get(&app, &browser, "/wealth/freshness").await;

    assert!(html.contains("Alice Alpha"));
    assert!(kind(&html, "wallet").contains(&sekondo::format::eve_time(wallet)));
    assert!(kind(&html, "assets").contains(&sekondo::format::eve_time(assets)));
    assert!(!html.contains("Stale"), "{html}");
}

#[tokio::test]
async fn assets_that_are_three_hours_old_are_stale_but_a_fresh_wallet_is_not() {
    let app = app().await;
    let browser = signed_in(&app).await;
    let now = OffsetDateTime::now_utc();
    sync(&app, ALICE, "wallet", Some(now - Duration::minutes(5))).await;
    sync(&app, ALICE, "assets", Some(now - Duration::hours(3))).await;

    let html = get(&app, &browser, "/wealth/freshness").await;

    assert!(kind(&html, "assets").contains("Stale"));
    assert!(!kind(&html, "wallet").contains("Stale"));
}

#[tokio::test]
async fn assets_under_two_hours_old_are_not_stale() {
    let app = app().await;
    let browser = signed_in(&app).await;
    let now = OffsetDateTime::now_utc();
    sync(&app, ALICE, "wallet", Some(now)).await;
    sync(&app, ALICE, "assets", Some(now - Duration::minutes(110))).await;

    let html = get(&app, &browser, "/wealth/freshness").await;

    assert!(!kind(&html, "assets").contains("Stale"));
}

#[tokio::test]
async fn a_character_needing_relogin_is_stale_in_both_and_links_to_login() {
    let app = app().await;
    let browser = signed_in(&app).await;
    let now = OffsetDateTime::now_utc();
    sync(&app, ALICE, "wallet", Some(now)).await;
    sync(&app, ALICE, "assets", Some(now)).await;
    app.state.repo.mark_needs_relogin(ALICE).await.unwrap();

    let html = get(&app, &browser, "/wealth/freshness").await;

    assert!(kind(&html, "wallet").contains("Stale"));
    assert!(kind(&html, "assets").contains("Stale"));
    assert!(html.contains(r#"href="/auth/add""#) && html.contains("Log in again"));
}

#[tokio::test]
async fn a_character_never_polled_waits_for_its_first_update() {
    let app = app().await;
    let browser = signed_in(&app).await;

    let html = get(&app, &browser, "/wealth/freshness").await;

    assert!(kind(&html, "wallet").contains("Waiting for first update"));
    assert!(kind(&html, "wallet").contains("never"));
}

#[tokio::test]
async fn only_the_selected_characters_of_the_account_are_shown() {
    let app = app().await;
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

async fn price(app: &TestApp, type_id: i64, price: Option<&str>) {
    let cents = price.map(|p| (p.parse::<f64>().unwrap() * 100.0).round() as i64);
    app.stores.market.seed_price(type_id, cents).await.unwrap();
}

async fn name(app: &TestApp, id: i64, name: &str) {
    app.stores
        .market
        .store_names(
            &[sekondo::store::NameRow {
                id,
                category: "x".to_owned(),
                name: Some(name.to_owned()),
            }],
            OffsetDateTime::now_utc(),
            None,
        )
        .await
        .unwrap();
}

async fn holds(
    app: &TestApp,
    item_id: i64,
    character: i64,
    type_id: i64,
    quantity: i64,
    copy: bool,
) {
    app.stores
        .wealth
        .seed_asset(
            character,
            &sekondo::esi::EsiAsset {
                item_id,
                type_id,
                location_id: 60003760,
                location_type: "station".to_owned(),
                location_flag: "Hangar".to_owned(),
                quantity,
                is_singleton: false,
                is_blueprint_copy: copy,
            },
            Some(60003760),
        )
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

#[tokio::test]
async fn unauthenticated_requests_for_the_list_are_redirected() {
    let app = app().await;
    let res = app.browser().get(&app.url("/wealth/content")).await;
    assert!(res.status().is_redirection());
    assert_eq!(Browser::location(&res), "/");
}

#[tokio::test]
async fn the_list_is_sorted_by_value_with_unpriced_rows_last_and_shows_the_total() {
    let app = app().await;
    let browser = signed_in(&app).await;
    price(&app, 34, Some("5.00")).await;
    price(&app, 35, Some("1000000.00")).await;
    name(&app, 34, "Tritanium").await;
    name(&app, 35, "Pyerite").await;
    name(&app, 60003760, "Jita IV").await;
    holds(&app, 1, ALICE, 34, 10, false).await; // 50.00
    holds(&app, 2, ALICE, 35, 1234, false).await; // 1,234,000,000.00
    holds(&app, 3, ALICE, 35, 2, true).await; // a blueprint copy: unpriced
    holds(&app, 4, ALICE, 99, 1, false).await; // no name, no price

    let html = get(&app, &browser, "/wealth/content").await;
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
    // The inventory card counts the unpriced rows.
    let inventory = kpi(&html, "kpi-inventory");
    assert_eq!(inventory.detail.as_deref(), Some("2 unpriced rows"));
}

#[tokio::test]
async fn amounts_are_shortened_with_the_exact_value_on_hover() {
    let app = app().await;
    let browser = signed_in(&app).await;
    price(&app, 35, Some("1234567890.12")).await;
    holds(&app, 1, ALICE, 35, 1, false).await;

    let html = get(&app, &browser, "/wealth/content").await;

    assert!(html.contains(">1.235b<"), "{html}");
    assert!(html.contains(r#"title="1,234,567,890.12 ISK""#), "{html}");
}

#[tokio::test]
async fn the_headers_sort_and_the_sorted_one_reverses() {
    let app = app().await;
    let browser = signed_in(&app).await;
    holds(&app, 1, ALICE, 34, 10, false).await;

    let html = get(&app, &browser, "/wealth/content").await;
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
        "/wealth/content?sort=item&dir=asc&range=30d&chars=90000001",
    )
    .await;
    assert!(by_item.contains(r#"aria-sort="ascending""#));
    assert!(
        by_item.contains(r#"href="/wealth?sort=item&#38;dir=desc&#38;range=30d""#),
        "{by_item}"
    );
}

#[tokio::test]
async fn an_empty_list_says_so() {
    let app = app().await;
    let browser = signed_in(&app).await;

    let html = get(&app, &browser, "/wealth/content").await;

    assert!(html.contains("No items."), "{html}");
    assert!(!html.contains("<table"));
}

#[tokio::test]
async fn the_list_covers_only_the_selected_characters_of_the_account() {
    let app = app().await;
    let alice = signed_in(&app).await;
    alice.sso_round_trip(&app, "/auth/add", BOB).await;
    let stranger = app.browser();
    stranger.sso_round_trip(&app, "/auth/login", 90000003).await;
    price(&app, 34, Some("1.00")).await;
    holds(&app, 1, ALICE, 34, 1, false).await;
    holds(&app, 2, BOB, 34, 2, false).await;
    holds(&app, 3, 90000003, 34, 999, false).await;

    let all = rows(&get(&app, &alice, "/wealth/content").await);
    assert_eq!(all.len(), 2);
    let narrowed = rows(&get(&app, &alice, &format!("/wealth/content?chars={BOB}")).await);
    assert_eq!(narrowed.len(), 1);
    assert_eq!(narrowed[0][0], "Bob Bravo");
    let sneaky = rows(&get(&app, &alice, "/wealth/content?chars=90000003").await);
    assert_eq!(
        sneaky.len(),
        2,
        "a foreign ID is ignored, so everything of the account shows"
    );
}

#[tokio::test]
async fn the_page_embeds_the_list_with_its_live_refresh() {
    let app = app().await;
    let browser = signed_in(&app).await;

    let html = get(&app, &browser, "/wealth?sort=item&dir=asc").await;

    assert!(html.contains(r#"id="wealth-items""#));
    assert!(html.contains(r#"hx-get="/wealth/content?sort=item&#38;dir=asc""#));
    assert!(html.contains("sse:wealth-updated, sse:orders-updated, htmx:sseOpen from:body"));
    assert!(!html.contains("sse:market-updated"));
}

// ---- the graph ----

#[tokio::test]
async fn unauthenticated_requests_for_the_graph_are_redirected() {
    let app = app().await;
    let res = app.browser().get(&app.url("/wealth/content")).await;
    assert!(res.status().is_redirection());
    assert_eq!(Browser::location(&res), "/");
}

#[tokio::test]
async fn without_history_the_graph_says_so_and_draws_nothing() {
    let app = app().await;
    let browser = signed_in(&app).await;

    let html = get(&app, &browser, "/wealth/content").await;

    assert!(html.contains("No history yet."));
    assert!(!html.contains("<canvas"));
}

#[tokio::test]
async fn the_graph_carries_four_series_and_the_range_links_push_the_page_url() {
    let app = app().await;
    let browser = signed_in(&app).await;
    let hour_ago = OffsetDateTime::now_utc() - Duration::hours(1);
    app.stores
        .wealth
        .store_wallet(ALICE, hour_ago, 500, hour_ago, None)
        .await
        .unwrap();

    let html = get(&app, &browser, "/wealth/content?range=30d&chars=90000001").await;

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
    assert!(html.contains(r#"hx-get="/wealth/content?range=90d""#));
    // A switch cancels a refresh still in flight, which would put the old range back.
    assert!(html.contains(r##"hx-sync="#content:replace""##));
    // The default range links to the bare page.
    assert!(html.contains(r#"hx-push-url="/wealth""#));
    // The live refresh keeps the range, and the filter's hidden input follows the switch.
    assert!(html.contains(r#"hx-get="/wealth/content?range=30d""#));
    assert!(html.contains(r#"id="wealth-range" name="range" value="30d" hx-swap-oob="true""#));
}

#[tokio::test]
async fn an_unknown_range_selects_seven_days() {
    let app = app().await;
    let browser = signed_in(&app).await;

    let html = get(&app, &browser, "/wealth/content?range=1y").await;

    assert!(html.contains(r#"value="7d""#));
    assert_eq!(html.matches(r#"aria-current="true""#).count(), 1);
}

#[tokio::test]
async fn a_character_needing_relogin_with_stale_wallet_and_assets_counts_once() {
    let app = app().await;
    let browser = signed_in(&app).await;
    let now = OffsetDateTime::now_utc();
    sync(&app, ALICE, "wallet", Some(now - Duration::hours(5))).await;
    sync(&app, ALICE, "assets", Some(now - Duration::hours(5))).await;
    app.state.repo.mark_needs_relogin(ALICE).await.unwrap();

    let html = get(&app, &browser, "/wealth/freshness").await;

    assert!(html.contains("Stale"), "{html}");
    assert!(!html.contains("warnings"), "no warning count: {html}");
}

#[tokio::test]
async fn stale_assets_alone_count_as_a_warning() {
    let app = app().await;
    let browser = signed_in(&app).await;
    let now = OffsetDateTime::now_utc();
    sync(&app, ALICE, "wallet", Some(now)).await;
    sync(&app, ALICE, "assets", Some(now - Duration::hours(3))).await;

    let html = get(&app, &browser, "/wealth/freshness").await;

    assert!(html.contains("Stale"), "{html}");
    assert!(!html.contains("warnings"), "no warning count: {html}");
}

#[tokio::test]
async fn fresh_wallet_and_assets_show_no_warning_count() {
    let app = app().await;
    let browser = signed_in(&app).await;
    let now = OffsetDateTime::now_utc();
    sync(&app, ALICE, "wallet", Some(now)).await;
    sync(&app, ALICE, "assets", Some(now)).await;

    let html = get(&app, &browser, "/wealth/freshness").await;

    assert!(!html.contains("warnings"), "{html}");
}

// ---- KPI cards ----

/// Alice holds 50m ISK in her wallet, sells 100m and has 20m in escrow, and her priced inventory is
/// worth 30m, with two rows without a price (blueprint copies).
async fn alice_worth_200m(app: &TestApp) {
    let now = OffsetDateTime::now_utc();
    let wealth = &app.stores.wealth;
    wealth
        .store_wallet(ALICE, now, 5_000_000_000, now, None)
        .await
        .unwrap();
    wealth
        .seed_poll_totals(
            ALICE,
            &sekondo::store::PollTotalsRow {
                polled_at: now,
                sell_cents: 10_000_000_000,
                escrow_cents: 2_000_000_000,
                sell_corp_cents: 0,
                escrow_corp_cents: 0,
                sell_count: 1,
                buy_count: 1,
            },
        )
        .await
        .unwrap();
    wealth
        .seed_inventory_value(ALICE, now, 3_000_000_000)
        .await
        .unwrap();
    holds(app, 1, ALICE, 34, 1, true).await;
    holds(app, 2, ALICE, 35, 1, true).await;
}

#[tokio::test]
async fn the_cards_read_the_current_figures_and_count_the_unpriced_rows() {
    let app = app().await;
    let browser = signed_in(&app).await;
    alice_worth_200m(&app).await;

    let html = get(&app, &browser, "/wealth").await;

    for (id, name, value) in [
        ("kpi-wealth", "Wealth", "200m"),
        ("kpi-wallet", "Wallet", "50m"),
        ("kpi-sell", "Sell orders", "100m"),
        ("kpi-escrow", "Buy escrow", "20m"),
        ("kpi-inventory", "Inventory", "30m"),
    ] {
        assert_eq!(kpi(&html, id).value, value, "{name}");
        assert!(
            html.contains(&format!(r#"<small class="name">{name}</small>"#)),
            "{name}"
        );
    }
    assert_eq!(
        kpi(&html, "kpi-inventory").detail.as_deref(),
        Some("2 unpriced rows")
    );
    assert_eq!(kpi(&html, "kpi-wealth").detail, None);
    assert_eq!(kpi(&html, "kpi-wealth").title, "200,000,000.00 ISK");
}

#[tokio::test]
async fn the_total_equals_the_dashboard_wealth_card_for_the_same_selection() {
    let app = app().await;
    let browser = signed_in(&app).await;
    alice_worth_200m(&app).await;

    let wealth = get(&app, &browser, "/wealth").await;
    let dashboard = get(&app, &browser, "/dashboard").await;

    let tile = dashboard.split(r#"id="tile-wealth""#).nth(1).unwrap();
    let tile = &tile[..tile.find("</article>").unwrap()];
    let title = tile
        .split(r#"title=""#)
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    assert_eq!(kpi(&wealth, "kpi-wealth").title, title);
}

#[tokio::test]
async fn the_range_does_not_change_the_cards() {
    let app = app().await;
    let browser = signed_in(&app).await;
    alice_worth_200m(&app).await;

    let cards = |html: &str| {
        [
            "kpi-wealth",
            "kpi-wallet",
            "kpi-sell",
            "kpi-escrow",
            "kpi-inventory",
        ]
        .map(|id| kpi(html, id).title)
    };
    let week = cards(&get(&app, &browser, "/wealth/content").await);
    let day = cards(&get(&app, &browser, "/wealth/content?range=24h").await);
    let quarter = cards(&get(&app, &browser, "/wealth/content?range=90d").await);

    assert_eq!(week, day);
    assert_eq!(week, quarter);
}

// ---- pagination ----

#[tokio::test]
async fn three_hundred_rows_read_page_one_of_twelve_and_the_cards_do_not_depend_on_the_page() {
    let app = app().await;
    let browser = signed_in(&app).await;
    for i in 0..300 {
        price(&app, 1000 + i, Some("1.00")).await;
        holds(&app, 100 + i, ALICE, 1000 + i, 1, false).await;
    }

    let first = get(&app, &browser, "/wealth/content?sort=item&dir=asc").await;
    let second = get(&app, &browser, "/wealth/content?sort=item&dir=asc&page=2").await;

    assert!(first.contains("Page 1 of 12"), "{first}");
    assert_eq!(rows(&first).len(), 25);
    assert!(second.contains("Page 2 of 12"));
    assert_eq!(rows(&second).len(), 25);
    assert_ne!(rows(&first)[0], rows(&second)[0]);
    assert_eq!(
        kpi(&first, "kpi-wealth").title,
        kpi(&second, "kpi-wealth").title
    );
    // The sorting survives in the links of the second page.
    let page = get(&app, &browser, "/wealth?sort=item&dir=asc&page=2")
        .await
        .replace("&#38;", "&");
    assert!(
        page.contains(r#"hx-get="/wealth/content?sort=item&dir=asc&page=2""#),
        "{page}"
    );
}

#[tokio::test]
async fn a_range_switch_and_a_sorting_start_on_the_first_page() {
    let app = app().await;
    let browser = signed_in(&app).await;
    holds(&app, 1, ALICE, 34, 1, false).await;

    let html = get(
        &app,
        &browser,
        "/wealth/content?page=2&per_page=50&range=24h",
    )
    .await
    .replace("&#38;", "&");

    let link = |needle: &str| {
        html.split("<a ")
            .find(|a| a.contains(needle))
            .unwrap()
            .to_owned()
    };
    for needle in ["range=30d", "sort=item"] {
        let link = link(needle);
        assert!(
            !link.contains("&page=") && !link.contains("?page="),
            "{link}"
        );
        assert!(link.contains("per_page=50"), "{link}");
    }
}
