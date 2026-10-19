//! The fee cards of the orders pages: sales tax, broker fee and minimum margin per character at
//! its fee location.

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

const LOCATION_SCOPE: &str = "esi-location.read_location.v1";

async fn grant_location_scope(pool: &PgPool, character: i64) {
    sqlx::query("UPDATE characters SET scopes = ARRAY[$2] WHERE character_id = $1")
        .bind(character)
        .bind(LOCATION_SCOPE)
        .execute(pool)
        .await
        .unwrap();
}

async fn revoke_location_scope(pool: &PgPool, character: i64) {
    sqlx::query("UPDATE characters SET scopes = array_remove(scopes, $2) WHERE character_id = $1")
        .bind(character)
        .bind(LOCATION_SCOPE)
        .execute(pool)
        .await
        .unwrap();
}

async fn docked(pool: &PgPool, character: i64, location: i64, kind: &str) {
    sqlx::query(
        "INSERT INTO character_locations (character_id, location_id, kind, updated_at) \
         VALUES ($1, $2, $3, now())",
    )
    .bind(character)
    .bind(location)
    .bind(kind)
    .execute(pool)
    .await
    .unwrap();
}

async fn page(app: &TestApp, browser: &Browser, path: &str) -> String {
    let res = browser.get(&app.url(path)).await;
    assert_eq!(res.status(), 200, "{path}");
    res.text().await.unwrap()
}

/// One fee card: what it reads, what hovering adds, its detail line and whether it offers to log
/// in again.
#[derive(Debug, PartialEq, Eq)]
struct Fee {
    text: String,
    title: Option<String>,
    detail: String,
    relogin: bool,
}

/// The three fee cards of a character: sales tax, broker fee, minimum margin.
fn fees_of(html: &str, character: i64) -> Vec<Fee> {
    ["sales-tax", "broker-fee", "min-margin"]
        .into_iter()
        .map(|key| {
            let id = format!(r#"id="fee-{character}-{key}""#);
            let at = html
                .find(&id)
                .unwrap_or_else(|| panic!("no card {id} in {html}"));
            let card = &html[at..];
            let card = &card[..card.find("</article>").unwrap()];
            let value = &card[card.find(r#"<p class="value""#).unwrap()..];
            let value = &value[..value.find("</p>").unwrap()];
            let title = value
                .split("title=\"")
                .nth(1)
                .map(|t| t.split('"').next().unwrap().to_owned())
                .filter(|t| !t.is_empty());
            let detail = &card[card.find(r#"<small class="detail""#).unwrap()..];
            let detail = detail[detail.find('>').unwrap() + 1..]
                .split("</small>")
                .next()
                .unwrap();
            Fee {
                text: value.split_once('>').unwrap().1.to_owned(),
                title,
                detail: detail.to_owned(),
                relogin: card.contains("Log in again"),
            }
        })
        .collect()
}

const JITA_NAME: &str = "Jita IV - Moon 4 - Caldari Navy Assembly Plant";

#[sqlx::test]
async fn an_npc_station_gives_its_rates_with_the_inputs_on_hover_and_names_character_and_station(
    pool: PgPool,
) {
    let (app, browser) = setup(&pool).await;
    name(&pool, JITA, JITA_NAME).await;
    trained(&pool, ALICE).await;
    grant_location_scope(&pool, ALICE).await;
    docked(&pool, ALICE, JITA, "station").await;

    let html = page(&app, &browser, "/orders/sell").await;

    let fees = fees_of(&html, ALICE);
    assert_eq!(fees[0].text, "3.38%");
    assert_eq!(fees[0].title.as_deref(), Some("3.3750% (Accounting 5)"));
    assert_eq!(fees[1].text, "1.41%");
    assert_eq!(
        fees[1].title.as_deref(),
        Some("1.4100% (Broker Relations 5, faction standing 2.00, corporation standing 1.50)")
    );
    assert_eq!(fees[2].text, "6.51%");
    assert_eq!(fees[2].title.as_deref(), Some("6.5063%"));
    for fee in &fees {
        assert_eq!(fee.detail, format!("Alice Alpha \u{b7} {JITA_NAME}"));
        assert!(!fee.relogin);
    }
    for card in ["Sales tax", "Broker fee", "Min. margin"] {
        assert!(
            html.contains(&format!(r#"<small class="name">{card}</small>"#)),
            "{card}"
        );
    }
}

#[sqlx::test]
async fn the_cards_are_on_the_buy_page_too(pool: PgPool) {
    let (app, browser) = setup(&pool).await;
    trained(&pool, ALICE).await;

    let html = page(&app, &browser, "/orders/buy").await;

    assert_eq!(fees_of(&html, ALICE)[0].text, "3.38%");
}

#[sqlx::test]
async fn a_docked_character_has_its_fees_at_its_station_whether_or_not_it_has_orders_there(
    pool: PgPool,
) {
    let (app, browser) = setup(&pool).await;
    name(&pool, AMARR, "Amarr VIII").await;
    order(&pool, 1, ALICE, JITA).await;
    trained(&pool, ALICE).await;
    grant_location_scope(&pool, ALICE).await;
    docked(&pool, ALICE, AMARR, "station").await;
    sqlx::query("INSERT INTO station_owners (station_id, owner_id, resolved_at) VALUES ($1, 1000086, now())")
        .bind(AMARR)
        .execute(&pool)
        .await
        .unwrap();

    let html = page(&app, &browser, "/orders/sell").await;

    let fees = fees_of(&html, ALICE);
    assert!(fees[0].detail.ends_with("Amarr VIII"), "{}", fees[0].detail);
    assert_eq!(
        fees[1].text, "1.50%",
        "no standings towards Amarr: 3 - 0.3 * 5"
    );
}

#[sqlx::test]
async fn a_structure_reads_owner_fee_and_no_minimum_margin(pool: PgPool) {
    let (app, browser) = setup(&pool).await;
    name(&pool, STRUCTURE, "Alpha Keepstar").await;
    trained(&pool, ALICE).await;
    grant_location_scope(&pool, ALICE).await;
    docked(&pool, ALICE, STRUCTURE, "structure").await;

    let fees = fees_of(&page(&app, &browser, "/orders/sell").await, ALICE);

    assert_eq!(fees[0].text, "3.38%");
    assert_eq!(fees[1].text, "0.5% + owner fee");
    assert_eq!(fees[2].text, "\u{2014}");
    assert!(fees[0].detail.ends_with("Alpha Keepstar"));
}

#[sqlx::test]
async fn a_character_whose_skills_were_not_polled_shows_dashes(pool: PgPool) {
    let (app, browser) = setup(&pool).await;

    let fees = fees_of(&page(&app, &browser, "/orders/sell").await, ALICE);

    for fee in fees {
        assert_eq!(fee.text, "\u{2014}");
        assert_eq!(fee.title, None);
    }
}

#[sqlx::test]
async fn a_station_without_a_known_owner_has_a_tax_but_no_fee(pool: PgPool) {
    let (app, browser) = setup(&pool).await;
    name(&pool, AMARR, "Amarr VIII").await;
    trained(&pool, ALICE).await;
    grant_location_scope(&pool, ALICE).await;
    docked(&pool, ALICE, AMARR, "station").await;

    let fees = fees_of(&page(&app, &browser, "/orders/sell").await, ALICE);

    assert_eq!(fees[0].text, "3.38%");
    assert_eq!(fees[1].text, "\u{2014}");
    assert_eq!(fees[2].text, "\u{2014}");
}

#[sqlx::test]
async fn without_the_location_scope_the_cards_name_jita_and_offer_to_log_in_again(pool: PgPool) {
    let (app, browser) = setup(&pool).await;
    trained(&pool, ALICE).await;
    revoke_location_scope(&pool, ALICE).await;

    let html = page(&app, &browser, "/orders/sell").await;

    let fees = fees_of(&html, ALICE);
    for fee in &fees {
        assert_eq!(fee.detail, format!("Alice Alpha \u{b7} {JITA_NAME}"));
        assert!(fee.relogin, "{fee:?}");
    }
    assert!(html.contains(r#"href="/auth/add""#));
}

#[sqlx::test]
async fn a_character_with_the_scope_that_was_never_docked_uses_jita_without_the_link(pool: PgPool) {
    let (app, browser) = setup(&pool).await;
    trained(&pool, ALICE).await;
    grant_location_scope(&pool, ALICE).await;

    let fees = fees_of(&page(&app, &browser, "/orders/sell").await, ALICE);

    assert!(fees[0].detail.ends_with(JITA_NAME));
    assert!(fees.iter().all(|f| !f.relogin));
}

#[sqlx::test]
async fn only_the_selected_characters_have_cards_in_the_order_they_were_linked(pool: PgPool) {
    let (app, browser) = setup(&pool).await;
    add_bob(&app, &browser).await;
    trained(&pool, ALICE).await;
    polled(&pool, BOB).await;

    let both = page(&app, &browser, "/orders/sell").await;
    assert!(
        both.find("fee-90000001-sales-tax").unwrap() < both.find("fee-90000002-sales-tax").unwrap()
    );

    let only_bob = page(&app, &browser, &format!("/orders/sell?chars={BOB}")).await;
    assert!(only_bob.contains("fee-90000002-sales-tax"));
    assert!(!only_bob.contains("fee-90000001-sales-tax"), "{only_bob}");
    assert_eq!(only_bob.matches(r#"id="fee-"#).count(), 3);
}

#[sqlx::test]
async fn the_freshness_does_not_list_fees(pool: PgPool) {
    let (app, browser) = setup(&pool).await;
    trained(&pool, ALICE).await;
    order(&pool, 1, ALICE, JITA).await;

    for path in ["/orders/freshness", "/orders/sell"] {
        let html = page(&app, &browser, path).await;
        let freshness = &html[html.find(r#"<ul class="freshness">"#).unwrap()..];
        assert!(freshness.contains("Alice Alpha"));
        assert!(!freshness.contains("<table"), "{path}: {freshness}");
        assert!(!freshness.contains("Sales tax"), "{path}");
    }
}

#[sqlx::test]
async fn the_orders_pages_follow_fees_updated_in_the_content_and_no_other_page_does(pool: PgPool) {
    let (app, browser) = setup(&pool).await;

    let trigger = |html: &str, id: &str| {
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
    for path in ["/orders/sell", "/orders/buy"] {
        let html = page(&app, &browser, path).await;
        assert!(
            trigger(&html, "freshness").contains("sse:fees-updated"),
            "{path}"
        );
        assert!(
            trigger(&html, "freshness").contains("sse:orders-updated"),
            "{path}"
        );
        // The fee cards and the margins in the table depend on the fees.
        assert!(
            trigger(&html, "content").contains("sse:fees-updated"),
            "{path}"
        );
    }

    let wealth = page(&app, &browser, "/wealth").await;
    let dashboard = page(&app, &browser, "/dashboard").await;
    assert!(!wealth.contains("fees-updated"));
    assert!(!dashboard.contains("fees-updated"));
}
