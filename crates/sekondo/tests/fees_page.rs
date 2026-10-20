//! The fee cards of the orders pages: sales tax, broker fee and minimum margin per character at
//! its fee location.

mod common;

use common::{AppBuilder, Browser, TestApp};
use sekondo::{fee_poller::Kind, store::StandingInput};
use time::OffsetDateTime;

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const JITA: i64 = 60003760;
const AMARR: i64 = 60008494;
const STRUCTURE: i64 = 1000000000001;

async fn setup() -> (TestApp, Browser) {
    let app = AppBuilder::new().with_stub().spawn().await;
    let browser = app.browser();
    browser.sso_round_trip(&app, "/auth/login", ALICE).await;
    (app, browser)
}

async fn add_bob(app: &TestApp, browser: &Browser) {
    let res = browser.sso_round_trip(app, "/auth/add", BOB).await;
    let _ = res;
}

async fn order(app: &TestApp, id: i64, character: i64, location: i64) {
    app.stores
        .orders
        .seed_order(
            character,
            &sekondo::esi::EsiOrder {
                order_id: id,
                type_id: 34,
                region_id: 10000002,
                location_id: location,
                is_buy_order: false,
                is_corporation: false,
                price_cents: 1000,
                volume_remain: 1,
                volume_total: 1,
                min_volume: 1,
                range: "station".to_owned(),
                escrow_cents: None,
                issued: OffsetDateTime::now_utc(),
                duration: 90,
            },
        )
        .await
        .unwrap();
}

async fn name(app: &TestApp, id: i64, name: &str) {
    app.stores
        .market
        .store_names(
            &[sekondo::store::NameRow {
                id,
                category: "station".to_owned(),
                name: Some(name.to_owned()),
            }],
            OffsetDateTime::now_utc(),
            None,
        )
        .await
        .unwrap();
}

async fn polled(app: &TestApp, character: i64) {
    for kind in [Kind::Skills, Kind::Standings] {
        app.stores
            .fees
            .seed_fee_success(character, kind, OffsetDateTime::now_utc())
            .await
            .unwrap();
    }
}

/// Accounting 5, Broker Relations 5, faction standing 2.0, corporation standing 1.5 (Caldari Navy).
async fn trained(app: &TestApp, character: i64) {
    polled(app, character).await;
    for (skill, level) in [(16622_i64, 5_i16), (3446, 5)] {
        app.stores
            .fees
            .seed_skill(character, skill, level)
            .await
            .unwrap();
    }
    for (from_id, kind, value) in [(500001_i64, "faction", "2.0"), (1000035, "npc_corp", "1.5")] {
        app.stores
            .fees
            .seed_standing(
                character,
                &StandingInput {
                    from_id,
                    from_type: kind.to_owned(),
                    standing: value.to_owned(),
                },
            )
            .await
            .unwrap();
    }
    app.stores
        .fees
        .insert_station_owner(JITA, 1000035, OffsetDateTime::now_utc())
        .await
        .unwrap();
}

const LOCATION_SCOPE: &str = "esi-location.read_location.v1";

async fn grant_location_scope(app: &TestApp, character: i64) {
    app.stores
        .accounts
        .set_scopes(character, &[LOCATION_SCOPE.to_owned()])
        .await
        .unwrap();
}

async fn revoke_location_scope(app: &TestApp, character: i64) {
    let mut scopes = app
        .stores
        .accounts
        .find_character(character)
        .await
        .unwrap()
        .unwrap()
        .scopes;
    scopes.retain(|scope| scope != LOCATION_SCOPE);
    app.stores
        .accounts
        .set_scopes(character, &scopes)
        .await
        .unwrap();
}

async fn docked(app: &TestApp, character: i64, location: i64, kind: &str) {
    let now = OffsetDateTime::now_utc();
    app.stores
        .fees
        .store_location(character, Some((location, kind)), now, now, None)
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

#[tokio::test]
async fn an_npc_station_gives_its_rates_with_the_inputs_on_hover_and_names_character_and_station() {
    let (app, browser) = setup().await;
    name(&app, JITA, JITA_NAME).await;
    trained(&app, ALICE).await;
    grant_location_scope(&app, ALICE).await;
    docked(&app, ALICE, JITA, "station").await;

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

#[tokio::test]
async fn the_cards_are_on_the_buy_page_too() {
    let (app, browser) = setup().await;
    trained(&app, ALICE).await;

    let html = page(&app, &browser, "/orders/buy").await;

    assert_eq!(fees_of(&html, ALICE)[0].text, "3.38%");
}

#[tokio::test]
async fn a_docked_character_has_its_fees_at_its_station_whether_or_not_it_has_orders_there() {
    let (app, browser) = setup().await;
    name(&app, AMARR, "Amarr VIII").await;
    order(&app, 1, ALICE, JITA).await;
    trained(&app, ALICE).await;
    grant_location_scope(&app, ALICE).await;
    docked(&app, ALICE, AMARR, "station").await;
    app.stores
        .fees
        .insert_station_owner(AMARR, 1000086, OffsetDateTime::now_utc())
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

#[tokio::test]
async fn a_structure_reads_owner_fee_and_no_minimum_margin() {
    let (app, browser) = setup().await;
    name(&app, STRUCTURE, "Alpha Keepstar").await;
    trained(&app, ALICE).await;
    grant_location_scope(&app, ALICE).await;
    docked(&app, ALICE, STRUCTURE, "structure").await;

    let fees = fees_of(&page(&app, &browser, "/orders/sell").await, ALICE);

    assert_eq!(fees[0].text, "3.38%");
    assert_eq!(fees[1].text, "0.5% + owner fee");
    assert_eq!(fees[2].text, "\u{2014}");
    assert!(fees[0].detail.ends_with("Alpha Keepstar"));
}

#[tokio::test]
async fn a_character_whose_skills_were_not_polled_shows_dashes() {
    let (app, browser) = setup().await;

    let fees = fees_of(&page(&app, &browser, "/orders/sell").await, ALICE);

    for fee in fees {
        assert_eq!(fee.text, "\u{2014}");
        assert_eq!(fee.title, None);
    }
}

#[tokio::test]
async fn a_station_without_a_known_owner_has_a_tax_but_no_fee() {
    let (app, browser) = setup().await;
    name(&app, AMARR, "Amarr VIII").await;
    trained(&app, ALICE).await;
    grant_location_scope(&app, ALICE).await;
    docked(&app, ALICE, AMARR, "station").await;

    let fees = fees_of(&page(&app, &browser, "/orders/sell").await, ALICE);

    assert_eq!(fees[0].text, "3.38%");
    assert_eq!(fees[1].text, "\u{2014}");
    assert_eq!(fees[2].text, "\u{2014}");
}

#[tokio::test]
async fn without_the_location_scope_the_cards_name_jita_and_offer_to_log_in_again() {
    let (app, browser) = setup().await;
    trained(&app, ALICE).await;
    revoke_location_scope(&app, ALICE).await;

    let html = page(&app, &browser, "/orders/sell").await;

    let fees = fees_of(&html, ALICE);
    for fee in &fees {
        assert_eq!(fee.detail, format!("Alice Alpha \u{b7} {JITA_NAME}"));
        assert!(fee.relogin, "{fee:?}");
    }
    assert!(html.contains(r#"href="/auth/add""#));
}

#[tokio::test]
async fn a_character_with_the_scope_that_was_never_docked_uses_jita_without_the_link() {
    let (app, browser) = setup().await;
    trained(&app, ALICE).await;
    grant_location_scope(&app, ALICE).await;

    let fees = fees_of(&page(&app, &browser, "/orders/sell").await, ALICE);

    assert!(fees[0].detail.ends_with(JITA_NAME));
    assert!(fees.iter().all(|f| !f.relogin));
}

#[tokio::test]
async fn only_the_selected_characters_have_cards_in_the_order_they_were_linked() {
    let (app, browser) = setup().await;
    add_bob(&app, &browser).await;
    trained(&app, ALICE).await;
    polled(&app, BOB).await;

    let both = page(&app, &browser, "/orders/sell").await;
    assert!(
        both.find("fee-90000001-sales-tax").unwrap() < both.find("fee-90000002-sales-tax").unwrap()
    );

    let only_bob = page(&app, &browser, &format!("/orders/sell?chars={BOB}")).await;
    assert!(only_bob.contains("fee-90000002-sales-tax"));
    assert!(!only_bob.contains("fee-90000001-sales-tax"), "{only_bob}");
    assert_eq!(only_bob.matches(r#"id="fee-"#).count(), 3);
}

#[tokio::test]
async fn the_freshness_does_not_list_fees() {
    let (app, browser) = setup().await;
    trained(&app, ALICE).await;
    order(&app, 1, ALICE, JITA).await;

    for path in ["/orders/freshness", "/orders/sell"] {
        let html = page(&app, &browser, path).await;
        let freshness = &html[html.find(r#"<ul class="freshness">"#).unwrap()..];
        assert!(freshness.contains("Alice Alpha"));
        assert!(!freshness.contains("<table"), "{path}: {freshness}");
        assert!(!freshness.contains("Sales tax"), "{path}");
    }
}

#[tokio::test]
async fn the_orders_pages_follow_fees_updated_in_the_content_and_no_other_page_does() {
    let (app, browser) = setup().await;

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
