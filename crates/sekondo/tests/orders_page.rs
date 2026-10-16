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
