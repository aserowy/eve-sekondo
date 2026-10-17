//! The market fees under each character's freshness entry on the orders page.

mod common;

use common::{AppBuilder, Browser, TestApp};
use sqlx::PgPool;

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const JITA: i64 = 60003760;
const AMARR: i64 = 60008494;
const STRUCTURE: i64 = 1000000000001;

async fn setup(pool: &PgPool) -> (TestApp, Browser) {
    let app = AppBuilder::new(pool.clone()).with_stub().spawn().await;
    let browser = app.browser();
    browser.sso_round_trip(&app, "/auth/login", ALICE).await;
    (app, browser)
}

async fn add_bob(app: &TestApp, browser: &Browser) {
    let res = browser.sso_round_trip(app, "/auth/add", BOB).await;
    let _ = res;
}

async fn order(pool: &PgPool, id: i64, character: i64, location: i64) {
    sqlx::query(
        "INSERT INTO current_orders (order_id, character_id, type_id, region_id, location_id, \
         is_buy_order, is_corporation, price, volume_remain, volume_total, min_volume, range, \
         issued, duration) \
         VALUES ($1, $2, 34, 10000002, $3, false, false, 10, 1, 1, 1, 'station', now(), 90)",
    )
    .bind(id)
    .bind(character)
    .bind(location)
    .execute(pool)
    .await
    .unwrap();
}

async fn name(pool: &PgPool, id: i64, name: &str) {
    sqlx::query(
        "INSERT INTO names (id, category, name, resolved_at) VALUES ($1, 'station', $2, now())",
    )
    .bind(id)
    .bind(name)
    .execute(pool)
    .await
    .unwrap();
}

async fn polled(pool: &PgPool, character: i64) {
    for kind in ["skills", "standings"] {
        sqlx::query(
            "INSERT INTO fee_sync (character_id, kind, next_poll_at, last_success_at) \
             VALUES ($1, $2, now(), now())",
        )
        .bind(character)
        .bind(kind)
        .execute(pool)
        .await
        .unwrap();
    }
}

/// Accounting 5, Broker Relations 5, faction standing 2.0, corporation standing 1.5 (Caldari Navy).
async fn trained(pool: &PgPool, character: i64) {
    polled(pool, character).await;
    for (skill, level) in [(16622_i64, 5_i16), (3446, 5)] {
        sqlx::query("INSERT INTO character_skills (character_id, skill_id, active_level) VALUES ($1, $2, $3)")
            .bind(character)
            .bind(skill)
            .bind(level)
            .execute(pool)
            .await
            .unwrap();
    }
    for (id, kind, value) in [(500001_i64, "faction", "2.0"), (1000035, "npc_corp", "1.5")] {
        sqlx::query(
            "INSERT INTO character_standings (character_id, from_id, from_type, standing) \
             VALUES ($1, $2, $3, $4::numeric)",
        )
        .bind(character)
        .bind(id)
        .bind(kind)
        .bind(value)
        .execute(pool)
        .await
        .unwrap();
    }
    sqlx::query("INSERT INTO station_owners (station_id, owner_id, resolved_at) VALUES ($1, 1000035, now()) ON CONFLICT DO NOTHING")
        .bind(JITA)
        .execute(pool)
        .await
        .unwrap();
}

async fn fragment(app: &TestApp, browser: &Browser, query: &str) -> String {
    let res = browser
        .get(&app.url(&format!("/orders/freshness{query}")))
        .await;
    assert_eq!(res.status(), 200);
    res.text().await.unwrap()
}

/// The cells of the row whose location is `location`, as `(text, title)` per figure.
fn figures(html: &str, location: &str) -> Vec<(String, Option<String>)> {
    let row = html
        .split("<tr>")
        .find(|row| row.contains(&format!("<td>{location}</td>")))
        .unwrap_or_else(|| panic!("no row for {location} in {html}"));
    row.split("<td data-fee=")
        .skip(1)
        .map(|cell| {
            let tag_end = cell.find('>').unwrap();
            let text = cell[tag_end + 1..].split("</td>").next().unwrap().trim();
            let title = cell[..tag_end]
                .split("title=\"")
                .nth(1)
                .map(|t| t.split('"').next().unwrap().to_owned());
            (text.to_owned(), title)
        })
        .collect()
}

#[sqlx::test]
async fn an_npc_station_lists_its_rates_with_the_inputs_on_hover(pool: PgPool) {
    let (app, browser) = setup(&pool).await;
    name(
        &pool,
        JITA,
        "Jita IV - Moon 4 - Caldari Navy Assembly Plant",
    )
    .await;
    order(&pool, 1, ALICE, JITA).await;
    trained(&pool, ALICE).await;

    let html = fragment(&app, &browser, "").await;

    let figures = figures(&html, "Jita IV - Moon 4 - Caldari Navy Assembly Plant");
    assert_eq!(figures[0].0, "3.38%");
    assert_eq!(figures[0].1.as_deref(), Some("3.3750% (Accounting 5)"));
    assert_eq!(figures[1].0, "1.41%");
    assert_eq!(
        figures[1].1.as_deref(),
        Some("1.4100% (Broker Relations 5, faction standing 2.00, corporation standing 1.50)")
    );
    assert_eq!(figures[2].0, "6.51%");
    assert_eq!(figures[2].1.as_deref(), Some("6.5063%"));
    for header in ["Location", "Sales tax", "Broker fee", "Min. margin"] {
        assert!(html.contains(header), "{header}");
    }
}

#[sqlx::test]
async fn a_station_and_a_structure_are_listed_side_by_side_ordered_by_name(pool: PgPool) {
    let (app, browser) = setup(&pool).await;
    name(&pool, JITA, "Jita IV").await;
    name(&pool, STRUCTURE, "Alpha Keepstar").await;
    order(&pool, 1, ALICE, JITA).await;
    order(&pool, 2, ALICE, STRUCTURE).await;
    trained(&pool, ALICE).await;

    let html = fragment(&app, &browser, "").await;

    let structure = figures(&html, "Alpha Keepstar");
    assert_eq!(structure[0].0, "3.38%");
    assert_eq!(structure[1].0, "0.5% + owner fee");
    assert_eq!(structure[2].0, "\u{2014}");
    assert_eq!(figures(&html, "Jita IV")[1].0, "1.41%");
    assert!(
        html.find("Alpha Keepstar").unwrap() < html.find("Jita IV").unwrap(),
        "ordered by location name"
    );
}

#[sqlx::test]
async fn a_character_whose_skills_were_not_polled_shows_dashes(pool: PgPool) {
    let (app, browser) = setup(&pool).await;
    name(&pool, JITA, "Jita IV").await;
    order(&pool, 1, ALICE, JITA).await;

    let html = fragment(&app, &browser, "").await;

    for (text, title) in figures(&html, "Jita IV") {
        assert_eq!(text, "\u{2014}");
        assert_eq!(title, None);
    }
}

#[sqlx::test]
async fn a_station_without_a_known_owner_has_a_tax_but_no_fee(pool: PgPool) {
    let (app, browser) = setup(&pool).await;
    name(&pool, AMARR, "Amarr VIII").await;
    order(&pool, 1, ALICE, AMARR).await;
    trained(&pool, ALICE).await;

    let figures = figures(&fragment(&app, &browser, "").await, "Amarr VIII");

    assert_eq!(figures[0].0, "3.38%");
    assert_eq!(figures[1].0, "\u{2014}");
    assert_eq!(figures[2].0, "\u{2014}");
}

#[sqlx::test]
async fn a_character_without_orders_lists_no_locations(pool: PgPool) {
    let (app, browser) = setup(&pool).await;
    trained(&pool, ALICE).await;

    let html = fragment(&app, &browser, "").await;

    assert!(html.contains("Alice Alpha"));
    assert!(!html.contains("<table"), "{html}");
}

#[sqlx::test]
async fn an_unnamed_location_shows_its_id(pool: PgPool) {
    let (app, browser) = setup(&pool).await;
    order(&pool, 1, ALICE, JITA).await;

    let html = fragment(&app, &browser, "").await;

    assert!(html.contains(&format!("<td>#{JITA}</td>")), "{html}");
}

#[sqlx::test]
async fn only_the_selected_characters_are_listed(pool: PgPool) {
    let (app, browser) = setup(&pool).await;
    add_bob(&app, &browser).await;
    name(&pool, JITA, "Jita IV").await;
    name(&pool, AMARR, "Amarr VIII").await;
    order(&pool, 1, ALICE, JITA).await;
    order(&pool, 2, BOB, AMARR).await;
    trained(&pool, ALICE).await;
    polled(&pool, BOB).await;

    let both = fragment(&app, &browser, "").await;
    assert!(both.contains("Jita IV") && both.contains("Amarr VIII"));

    let only_bob = fragment(&app, &browser, &format!("?chars={BOB}")).await;
    assert!(only_bob.contains("Amarr VIII"), "{only_bob}");
    assert!(!only_bob.contains("Jita IV"), "{only_bob}");
    assert!(!only_bob.contains("Alice Alpha"), "{only_bob}");
}

#[sqlx::test]
async fn the_orders_page_follows_fees_updated_in_both_sections_and_no_other_page_does(
    pool: PgPool,
) {
    let (app, browser) = setup(&pool).await;

    let res = browser.get(&app.url("/orders")).await;
    let html = res.text().await.unwrap();

    let trigger = |id: &str| {
        let start = html.find(&format!(r#"id="{id}""#)).unwrap();
        let tag = &html[start..];
        let tag = &tag[..tag.find('>').unwrap()];
        tag.split(r#"hx-trigger=""#)
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .to_owned()
    };
    assert!(trigger("freshness").contains("sse:fees-updated"));
    assert!(trigger("freshness").contains("sse:orders-updated"));
    // The margins in the tables depend on the fees.
    assert!(trigger("order-tables").contains("sse:fees-updated"));

    let wealth = browser.get(&app.url("/wealth")).await.text().await.unwrap();
    let dashboard = browser
        .get(&app.url("/dashboard"))
        .await
        .text()
        .await
        .unwrap();
    assert!(!wealth.contains("fees-updated"));
    assert!(!dashboard.contains("fees-updated"));
}
