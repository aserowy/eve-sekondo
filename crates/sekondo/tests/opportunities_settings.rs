mod common;

use common::{AppBuilder, Browser, TestApp};
use sekondo::opportunities::settings::Settings;

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
    // The form names the table of the page that saves; these tests read the classic one.
    let mut pairs = pairs.to_vec();
    if !pairs.iter().any(|(key, _)| *key == "table") {
        pairs.push(("table", "classic"));
    }
    let mut request = browser
        .post(&app.url("/opportunities/settings"))
        .header("origin", &app.base_url)
        .header("hx-request", "true")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(encoded(&pairs));
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

async fn stored(app: &TestApp) -> Vec<Settings> {
    let mut all = Vec::new();
    for account in app.stores.accounts.account_ids().await.unwrap() {
        all.extend(
            app.stores
                .opportunities
                .load_settings(account)
                .await
                .unwrap(),
        );
    }
    all
}

/// An item that passes the shared rules: a price of 100 to 130, calm, with a huge volume.
async fn item(app: &TestApp, type_id: i64, name: &str, volume: f64) {
    let now = time::OffsetDateTime::now_utc();
    app.stores
        .station
        .seed_item_stats(
            10000002,
            60003760,
            type_id,
            &sekondo::store::ItemStatsRecord {
                book_at: Some(now),
                best_buy: Some("100".to_owned()),
                best_sell: Some("130".to_owned()),
                buy_near_orders: Some(0),
                sell_near_orders: Some(0),
                rho: Some(0.5),
                indicators_at: Some(now),
                volume_7d: Some(volume),
                volume_30d: Some(volume),
                sigma: Some(0.02),
                returns: Some(29),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    app.stores
        .market
        .store_names(
            &[sekondo::store::NameRow {
                id: type_id,
                category: "inventory_type".to_owned(),
                name: Some(name.to_owned()),
            }],
            now,
            None,
        )
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

#[tokio::test]
async fn a_first_visit_shows_the_default_filters() {
    let app = AppBuilder::new().with_stub().spawn().await;
    let browser = signed_in(&app, ALICE).await;

    let html = browser
        .get(&app.url("/opportunities/classic"))
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

#[tokio::test]
async fn saving_stores_the_settings_per_account_and_answers_the_tables() {
    let app = AppBuilder::new().with_stub().spawn().await;
    let browser = signed_in(&app, ALICE).await;
    let token = csrf_token(&app, &browser).await;
    item(&app, 34, "Tritanium", 100_000_000.0).await;

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
    assert!(html.contains(r#"id="classic""#), "the table is answered");
    assert!(!html.contains("<html"), "a fragment");
    // Out of band: the Fees group. An accepted form marks no field invalid and says nothing.
    assert!(html.contains(r#"<form id="fee-filter""#), "{html}");
    assert!(
        !html.contains("aria-invalid") && !html.contains(r#"-error">"#),
        "{html}"
    );
    assert_eq!(
        stored(&app).await,
        [Settings {
            min_margin: 0.075,
            min_isk_per_day: 10_000_000.0,
            search: "trit".to_owned(),
            capital: 2_000_000_000.0,
        }]
    );
}

#[tokio::test]
async fn the_stored_settings_are_what_the_page_shows_after_signing_in_again() {
    let app = AppBuilder::new().with_stub().spawn().await;
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
        .get(&app.url("/opportunities/classic"))
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

#[tokio::test]
async fn a_capital_of_zero_is_rejected_with_a_message_and_the_stored_settings_stay() {
    let app = AppBuilder::new().with_stub().spawn().await;
    let browser = signed_in(&app, ALICE).await;
    let token = csrf_token(&app, &browser).await;
    assert_eq!(
        save(&app, &browser, Some(&token), &VALID).await.status(),
        200
    );
    let before = stored(&app).await;

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
    // The answer is the filter form alone, with what was typed, the rejected field marked and its
    // message right below it; the other fields stay clean. Nothing was stored.
    assert!(html.contains(r#"<form id="opportunity-filters""#), "{html}");
    assert!(
        html.contains(r##"data-invalid-target="#opportunity-filters""##),
        "{html}"
    );
    assert!(
        html.contains(r#"id="capital" type="text" inputmode="decimal" name="capital" value="0" aria-invalid="true" aria-describedby="capital-error""#),
        "{html}"
    );
    assert!(html.contains(r#"<small id="capital-error">"#), "{html}");
    assert_eq!(html.matches("aria-invalid").count(), 1, "{html}");
    assert_eq!(html.matches("<small").count(), 1, "{html}");
    assert!(html.contains(r#"name="min_margin" value="9""#), "{html}");
    assert!(!html.contains(r#"id="classic""#), "{html}");
    assert_eq!(stored(&app).await, before);
}

#[tokio::test]
async fn invalid_values_never_reach_the_database() {
    let app = AppBuilder::new().with_stub().spawn().await;
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
    assert_eq!(stored(&app).await, Vec::<Settings>::new());
}

#[tokio::test]
async fn saving_without_the_csrf_token_is_refused_and_stores_nothing() {
    let app = AppBuilder::new().with_stub().spawn().await;
    let browser = signed_in(&app, ALICE).await;

    let without = save(&app, &browser, None, &VALID).await;
    let wrong = save(&app, &browser, Some("not-the-token"), &VALID).await;

    assert_eq!(without.status(), 403);
    assert_eq!(wrong.status(), 403);
    assert_eq!(stored(&app).await, Vec::<Settings>::new());
}

#[tokio::test]
async fn an_unauthenticated_post_is_refused() {
    let app = AppBuilder::new().with_stub().spawn().await;
    let anonymous = app.browser();

    let res = save(&app, &anonymous, Some("anything"), &VALID).await;

    assert!(
        res.status() == 403 || res.status().is_redirection(),
        "{}",
        res.status()
    );
    assert_eq!(stored(&app).await, Vec::<Settings>::new());
}

#[tokio::test]
async fn one_accounts_settings_do_not_change_another_accounts() {
    let app = AppBuilder::new().with_stub().spawn().await;
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
        .get(&app.url("/opportunities/classic"))
        .await
        .text()
        .await
        .unwrap();
    assert!(html.contains(r#"name="min_margin" value="5""#), "{html}");
    assert!(html.contains(r#"name="capital" value="100000000""#));
    assert!(!html.contains("only-alice"));
    assert_eq!(stored(&app).await.len(), 1);
}

#[tokio::test]
async fn raising_the_capital_removes_an_item_that_can_no_longer_fill_in_time() {
    let app = AppBuilder::new().with_stub().spawn().await;
    let browser = signed_in(&app, ALICE).await;
    let token = csrf_token(&app, &browser).await;
    // 10 million traded a day: fine for the default capital, far too slow for 2 billion.
    item(&app, 34, "Tritanium", 10_000_000.0).await;
    let first = browser
        .get(&app.url("/opportunities/classic/content"))
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

#[tokio::test]
async fn the_name_search_narrows_every_table_without_regard_to_case() {
    let app = AppBuilder::new().with_stub().spawn().await;
    let browser = signed_in(&app, ALICE).await;
    let token = csrf_token(&app, &browser).await;
    item(&app, 34, "Tritanium", 100_000_000.0).await;
    item(&app, 35, "Pyerite", 100_000_000.0).await;

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
        .get(&app.url("/opportunities/classic/content"))
        .await
        .text()
        .await
        .unwrap();
    assert_eq!(rows(&page, "classic"), [35]);
}

#[tokio::test]
async fn the_answer_keeps_the_selection_and_the_sorting_of_the_view() {
    let app = AppBuilder::new().with_stub().spawn().await;
    let browser = signed_in(&app, ALICE).await;
    let token = csrf_token(&app, &browser).await;
    item(&app, 34, "Tritanium", 100_000_000.0).await;
    item(&app, 35, "Pyerite", 200_000_000.0).await;
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

#[tokio::test]
async fn the_live_content_answer_brings_the_fees_group_and_the_sub_menu_but_leaves_the_filter_form()
{
    let app = AppBuilder::new().with_stub().spawn().await;
    let browser = signed_in(&app, ALICE).await;
    item(&app, 34, "Tritanium", 10_000_000.0).await;

    let html = browser
        .get(&app.url("/opportunities/classic/content"))
        .await
        .text()
        .await
        .unwrap();

    assert!(html.contains(r#"<form id="fee-filter""#), "{html}");
    assert!(
        !html.contains(r#"id="opportunity-filters""#) && !html.contains("aria-invalid"),
        "{html}"
    );
    // The sub-menu's links carry every sorting, so the answer brings them out of band, without counts.
    assert!(
        html.contains(r#"<nav class="sub-menu" id="sub-menu""#)
            && html.contains(r#"hx-swap-oob="true""#)
    );
    assert!(!html.contains(r#"class="count""#), "no row counts");
    assert_eq!(rows(&html, "classic"), [34]);
    assert!(
        !html.contains(r#"<section id="trend""#),
        "one table per page"
    );
}

#[tokio::test]
async fn a_saved_filter_goes_back_to_the_first_page_in_the_address_bar() {
    let app = AppBuilder::new().with_stub().spawn().await;
    let browser = signed_in(&app, ALICE).await;
    let token = csrf_token(&app, &browser).await;
    let mut pairs = VALID.to_vec();
    pairs.extend([
        ("cs", "item"),
        ("cd", "asc"),
        ("per_page", "25"),
        ("page", "3"),
    ]);

    let res = save(&app, &browser, Some(&token), &pairs).await;

    assert_eq!(res.status(), 200);
    assert_eq!(
        res.headers()["hx-replace-url"],
        "/opportunities/classic?cs=item&cd=asc&per_page=25"
    );
    let html = res.text().await.unwrap();
    assert!(html.contains("Page 1 of 1") || !html.contains(r#"class="pagination""#));
}
