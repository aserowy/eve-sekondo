mod common;

use common::{AppBuilder, Browser, TestApp, kpi};
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const JITA: i64 = 60003760;

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

async fn name(pool: &PgPool, id: i64, name: &str) {
    sqlx::query("INSERT INTO names (id, category, name, resolved_at) VALUES ($1, 'x', $2, now())")
        .bind(id)
        .bind(name)
        .execute(pool)
        .await
        .unwrap();
}

async fn trade(
    pool: &PgPool,
    table: &str,
    (id, character): (i64, i64),
    (quantity, price): (i64, &str),
    at: OffsetDateTime,
) {
    let date = if table == "sales" {
        "sold_at"
    } else {
        "bought_at"
    };
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO {table} (transaction_id, character_id, type_id, location_id, quantity, \
         unit_price, {date}) VALUES ($1, $2, 34, {JITA}, $3, $4::numeric, $5)"
    )))
    .bind(id)
    .bind(character)
    .bind(quantity)
    .bind(price)
    .bind(at)
    .execute(pool)
    .await
    .unwrap();
}

async fn cost(pool: &PgPool, sale: i64, covered: i64, cost: &str) {
    sqlx::query(
        "INSERT INTO sale_costs (transaction_id, covered_units, cost) VALUES ($1, $2, $3::numeric)",
    )
    .bind(sale)
    .bind(covered)
    .bind(cost)
    .execute(pool)
    .await
    .unwrap();
}

/// Alice has Accounting 5 and Broker Relations 5 with standings that give 3.375% and 1.41% in
/// Jita 4-4.
async fn alice_with_fees(pool: &PgPool) {
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
        sqlx::query(statement).execute(pool).await.unwrap();
    }
}

#[sqlx::test]
async fn unauthenticated_requests_are_redirected_to_the_landing_page(pool: PgPool) {
    let app = app(pool).await;
    for path in ["/trades", "/trades/table", "/trades/freshness"] {
        let res = app.browser().get(&app.url(path)).await;
        assert!(res.status().is_redirection(), "{path}");
        assert_eq!(Browser::location(&res), "/", "{path}");
    }
}

#[sqlx::test]
async fn the_page_has_its_title_and_the_character_filter_and_the_range_in_the_filter_section(
    pool: PgPool,
) {
    let app = app(pool).await;
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

#[sqlx::test]
async fn the_default_and_an_unknown_range_are_seven_days(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    let now = OffsetDateTime::now_utc();
    trade(
        &pool,
        "sales",
        (1, ALICE),
        (10, "150"),
        now - Duration::days(3),
    )
    .await;
    trade(
        &pool,
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

#[sqlx::test]
async fn a_range_switch_keeps_the_selection_in_every_link(pool: PgPool) {
    let app = app(pool).await;
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

#[sqlx::test]
async fn foreign_characters_in_chars_are_ignored(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    let stranger = app.browser();
    stranger.sso_round_trip(&app, "/auth/login", 90000003).await;
    let now = OffsetDateTime::now_utc();
    trade(
        &pool,
        "sales",
        (1, ALICE),
        (10, "150"),
        now - Duration::hours(2),
    )
    .await;
    trade(
        &pool,
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

#[sqlx::test]
async fn a_sale_row_reads_with_its_value_and_margin(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    alice_with_fees(&pool).await;
    name(&pool, 34, "Tritanium").await;
    name(
        &pool,
        JITA,
        "Jita IV - Moon 4 - Caldari Navy Assembly Plant",
    )
    .await;
    let sold_at = OffsetDateTime::now_utc() - Duration::hours(3);
    trade(&pool, "sales", (1, ALICE), (10, "150.00"), sold_at).await;
    cost(&pool, 1, 10, "1000.00").await;

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

#[sqlx::test]
async fn a_purchase_row_has_an_empty_margin(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    let at = OffsetDateTime::now_utc() - Duration::hours(3);
    trade(&pool, "purchases", (1, ALICE), (1000, "2500000.00"), at).await;

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

#[sqlx::test]
async fn a_sale_with_a_partly_known_cost_reads_a_dash_and_says_how_much_is_known(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    alice_with_fees(&pool).await;
    let at = OffsetDateTime::now_utc() - Duration::hours(3);
    trade(&pool, "sales", (1, ALICE), (10, "150.00"), at).await;
    cost(&pool, 1, 4, "400.00").await;

    let html = get(&app, &browser, "/trades/table").await;

    assert!(
        html.contains(r#"title="cost known for 4 of 10 units""#),
        "{html}"
    );
    assert!(html.contains('\u{2014}'));
}

#[sqlx::test]
async fn a_negative_margin_is_in_the_error_style(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    alice_with_fees(&pool).await;
    let at = OffsetDateTime::now_utc() - Duration::hours(3);
    trade(&pool, "sales", (1, ALICE), (10, "90.00"), at).await;
    cost(&pool, 1, 10, "1000.00").await;

    let html = get(&app, &browser, "/trades/table").await;

    assert!(
        html.contains(r#"<mark class="badge loss">-143.07</mark>"#),
        "{html}"
    );
}

#[sqlx::test]
async fn the_kpi_cards_sum_the_range_and_count_the_sales_left_out(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    alice_with_fees(&pool).await;
    let now = OffsetDateTime::now_utc();
    trade(
        &pool,
        "purchases",
        (1, ALICE),
        (10, "100.00"),
        now - Duration::hours(9),
    )
    .await;
    trade(
        &pool,
        "sales",
        (2, ALICE),
        (10, "150.00"),
        now - Duration::hours(8),
    )
    .await;
    cost(&pool, 2, 10, "1000.00").await;
    trade(
        &pool,
        "sales",
        (3, ALICE),
        (4, "150.00"),
        now - Duration::hours(7),
    )
    .await;
    cost(&pool, 3, 0, "0.00").await;

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

#[sqlx::test]
async fn the_margin_card_has_no_note_when_every_sale_has_a_margin_and_is_red_when_negative(
    pool: PgPool,
) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    alice_with_fees(&pool).await;
    let now = OffsetDateTime::now_utc();
    trade(
        &pool,
        "sales",
        (2, ALICE),
        (10, "150.00"),
        now - Duration::hours(8),
    )
    .await;
    cost(&pool, 2, 10, "1000.00").await;

    let html = get(&app, &browser, "/trades/table").await;
    assert!(!kpi(&html, "kpi-margin").title.contains("left out"));

    trade(
        &pool,
        "sales",
        (3, ALICE),
        (10, "90.00"),
        now - Duration::hours(7),
    )
    .await;
    cost(&pool, 3, 10, "1000.00").await;
    let html = get(&app, &browser, "/trades/table").await;
    // 428.23 - 143.07
    let margin = kpi(&html, "kpi-margin");
    assert_eq!(margin.value, "285.16");

    sqlx::query("DELETE FROM sale_costs WHERE transaction_id = 2")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM sales WHERE transaction_id = 2")
        .execute(&pool)
        .await
        .unwrap();
    let html = get(&app, &browser, "/trades/table").await;
    let margin = kpi(&html, "kpi-margin");
    assert_eq!(margin.value, "-143.07");
    assert!(margin.class.starts_with("kpi error"), "{}", margin.class);
}

#[sqlx::test]
async fn with_only_a_purchase_the_margin_card_reads_zero(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    let at = OffsetDateTime::now_utc() - Duration::hours(3);
    trade(&pool, "purchases", (1, ALICE), (10, "100.00"), at).await;

    let html = get(&app, &browser, "/trades/table").await;

    assert_eq!(kpi(&html, "kpi-margin").value, "0.00");
}

// ---- pagination ----

/// Alice sold `count` times, one per minute, the newest has transaction ID `count`.
async fn many_sales(pool: &PgPool, count: i64) {
    let now = OffsetDateTime::now_utc();
    for id in 1..=count {
        trade(
            pool,
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

#[sqlx::test]
async fn the_totals_span_every_page_while_the_table_shows_fifty(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    many_sales(&pool, 80).await;

    let html = get(&app, &browser, "/trades/table").await;

    assert_eq!(transaction_ids(&html).len(), 50);
    let sold = kpi(&html, "kpi-sold");
    assert_eq!(sold.title, "8,000.00 ISK", "80 sales of 100.00 ISK");
    assert!(html.contains("Page 1 of 2"));
}

#[sqlx::test]
async fn page_three_of_120_shows_the_101st_to_the_120th_newest_trade(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    many_sales(&pool, 120).await;

    let html = get(&app, &browser, "/trades/table?page=3&range=30d").await;

    // Newest first: transaction 120 is the newest, so the third page holds 20 down to 1.
    assert_eq!(transaction_ids(&html), (1..=20).rev().collect::<Vec<_>>());
    assert!(html.contains("Page 3 of 3"));
    let first = get(&app, &browser, "/trades/table").await;
    assert_eq!(transaction_ids(&first)[0], 120);
}

#[sqlx::test]
async fn a_page_past_the_end_is_the_last_page_and_the_range_links_start_on_page_one(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    many_sales(&pool, 60).await;

    let html = get(&app, &browser, "/trades/table?page=9&per_page=25").await;

    assert!(html.contains("Page 3 of 3"));
    assert_eq!(transaction_ids(&html).len(), 10);
    let unescaped = html.replace("&#38;", "&");
    let range_link = unescaped
        .split("<a ")
        .find(|a| a.contains("range=30d"))
        .unwrap();
    assert!(range_link.contains("per_page=25"), "{range_link}");
    assert!(
        !range_link.contains("&page=") && !range_link.contains("?page="),
        "a new range starts on page 1: {range_link}"
    );
}

#[sqlx::test]
async fn the_live_refresh_url_of_the_page_carries_the_page_and_the_range(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    many_sales(&pool, 120).await;

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

async fn sync(pool: &PgPool, character: i64, last: Option<OffsetDateTime>) {
    sqlx::query(
        "INSERT INTO wealth_sync (character_id, kind, next_poll_at, last_success_at) \
         VALUES ($1, 'transactions', now(), $2)",
    )
    .bind(character)
    .bind(last)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test]
async fn the_freshness_shows_the_transactions_times_and_stale_after_two_hours(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    let now = OffsetDateTime::now_utc();
    let fresh = now - Duration::minutes(5);
    sync(&pool, ALICE, Some(fresh)).await;

    let html = get(&app, &browser, "/trades/freshness").await;
    assert!(html.contains("Alice Alpha"));
    assert!(html.contains(&sekondo::format::eve_time(fresh)));
    assert!(!html.contains("Stale"), "{html}");

    sqlx::query("UPDATE wealth_sync SET last_success_at = $1")
        .bind(now - Duration::hours(3))
        .execute(&pool)
        .await
        .unwrap();
    let html = get(&app, &browser, "/trades/freshness").await;
    assert!(html.contains("Stale"));
    assert!(!html.contains("warnings"), "no warning count: {html}");
}

#[sqlx::test]
async fn a_character_needing_relogin_is_stale_and_links_to_login_and_a_new_one_waits(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    let html = get(&app, &browser, "/trades/freshness").await;
    assert!(html.contains("Waiting for first update"));

    sync(&pool, ALICE, Some(OffsetDateTime::now_utc())).await;
    app.state.repo.mark_needs_relogin(ALICE).await.unwrap();
    let html = get(&app, &browser, "/trades/freshness").await;

    assert!(html.contains("Stale"));
    assert!(html.contains(r#"href="/auth/add""#) && html.contains("Log in again"));
}

#[sqlx::test]
async fn the_freshness_does_not_depend_on_the_range_and_covers_only_the_selection(pool: PgPool) {
    let app = app(pool.clone()).await;
    let browser = signed_in(&app).await;
    browser.sso_round_trip(&app, "/auth/add", BOB).await;

    let html = get(&app, &browser, &format!("/trades/freshness?chars={BOB}")).await;

    assert!(html.contains("Bob Bravo") && !html.contains("Alice Alpha"));
    let page = get(&app, &browser, "/trades?range=30d").await;
    assert!(page.contains(r#"hx-get="/trades/freshness""#), "{page}");
}
