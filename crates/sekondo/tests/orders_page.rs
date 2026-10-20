mod common;

use common::{AppBuilder, Browser, TestApp, kpi};
use serde_json::{Value, json};

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const CAROL: i64 = 90000003;

async fn app() -> TestApp {
    AppBuilder::new().with_stub().spawn().await
}

async fn signed_in(app: &TestApp, character: i64) -> Browser {
    let browser = app.browser();
    browser.sso_round_trip(app, "/auth/login", character).await;
    browser
}

async fn get_one(app: &TestApp, browser: &Browser, path: &str) -> String {
    let res = browser.get(&app.url(path)).await;
    assert_eq!(res.status(), 200, "{path}");
    res.text().await.unwrap()
}

/// A page by its path. `/orders` and `/orders?…` stand for both orders pages, the sell page and
/// the buy page one after the other with the same query, so that a test can look at orders of
/// either side.
async fn get(app: &TestApp, browser: &Browser, path: &str) -> String {
    match path.strip_prefix("/orders") {
        Some(query) if query.is_empty() || query.starts_with('?') => {
            let sell = get_one(app, browser, &format!("/orders/sell{query}")).await;
            let buy = get_one(app, browser, &format!("/orders/buy{query}")).await;
            sell + &buy
        }
        _ => get_one(app, browser, path).await,
    }
}

/// The part of the page between two markers.
fn between<'a>(html: &'a str, from: &str, to: &str) -> &'a str {
    let start = html
        .find(from)
        .unwrap_or_else(|| panic!("no {from} in {html}"));
    let rest = &html[start..];
    &rest[..rest.find(to).unwrap_or(rest.len())]
}

fn sell_table(html: &str) -> &str {
    between(html, r#"id="sell-orders""#, "</section>")
}

fn buy_table(html: &str) -> &str {
    between(html, r#"id="buy-orders""#, "</section>")
}

/// An order: its figures row and its details row.
fn row(html: &str, order_id: i64) -> &str {
    let marker = format!(r#"id="order-{order_id}""#);
    between(html, &marker, "</tbody>")
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
async fn with_polled_fixture() -> (TestApp, Browser) {
    let app = app().await;
    let browser = signed_in(&app, ALICE).await;
    app.state.poller.poll_character(ALICE).await;
    (app, browser)
}

#[tokio::test]
async fn sell_and_buy_orders_are_in_separate_tables() {
    let (app, browser) = with_polled_fixture().await;

    let html = get(&app, &browser, "/orders").await;

    let sell = sell_table(&html);
    assert!(sell.contains(r#"id="order-5001""#) && sell.contains(r#"id="order-5002""#));
    assert!(!sell.contains(r#"id="order-5003""#));
    let buy = buy_table(&html);
    assert!(buy.contains(r#"id="order-5003""#));
    assert!(!buy.contains(r#"id="order-5001""#));
}

#[tokio::test]
async fn a_sell_row_shows_every_column() {
    let (app, browser) = with_polled_fixture().await;

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

#[tokio::test]
async fn a_buy_row_shows_the_escrow_and_the_structure_name() {
    let (app, browser) = with_polled_fixture().await;

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

#[tokio::test]
async fn an_unresolved_name_shows_a_placeholder_and_an_inaccessible_structure_says_so() {
    let app = app().await;
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

#[tokio::test]
async fn corporation_orders_carry_a_marker() {
    let app = app().await;
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

#[tokio::test]
async fn time_left_and_the_issue_date_are_shown_in_whole_units_and_eve_time() {
    let app = app().await;
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

#[tokio::test]
async fn without_orders_both_tables_say_so() {
    let app = app().await;
    let browser = signed_in(&app, CAROL).await;
    app.state.poller.poll_character(CAROL).await;

    let html = get(&app, &browser, "/orders").await;

    assert!(sell_table(&html).contains("No sell orders"));
    assert!(buy_table(&html).contains("No buy orders"));
}

#[tokio::test]
async fn only_orders_of_the_own_account_are_shown() {
    let app = app().await;
    let alice = signed_in(&app, ALICE).await;
    let _bob = signed_in(&app, BOB).await;
    app.state.poller.poll_character(ALICE).await;
    app.state.poller.poll_character(BOB).await;

    let html = get(&app, &alice, "/orders").await;

    assert!(html.contains(r#"id="order-5001""#));
    assert!(!html.contains(r#"id="order-6001""#));
    assert!(!html.contains("Bob Bravo"));
}

#[tokio::test]
async fn an_unauthenticated_request_is_redirected_to_the_landing_page() {
    let app = app().await;
    for path in [
        "/orders/sell",
        "/orders/buy",
        "/orders/sell/content",
        "/orders/buy/content",
        "/orders/freshness",
    ] {
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

#[tokio::test]
async fn the_default_sorting_is_by_item_name_ascending() {
    let (app, browser) = with_polled_fixture().await;

    let html = get(&app, &browser, "/orders").await;

    // Pyerite before Tritanium.
    assert!(position(&html, 5002) < position(&html, 5001));
}

#[tokio::test]
async fn sorting_by_price_can_be_reversed_and_is_kept_in_the_header_links() {
    let (app, browser) = with_polled_fixture().await;

    let asc = get(&app, &browser, "/orders?sort=price&dir=asc").await;
    assert!(position(&asc, 5002) < position(&asc, 5001));
    // The header now offers the reverse order.
    assert!(unescaped(&asc).contains("sort=price&dir=desc"), "{asc}");

    let desc = get(&app, &browser, "/orders?sort=price&dir=desc").await;
    assert!(position(&desc, 5001) < position(&desc, 5002));
    assert!(unescaped(&desc).contains("sort=price&dir=asc"));
}

#[tokio::test]
async fn the_buy_table_is_sorted_independently() {
    let app = app().await;
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

#[tokio::test]
async fn unknown_sort_values_never_reach_sql_and_fall_back_to_the_default() {
    let (app, browser) = with_polled_fixture().await;

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

#[tokio::test]
async fn the_details_columns_are_no_longer_sortable_and_fall_back_to_the_default() {
    let (app, browser) = with_polled_fixture().await;

    for name in ["character", "location", "region", "issued"] {
        for dir in ["asc", "desc"] {
            let html = get(&app, &browser, &format!("/orders?sort={name}&dir={dir}")).await;
            // The default is the item name, ascending, whatever direction was asked for.
            assert!(
                position(&html, 5002) < position(&html, 5001),
                "{name} {dir}"
            );
            assert!(
                unescaped(&html).contains("sort=price&dir=asc"),
                "{name} {dir}"
            );
            assert!(
                !unescaped(&html).contains(&format!("sort={name}")),
                "{name} {dir}"
            );
        }
    }
}

#[tokio::test]
async fn the_content_fragments_have_no_page_around_them_and_follow_the_query() {
    let (app, browser) = with_polled_fixture().await;

    let sell = get_one(&app, &browser, "/orders/sell/content?sort=price&dir=desc").await;
    let buy = get_one(&app, &browser, "/orders/buy/content").await;

    for html in [&sell, &buy] {
        assert!(!html.contains("<html"));
        // The sub-menu comes out of band: its links carry both sortings.
        assert!(
            html.contains(r#"<nav class="sub-menu" id="sub-menu""#)
                && html.contains(r#"hx-swap-oob="true""#)
        );
        assert!(html.starts_with(r#"<div class="kpis">"#), "{html}");
    }
    assert!(sell.contains(r#"id="sell-orders""#) && !sell.contains(r#"id="buy-orders""#));
    assert!(buy.contains(r#"id="buy-orders""#) && !buy.contains(r#"id="sell-orders""#));
    assert!(position(&sell, 5001) < position(&sell, 5002));
}

#[tokio::test]
async fn a_foreign_character_id_in_the_url_shows_the_own_characters() {
    let app = app().await;
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

/// The text of an order's details line, without markup.
fn details_text(html: &str, order_id: i64) -> String {
    let details = between(row(html, order_id), r#"class="details""#, "</tr>");
    cell_texts(details).remove(0)
}

#[tokio::test]
async fn each_order_has_a_details_line_with_issued_character_location_and_region() {
    let (app, browser) = with_polled_fixture().await;

    let html = get(&app, &browser, "/orders").await;

    // The stub issues its orders relative to now, so the time is only checked for its shape.
    for (order, rest) in [
        (
            5001,
            " \u{b7} Alice Alpha \u{b7} Jita IV - Moon 4 - Caldari Navy Assembly Plant \u{b7} The Forge",
        ),
        (
            5003,
            " \u{b7} Alice Alpha \u{b7} Alpha Keepstar \u{b7} The Forge",
        ),
    ] {
        let text = details_text(&html, order);
        let (issued, tail) = text.split_at(16);
        let shape: String = issued
            .chars()
            .map(|c| if c.is_ascii_digit() { '9' } else { c })
            .collect();
        assert_eq!(shape, "9999-99-99 99:99", "{text}");
        assert_eq!(tail, rest);
    }
    // The details row spans every column of its table.
    for (table, order) in [(sell_table(&html), 5001), (buy_table(&html), 5003)] {
        let columns = header_labels(table).len();
        assert!(
            row(table, order).contains(&format!(r#"colspan="{columns}""#)),
            "{columns}"
        );
    }
    // Both tables put each order in its own body, figures row first.
    assert!(html.contains(r#"<tbody id="order-5001" class="order">"#));
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
    let account = app
        .state
        .repo
        .find_character(90000001)
        .await
        .unwrap()
        .unwrap()
        .account_id;
    let pairs = app.stores.market.account_pairs(account).await.unwrap();
    for (region, type_id) in pairs {
        app.state.market.refresh_pair(region, type_id).await;
    }
}

async fn alice_sells(orders: Vec<Value>) -> (TestApp, Browser) {
    let app = app().await;
    let browser = signed_in(&app, ALICE).await;
    app.stub_control("orders", json!({"character_id": ALICE, "orders": orders}))
        .await;
    app.state.poller.poll_character(ALICE).await;
    (app, browser)
}

#[tokio::test]
async fn a_top_order_reads_top_in_the_success_style_and_has_no_gap() {
    let (app, browser) = alice_sells(vec![order(1, 34, STATION, false, 1_500_000.0)]).await;
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
    assert_eq!(cells[2], "Top");
    assert_eq!(cells[3], "", "no gap for a top order");
}

#[tokio::test]
async fn an_undercut_order_shows_the_count_and_the_gap_with_the_exact_value_on_hover() {
    let (app, browser) = alice_sells(vec![order(1, 34, STATION, false, 1_012_500.0)]).await;
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

#[tokio::test]
async fn an_order_without_market_data_shows_a_dash_for_standing_and_gap() {
    let (app, browser) = with_polled_fixture().await;

    let html = get(&app, &browser, "/orders").await;
    let cells = cell_texts(row(&html, 5001));

    assert_eq!(cells[2], "\u{2014}");
    assert_eq!(cells[3], "\u{2014}");
    assert!(!row(&html, 5001).contains("badge top"));
}

#[tokio::test]
async fn the_standing_and_gap_columns_follow_the_price_in_both_tables() {
    let (app, browser) = with_polled_fixture().await;

    let html = get(&app, &browser, "/orders").await;

    // The margin follows the value in the sell table; the buy table has none.
    for (table, amount_columns) in [
        (sell_table(&html), vec!["Value", "Margin"]),
        (buy_table(&html), vec!["Escrow"]),
    ] {
        let mut expected = vec![
            "Item",
            "Price",
            "Standing",
            "Behind by",
            "Undercut volume",
            "Volume",
        ];
        expected.extend(amount_columns);
        expected.extend(["Time left"]);
        assert_eq!(header_labels(table), expected);
    }
}

// ---- undercut volume ----

fn with_remaining(mut order: Value, units: i64) -> Value {
    order["volume_remain"] = json!(units);
    order
}

#[tokio::test]
async fn the_undercut_volume_is_a_whole_number_with_separators_and_empty_for_a_top_order() {
    let (app, browser) = alice_sells(vec![
        order(1, 34, STATION, false, 1_012_500.0),
        order(2, 35, STATION, false, 100.0),
    ])
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

    assert_eq!(cell_texts(row(&html, 1))[4], "1,250");
    assert_eq!(cell_texts(row(&html, 2))[4], "", "a top order has none");
}

#[tokio::test]
async fn a_buy_orders_undercut_volume_sums_the_reaching_orders() {
    let (app, browser) = alice_sells(vec![order(1, 34, STATION, true, 50.0)]).await;
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

    assert_eq!(cell_texts(row(buy_table(&html), 1))[4], "1,300");
}

#[tokio::test]
async fn an_order_without_market_data_shows_a_dash_for_the_undercut_volume() {
    let (app, browser) = with_polled_fixture().await;

    let html = get(&app, &browser, "/orders").await;

    assert_eq!(cell_texts(row(&html, 5001))[4], "\u{2014}");
}

#[tokio::test]
async fn sorting_by_undercut_volume_counts_a_top_order_as_zero_and_puts_no_data_last() {
    let (app, browser) = alice_sells(vec![
        order(1, 34, STATION, false, 100.0),
        order(2, 35, STATION, false, 100.0),
        order(3, 36, STATION, false, 100.0),
        order(4, 37, STRUCTURE, false, 100.0),
    ])
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
async fn standing_mix() -> (TestApp, Browser) {
    let (app, browser) = alice_sells(vec![
        order(1, 34, STATION, false, 100.0),
        order(2, 35, STATION, false, 100.0),
        order(3, 36, STATION, false, 100.0),
        order(4, 37, STATION, false, 100.0),
        order(5, 38, STRUCTURE, false, 100.0),
    ])
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

#[tokio::test]
async fn sorting_by_standing_reads_the_number_and_puts_no_data_last() {
    let (app, browser) = standing_mix().await;

    let asc = get(&app, &browser, "/orders?sort=standing&dir=asc").await;
    assert_eq!(order_of_rows(&asc, &[1, 2, 3, 4, 5]), [1, 4, 2, 3, 5]);

    let desc = get(&app, &browser, "/orders?sort=standing&dir=desc").await;
    assert_eq!(order_of_rows(&desc, &[1, 2, 3, 4, 5]), [3, 2, 4, 1, 5]);
}

#[tokio::test]
async fn the_standing_header_links_to_its_own_sorting_and_reverses_it() {
    let (app, browser) = standing_mix().await;

    let default = unescaped(&get(&app, &browser, "/orders").await);
    assert!(default.contains("sort=standing&dir=asc"), "{default}");
    let sorted = unescaped(&get(&app, &browser, "/orders?sort=standing&dir=asc").await);
    assert!(sorted.contains("sort=standing&dir=desc"));
    assert!(sorted.contains(r#"aria-sort="ascending""#));
}

#[tokio::test]
async fn sorting_by_gap_counts_a_top_order_as_no_difference_and_puts_no_data_last() {
    let (app, browser) = alice_sells(vec![
        order(1, 34, STATION, false, 100.0),
        order(2, 35, STATION, false, 105.0),
        order(3, 36, STATION, false, 180.0),
        order(4, 37, STRUCTURE, false, 100.0),
    ])
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

#[tokio::test]
async fn an_unknown_sort_name_still_falls_back_to_the_default() {
    let (app, browser) = standing_mix().await;

    let html = get(&app, &browser, "/orders?sort=standing%3B%20DROP&dir=desc").await;

    // The default is the item name, ascending: the unnamed items sort by their number.
    assert_eq!(order_of_rows(&html, &[1, 2, 3, 4, 5]).len(), 5);
    assert!(unescaped(&html).contains("sort=standing&dir=asc"));
}

// ---- margin ----

/// Alice has fee data for Jita 4-4 (Accounting 5, Broker Relations 5, faction standing 2.0 and
/// corporation standing 1.5: a sales tax of 3.375% and a broker fee of 1.41%).
async fn alice_with_fees(app: &TestApp) {
    let now = app.state.clock.now();
    let fees = &app.stores.fees;
    fees.store_skills(90000001, &[16622, 3446], &[5, 5], now, now, None)
        .await
        .unwrap();
    let standing = |from_id: i64, from_type: &str, standing: &str| sekondo::store::StandingInput {
        from_id,
        from_type: from_type.to_owned(),
        standing: standing.to_owned(),
    };
    fees.store_standings(
        90000001,
        &[
            standing(500001, "faction", "2.0"),
            standing(1000035, "npc_corp", "1.5"),
        ],
        now,
        now,
        None,
    )
    .await
    .unwrap();
    fees.insert_station_owner(60003760, 1000035, now)
        .await
        .unwrap();
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
async fn margin_mix() -> (TestApp, Browser) {
    let (app, browser) = alice_sells(vec![
        order(1, 34, STATION, false, 150.0),
        order(2, 36, STATION, false, 100.0),
        order(3, 35, STATION, false, 120.0),
    ])
    .await;
    alice_with_fees(&app).await;
    alice_bought(&app, &[(1, 34, 8), (2, 36, 8), (3, 35, 3)]).await;
    (app, browser)
}

#[tokio::test]
async fn the_sell_table_has_a_margin_column_after_the_value_and_the_buy_table_has_none() {
    let (app, browser) = with_polled_fixture().await;

    let html = get(&app, &browser, "/orders").await;

    let sell = header_labels(sell_table(&html));
    let value = sell.iter().position(|l| l == "Value").unwrap();
    assert_eq!(sell[value + 1], "Margin");
    assert!(!header_labels(buy_table(&html)).contains(&"Margin".to_owned()));
    // The cells follow the headers.
    let cells = cell_texts(row(sell_table(&html), 5001));
    assert_eq!(
        cells.len(),
        sell.len() + 1,
        "the figures, then the details line"
    );
}

#[tokio::test]
async fn a_margin_shows_short_with_the_exact_value_and_the_cost_per_unit_on_hover() {
    let (app, browser) = margin_mix().await;

    let html = get(&app, &browser, "/orders").await;
    let r = row(&html, 1);

    // 4 * 150 * 0.95215 - 4 * 100 = 171.29
    assert!(r.contains(">171.29<"), "{r}");
    assert!(r.contains("171.29 ISK"), "{r}");
    assert!(r.contains("cost per unit 100.00 ISK"), "{r}");
    assert!(!r.contains("badge loss"), "{r}");
}

#[tokio::test]
async fn a_negative_margin_is_in_the_error_style() {
    let (app, browser) = margin_mix().await;

    let html = get(&app, &browser, "/orders").await;
    let r = row(&html, 2);

    // 4 * 100 * 0.95215 - 4 * 100 = -19.14
    assert!(
        r.contains(r#"<mark class="badge loss">-19.14</mark>"#),
        "{r}"
    );
}

#[tokio::test]
async fn a_partly_known_cost_shows_a_dash_and_how_many_units_are_known_on_hover() {
    let (app, browser) = margin_mix().await;

    let html = get(&app, &browser, "/orders").await;
    let r = row(&html, 3);

    assert!(r.contains(r#"title="cost known for 3 of 8 units""#), "{r}");
    assert!(r.contains(">\u{2014}<"), "{r}");
}

#[tokio::test]
async fn sorting_by_margin_puts_orders_without_one_last_in_both_directions() {
    let (app, browser) = margin_mix().await;

    let desc = get(&app, &browser, "/orders?sort=margin&dir=desc").await;
    assert_eq!(order_of_rows(&desc, &[1, 2, 3]), [1, 2, 3]);

    let asc = get(&app, &browser, "/orders?sort=margin&dir=asc").await;
    assert_eq!(order_of_rows(&asc, &[1, 2, 3]), [2, 1, 3]);
    assert!(unescaped(&asc).contains("sort=margin&dir=desc"));
}

// ---- the two pages ----

#[tokio::test]
async fn the_orders_entry_redirects_to_the_sell_page_and_keeps_the_query() {
    let (app, browser) = with_polled_fixture().await;

    let plain = browser.get(&app.url("/orders")).await;
    assert_eq!(plain.status(), 303);
    assert_eq!(Browser::location(&plain), "/orders/sell");

    let query = format!("chars={ALICE}&sort=price&dir=desc&bsort=amount&bdir=asc&page=2");
    let with_query = browser.get(&app.url(&format!("/orders?{query}"))).await;
    assert_eq!(with_query.status(), 303);
    assert_eq!(
        Browser::location(&with_query),
        format!("/orders/sell?{query}")
    );
}

#[tokio::test]
async fn an_unauthenticated_orders_entry_ends_at_the_landing_page() {
    let app = app().await;
    let browser = app.browser();

    let entry = browser.get(&app.url("/orders")).await;
    assert_eq!(Browser::location(&entry), "/orders/sell");
    let page = browser.get(&app.url("/orders/sell")).await;
    assert_eq!(Browser::location(&page), "/");
}

#[tokio::test]
async fn each_side_lists_only_its_orders() {
    let (app, browser) = with_polled_fixture().await;

    let sell = get_one(&app, &browser, "/orders/sell").await;
    let buy = get_one(&app, &browser, "/orders/buy").await;

    assert!(sell.contains(r#"id="order-5001""#) && sell.contains(r#"id="order-5002""#));
    assert!(!sell.contains(r#"id="order-5003""#) && !sell.contains(r#"id="buy-orders""#));
    assert!(buy.contains(r#"id="order-5003""#));
    assert!(!buy.contains(r#"id="order-5001""#) && !buy.contains(r#"id="sell-orders""#));
}

/// The links of the page's sub-menu: text, href and whether it is the current page.
fn sub_menu(html: &str) -> Vec<(String, String, bool)> {
    let menu = between(html, r#"<nav class="sub-menu""#, "</nav>");
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
            let text = a.split_once('>').unwrap().1.split('<').next().unwrap();
            (
                text.to_owned(),
                unescaped(href),
                a.contains(r#"aria-current="page""#),
            )
        })
        .collect()
}

#[tokio::test]
async fn the_sub_menu_has_sell_and_buy_as_text_and_marks_the_current_page() {
    let (app, browser) = with_polled_fixture().await;

    let sell = sub_menu(&get_one(&app, &browser, "/orders/sell").await);
    let buy = sub_menu(&get_one(&app, &browser, "/orders/buy").await);

    assert_eq!(
        sell,
        [
            ("Sell".to_owned(), "/orders/sell".to_owned(), true),
            ("Buy".to_owned(), "/orders/buy".to_owned(), false)
        ]
    );
    assert_eq!(
        buy,
        [
            ("Sell".to_owned(), "/orders/sell".to_owned(), false),
            ("Buy".to_owned(), "/orders/buy".to_owned(), true)
        ]
    );
}

#[tokio::test]
async fn the_sub_menu_carries_the_selection_and_both_sortings_but_no_paging() {
    let (app, browser) = with_polled_fixture().await;
    browser.sso_round_trip(&app, "/auth/add", BOB).await;

    let html = get_one(
        &app,
        &browser,
        &format!("/orders/buy?chars={ALICE}&sort=price&dir=desc&bsort=amount&bdir=desc&page=2&per_page=25"),
    )
    .await;

    let links = sub_menu(&html);
    let query = format!("?chars={ALICE}&sort=price&dir=desc&bsort=amount&bdir=desc");
    assert_eq!(links[0].1, format!("/orders/sell{query}"));
    assert_eq!(links[1].1, format!("/orders/buy{query}"));
}

#[tokio::test]
async fn sorting_the_buy_table_survives_a_round_trip_through_the_sub_menu() {
    let (app, browser) = with_polled_fixture().await;

    let buy = get_one(&app, &browser, "/orders/buy?bsort=price&bdir=desc").await;
    let sell_link = sub_menu(&buy)[0].1.clone();
    let sell = get_one(&app, &browser, &sell_link).await;
    let back = sub_menu(&sell)[1].1.clone();
    let buy_again = get_one(&app, &browser, &back).await;

    assert!(back.contains("bsort=price&bdir=desc"), "{back}");
    assert!(buy_again.contains(r#"aria-sort="descending""#));
}

// ---- KPI cards ----

#[tokio::test]
async fn the_sell_kpis_cover_every_order_with_the_net_margin_on_its_known_cost() {
    let (app, browser) = margin_mix().await;

    let html = get_one(&app, &browser, "/orders/sell").await;

    assert_eq!(kpi(&html, "kpi-sell-count").value, "3");
    let value = kpi(&html, "kpi-sell-total");
    // 4 * 150 + 4 * 100 + 4 * 120
    assert_eq!(
        (value.value.as_str(), value.title.as_str()),
        ("1.48k", "1,480.00 ISK")
    );
    let net = kpi(&html, "kpi-net-margin");
    // 171.29 - 19.14 on a known cost of 800.00 (orders 1 and 2); order 3 has no margin.
    assert_eq!(net.value, "152.15");
    assert_eq!(net.title, "152.15 ISK; 1 order left out");
    assert_eq!(net.detail.as_deref(), Some("19.02%"));
    assert!(!net.class.starts_with("kpi error"), "{}", net.class);
}

#[tokio::test]
async fn a_negative_net_margin_is_in_the_error_style() {
    let (app, browser) = alice_sells(vec![order(2, 36, STATION, false, 100.0)]).await;
    alice_with_fees(&app).await;
    alice_bought(&app, &[(2, 36, 8)]).await;

    let html = get_one(&app, &browser, "/orders/sell").await;

    let net = kpi(&html, "kpi-net-margin");
    assert_eq!(net.value, "-19.14");
    assert!(net.class.starts_with("kpi error"), "{}", net.class);
    assert_eq!(net.detail.as_deref(), Some("-4.79%"));
    assert_eq!(net.title, "-19.14 ISK");
}

#[tokio::test]
async fn without_a_margin_the_net_margin_reads_a_dash() {
    let (app, browser) = with_polled_fixture().await;

    let html = get_one(&app, &browser, "/orders/sell").await;

    let net = kpi(&html, "kpi-net-margin");
    assert_eq!(net.value, "\u{2014}");
    assert_eq!(net.detail, None);
}

#[tokio::test]
async fn the_buy_kpis_are_the_count_and_the_escrow_and_there_is_no_net_margin() {
    let (app, browser) = with_polled_fixture().await;

    let html = get_one(&app, &browser, "/orders/buy").await;

    assert_eq!(kpi(&html, "kpi-buy-count").value, "1");
    let escrow = kpi(&html, "kpi-buy-total");
    assert_eq!(
        (escrow.value.as_str(), escrow.title.as_str()),
        ("2.5m", "2,500,000.00 ISK")
    );
    assert!(!html.contains("kpi-net-margin") && !html.contains("kpi-sell-count"));
}

#[tokio::test]
async fn the_corporation_share_is_the_detail_line_of_the_value() {
    let app = app().await;
    let browser = signed_in(&app, ALICE).await;
    let mut corp = order(1, 34, STATION, false, 100.0);
    corp["is_corporation"] = json!(true);
    app.stub_control(
        "orders",
        json!({"character_id": ALICE, "orders": [corp, order(2, 35, STATION, false, 100.0)]}),
    )
    .await;
    app.state.poller.poll_character(ALICE).await;

    let html = get_one(&app, &browser, "/orders/sell").await;

    let value = kpi(&html, "kpi-sell-total");
    assert_eq!(value.value, "800.00");
    assert_eq!(value.detail.as_deref(), Some("of which 400.00 corp"));
}

#[tokio::test]
async fn the_sell_and_buy_kpis_equal_the_dashboard_totals_for_the_same_selection() {
    let app = app().await;
    let browser = signed_in(&app, ALICE).await;
    browser.sso_round_trip(&app, "/auth/add", BOB).await;
    let mut corp = order(3, 36, STATION, false, 77.0);
    corp["is_corporation"] = json!(true);
    for (character, orders) in [
        (
            ALICE,
            vec![
                order(1, 34, STATION, false, 150.0),
                corp,
                order(4, 34, STATION, true, 5.0),
            ],
        ),
        (
            BOB,
            vec![
                order(2, 35, STATION, false, 12.5),
                order(5, 35, STATION, true, 6.0),
            ],
        ),
    ] {
        app.stub_control(
            "orders",
            json!({"character_id": character, "orders": orders}),
        )
        .await;
        app.state.poller.poll_character(character).await;
    }

    for selection in ["", &format!("?chars={BOB}")] {
        let dashboard = get_one(&app, &browser, &format!("/dashboard{selection}")).await;
        let sell = get_one(&app, &browser, &format!("/orders/sell{selection}")).await;
        let buy = get_one(&app, &browser, &format!("/orders/buy{selection}")).await;

        let tile = |id: &str| {
            let tile = between(&dashboard, &format!(r#"id="{id}""#), "</article>");
            let amount = between(tile, r#"class="value""#, "</p>");
            let title = amount
                .split(r#"title=""#)
                .nth(1)
                .and_then(|t| t.split('"').next());
            let text = amount.split_once('>').unwrap().1.trim_end_matches("</p>");
            (text.to_owned(), title.map(str::to_owned))
        };
        let card = |html: &str, id: &str| {
            let k = kpi(html, id);
            (k.value, Some(k.title))
        };
        assert_eq!(
            card(&sell, "kpi-sell-total"),
            tile("tile-sell-value"),
            "{selection}"
        );
        assert_eq!(
            card(&buy, "kpi-buy-total"),
            tile("tile-escrow"),
            "{selection}"
        );
        assert_eq!(
            card(&sell, "kpi-sell-count").0,
            tile("tile-sell-count").0,
            "{selection}"
        );
        assert_eq!(
            card(&buy, "kpi-buy-count").0,
            tile("tile-buy-count").0,
            "{selection}"
        );
    }
}

// ---- pagination ----

/// Alice with `count` sell orders (ids 1..=count, price 10 + id), and one buy order.
async fn many_sells(count: i64) -> (TestApp, Browser) {
    let mut orders: Vec<Value> = (1..=count)
        .map(|id| order(id, 34, STATION, false, 10.0 + id as f64))
        .collect();
    orders.push(order(9001, 35, STATION, true, 5.0));
    alice_sells(orders).await
}

fn ids_in_page(html: &str) -> Vec<i64> {
    html.split(r#"<tbody id="order-"#)
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap().parse().unwrap())
        .collect()
}

#[tokio::test]
async fn the_second_page_shows_the_51st_to_the_70th_order_by_price() {
    let (app, browser) = many_sells(70).await;

    let first = get_one(&app, &browser, "/orders/sell?sort=price&dir=asc").await;
    let second = get_one(&app, &browser, "/orders/sell?sort=price&dir=asc&page=2").await;

    assert_eq!(ids_in_page(&first), (1..=50).collect::<Vec<_>>());
    assert_eq!(ids_in_page(&second), (51..=70).collect::<Vec<_>>());
    assert!(first.contains("Page 1 of 2") && second.contains("Page 2 of 2"));
}

#[tokio::test]
async fn the_kpis_cover_every_page() {
    let (app, browser) = many_sells(70).await;

    let second = get_one(&app, &browser, "/orders/sell?page=2").await;

    assert_eq!(kpi(&second, "kpi-sell-count").value, "70");
}

#[tokio::test]
async fn a_page_past_the_end_shows_the_last_page_and_keeps_it_in_the_refresh_url() {
    let (app, browser) = many_sells(120).await;

    let html = get_one(&app, &browser, "/orders/sell?page=9").await;

    assert!(html.contains("Page 3 of 3"), "{html}");
    assert_eq!(ids_in_page(&html).len(), 20);
    let refresh = unescaped(&html);
    assert!(
        refresh.contains(r#"hx-get="/orders/sell/content?page=3""#),
        "the refresh URL carries the page shown: {refresh}"
    );
}

#[tokio::test]
async fn the_page_size_is_taken_from_the_url_and_unknown_sizes_fall_back_to_fifty() {
    let (app, browser) = many_sells(120).await;

    let all = get_one(&app, &browser, "/orders/sell?per_page=200").await;
    assert_eq!(ids_in_page(&all).len(), 120);
    assert!(all.contains("Page 1 of 1"));
    let small = get_one(&app, &browser, "/orders/sell?per_page=25&page=5").await;
    assert_eq!(ids_in_page(&small).len(), 20);
    assert!(small.contains("Page 5 of 5"));
    let odd = get_one(&app, &browser, "/orders/sell?per_page=30").await;
    assert_eq!(ids_in_page(&odd).len(), 50);
}

#[tokio::test]
async fn sorting_goes_back_to_the_first_page_and_keeps_the_page_size() {
    let (app, browser) = many_sells(120).await;

    let html = unescaped(&get_one(&app, &browser, "/orders/sell?page=2&per_page=25").await);

    let price_link = html
        .split("<a ")
        .find(|a| a.contains("sort=price"))
        .unwrap_or_else(|| panic!("no price header in {html}"));
    assert!(price_link.contains("per_page=25"), "{price_link}");
    assert!(
        !price_link.contains("&page=") && !price_link.contains("?page="),
        "{price_link}"
    );
}

#[tokio::test]
async fn the_pagination_bar_is_not_shown_without_orders() {
    let app = app().await;
    let browser = signed_in(&app, CAROL).await;

    let html = get_one(&app, &browser, "/orders/sell").await;

    assert!(html.contains("No sell orders."));
    assert!(!html.contains(r#"class="pagination""#));
}

#[tokio::test]
async fn the_next_link_goes_through_the_content_fragment_with_the_page_url_for_the_address_bar() {
    let (app, browser) = many_sells(120).await;

    let html = unescaped(&get_one(&app, &browser, "/orders/sell?sort=price&dir=desc").await);

    let next = html
        .split("<a ")
        .find(|a| a.contains(r#"rel="next""#))
        .unwrap();
    assert!(
        next.contains(r#"href="/orders/sell?sort=price&dir=desc&page=2""#),
        "{next}"
    );
    assert!(
        next.contains(r#"hx-get="/orders/sell/content?sort=price&dir=desc&page=2""#),
        "{next}"
    );
    assert!(
        next.contains(r#"hx-push-url="/orders/sell?sort=price&dir=desc&page=2""#),
        "{next}"
    );
}

#[tokio::test]
async fn the_buy_page_has_its_own_paging() {
    let (app, browser) = many_sells(70).await;

    let sell = get_one(&app, &browser, "/orders/sell?page=2").await;
    let buy = get_one(&app, &browser, "/orders/buy?page=2").await;

    assert!(sell.contains("Page 2 of 2"));
    assert!(buy.contains("Page 1 of 1"), "one buy order only: {buy}");
    assert_eq!(ids_in_page(&buy), [9001]);
}
