mod common;

use common::{AppBuilder, Browser, TestApp};
use sqlx::PgPool;

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;

async fn signed_in(app: &TestApp, character: i64) -> Browser {
    let browser = app.browser();
    browser.sso_round_trip(app, "/auth/login", character).await;
    browser
}

async fn csrf_token(app: &TestApp, browser: &Browser) -> String {
    let html = browser
        .get(&app.url("/characters"))
        .await
        .text()
        .await
        .unwrap();
    html.split(r#"<meta name="csrf-token" content=""#)
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .to_owned()
}

fn encoded(pairs: &[(&str, &str)]) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in pairs {
        serializer.append_pair(key, value);
    }
    serializer.finish()
}

/// Posts the filter form like htmx does: with the CSRF token in a header.
async fn save(
    app: &TestApp,
    browser: &Browser,
    token: Option<&str>,
    pairs: &[(&str, &str)],
) -> reqwest::Response {
    let mut request = browser
        .post(&app.url("/opportunities/settings"))
        .header("origin", &app.base_url)
        .header("hx-request", "true")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(encoded(pairs));
    if let Some(token) = token {
        request = request.header("x-csrf-token", token);
    }
    browser.send(request).await
}

const VALID: [(&str, &str); 4] = [
    ("min_margin", "5"),
    ("min_isk_per_day", "5000000"),
    ("search", ""),
    ("capital", "100000000"),
];

async fn stored(pool: &PgPool) -> Vec<(String, String, String, String)> {
    sqlx::query_as(
        "SELECT min_margin::text, min_isk_per_day::text, search, capital::text
         FROM opportunity_settings ORDER BY account_id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

/// An item that passes the shared rules: a price of 100 to 130, calm, with a huge volume.
async fn item(pool: &PgPool, type_id: i64, name: &str, volume: f64) {
    sqlx::query(
        "INSERT INTO station_item_stats
             (region_id, station_id, type_id, book_at, best_buy, best_sell, buy_near_orders,
              sell_near_orders, rho, indicators_at, volume_7d, volume_30d, sigma, returns)
         VALUES (10000002, 60003760, $1, now(), 100, 130, 0, 0, 0.5, now(), $2, $2, 0.02, 29)",
    )
    .bind(type_id)
    .bind(volume)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO names (id, category, name, resolved_at) VALUES ($1, 'inventory_type', $2, now())")
        .bind(type_id)
        .bind(name)
        .execute(pool)
        .await
        .unwrap();
}

fn rows(html: &str, table: &str) -> Vec<i64> {
    let start = html
        .find(&format!(r#"<section id="{table}""#))
        .unwrap_or_else(|| panic!("no {table} section in {html}"));
    let rest = &html[start..];
    let section = &rest[..rest.find("</section>").unwrap_or(rest.len())];
    section
        .split(r#"data-type-id=""#)
        .skip(1)
        .map(|part| part.split('"').next().unwrap().parse().unwrap())
        .collect()
}

#[sqlx::test]
async fn a_first_visit_shows_the_default_filters(pool: PgPool) {
    let app = AppBuilder::new(pool).with_stub().spawn().await;
    let browser = signed_in(&app, ALICE).await;

    let html = browser
        .get(&app.url("/opportunities"))
        .await
        .text()
        .await
        .unwrap();

    let form = html
        .split_once(r#"id="opportunity-filters""#)
        .expect("a filter bar")
        .1;
    for (name, value) in [
        ("min_margin", "5"),
        ("min_isk_per_day", "5000000"),
        ("search", ""),
        ("capital", "100000000"),
    ] {
        assert!(
            form.contains(&format!(r#"name="{name}" value="{value}""#)),
            "{name}={value} missing in {form}"
        );
    }
}

#[sqlx::test]
async fn saving_stores_the_settings_per_account_and_answers_the_tables(pool: PgPool) {
    let app = AppBuilder::new(pool.clone()).with_stub().spawn().await;
    let browser = signed_in(&app, ALICE).await;
    let token = csrf_token(&app, &browser).await;
    item(&pool, 34, "Tritanium", 100_000_000.0).await;

    let res = save(
        &app,
        &browser,
        Some(&token),
        &[
            ("min_margin", "7.5"),
            ("min_isk_per_day", "10,000,000"),
            ("search", " trit "),
            ("capital", "2000000000"),
        ],
    )
    .await;

    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();
    assert!(html.contains(r#"id="classic""#), "the tables are answered");
    assert!(!html.contains("<html"), "a fragment");
    // Out of band: the sub-menu with the new counts, the context text, and the cleared error text.
    assert!(html.contains(r#"id="section-nav""#), "{html}");
    assert!(html.contains(r#"id="filter-context""#), "{html}");
    assert!(
        html.contains(r#"<span id="filter-error" class="filter-error" role="alert" hx-swap-oob="true"></span>"#),
        "{html}"
    );
    assert_eq!(
        stored(&pool).await,
        [(
            "0.075".to_owned(),
            "10000000".to_owned(),
            "trit".to_owned(),
            "2000000000".to_owned()
        )]
    );
}

#[sqlx::test]
async fn the_stored_settings_are_what_the_page_shows_after_signing_in_again(pool: PgPool) {
    let app = AppBuilder::new(pool).with_stub().spawn().await;
    let browser = signed_in(&app, ALICE).await;
    let token = csrf_token(&app, &browser).await;
    save(
        &app,
        &browser,
        Some(&token),
        &[
            ("min_margin", "7.5"),
            ("min_isk_per_day", "1000"),
            ("search", "pyer"),
            ("capital", "2000000000"),
        ],
    )
    .await;

    let again = signed_in(&app, ALICE).await;
    let html = again
        .get(&app.url("/opportunities"))
        .await
        .text()
        .await
        .unwrap();

    for (name, value) in [
        ("min_margin", "7.5"),
        ("min_isk_per_day", "1000"),
        ("search", "pyer"),
        ("capital", "2000000000"),
    ] {
        assert!(
            html.contains(&format!(r#"name="{name}" value="{value}""#)),
            "{name}"
        );
    }
}

#[sqlx::test]
async fn a_capital_of_zero_is_rejected_with_a_message_and_the_stored_settings_stay(pool: PgPool) {
    let app = AppBuilder::new(pool.clone()).with_stub().spawn().await;
    let browser = signed_in(&app, ALICE).await;
    let token = csrf_token(&app, &browser).await;
    assert_eq!(
        save(&app, &browser, Some(&token), &VALID).await.status(),
        200
    );
    let before = stored(&pool).await;

    let res = save(
        &app,
        &browser,
        Some(&token),
        &[
            ("min_margin", "9"),
            ("min_isk_per_day", "1"),
            ("search", "x"),
            ("capital", "0"),
        ],
    )
    .await;

    assert_eq!(res.status(), 422);
    let html = res.text().await.unwrap();
    assert!(
        html.contains("capital") && html.contains("positive"),
        "{html}"
    );
    // The answer is the error text of the filter box alone (the form keeps what was typed), and
    // nothing was stored.
    assert!(html.contains(r#"id="filter-error""#), "{html}");
    assert!(html.contains(r#"role="alert""#));
    assert!(
        !html.contains("<form") && !html.contains(r#"id="classic""#),
        "{html}"
    );
    assert_eq!(stored(&pool).await, before);
}

#[sqlx::test]
async fn invalid_values_never_reach_the_database(pool: PgPool) {
    let app = AppBuilder::new(pool.clone()).with_stub().spawn().await;
    let browser = signed_in(&app, ALICE).await;
    let token = csrf_token(&app, &browser).await;

    for (field, value) in [
        ("min_margin", "-1"),
        ("min_margin", "lots"),
        ("min_isk_per_day", "-5"),
        ("capital", "-100"),
        ("capital", "NaN"),
        ("capital", ""),
    ] {
        let mut pairs = VALID.to_vec();
        pairs.retain(|(name, _)| *name != field);
        pairs.push((field, value));
        let res = save(&app, &browser, Some(&token), &pairs).await;
        assert_eq!(res.status(), 422, "{field}={value}");
    }
    assert_eq!(stored(&pool).await, Vec::new());
}

#[sqlx::test]
async fn saving_without_the_csrf_token_is_refused_and_stores_nothing(pool: PgPool) {
    let app = AppBuilder::new(pool.clone()).with_stub().spawn().await;
    let browser = signed_in(&app, ALICE).await;

    let without = save(&app, &browser, None, &VALID).await;
    let wrong = save(&app, &browser, Some("not-the-token"), &VALID).await;

    assert_eq!(without.status(), 403);
    assert_eq!(wrong.status(), 403);
    assert_eq!(stored(&pool).await, Vec::new());
}

#[sqlx::test]
async fn an_unauthenticated_post_is_refused(pool: PgPool) {
    let app = AppBuilder::new(pool.clone()).with_stub().spawn().await;
    let anonymous = app.browser();

    let res = save(&app, &anonymous, Some("anything"), &VALID).await;

    assert!(
        res.status() == 403 || res.status().is_redirection(),
        "{}",
        res.status()
    );
    assert_eq!(stored(&pool).await, Vec::new());
}

#[sqlx::test]
async fn one_accounts_settings_do_not_change_another_accounts(pool: PgPool) {
    let app = AppBuilder::new(pool.clone()).with_stub().spawn().await;
    let alice = signed_in(&app, ALICE).await;
    let bob = signed_in(&app, BOB).await;
    let alice_token = csrf_token(&app, &alice).await;

    save(
        &app,
        &alice,
        Some(&alice_token),
        &[
            ("min_margin", "30"),
            ("min_isk_per_day", "9"),
            ("search", "only-alice"),
            ("capital", "5"),
        ],
    )
    .await;

    let html = bob
        .get(&app.url("/opportunities"))
        .await
        .text()
        .await
        .unwrap();
    assert!(html.contains(r#"name="min_margin" value="5""#), "{html}");
    assert!(html.contains(r#"name="capital" value="100000000""#));
    assert!(!html.contains("only-alice"));
    assert_eq!(stored(&pool).await.len(), 1);
}

#[sqlx::test]
async fn raising_the_capital_removes_an_item_that_can_no_longer_fill_in_time(pool: PgPool) {
    let app = AppBuilder::new(pool.clone()).with_stub().spawn().await;
    let browser = signed_in(&app, ALICE).await;
    let token = csrf_token(&app, &browser).await;
    // 10 million traded a day: fine for the default capital, far too slow for 2 billion.
    item(&pool, 34, "Tritanium", 10_000_000.0).await;
    let first = browser
        .get(&app.url("/opportunities/tables"))
        .await
        .text()
        .await
        .unwrap();
    assert_eq!(rows(&first, "classic"), [34]);

    let res = save(
        &app,
        &browser,
        Some(&token),
        &[
            ("min_margin", "5"),
            ("min_isk_per_day", "5000000"),
            ("search", ""),
            ("capital", "2000000000"),
        ],
    )
    .await;

    let html = res.text().await.unwrap();
    assert_eq!(rows(&html, "classic"), Vec::<i64>::new());
}

#[sqlx::test]
async fn the_name_search_narrows_every_table_without_regard_to_case(pool: PgPool) {
    let app = AppBuilder::new(pool.clone()).with_stub().spawn().await;
    let browser = signed_in(&app, ALICE).await;
    let token = csrf_token(&app, &browser).await;
    item(&pool, 34, "Tritanium", 100_000_000.0).await;
    item(&pool, 35, "Pyerite", 100_000_000.0).await;

    let res = save(
        &app,
        &browser,
        Some(&token),
        &[
            ("min_margin", "5"),
            ("min_isk_per_day", "5000000"),
            ("search", "PYER"),
            ("capital", "100000000"),
        ],
    )
    .await;

    let html = res.text().await.unwrap();
    assert_eq!(rows(&html, "classic"), [35]);
    // The page itself applies the stored search too.
    let page = browser
        .get(&app.url("/opportunities/tables"))
        .await
        .text()
        .await
        .unwrap();
    assert_eq!(rows(&page, "classic"), [35]);
}

#[sqlx::test]
async fn the_answer_keeps_the_selection_and_the_sorting_of_the_view(pool: PgPool) {
    let app = AppBuilder::new(pool.clone()).with_stub().spawn().await;
    let browser = signed_in(&app, ALICE).await;
    let token = csrf_token(&app, &browser).await;
    item(&pool, 34, "Tritanium", 100_000_000.0).await;
    item(&pool, 35, "Pyerite", 200_000_000.0).await;
    let mut pairs = VALID.to_vec();
    pairs.extend([("cs", "item"), ("cd", "desc")]);

    let res = save(&app, &browser, Some(&token), &pairs).await;

    let html = res.text().await.unwrap();
    assert_eq!(
        rows(&html, "classic"),
        [34, 35],
        "sorted by name, descending"
    );
}

#[sqlx::test]
async fn the_live_tables_answer_brings_the_sub_menu_and_context_but_leaves_the_error_text(
    pool: PgPool,
) {
    let app = AppBuilder::new(pool.clone()).with_stub().spawn().await;
    let browser = signed_in(&app, ALICE).await;
    item(&pool, 34, "Tritanium", 10_000_000.0).await;

    let html = browser
        .get(&app.url("/opportunities/tables"))
        .await
        .text()
        .await
        .unwrap();

    assert!(html.contains(r#"id="section-nav""#), "{html}");
    assert!(html.contains(r#"id="filter-context""#), "{html}");
    assert!(!html.contains(r#"id="filter-error""#), "{html}");
    // The sub-menu counts the rows of every table.
    let nav = html.split_once(r#"id="section-nav""#).unwrap().1;
    let classic = nav.split_once(r##"href="#classic""##).unwrap().1;
    assert!(
        classic
            .split("</a>")
            .next()
            .unwrap()
            .contains(r#"<span class="count">1</span>"#),
        "{classic}"
    );
}
