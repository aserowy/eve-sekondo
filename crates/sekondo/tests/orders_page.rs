mod common;

use common::{AppBuilder, Browser, TestApp};
use serde_json::{Value, json};
use sqlx::PgPool;

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const CAROL: i64 = 90000003;

async fn app(pool: PgPool) -> TestApp {
    AppBuilder::new(pool).with_stub().spawn().await
}

async fn signed_in(app: &TestApp, character: i64) -> Browser {
    let browser = app.browser();
    browser.sso_round_trip(app, "/auth/login", character).await;
    browser
}

async fn get(app: &TestApp, browser: &Browser, path: &str) -> String {
    let res = browser.get(&app.url(path)).await;
    assert_eq!(res.status(), 200, "{path}");
    res.text().await.unwrap()
}

/// The part of the page between two markers (the sell and the buy table).
fn between<'a>(html: &'a str, from: &str, to: &str) -> &'a str {
    let start = html
        .find(from)
        .unwrap_or_else(|| panic!("no {from} in {html}"));
    let rest = &html[start..];
    &rest[..rest.find(to).unwrap_or(rest.len())]
}

fn sell_table(html: &str) -> &str {
    between(html, r#"id="sell-orders""#, r#"id="buy-orders""#)
}

fn buy_table(html: &str) -> &str {
    between(html, r#"id="buy-orders""#, "</section>")
}

fn row(html: &str, order_id: i64) -> &str {
    let marker = format!(r#"id="order-{order_id}""#);
    between(html, &marker, "</tr>")
}

fn order(id: i64, type_id: i64, location: i64, buy: bool, price: f64) -> Value {
    let mut o = json!({
        "order_id": id, "type_id": type_id, "region_id": 10000002, "location_id": location,
        "is_buy_order": buy, "is_corporation": false, "price": price,
        "volume_remain": 4, "volume_total": 8, "min_volume": 1, "range": "station",
        "issued": "2026-10-10T12:00:00Z", "duration": 90
    });
    if buy {
        o["escrow"] = json!(1000.0);
    }
    o
}

/// Alice's fixture: sells 5001 (Tritanium, 10 of 20 at 1.5m) and 5002 (Pyerite, 100 at 950.50),
/// buy 5003 (Mexallon, escrow 2.5m, in a structure).
async fn with_polled_fixture(pool: PgPool) -> (TestApp, Browser) {
    let app = app(pool).await;
    let browser = signed_in(&app, ALICE).await;
    app.state.poller.poll_character(ALICE).await;
    (app, browser)
}

#[sqlx::test]
async fn sell_and_buy_orders_are_in_separate_tables(pool: PgPool) {
    let (app, browser) = with_polled_fixture(pool).await;

    let html = get(&app, &browser, "/orders").await;

    let sell = sell_table(&html);
    assert!(sell.contains(r#"id="order-5001""#) && sell.contains(r#"id="order-5002""#));
    assert!(!sell.contains(r#"id="order-5003""#));
    let buy = buy_table(&html);
    assert!(buy.contains(r#"id="order-5003""#));
    assert!(!buy.contains(r#"id="order-5001""#));
}

#[sqlx::test]
async fn a_sell_row_shows_every_column(pool: PgPool) {
    let (app, browser) = with_polled_fixture(pool).await;

    let html = get(&app, &browser, "/orders").await;
    let r = row(&html, 5001);

    assert!(r.contains("Alice Alpha"));
    assert!(r.contains("Tritanium"));
    assert!(r.contains("Jita IV - Moon 4 - Caldari Navy Assembly Plant"));
    assert!(r.contains("The Forge"));
    assert!(r.contains("10 / 20"));
    // The price, and the value (price times remaining volume), each with its exact amount.
    assert!(
        r.contains(">1.5m<") && r.contains(r#"title="1,500,000.00 ISK""#),
        "{r}"
    );
    assert!(
        r.contains(">15m<") && r.contains(r#"title="15,000,000.00 ISK""#),
        "{r}"
    );
    let small = row(&html, 5002);
    assert!(small.contains(">950.50<"), "{small}");
}

#[sqlx::test]
async fn a_buy_row_shows_the_escrow_and_the_structure_name(pool: PgPool) {
    let (app, browser) = with_polled_fixture(pool).await;

    let html = get(&app, &browser, "/orders").await;
    let r = row(&html, 5003);

    assert!(r.contains("Mexallon"));
    assert!(r.contains("Alpha Keepstar"));
    assert!(
        r.contains(">2.5m<") && r.contains(r#"title="2,500,000.00 ISK""#),
        "{r}"
    );
    assert!(r.contains("15 / 20"));
}

#[sqlx::test]
async fn an_unresolved_name_shows_a_placeholder_and_an_inaccessible_structure_says_so(
    pool: PgPool,
) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app, ALICE).await;
    // 999999 is no known type, and the structure is refused.
    app.stub_control(
        "structure-access",
        json!({"structure_id": 1000000000001_i64, "allowed": false}),
    )
    .await;
    app.stub_control(
        "orders",
        json!({"character_id": ALICE, "orders": [
            order(1, 999999, 60003760, false, 10.0),
            order(2, 34, 1000000000001_i64, true, 10.0),
        ]}),
    )
    .await;
    app.state.poller.poll_character(ALICE).await;

    let html = get(&app, &browser, "/orders").await;

    assert!(row(&html, 1).contains("#999999"), "{}", row(&html, 1));
    assert!(
        row(&html, 2).contains("Unknown structure (1000000000001)"),
        "{}",
        row(&html, 2)
    );
}

#[sqlx::test]
async fn corporation_orders_carry_a_marker(pool: PgPool) {
    let app = app(pool).await;
    let browser = signed_in(&app, ALICE).await;
    let mut corp = order(1, 34, 60003760, false, 10.0);
    corp["is_corporation"] = json!(true);
    app.stub_control(
        "orders",
        json!({"character_id": ALICE, "orders": [corp, order(2, 35, 60003760, false, 10.0)]}),
    )
    .await;
    app.state.poller.poll_character(ALICE).await;

    let html = get(&app, &browser, "/orders").await;

    assert!(row(&html, 1).contains("Corp"));
    assert!(!row(&html, 2).contains("Corp"));
}

#[sqlx::test]
async fn time_left_and_the_issue_date_are_shown_in_whole_units_and_eve_time(pool: PgPool) {
    let app = app(pool).await;
    let browser = signed_in(&app, ALICE).await;
    let issued =
        time::OffsetDateTime::now_utc() - time::Duration::days(80) - time::Duration::minutes(115);
    let mut o = order(1, 34, 60003760, false, 10.0);
    o["issued"] = json!(
        issued
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap()
    );
    app.stub_control("orders", json!({"character_id": ALICE, "orders": [o]}))
        .await;
    app.state.poller.poll_character(ALICE).await;

    let html = get(&app, &browser, "/orders").await;

    let r = row(&html, 1);
    assert!(r.contains("9 days 22 hours"), "{r}");
    let expected = sekondo::format::eve_time(issued);
    assert!(r.contains(&expected), "{expected} in {r}");
}

#[sqlx::test]
async fn without_orders_both_tables_say_so(pool: PgPool) {
    let app = app(pool).await;
    let browser = signed_in(&app, CAROL).await;
    app.state.poller.poll_character(CAROL).await;

    let html = get(&app, &browser, "/orders").await;

    assert!(sell_table(&html).contains("No sell orders"));
    assert!(buy_table(&html).contains("No buy orders"));
}

#[sqlx::test]
async fn only_orders_of_the_own_account_are_shown(pool: PgPool) {
    let app = app(pool).await;
    let alice = signed_in(&app, ALICE).await;
    let _bob = signed_in(&app, BOB).await;
    app.state.poller.poll_character(ALICE).await;
    app.state.poller.poll_character(BOB).await;

    let html = get(&app, &alice, "/orders").await;

    assert!(html.contains(r#"id="order-5001""#));
    assert!(!html.contains(r#"id="order-6001""#));
    assert!(!html.contains("Bob Bravo"));
}

#[sqlx::test]
async fn an_unauthenticated_request_is_redirected_to_the_landing_page(pool: PgPool) {
    let app = app(pool).await;
    for path in ["/orders", "/orders/tables", "/orders/freshness"] {
        let res = app.browser().get(&app.url(path)).await;
        assert!(res.status().is_redirection(), "{path}");
        assert_eq!(Browser::location(&res), "/", "{path}");
    }
}

/// The page with HTML-escaped ampersands turned back, so URLs can be compared as written.
fn unescaped(html: &str) -> String {
    html.replace("&#38;", "&").replace("&amp;", "&")
}

fn position(html: &str, order_id: i64) -> usize {
    html.find(&format!(r#"id="order-{order_id}""#))
        .unwrap_or_else(|| panic!("order {order_id} missing"))
}

#[sqlx::test]
async fn the_default_sorting_is_by_item_name_ascending(pool: PgPool) {
    let (app, browser) = with_polled_fixture(pool).await;

    let html = get(&app, &browser, "/orders").await;

    // Pyerite before Tritanium.
    assert!(position(&html, 5002) < position(&html, 5001));
}

#[sqlx::test]
async fn sorting_by_price_can_be_reversed_and_is_kept_in_the_header_links(pool: PgPool) {
    let (app, browser) = with_polled_fixture(pool).await;

    let asc = get(&app, &browser, "/orders?sort=price&dir=asc").await;
    assert!(position(&asc, 5002) < position(&asc, 5001));
    // The header now offers the reverse order.
    assert!(unescaped(&asc).contains("sort=price&dir=desc"), "{asc}");

    let desc = get(&app, &browser, "/orders?sort=price&dir=desc").await;
    assert!(position(&desc, 5001) < position(&desc, 5002));
    assert!(unescaped(&desc).contains("sort=price&dir=asc"));
}

#[sqlx::test]
async fn the_buy_table_is_sorted_independently(pool: PgPool) {
    let app = app(pool).await;
    let browser = signed_in(&app, ALICE).await;
    app.stub_control(
        "orders",
        json!({"character_id": ALICE, "orders": [
            order(1, 34, 60003760, true, 5.0),
            order(2, 35, 60003760, true, 9.0),
            order(3, 34, 60003760, false, 5.0),
            order(4, 35, 60003760, false, 9.0),
        ]}),
    )
    .await;
    app.state.poller.poll_character(ALICE).await;

    let html = get(&app, &browser, "/orders?bsort=price&bdir=desc").await;

    assert!(
        position(&html, 2) < position(&html, 1),
        "buy table by price desc"
    );
    // Pyerite sorts before Tritanium.
    assert!(
        position(&html, 4) < position(&html, 3),
        "sell table keeps item asc"
    );
}

#[sqlx::test]
async fn unknown_sort_values_never_reach_sql_and_fall_back_to_the_default(pool: PgPool) {
    let (app, browser) = with_polled_fixture(pool).await;

    for query in [
        "sort=price;DROP TABLE current_orders&dir=desc",
        "sort=o.price&dir=sideways",
        "sort=&bsort='",
        "sort=item%00&dir=%27",
    ] {
        let html = get(&app, &browser, &format!("/orders?{query}")).await;
        assert!(position(&html, 5002) < position(&html, 5001), "{query}");
    }
}

#[sqlx::test]
async fn the_tables_fragment_has_no_page_around_it_and_follows_the_query(pool: PgPool) {
    let (app, browser) = with_polled_fixture(pool).await;

    let html = get(&app, &browser, "/orders/tables?sort=price&dir=desc").await;

    assert!(!html.contains("<html") && !html.contains("<nav"));
    assert!(html.contains(r#"id="sell-orders""#) && html.contains(r#"id="buy-orders""#));
    assert!(position(&html, 5001) < position(&html, 5002));
}

#[sqlx::test]
async fn a_foreign_character_id_in_the_url_shows_the_own_characters(pool: PgPool) {
    let app = app(pool).await;
    let alice = signed_in(&app, ALICE).await;
    let _carol = signed_in(&app, CAROL).await;
    app.state.poller.poll_character(ALICE).await;
    app.stub_control(
        "orders",
        json!({"character_id": CAROL, "orders": [order(7001, 34, 60003760, false, 1.0)]}),
    )
    .await;
    app.state.poller.poll_character(CAROL).await;

    let html = get(&app, &alice, &format!("/orders?chars={CAROL}")).await;

    assert!(html.contains(r#"id="order-5001""#));
    assert!(!html.contains(r#"id="order-7001""#));
    assert!(!html.contains("Carol Charlie"));
}

/// The labels of a table's header cells, without the sort arrows.
fn header_labels(table: &str) -> Vec<String> {
    table
        .split("<th scope")
        .skip(1)
        .map(|th| {
            let link = th.split("</a>").next().unwrap();
            let text = link.rsplit('>').next().unwrap();
            text.trim_end_matches([' ', '\u{25B2}', '\u{25BC}'])
                .to_owned()
        })
        .collect()
}

/// The text of a row's cells.
fn cell_texts(row: &str) -> Vec<String> {
    row.split("<td")
        .skip(1)
        .map(|td| {
            let inner = td.split("</td>").next().unwrap();
            let text = inner.split_once('>').map_or("", |(_, rest)| rest);
            // The text without any markup inside the cell.
            text.split('<')
                .map(|part| part.split_once('>').map_or(part, |(_, after)| after))
                .collect::<String>()
                .trim()
                .to_owned()
        })
        .collect()
}

#[sqlx::test]
async fn location_and_region_come_last_in_both_tables(pool: PgPool) {
    let (app, browser) = with_polled_fixture(pool).await;

    let html = get(&app, &browser, "/orders").await;

    for (table, amount) in [(sell_table(&html), "Value"), (buy_table(&html), "Escrow")] {
        let labels = header_labels(table);
        assert_eq!(
            labels[labels.len() - 3..],
            ["Time left", "Location", "Region"],
            "{labels:?}"
        );
        assert!(labels.contains(&amount.to_owned()), "{labels:?}");
    }
    let sell = cell_texts(row(&html, 5001));
    assert_eq!(
        sell[sell.len() - 2],
        "Jita IV - Moon 4 - Caldari Navy Assembly Plant"
    );
    assert_eq!(sell[sell.len() - 1], "The Forge");
    let buy = cell_texts(row(&html, 5003));
    assert_eq!(buy[buy.len() - 2], "Alpha Keepstar");
    assert_eq!(buy[buy.len() - 1], "The Forge");
    assert_eq!(sell.len(), header_labels(sell_table(&html)).len());
}

// ---- standing and gap ----

const STATION: i64 = 60003760;
const STRUCTURE: i64 = 1000000000001;

fn public_sell(id: i64, type_id: i64, price: f64) -> Value {
    json!({
        "order_id": id, "type_id": type_id, "location_id": STATION, "system_id": 30000142,
        "is_buy_order": false, "price": price, "range": "station",
        "duration": 90, "issued": "2026-10-11T12:00:00Z", "min_volume": 1,
        "volume_remain": 3, "volume_total": 5,
    })
}

/// Puts the market of The Forge in the stub and refreshes every pair the orders use.
async fn with_market(app: &TestApp, market: Vec<Value>) {
    app.stub_control(
        "market-orders",
        json!({"region_id": 10000002, "orders": market}),
    )
    .await;
    app.state.market.tick().await;
    let pairs: Vec<(i64, i64)> = sqlx::query_as("SELECT region_id, type_id FROM market_sync")
        .fetch_all(&app.state.pool)
        .await
        .unwrap();
    for (region, type_id) in pairs {
        app.state.market.refresh_pair(region, type_id).await;
    }
}

async fn alice_sells(pool: PgPool, orders: Vec<Value>) -> (TestApp, Browser) {
    let app = app(pool).await;
    let browser = signed_in(&app, ALICE).await;
    app.stub_control("orders", json!({"character_id": ALICE, "orders": orders}))
        .await;
    app.state.poller.poll_character(ALICE).await;
    (app, browser)
}

#[sqlx::test]
async fn a_top_order_reads_top_in_the_success_style_and_has_no_gap(pool: PgPool) {
    let (app, browser) = alice_sells(pool, vec![order(1, 34, STATION, false, 1_500_000.0)]).await;
    with_market(
        &app,
        vec![
            public_sell(1, 34, 1_500_000.0),
            public_sell(2, 34, 2_000_000.0),
        ],
    )
    .await;

    let html = get(&app, &browser, "/orders").await;
    let r = row(&html, 1);

    assert!(r.contains(r#"<mark class="badge top">Top</mark>"#), "{r}");
    let cells = cell_texts(r);
    assert_eq!(cells[3], "Top");
    assert_eq!(cells[4], "", "no gap for a top order");
}

#[sqlx::test]
async fn an_undercut_order_shows_the_count_and_the_gap_with_the_exact_value_on_hover(pool: PgPool) {
    let (app, browser) = alice_sells(pool, vec![order(1, 34, STATION, false, 1_012_500.0)]).await;
    with_market(
        &app,
        vec![
            public_sell(1, 34, 1_012_500.0),
            public_sell(2, 34, 1_000_000.0),
            public_sell(3, 34, 1_005_000.0),
            public_sell(4, 34, 1_010_000.0),
        ],
    )
    .await;

    let html = get(&app, &browser, "/orders").await;
    let r = row(&html, 1);

    assert!(
        r.contains(r#"<mark class="badge undercut">Undercut 3</mark>"#),
        "{r}"
    );
    assert!(r.contains(r#"title="12,500.00 ISK">12.5k (1.3%)<"#), "{r}");
}

#[sqlx::test]
async fn an_order_without_market_data_shows_a_dash_for_standing_and_gap(pool: PgPool) {
    let (app, browser) = with_polled_fixture(pool).await;

    let html = get(&app, &browser, "/orders").await;
    let cells = cell_texts(row(&html, 5001));

    assert_eq!(cells[3], "\u{2014}");
    assert_eq!(cells[4], "\u{2014}");
    assert!(!row(&html, 5001).contains("badge top"));
}

#[sqlx::test]
async fn the_standing_and_gap_columns_follow_the_price_in_both_tables(pool: PgPool) {
    let (app, browser) = with_polled_fixture(pool).await;

    let html = get(&app, &browser, "/orders").await;

    // The margin follows the value in the sell table; the buy table has none.
    for (table, amount_columns) in [
        (sell_table(&html), vec!["Value", "Margin"]),
        (buy_table(&html), vec!["Escrow"]),
    ] {
        let mut expected = vec![
            "Character",
            "Item",
            "Price",
            "Standing",
            "Behind by",
            "Undercut volume",
            "Volume",
        ];
        expected.extend(amount_columns);
        expected.extend(["Issued", "Time left", "Location", "Region"]);
        assert_eq!(header_labels(table), expected);
    }
}

// ---- undercut volume ----

fn with_remaining(mut order: Value, units: i64) -> Value {
    order["volume_remain"] = json!(units);
    order
}

#[sqlx::test]
async fn the_undercut_volume_is_a_whole_number_with_separators_and_empty_for_a_top_order(
    pool: PgPool,
) {
    let (app, browser) = alice_sells(
        pool,
        vec![
            order(1, 34, STATION, false, 1_012_500.0),
            order(2, 35, STATION, false, 100.0),
        ],
    )
    .await;
    with_market(
        &app,
        vec![
            public_sell(1, 34, 1_012_500.0),
            with_remaining(public_sell(2, 34, 1_000_000.0), 500),
            with_remaining(public_sell(3, 34, 1_005_000.0), 500),
            with_remaining(public_sell(4, 34, 1_010_000.0), 250),
            public_sell(5, 35, 100.0),
            with_remaining(public_sell(6, 35, 200.0), 40),
        ],
    )
    .await;

    let html = get(&app, &browser, "/orders").await;

    assert_eq!(cell_texts(row(&html, 1))[5], "1,250");
    assert_eq!(cell_texts(row(&html, 2))[5], "", "a top order has none");
}

#[sqlx::test]
async fn a_buy_orders_undercut_volume_sums_the_reaching_orders(pool: PgPool) {
    let (app, browser) = alice_sells(pool, vec![order(1, 34, STATION, true, 50.0)]).await;
    let buy = |id: i64, price: f64, range: &str, remaining: i64| {
        let mut o = with_remaining(public_sell(id, 34, price), remaining);
        o["is_buy_order"] = json!(true);
        o["range"] = json!(range);
        o
    };
    with_market(
        &app,
        vec![
            buy(1, 50.0, "station", 9),
            buy(2, 55.0, "region", 300),
            buy(3, 60.0, "region", 1000),
        ],
    )
    .await;

    let html = get(&app, &browser, "/orders").await;

    assert_eq!(cell_texts(row(buy_table(&html), 1))[5], "1,300");
}

#[sqlx::test]
async fn an_order_without_market_data_shows_a_dash_for_the_undercut_volume(pool: PgPool) {
    let (app, browser) = with_polled_fixture(pool).await;

    let html = get(&app, &browser, "/orders").await;

    assert_eq!(cell_texts(row(&html, 5001))[5], "\u{2014}");
}

#[sqlx::test]
async fn sorting_by_undercut_volume_counts_a_top_order_as_zero_and_puts_no_data_last(pool: PgPool) {
    let (app, browser) = alice_sells(
        pool,
        vec![
            order(1, 34, STATION, false, 100.0),
            order(2, 35, STATION, false, 100.0),
            order(3, 36, STATION, false, 100.0),
            order(4, 37, STRUCTURE, false, 100.0),
        ],
    )
    .await;
    with_market(
        &app,
        vec![
            public_sell(10, 34, 90.0),
            with_remaining(public_sell(20, 35, 90.0), 300),
            public_sell(30, 36, 100.0),
        ],
    )
    .await;
    // Item 34: 3 in front (the fixture's default), 35: 300, 36: top, 37: no data.

    let asc = get(&app, &browser, "/orders?sort=undercut-volume&dir=asc").await;
    assert_eq!(order_of_rows(&asc, &[1, 2, 3, 4]), [3, 1, 2, 4]);

    let desc = get(&app, &browser, "/orders?sort=undercut-volume&dir=desc").await;
    assert_eq!(order_of_rows(&desc, &[1, 2, 3, 4]), [2, 1, 3, 4]);
}

// ---- sorting by standing and gap ----

/// Five sell orders: ids 1 to 5 are top, undercut 3 times, 12 times, once, and without market
/// data (in a structure).
async fn standing_mix(pool: PgPool) -> (TestApp, Browser) {
    let (app, browser) = alice_sells(
        pool,
        vec![
            order(1, 34, STATION, false, 100.0),
            order(2, 35, STATION, false, 100.0),
            order(3, 36, STATION, false, 100.0),
            order(4, 37, STATION, false, 100.0),
            order(5, 38, STRUCTURE, false, 100.0),
        ],
    )
    .await;
    let mut market = vec![public_sell(100, 34, 120.0)];
    for (item, count) in [(35, 3), (36, 12), (37, 1)] {
        for n in 0..count {
            market.push(public_sell(1000 * item + n, item, 99.0 - n as f64 * 0.01));
        }
    }
    with_market(&app, market).await;
    (app, browser)
}

fn order_of_rows(html: &str, ids: &[i64]) -> Vec<i64> {
    let mut found: Vec<(usize, i64)> = ids.iter().map(|id| (position(html, *id), *id)).collect();
    found.sort_unstable();
    found.into_iter().map(|(_, id)| id).collect()
}

#[sqlx::test]
async fn sorting_by_standing_reads_the_number_and_puts_no_data_last(pool: PgPool) {
    let (app, browser) = standing_mix(pool).await;

    let asc = get(&app, &browser, "/orders?sort=standing&dir=asc").await;
    assert_eq!(order_of_rows(&asc, &[1, 2, 3, 4, 5]), [1, 4, 2, 3, 5]);

    let desc = get(&app, &browser, "/orders?sort=standing&dir=desc").await;
    assert_eq!(order_of_rows(&desc, &[1, 2, 3, 4, 5]), [3, 2, 4, 1, 5]);
}

#[sqlx::test]
async fn the_standing_header_links_to_its_own_sorting_and_reverses_it(pool: PgPool) {
    let (app, browser) = standing_mix(pool).await;

    let default = unescaped(&get(&app, &browser, "/orders").await);
    assert!(default.contains("sort=standing&dir=asc"), "{default}");
    let sorted = unescaped(&get(&app, &browser, "/orders?sort=standing&dir=asc").await);
    assert!(sorted.contains("sort=standing&dir=desc"));
    assert!(sorted.contains(r#"aria-sort="ascending""#));
}

#[sqlx::test]
async fn sorting_by_gap_counts_a_top_order_as_no_difference_and_puts_no_data_last(pool: PgPool) {
    let (app, browser) = alice_sells(
        pool,
        vec![
            order(1, 34, STATION, false, 100.0),
            order(2, 35, STATION, false, 105.0),
            order(3, 36, STATION, false, 180.0),
            order(4, 37, STRUCTURE, false, 100.0),
        ],
    )
    .await;
    with_market(
        &app,
        vec![
            public_sell(10, 34, 120.0),
            public_sell(20, 35, 100.0),
            public_sell(30, 36, 100.0),
        ],
    )
    .await;

    let asc = get(&app, &browser, "/orders?sort=gap&dir=asc").await;
    assert_eq!(order_of_rows(&asc, &[1, 2, 3, 4]), [1, 2, 3, 4]);

    let desc = get(&app, &browser, "/orders?sort=gap&dir=desc").await;
    assert_eq!(order_of_rows(&desc, &[1, 2, 3, 4]), [3, 2, 1, 4]);
}

#[sqlx::test]
async fn an_unknown_sort_name_still_falls_back_to_the_default(pool: PgPool) {
    let (app, browser) = standing_mix(pool).await;

    let html = get(&app, &browser, "/orders?sort=standing%3B%20DROP&dir=desc").await;

    // The default is the item name, ascending: the unnamed items sort by their number.
    assert_eq!(order_of_rows(&html, &[1, 2, 3, 4, 5]).len(), 5);
    assert!(unescaped(&html).contains("sort=standing&dir=asc"));
}

// ---- margin ----

/// Alice has fee data for Jita 4-4 (Accounting 5, Broker Relations 5, faction standing 2.0 and
/// corporation standing 1.5: a sales tax of 3.375% and a broker fee of 1.41%).
async fn alice_with_fees(app: &TestApp) {
    for statement in [
        "INSERT INTO fee_sync (character_id, kind, next_poll_at, last_success_at) \
         VALUES (90000001, 'skills', now(), now()), (90000001, 'standings', now(), now())",
        "INSERT INTO character_skills (character_id, skill_id, active_level) \
         VALUES (90000001, 16622, 5), (90000001, 3446, 5)",
        "INSERT INTO character_standings (character_id, from_id, from_type, standing) \
         VALUES (90000001, 500001, 'faction', 2.0), (90000001, 1000035, 'npc_corp', 1.5)",
        "INSERT INTO station_owners (station_id, owner_id, resolved_at) \
         VALUES (60003760, 1000035, now())",
    ] {
        sqlx::query(statement)
            .execute(&app.state.pool)
            .await
            .unwrap();
    }
}

/// Alice bought `quantity` of `type_id` at 100.00 before her orders were issued.
async fn alice_bought(app: &TestApp, bought: &[(i64, i64, i64)]) {
    let transactions: Vec<Value> = bought
        .iter()
        .map(|(id, type_id, quantity)| {
            json!({
                "transaction_id": id, "date": "2026-10-01T12:00:00Z", "type_id": type_id,
                "location_id": STATION, "quantity": quantity, "unit_price": 100.0,
                "is_buy": true, "is_personal": true,
            })
        })
        .collect();
    app.stub_control(
        "transactions",
        json!({"character_id": ALICE, "transactions": transactions}),
    )
    .await;
    app.state
        .wealth
        .poll(ALICE, sekondo::wealth_poller::Kind::Transactions)
        .await;
}

/// Sell orders 1 (earns 171.29), 2 (loses 19.14) and 3 (cost known for 3 of 8 units), all
/// 4 remaining of 8.
async fn margin_mix(pool: PgPool) -> (TestApp, Browser) {
    let (app, browser) = alice_sells(
        pool,
        vec![
            order(1, 34, STATION, false, 150.0),
            order(2, 36, STATION, false, 100.0),
            order(3, 35, STATION, false, 120.0),
        ],
    )
    .await;
    alice_with_fees(&app).await;
    alice_bought(&app, &[(1, 34, 8), (2, 36, 8), (3, 35, 3)]).await;
    (app, browser)
}

#[sqlx::test]
async fn the_sell_table_has_a_margin_column_after_the_value_and_the_buy_table_has_none(
    pool: PgPool,
) {
    let (app, browser) = with_polled_fixture(pool).await;

    let html = get(&app, &browser, "/orders").await;

    let sell = header_labels(sell_table(&html));
    let value = sell.iter().position(|l| l == "Value").unwrap();
    assert_eq!(sell[value + 1], "Margin");
    assert!(!header_labels(buy_table(&html)).contains(&"Margin".to_owned()));
    // The cells follow the headers.
    let cells = cell_texts(row(sell_table(&html), 5001));
    assert_eq!(cells.len(), sell.len());
}

#[sqlx::test]
async fn a_margin_shows_short_with_the_exact_value_and_the_cost_per_unit_on_hover(pool: PgPool) {
    let (app, browser) = margin_mix(pool).await;

    let html = get(&app, &browser, "/orders").await;
    let r = row(&html, 1);

    // 4 * 150 * 0.95215 - 4 * 100 = 171.29
    assert!(r.contains(">171.29<"), "{r}");
    assert!(r.contains("171.29 ISK"), "{r}");
    assert!(r.contains("cost per unit 100.00 ISK"), "{r}");
    assert!(!r.contains("badge loss"), "{r}");
}

#[sqlx::test]
async fn a_negative_margin_is_in_the_error_style(pool: PgPool) {
    let (app, browser) = margin_mix(pool).await;

    let html = get(&app, &browser, "/orders").await;
    let r = row(&html, 2);

    // 4 * 100 * 0.95215 - 4 * 100 = -19.14
    assert!(
        r.contains(r#"<mark class="badge loss">-19.14</mark>"#),
        "{r}"
    );
}

#[sqlx::test]
async fn a_partly_known_cost_shows_a_dash_and_how_many_units_are_known_on_hover(pool: PgPool) {
    let (app, browser) = margin_mix(pool).await;

    let html = get(&app, &browser, "/orders").await;
    let r = row(&html, 3);

    assert!(r.contains(r#"title="cost known for 3 of 8 units""#), "{r}");
    assert!(r.contains(">\u{2014}<"), "{r}");
}

#[sqlx::test]
async fn sorting_by_margin_puts_orders_without_one_last_in_both_directions(pool: PgPool) {
    let (app, browser) = margin_mix(pool).await;

    let desc = get(&app, &browser, "/orders?sort=margin&dir=desc").await;
    assert_eq!(order_of_rows(&desc, &[1, 2, 3]), [1, 2, 3]);

    let asc = get(&app, &browser, "/orders?sort=margin&dir=asc").await;
    assert_eq!(order_of_rows(&asc, &[1, 2, 3]), [2, 1, 3]);
    assert!(unescaped(&asc).contains("sort=margin&dir=desc"));
}
