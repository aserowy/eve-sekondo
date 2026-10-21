mod common;

use common::{AppBuilder, Browser, TestApp, kpi};
use time::{Duration, OffsetDateTime};

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const JITA: i64 = 60003760;

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

/// ISK as written in the figures, in cents.
fn cents(isk: &str) -> i64 {
    (isk.parse::<f64>().unwrap() * 100.0).round() as i64
}

async fn trade(
    app: &TestApp,
    table: &str,
    (id, character): (i64, i64),
    (quantity, price): (i64, &str),
    at: OffsetDateTime,
) {
    app.stores
        .trades
        .seed_trade(
            character,
            &sekondo::store::TradeSeed {
                transaction_id: id,
                is_sale: table == "sales",
                type_id: 34,
                location_id: JITA,
                quantity,
                unit_price_cents: cents(price),
                at,
            },
        )
        .await
        .unwrap();
}

async fn cost(app: &TestApp, sale: i64, covered: i64, cost: &str) {
    app.stores
        .trades
        .seed_sale_cost(sale, covered, cents(cost))
        .await
        .unwrap();
}

/// Alice has Accounting 5 and Broker Relations 5 with standings that give 3.375% and 1.41% in
/// Jita 4-4.
async fn alice_with_fees(app: &TestApp) {
    let now = OffsetDateTime::now_utc();
    let fees = &app.stores.fees;
    for kind in [
        sekondo::fee_poller::Kind::Skills,
        sekondo::fee_poller::Kind::Standings,
    ] {
        fees.seed_fee_success(ALICE, kind, now).await.unwrap();
    }
    for skill in [16622, 3446] {
        fees.seed_skill(ALICE, skill, 5).await.unwrap();
    }
    for (from_id, from_type, standing) in [(500001, "faction", "2.0"), (1000035, "npc_corp", "1.5")]
    {
        fees.seed_standing(
            ALICE,
            &sekondo::store::StandingInput {
                from_id,
                from_type: from_type.to_owned(),
                standing: standing.to_owned(),
            },
        )
        .await
        .unwrap();
    }
    fees.insert_station_owner(JITA, 1000035, now).await.unwrap();
}

#[tokio::test]
async fn unauthenticated_requests_are_redirected_to_the_landing_page() {
    let app = app().await;
    for path in ["/trades", "/trades/table", "/trades/freshness"] {
        let res = app.browser().get(&app.url(path)).await;
        assert!(res.status().is_redirection(), "{path}");
        assert_eq!(Browser::location(&res), "/", "{path}");
    }
}

#[tokio::test]
async fn the_page_has_its_title_and_the_character_filter_and_the_range_in_the_filter_section() {
    let app = app().await;
    let browser = signed_in(&app).await;

    let html = get(&app, &browser, "/trades").await;

    assert!(html.contains("<h1>Trades</h1>"), "one page title");
    assert_eq!(html.matches("<h1").count(), 1);
    assert!(!html.contains("panel"), "no self-made panels");
    assert!(html.contains(r#"data-character-filter"#));
    assert!(html.contains(r#"hx-get="/trades""#));
    let filter = html.split(r#"id="filter-box""#).nth(1).unwrap();
    let filter = &filter[..filter.find(r#"id="content""#).unwrap()];
    assert!(filter.contains("Alice Alpha"), "{filter}");
    assert!(filter.contains(r#"id="trades-range-control""#), "{filter}");
    assert!(html.contains("No trades in this range"), "empty state");
    assert!(
        !html.contains(r#"class="pagination""#),
        "nothing to page through"
    );
    assert!(
        !html.contains("<nav class=\"sub-menu\""),
        "no sub-menu on the trades page"
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
    assert_eq!(current, ["Trades"]);
}

#[tokio::test]
async fn the_default_and_an_unknown_range_are_seven_days() {
    let app = app().await;
    let browser = signed_in(&app).await;
    let now = OffsetDateTime::now_utc();
    trade(
        &app,
        "sales",
        (1, ALICE),
        (10, "150"),
        now - Duration::days(3),
    )
    .await;
    trade(
        &app,
        "sales",
        (2, ALICE),
        (10, "150"),
        now - Duration::days(10),
    )
    .await;

    for path in ["/trades/table", "/trades/table?range=1y"] {
        let html = get(&app, &browser, path).await;
        assert!(html.contains(r#"data-transaction="1""#), "{path}");
        assert!(!html.contains(r#"data-transaction="2""#), "{path}");
        assert!(html.contains(r#"href="/trades?range=30d""#), "{path}");
        assert!(
            html.contains(r#"aria-current="true">7 days"#),
            "{path}: {html}"
        );
    }

    let month = get(&app, &browser, "/trades/table?range=30d").await;
    assert!(month.contains(r#"data-transaction="2""#));
}

#[tokio::test]
async fn a_range_switch_keeps_the_selection_in_every_link() {
    let app = app().await;
    let browser = signed_in(&app).await;
    browser.sso_round_trip(&app, "/auth/add", BOB).await;

    let html = get(&app, &browser, &format!("/trades/table?chars={BOB}")).await;

    assert!(
        html.contains(&format!(r#"href="/trades?chars={BOB}&#38;range=30d""#)),
        "{html}"
    );
    assert!(html.contains(&format!(
        r#"hx-get="/trades/table?chars={BOB}&#38;range=30d""#
    )));
}

#[tokio::test]
async fn foreign_characters_in_chars_are_ignored() {
    let app = app().await;
    let browser = signed_in(&app).await;
    let stranger = app.browser();
    stranger.sso_round_trip(&app, "/auth/login", 90000003).await;
    let now = OffsetDateTime::now_utc();
    trade(
        &app,
        "sales",
        (1, ALICE),
        (10, "150"),
        now - Duration::hours(2),
    )
    .await;
    trade(
        &app,
        "sales",
        (2, 90000003),
        (10, "150"),
        now - Duration::hours(2),
    )
    .await;

    let html = get(&app, &browser, "/trades/table?chars=90000003").await;

    assert!(html.contains(r#"data-transaction="1""#));
    assert!(!html.contains(r#"data-transaction="2""#));
}

#[tokio::test]
async fn a_sale_row_reads_with_its_value_and_margin() {
    let app = app().await;
    let browser = signed_in(&app).await;
    alice_with_fees(&app).await;
    name(&app, 34, "Tritanium").await;
    name(&app, JITA, "Jita IV - Moon 4 - Caldari Navy Assembly Plant").await;
    let sold_at = OffsetDateTime::now_utc() - Duration::hours(3);
    trade(&app, "sales", (1, ALICE), (10, "150.00"), sold_at).await;
    cost(&app, 1, 10, "1000.00").await;

    let html = get(&app, &browser, "/trades/table").await;

    let row = html.split(r#"data-transaction="1""#).nth(1).unwrap();
    let row = &row[..row.find("</tr>").unwrap()];
    for text in [
        &sekondo::format::eve_time(sold_at),
        "Alice Alpha",
        "Tritanium",
        "Jita IV - Moon 4 - Caldari Navy Assembly Plant",
        "Sell",
        ">10<",
        ">150.00<",
        ">1.5k<",
        ">428.23<",
    ] {
        assert!(row.contains(text), "{text}: {row}");
    }
    assert!(
        row.contains("428.23 ISK; cost per unit 100.00 ISK"),
        "{row}"
    );
}

#[tokio::test]
async fn a_purchase_row_has_an_empty_margin() {
    let app = app().await;
    let browser = signed_in(&app).await;
    let at = OffsetDateTime::now_utc() - Duration::hours(3);
    trade(&app, "purchases", (1, ALICE), (1000, "2500000.00"), at).await;

    let html = get(&app, &browser, "/trades/table").await;

    let row = html.split(r#"data-transaction="1""#).nth(1).unwrap();
    let row = &row[..row.find("</tr>").unwrap()];
    for text in ["Buy", ">1,000<", ">2.5m<", ">2.5b<"] {
        assert!(row.contains(text), "{text}: {row}");
    }
    let margin = row.split(r#"data-column="margin""#).nth(1).unwrap();
    assert!(
        margin.starts_with("></td>") || margin.starts_with(" class"),
        "{margin}"
    );
    assert!(!margin.contains('\u{2014}'), "{margin}");
}

#[tokio::test]
async fn a_sale_with_a_partly_known_cost_reads_a_dash_and_says_how_much_is_known() {
    let app = app().await;
    let browser = signed_in(&app).await;
    alice_with_fees(&app).await;
    let at = OffsetDateTime::now_utc() - Duration::hours(3);
    trade(&app, "sales", (1, ALICE), (10, "150.00"), at).await;
    cost(&app, 1, 4, "400.00").await;

    let html = get(&app, &browser, "/trades/table").await;

    assert!(
        html.contains(r#"title="cost known for 4 of 10 units""#),
        "{html}"
    );
    assert!(html.contains('\u{2014}'));
}

#[tokio::test]
async fn a_negative_margin_is_in_the_error_style() {
    let app = app().await;
    let browser = signed_in(&app).await;
    alice_with_fees(&app).await;
    let at = OffsetDateTime::now_utc() - Duration::hours(3);
    trade(&app, "sales", (1, ALICE), (10, "90.00"), at).await;
    cost(&app, 1, 10, "1000.00").await;

    let html = get(&app, &browser, "/trades/table").await;

    assert!(
        html.contains(r#"<mark class="badge loss">-143.07</mark>"#),
        "{html}"
    );
}

#[tokio::test]
async fn the_kpi_cards_sum_the_range_and_count_the_sales_left_out() {
    let app = app().await;
    let browser = signed_in(&app).await;
    alice_with_fees(&app).await;
    let now = OffsetDateTime::now_utc();
    trade(
        &app,
        "purchases",
        (1, ALICE),
        (10, "100.00"),
        now - Duration::hours(9),
    )
    .await;
    trade(
        &app,
        "sales",
        (2, ALICE),
        (10, "150.00"),
        now - Duration::hours(8),
    )
    .await;
    cost(&app, 2, 10, "1000.00").await;
    trade(
        &app,
        "sales",
        (3, ALICE),
        (4, "150.00"),
        now - Duration::hours(7),
    )
    .await;
    cost(&app, 3, 0, "0.00").await;

    let html = get(&app, &browser, "/trades/table").await;

    // Bought 1,000.00; sold 1,500.00 + 600.00; margin 428.23 and one sale without.
    let bought = kpi(&html, "kpi-bought");
    assert_eq!(
        (bought.value.as_str(), bought.title.as_str()),
        ("1k", "1,000.00 ISK")
    );
    let sold = kpi(&html, "kpi-sold");
    assert_eq!(
        (sold.value.as_str(), sold.title.as_str()),
        ("2.1k", "2,100.00 ISK")
    );
    let margin = kpi(&html, "kpi-margin");
    assert_eq!(margin.value, "428.23");
    assert_eq!(margin.title, "428.23 ISK; 1 sale left out");
    assert!(!margin.class.starts_with("kpi error"));
    assert!(!html.contains("<tfoot"), "the table has no totals row");
}

#[tokio::test]
async fn the_margin_card_has_no_note_when_every_sale_has_a_margin_and_is_red_when_negative() {
    let app = app().await;
    let browser = signed_in(&app).await;
    alice_with_fees(&app).await;
    let now = OffsetDateTime::now_utc();
    trade(
        &app,
        "sales",
        (2, ALICE),
        (10, "150.00"),
        now - Duration::hours(8),
    )
    .await;
    cost(&app, 2, 10, "1000.00").await;

    let html = get(&app, &browser, "/trades/table").await;
    assert!(!kpi(&html, "kpi-margin").title.contains("left out"));

    trade(
        &app,
        "sales",
        (3, ALICE),
        (10, "90.00"),
        now - Duration::hours(7),
    )
    .await;
    cost(&app, 3, 10, "1000.00").await;
    let html = get(&app, &browser, "/trades/table").await;
    // 428.23 - 143.07
    let margin = kpi(&html, "kpi-margin");
    assert_eq!(margin.value, "285.16");

    app.stores.trades.delete_sale(2).await.unwrap();
    let html = get(&app, &browser, "/trades/table").await;
    let margin = kpi(&html, "kpi-margin");
    assert_eq!(margin.value, "-143.07");
    assert!(margin.class.starts_with("kpi error"), "{}", margin.class);
}

#[tokio::test]
async fn with_only_a_purchase_the_margin_card_reads_zero() {
    let app = app().await;
    let browser = signed_in(&app).await;
    let at = OffsetDateTime::now_utc() - Duration::hours(3);
    trade(&app, "purchases", (1, ALICE), (10, "100.00"), at).await;

    let html = get(&app, &browser, "/trades/table").await;

    assert_eq!(kpi(&html, "kpi-margin").value, "0.00");
}

// ---- pagination ----

/// Alice sold `count` times, one per minute, the newest has transaction ID `count`.
async fn many_sales(app: &TestApp, count: i64) {
    let now = OffsetDateTime::now_utc();
    for id in 1..=count {
        trade(
            app,
            "sales",
            (id, ALICE),
            (1, "100.00"),
            now - Duration::minutes(count - id + 1),
        )
        .await;
    }
}

fn transaction_ids(html: &str) -> Vec<i64> {
    html.split(r#"data-transaction=""#)
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap().parse().unwrap())
        .collect()
}

#[tokio::test]
async fn the_totals_span_every_page_while_the_table_shows_twenty_five() {
    let app = app().await;
    let browser = signed_in(&app).await;
    many_sales(&app, 80).await;

    let html = get(&app, &browser, "/trades/table").await;

    assert_eq!(transaction_ids(&html).len(), 25);
    let sold = kpi(&html, "kpi-sold");
    assert_eq!(sold.title, "8,000.00 ISK", "80 sales of 100.00 ISK");
    assert!(html.contains("Page 1 of 4"));
}

#[tokio::test]
async fn page_three_of_120_shows_the_101st_to_the_120th_newest_trade() {
    let app = app().await;
    let browser = signed_in(&app).await;
    many_sales(&app, 120).await;

    let html = get(&app, &browser, "/trades/table?page=3&range=30d&per_page=50").await;

    // Newest first: transaction 120 is the newest, so the third page holds 20 down to 1.
    assert_eq!(transaction_ids(&html), (1..=20).rev().collect::<Vec<_>>());
    assert!(html.contains("Page 3 of 3"));
    let first = get(&app, &browser, "/trades/table").await;
    assert_eq!(transaction_ids(&first)[0], 120);
}

#[tokio::test]
async fn a_page_past_the_end_is_the_last_page_and_the_range_links_start_on_page_one() {
    let app = app().await;
    let browser = signed_in(&app).await;
    many_sales(&app, 60).await;

    let html = get(&app, &browser, "/trades/table?page=9&per_page=10").await;

    assert!(html.contains("Page 6 of 6"));
    assert_eq!(transaction_ids(&html).len(), 10);
    let unescaped = html.replace("&#38;", "&");
    let range_link = unescaped
        .split("<a ")
        .find(|a| a.contains("range=30d"))
        .unwrap();
    assert!(range_link.contains("per_page=10"), "{range_link}");
    assert!(
        !range_link.contains("&page=") && !range_link.contains("?page="),
        "a new range starts on page 1: {range_link}"
    );
}

#[tokio::test]
async fn the_live_refresh_url_of_the_page_carries_the_page_and_the_range() {
    let app = app().await;
    let browser = signed_in(&app).await;
    many_sales(&app, 120).await;

    let html = get(&app, &browser, "/trades?range=30d&page=2")
        .await
        .replace("&#38;", "&");

    assert!(
        html.contains(r#"hx-get="/trades/table?range=30d&page=2""#),
        "{html}"
    );
    assert!(html.contains(r#"data-live="/trades/table""#));
    assert!(html.contains(r#"hx-get="/trades/freshness""#), "{html}");
}

// ---- freshness ----

async fn sync(app: &TestApp, character: i64, last: Option<OffsetDateTime>) {
    let wealth = &app.stores.wealth;
    let next = OffsetDateTime::now_utc();
    match last {
        Some(at) => {
            wealth
                .store_transactions(character, at, &[], next, None)
                .await
                .unwrap();
        }
        None => wealth
            .insert_schedule(
                sekondo::wealth_poller::Kind::Transactions,
                &[character],
                &[next],
            )
            .await
            .unwrap(),
    }
}

#[tokio::test]
async fn the_freshness_shows_the_transactions_times_and_stale_after_two_hours() {
    let app = app().await;
    let browser = signed_in(&app).await;
    let now = OffsetDateTime::now_utc();
    let fresh = now - Duration::minutes(5);
    sync(&app, ALICE, Some(fresh)).await;

    let html = get(&app, &browser, "/trades/freshness").await;
    assert!(html.contains("Alice Alpha"));
    assert!(html.contains(&sekondo::format::eve_time(fresh)));
    assert!(!html.contains("Stale"), "{html}");

    sync(&app, ALICE, Some(now - Duration::hours(3))).await;
    let html = get(&app, &browser, "/trades/freshness").await;
    assert!(html.contains("Stale"));
    assert!(!html.contains("warnings"), "no warning count: {html}");
}

#[tokio::test]
async fn a_character_needing_relogin_is_stale_and_links_to_login_and_a_new_one_waits() {
    let app = app().await;
    let browser = signed_in(&app).await;
    let html = get(&app, &browser, "/trades/freshness").await;
    assert!(html.contains("Waiting for first update"));

    sync(&app, ALICE, Some(OffsetDateTime::now_utc())).await;
    app.state.repo.mark_needs_relogin(ALICE).await.unwrap();
    let html = get(&app, &browser, "/trades/freshness").await;

    assert!(html.contains("Stale"));
    assert!(html.contains(r#"href="/auth/add""#) && html.contains("Log in again"));
}

#[tokio::test]
async fn the_freshness_does_not_depend_on_the_range_and_covers_only_the_selection() {
    let app = app().await;
    let browser = signed_in(&app).await;
    browser.sso_round_trip(&app, "/auth/add", BOB).await;

    let html = get(&app, &browser, &format!("/trades/freshness?chars={BOB}")).await;

    assert!(html.contains("Bob Bravo") && !html.contains("Alice Alpha"));
    let page = get(&app, &browser, "/trades?range=30d").await;
    assert!(page.contains(r#"hx-get="/trades/freshness""#), "{page}");
}
