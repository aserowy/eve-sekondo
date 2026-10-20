mod common;

use common::{AppBuilder, Browser, TestApp};

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const JITA_44: i64 = 60003760;

/// Alice has trained skills (b 1.5%, t 3.375%); Bob's fees are not known, so he counts with the
/// base rates. Both are on one account, and one item is listed.
async fn setup() -> (TestApp, Browser) {
    let app = AppBuilder::new().with_stub().spawn().await;
    let browser = app.browser();
    browser.sso_round_trip(&app, "/auth/login", ALICE).await;
    browser.sso_round_trip(&app, "/auth/add", BOB).await;
    let now = time::OffsetDateTime::now_utc();
    let fees = &app.stores.fees;
    fees.store_skills(ALICE, &[16622, 3446], &[5, 5], now, now, None)
        .await
        .unwrap();
    fees.store_standings(ALICE, &[], now, now, None)
        .await
        .unwrap();
    fees.insert_station_owner(JITA_44, 1000035, now)
        .await
        .unwrap();
    app.stores
        .station
        .seed_item_stats(
            10000002,
            JITA_44,
            34,
            &sekondo::store::ItemStatsRecord {
                book_at: Some(now),
                best_buy: Some("100".to_owned()),
                best_sell: Some("130".to_owned()),
                buy_near_orders: Some(0),
                sell_near_orders: Some(0),
                rho: Some(0.5),
                indicators_at: Some(now),
                volume_7d: Some(100_000_000.0),
                volume_30d: Some(100_000_000.0),
                sigma: Some(0.02),
                returns: Some(29),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    (app, browser)
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

/// Posts the Fees group like htmx does: with the CSRF token in a header when there is one.
async fn choose(
    app: &TestApp,
    browser: &Browser,
    token: Option<&str>,
    pairs: &[(&str, &str)],
) -> reqwest::Response {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in pairs {
        serializer.append_pair(key, value);
    }
    let mut request = browser
        .post(&app.url("/opportunities/fee-character"))
        .header("origin", &app.base_url)
        .header("hx-request", "true")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(serializer.finish());
    if let Some(token) = token {
        request = request.header("x-csrf-token", token);
    }
    browser.send(request).await
}

async fn stored(app: &TestApp, browser: &Browser) -> Option<i64> {
    // The account of the signed-in browser: the only one with a stored choice, or none.
    let _ = browser;
    let mut found = None;
    for account in app.stores.accounts.account_ids().await.unwrap() {
        found = found.or(app
            .stores
            .opportunities
            .load_fee_character(account)
            .await
            .unwrap());
    }
    found
}

fn radio(id: i64, checked: bool) -> String {
    format!(
        r#"<input type="radio" name="fee_character" value="{id}"{}>"#,
        if checked { " checked" } else { "" }
    )
}

/// The text of a cell of the classic row of item 34.
fn net(html: &str) -> String {
    let section = &html[html
        .find(r#"<section id="classic""#)
        .expect("classic table")..];
    let row = &section[section.find(r#"data-type-id="34""#).expect("a row")..];
    let row = &row[..row.find("</tr>").unwrap()];
    let cell = &row[row.find(r#"data-col="net""#).unwrap()..];
    let text = cell
        .split_once('>')
        .unwrap()
        .1
        .split("</td>")
        .next()
        .unwrap();
    text.trim().to_owned()
}

fn figure(html: &str, key: &str) -> String {
    let row = &html[html
        .find(&format!(r#"data-figure="{key}""#))
        .expect("figure")..];
    let row = &row[..row.find("</tr>").unwrap()];
    let value = row.split_once("data-value").unwrap().1;
    value
        .split_once('>')
        .unwrap()
        .1
        .split("</td>")
        .next()
        .unwrap()
        .trim()
        .to_owned()
}

#[tokio::test]
async fn the_page_checks_the_character_with_the_lowest_margin_when_none_was_chosen() {
    let (app, browser) = setup().await;

    let html = browser
        .get(&app.url("/opportunities/classic"))
        .await
        .text()
        .await
        .unwrap();

    assert!(html.contains(r#"<form id="fee-filter""#), "{html}");
    assert!(html.contains("<legend>Fees</legend>"), "{html}");
    assert!(html.contains(&radio(ALICE, true)), "{html}");
    assert!(html.contains(&radio(BOB, false)), "{html}");
    assert!(html.contains("Broker fee 1.50%, sales tax 3.38%"), "{html}");
    assert_eq!(net(&html), "22.07");
}

#[tokio::test]
async fn choosing_a_character_saves_it_and_answers_page_one_recomputed() {
    let (app, browser) = setup().await;
    let token = csrf_token(&app, &browser).await;

    let res = choose(
        &app,
        &browser,
        Some(&token),
        &[
            ("fee_character", &BOB.to_string()),
            ("table", "classic"),
            ("page", "2"),
        ],
    )
    .await;

    assert_eq!(res.status(), 200);
    assert!(
        res.headers().get("hx-replace-url").is_some(),
        "the address bar goes back to page 1"
    );
    let html = res.text().await.unwrap();
    assert!(!html.contains("<html"), "a fragment");
    assert!(html.contains(r#"id="classic""#), "{html}");
    // Out of band, the group follows: Bob is checked, and the note names the base rates.
    assert!(html.contains(r#"<form id="fee-filter""#), "{html}");
    assert!(html.contains(&radio(BOB, true)), "{html}");
    assert!(html.contains(&radio(ALICE, false)), "{html}");
    assert!(html.contains("Base rates"), "{html}");
    // c = 100 * 1.03 = 103, r = 129.90 * (1 - 0.03 - 0.075) = 116.2605.
    assert_eq!(net(&html), "13.26");
    assert_eq!(stored(&app, &browser).await, Some(BOB));
    // The next visit uses it, on any table page.
    let again = browser
        .get(&app.url("/opportunities/classic"))
        .await
        .text()
        .await
        .unwrap();
    assert!(again.contains(&radio(BOB, true)), "{again}");
    assert_eq!(net(&again), "13.26");
}

#[tokio::test]
async fn choosing_on_the_detail_page_answers_the_detail_fragment() {
    let (app, browser) = setup().await;
    let token = csrf_token(&app, &browser).await;

    let res = choose(
        &app,
        &browser,
        Some(&token),
        &[
            ("fee_character", &BOB.to_string()),
            ("table", "classic"),
            ("type_id", "34"),
        ],
    )
    .await;

    assert_eq!(res.status(), 200);
    assert!(
        res.headers().get("hx-replace-url").is_none(),
        "the detail page keeps its address"
    );
    let html = res.text().await.unwrap();
    assert!(html.contains(r#"id="figures""#), "{html}");
    assert!(html.contains(&radio(BOB, true)), "{html}");
    assert_eq!(figure(&html, "net"), "13.26");
    // The detail page itself carries the choice.
    let page = browser
        .get(&app.url("/opportunities/classic/34"))
        .await
        .text()
        .await
        .unwrap();
    assert!(page.contains(&radio(BOB, true)), "{page}");
}

#[tokio::test]
async fn a_character_of_another_account_is_rejected_and_the_choice_stays() {
    let (app, browser) = setup().await;
    let token = csrf_token(&app, &browser).await;
    choose(
        &app,
        &browser,
        Some(&token),
        &[("fee_character", &BOB.to_string()), ("table", "classic")],
    )
    .await;
    // Carol belongs to a second account.
    let other = app.browser();
    other.sso_round_trip(&app, "/auth/login", 90000003).await;

    let res = choose(
        &app,
        &browser,
        Some(&token),
        &[("fee_character", "90000003"), ("table", "classic")],
    )
    .await;

    assert_eq!(res.status(), 422);
    assert_eq!(stored(&app, &browser).await, Some(BOB), "unchanged");
}

#[tokio::test]
async fn a_request_without_the_csrf_token_is_rejected_and_nothing_is_saved() {
    let (app, browser) = setup().await;

    let res = choose(
        &app,
        &browser,
        None,
        &[("fee_character", &BOB.to_string()), ("table", "classic")],
    )
    .await;

    assert_eq!(res.status(), 403);
    assert_eq!(stored(&app, &browser).await, None);
}

#[tokio::test]
async fn a_choice_does_not_reach_another_account() {
    let (app, browser) = setup().await;
    let token = csrf_token(&app, &browser).await;
    let other = app.browser();
    other.sso_round_trip(&app, "/auth/login", 90000003).await;

    choose(
        &app,
        &browser,
        Some(&token),
        &[("fee_character", &BOB.to_string()), ("table", "classic")],
    )
    .await;

    let html = other
        .get(&app.url("/opportunities/classic"))
        .await
        .text()
        .await
        .unwrap();
    assert!(html.contains(&radio(90000003, true)), "{html}");
    assert!(!html.contains(&radio(BOB, false)), "{html}");
}

#[tokio::test]
async fn a_chosen_character_that_is_deselected_gives_way_to_the_best_one() {
    let (app, browser) = setup().await;
    let token = csrf_token(&app, &browser).await;
    choose(
        &app,
        &browser,
        Some(&token),
        &[("fee_character", &BOB.to_string()), ("table", "classic")],
    )
    .await;

    let html = browser
        .get(&app.url(&format!("/opportunities/classic?chars={ALICE}")))
        .await
        .text()
        .await
        .unwrap();

    assert!(html.contains(&radio(ALICE, true)), "{html}");
    assert!(!html.contains(&format!(r#"name="fee_character" value="{BOB}""#)));
    assert_eq!(stored(&app, &browser).await, Some(BOB), "the choice stays");
}
