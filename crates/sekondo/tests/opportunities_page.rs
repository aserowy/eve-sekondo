mod common;

use common::{AppBuilder, Browser, TestApp};
use time::{Duration, OffsetDateTime};

const ALICE: i64 = 90000001;
const BOB: i64 = 90000002;
const STATION_NAME: &str = "Jita IV - Moon 4 - Caldari Navy Assembly Plant";

async fn setup() -> (TestApp, Browser) {
    let app = AppBuilder::new().with_stub().spawn().await;
    let browser = app.browser();
    browser.sso_round_trip(&app, "/auth/login", ALICE).await;
    (app, browser)
}

async fn get(app: &TestApp, browser: &Browser, path: &str) -> String {
    let res = browser.get(&app.url(path)).await;
    assert_eq!(res.status(), 200, "{path}");
    res.text().await.unwrap()
}

/// The freshness section of the page.
fn freshness(html: &str) -> &str {
    let start = html.find(r#"id="freshness""#).expect("a freshness section");
    let rest = &html[start..];
    &rest[..rest.find("</section>").unwrap()]
}

const REGION: i64 = 10000002;
const STATION: i64 = 60003760;

/// A book refresh that succeeded at `last_success`, and failed with `error` after that.
async fn book_sync(app: &TestApp, last_success: OffsetDateTime, error: Option<&str>) {
    let station = &app.stores.station;
    station
        .replace_station_book(
            REGION,
            STATION,
            last_success,
            &[],
            last_success + Duration::minutes(5),
            None,
        )
        .await
        .unwrap();
    if let Some(reason) = error {
        station
            .record_book_failure(
                REGION,
                STATION,
                last_success,
                reason,
                Duration::seconds(30),
                Duration::minutes(5),
            )
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn an_unauthenticated_request_is_redirected_to_the_landing_page() {
    let app = AppBuilder::new().with_stub().spawn().await;
    for path in [
        "/opportunities/expansion",
        "/opportunities/classic",
        "/opportunities/reversion",
        "/opportunities/trend",
        "/opportunities/trend/content",
        "/opportunities/freshness",
    ] {
        let res = app.browser().get(&app.url(path)).await;
        assert!(res.status().is_redirection(), "{path}");
        assert_eq!(Browser::location(&res), "/", "{path}");
    }
}

#[tokio::test]
async fn the_entry_redirects_to_the_first_table_and_keeps_the_query() {
    let (app, browser) = setup().await;

    let plain = browser.get(&app.url("/opportunities")).await;
    assert_eq!(plain.status(), 303);
    assert_eq!(Browser::location(&plain), "/opportunities/expansion");

    let query = format!("chars={ALICE}&cs=item&cd=asc&ts=sigma&td=desc&page=2");
    let with_query = browser
        .get(&app.url(&format!("/opportunities?{query}")))
        .await;
    assert_eq!(with_query.status(), 303);
    assert_eq!(
        Browser::location(&with_query),
        format!("/opportunities/expansion?{query}")
    );
}

#[tokio::test]
async fn an_unknown_table_is_not_found() {
    let (app, browser) = setup().await;

    for path in [
        "/opportunities/arbitrage",
        "/opportunities/arbitrage/content",
    ] {
        let res = browser.get(&app.url(path)).await;
        assert_eq!(res.status(), 404, "{path}");
    }
}

#[tokio::test]
async fn each_page_shows_one_table_below_the_sub_menu_and_the_filters_with_the_freshness_last() {
    let (app, browser) = setup().await;

    for (table, title) in [
        ("expansion", "Recurring spread expansion"),
        ("classic", "Classic spread trade"),
        ("reversion", "Mean reversion"),
        ("trend", "Trend"),
    ] {
        let html = get(&app, &browser, &format!("/opportunities/{table}")).await;

        assert!(html.contains(STATION_NAME), "{table}");
        assert!(
            html.contains(&format!("<h1>{title}</h1>")),
            "{table}: the table names the page"
        );
        assert_eq!(html.matches("<h1").count(), 1, "{table}");
        assert!(!html.contains("panel"), "no self-made panels");
        let at = |needle: &str| {
            html.find(needle)
                .unwrap_or_else(|| panic!("{needle} missing in {table}"))
        };
        let positions = [
            at(r#"<nav class="sub-menu""#),
            at(r#"id="filter-box""#),
            at(&format!(r#"<section id="{table}""#)),
            at(r#"id="freshness""#),
        ];
        assert!(
            positions.windows(2).all(|w| w[0] < w[1]),
            "{table}: {positions:?}"
        );
        assert!(
            !html.contains(&format!("<h2>{title}</h2>")),
            "{table}: titled once, by the page title"
        );
        for other in ["expansion", "classic", "reversion", "trend"] {
            if other != table {
                assert!(
                    !html.contains(&format!(r#"<section id="{other}""#)),
                    "{table} page shows {other}"
                );
            }
        }
    }
}

/// The links of the page sub-menu: text, href and whether it is the current page.
fn sub_menu(html: &str) -> Vec<(String, String, bool)> {
    let menu = html.split_once(r#"<nav class="sub-menu""#).unwrap().1;
    let menu = &menu[..menu.find("</nav>").unwrap()];
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
            let text = a.split_once('>').unwrap().1.split("</a>").next().unwrap();
            (
                text.to_owned(),
                href.replace("&#38;", "&"),
                a.contains(r#"aria-current="page""#),
            )
        })
        .collect()
}

#[tokio::test]
async fn the_sub_menu_lists_the_four_tables_as_text_without_counts_and_marks_the_current_one() {
    let (app, browser) = setup().await;
    // A classic row that would have made a number.
    let now = OffsetDateTime::now_utc();
    app.stores
        .station
        .seed_item_stats(
            REGION,
            STATION,
            34,
            &sekondo::store::ItemStatsRecord {
                book_at: Some(now),
                best_buy: Some("100".to_owned()),
                best_sell: Some("130".to_owned()),
                buy_near_orders: Some(0),
                sell_near_orders: Some(0),
                rho: Some(0.5),
                indicators_at: Some(now),
                volume_7d: Some(100000000.0),
                volume_30d: Some(100000000.0),
                sigma: Some(0.02),
                returns: Some(29),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let html = get(&app, &browser, "/opportunities/trend").await;

    let links = sub_menu(&html);
    let texts: Vec<_> = links.iter().map(|l| l.0.as_str()).collect();
    assert_eq!(
        texts,
        [
            "Recurring spread expansion",
            "Classic spread trade",
            "Mean reversion",
            "Trend"
        ]
    );
    let current: Vec<_> = links.iter().filter(|l| l.2).map(|l| l.0.as_str()).collect();
    assert_eq!(current, ["Trend"]);
    assert!(!html.contains(r#"class="count""#), "no row counts");
}

#[tokio::test]
async fn the_sub_menu_carries_the_selection_and_every_sorting_but_no_paging() {
    let (app, browser) = setup().await;
    browser.sso_round_trip(&app, "/auth/add", BOB).await;

    let html = get(
        &app,
        &browser,
        &format!(
            "/opportunities/classic?chars={BOB}&cs=item&cd=asc&ts=sigma&td=desc&page=2&per_page=25"
        ),
    )
    .await;

    let query = format!("?chars={BOB}&cs=item&cd=asc&ts=sigma&td=desc");
    for (label, href, _) in sub_menu(&html) {
        let table = match label.as_str() {
            "Recurring spread expansion" => "expansion",
            "Classic spread trade" => "classic",
            "Mean reversion" => "reversion",
            _ => "trend",
        };
        assert_eq!(href, format!("/opportunities/{table}{query}"), "{label}");
    }
}

#[tokio::test]
async fn a_recent_book_refresh_shows_its_time_and_is_not_stale() {
    let (app, browser) = setup().await;
    let last = OffsetDateTime::now_utc() - Duration::minutes(5);
    book_sync(&app, last, None).await;

    let html = get(&app, &browser, "/opportunities/expansion").await;
    let strip = freshness(&html);

    assert!(strip.contains(&sekondo::format::eve_time(last)), "{strip}");
    assert!(!strip.contains("Stale"), "{strip}");
    assert!(!strip.contains("Waiting for first refresh"), "{strip}");
}

#[tokio::test]
async fn a_book_refresh_forty_minutes_ago_is_marked_stale() {
    let (app, browser) = setup().await;
    let last = OffsetDateTime::now_utc() - Duration::minutes(40);
    book_sync(&app, last, None).await;

    let html = get(&app, &browser, "/opportunities/expansion").await;
    let strip = freshness(&html);

    assert!(strip.contains(&sekondo::format::eve_time(last)), "{strip}");
    assert!(strip.contains("Stale"), "{strip}");
}

#[tokio::test]
async fn a_book_that_was_never_refreshed_is_stale_and_waiting() {
    let (app, browser) = setup().await;

    let html = get(&app, &browser, "/opportunities/expansion").await;
    let strip = freshness(&html);

    assert!(strip.contains("Stale"), "{strip}");
    assert!(strip.contains("Waiting for first refresh"), "{strip}");
}

#[tokio::test]
async fn the_last_errors_of_the_book_and_the_history_are_shown() {
    let (app, browser) = setup().await;
    book_sync(&app, OffsetDateTime::now_utc(), Some("HTTP 503")).await;
    app.stores
        .station
        .finish_history_pass(
            REGION,
            STATION,
            OffsetDateTime::now_utc() - Duration::hours(2),
            true,
            Some("HTTP 502"),
        )
        .await
        .unwrap();

    let html = get(&app, &browser, "/opportunities/freshness").await;

    assert!(html.contains("HTTP 503"), "{html}");
    assert!(html.contains("HTTP 502"), "{html}");
    assert!(!html.contains("<html"), "a fragment, not a page");
}

#[tokio::test]
async fn the_days_of_own_snapshots_are_counted() {
    let (app, browser) = setup().await;
    let rows: Vec<_> = [1, 2, 3]
        .into_iter()
        .map(|back| sekondo::store::SeedSnapshotRow {
            recorded_at: OffsetDateTime::now_utc() - Duration::days(back) - Duration::hours(2),
            type_id: 34,
            best_buy: 100.0,
            best_sell: 110.0,
            buy_near_units: 1,
            buy_near_orders: 1,
            sell_near_units: 1,
            sell_near_orders: 1,
            buy_units: 1,
            sell_units: 1,
            buy_taken: 0,
            sell_taken: 0,
        })
        .collect();
    app.stores
        .station
        .seed_snapshots(REGION, STATION, &rows)
        .await
        .unwrap();

    let html = get(&app, &browser, "/opportunities/freshness").await;

    assert!(html.contains("3 days of own snapshots"), "{html}");
}

#[tokio::test]
async fn the_navigation_links_to_the_first_table_and_marks_opportunities_current_on_every_table() {
    let (app, browser) = setup().await;

    for table in ["expansion", "classic", "reversion", "trend"] {
        let html = get(&app, &browser, &format!("/opportunities/{table}")).await;

        let nav = html.split_once("</aside>").unwrap().0;
        assert!(
            nav.contains(r#"href="/opportunities/expansion" aria-label="Opportunities""#)
                || nav.contains(r#"href="/opportunities/expansion""#),
            "{nav}"
        );
        let current: Vec<_> = nav
            .split("<a ")
            .skip(1)
            .filter(|a| a.contains(r#"aria-current="page""#))
            .map(|a| {
                a.split(r#"aria-label=""#)
                    .nth(1)
                    .unwrap()
                    .split('"')
                    .next()
                    .unwrap()
            })
            .collect();
        assert_eq!(current, ["Opportunities"], "{table}");
    }
}

#[tokio::test]
async fn the_navigation_carries_a_narrowed_selection_to_and_from_the_page() {
    let (app, browser) = setup().await;
    browser.sso_round_trip(&app, "/auth/add", BOB).await;
    let unescaped = |html: String| html.replace("&#38;", "&").replace("&amp;", "&");

    for path in [
        format!("/orders/sell?chars={BOB}"),
        format!("/dashboard?chars={BOB}"),
        format!("/wealth?chars={BOB}"),
        format!("/opportunities/classic?chars={BOB}"),
    ] {
        let html = unescaped(get(&app, &browser, &path).await);
        assert!(
            html.contains(&format!(r#"href="/opportunities/expansion?chars={BOB}""#)),
            "{path} does not carry the selection to the page"
        );
        assert!(
            html.contains(&format!(r#"href="/orders/sell?chars={BOB}""#)),
            "{path}"
        );
    }
}

// ---- the explainer card ----

/// The explainer card of a page.
fn explainer(html: &str) -> &str {
    let start = html
        .find(r#"<article class="explainer""#)
        .expect("an explainer card");
    let rest = &html[start..];
    &rest[..rest.find("</article>").unwrap() + "</article>".len()]
}

/// The cells of the first legend table in `html`: the header row, then one row per entry with
/// the label (the row header), the name, the meaning and the formula cell's inner HTML.
fn legend_rows(html: &str) -> Vec<Vec<String>> {
    let start = html
        .find(r#"<table class="explainer-table legend">"#)
        .expect("a legend table");
    let table = &html[start..start + html[start..].find("</table>").unwrap()];
    assert!(!table.contains(r#"class="data""#));
    table
        .split("<tr>")
        .skip(1)
        .map(|row| {
            let row = &row[..row.find("</tr>").unwrap()];
            row.split("<t")
                .skip(1)
                .map(|cell| {
                    let inner = cell.split_once('>').unwrap().1;
                    inner[..inner.find("</t").unwrap()].trim().to_owned()
                })
                .collect()
        })
        .collect()
}

/// The labels of a legend's entries, in order.
fn legend_labels(html: &str) -> Vec<String> {
    let rows = legend_rows(html);
    assert_eq!(rows[0], ["Label", "Name", "Meaning", "Formula"]);
    rows[1..].iter().map(|row| row[0].clone()).collect()
}

#[tokio::test]
async fn every_table_page_has_a_closed_explainer_between_the_pagination_and_the_freshness() {
    let (app, browser) = setup().await;

    for table in ["expansion", "classic", "reversion", "trend"] {
        let html = get(&app, &browser, &format!("/opportunities/{table}")).await;
        let at = |needle: &str| {
            html.find(needle)
                .unwrap_or_else(|| panic!("{needle} missing in {table}"))
        };
        let (section, card, footer) = (
            at(&format!(r#"<section id="{table}""#)),
            at(r#"<article class="explainer""#),
            at(r#"id="freshness""#),
        );
        assert!(section < card && card < footer, "{table}");
        let card = explainer(&html);
        assert!(card.contains("<details>"), "{table}: closed on load");
        assert!(
            card.contains("<summary>How to read this table</summary>"),
            "{table}"
        );
        assert!(!card.contains(r#"class="data""#), "{table}: no data table");

        // Outside the swapped content: the content fragment does not carry it.
        let content = get(&app, &browser, &format!("/opportunities/{table}/content")).await;
        assert!(!content.contains("explainer"), "{table}");
    }
}

#[tokio::test]
async fn the_explainer_legend_lists_the_table_columns_only() {
    let (app, browser) = setup().await;

    let html = get(&app, &browser, "/opportunities/reversion").await;

    assert_eq!(
        legend_labels(explainer(&html)),
        [
            "Item", "B", "S", "Net", "m", "ISK/d", "%B", "RSI", "V7", "V30", "\u{03C3}", "T_max",
            "T_fill", "V_min", "n b/s",
        ]
    );
    let classic = get(&app, &browser, "/opportunities/classic").await;
    let labels = legend_labels(explainer(&classic));
    assert!(
        !labels
            .iter()
            .any(|l| ["Window", "SMA5", "%B"].contains(&l.as_str()))
    );
}

#[tokio::test]
async fn the_classic_explainer_works_through_its_example_and_playbook() {
    let (app, browser) = setup().await;

    let html = get(&app, &browser, "/opportunities/classic").await;
    let card = explainer(&html);

    for text in [
        "11,490.00",
        "10,141.00",
        "7.88%",
        "9,860",
        "39,440",
        "5.33M",
        "10,010.00",
        "7.77%",
    ] {
        assert!(card.contains(text), "{text} missing: {card}");
    }
    let parts = [
        "Legend",
        "Signal",
        "Shared figures",
        "Worked example",
        "Order playbook",
    ]
    .map(|title| {
        card.find(&format!("<h3>{title}</h3>"))
            .unwrap_or_else(|| panic!("{title} missing"))
    });
    assert!(parts.windows(2).all(|w| w[0] < w[1]), "{parts:?}");
}

/// The worked example's own strings of each table.
const EXAMPLES: [(&str, &[&str]); 4] = [
    (
        "expansion",
        &["12,520.00", "17.55%", "11.87M", "daily 18:00"],
    ),
    ("classic", &["11,490.00", "7.88%", "5.33M", "7.77%"]),
    (
        "reversion",
        &[
            "9,000.00",
            "10,530.00",
            "9.85%",
            "2.70 days",
            "48,756",
            "5.99M",
            "9,001.00",
        ],
    ),
    ("trend", &["11,790.00", "10.70%", "7.23M", "10.59%"]),
];

#[tokio::test]
async fn each_explainer_shows_its_own_example_and_none_of_the_others() {
    let (app, browser) = setup().await;

    for (table, own) in EXAMPLES {
        let html = get(&app, &browser, &format!("/opportunities/{table}")).await;
        let card = explainer(&html);
        for text in own {
            assert!(card.contains(text), "{table}: {text} missing");
        }
        for (other, theirs) in EXAMPLES {
            if other != table {
                for text in theirs {
                    assert!(!card.contains(text), "{table} shows {other}'s {text}");
                }
            }
        }
    }
}

#[tokio::test]
async fn the_shared_figures_are_worded_the_same_in_every_explainer() {
    let (app, browser) = setup().await;

    let mut shared = Vec::new();
    for table in ["expansion", "classic", "reversion", "trend"] {
        let html = get(&app, &browser, &format!("/opportunities/{table}")).await;
        let card = explainer(&html);
        let start = card
            .find("<h3>Shared figures</h3>")
            .expect("shared figures");
        let end = card
            .find("<h3>Worked example</h3>")
            .expect("a worked example");
        shared.push(card[start..end].to_owned());
    }
    assert!(shared.windows(2).all(|w| w[0] == w[1]), "{shared:#?}");
}

#[tokio::test]
async fn an_empty_table_shows_no_opportunities_and_then_its_explainer() {
    let (app, browser) = setup().await;

    let html = get(&app, &browser, "/opportunities/trend").await;

    let empty = html
        .find("<p>No opportunities.</p>")
        .expect("an empty table");
    let card = html.find(r#"<article class="explainer""#).unwrap();
    assert!(empty < card);
    assert!(explainer(&html).contains("11,790.00"));
}

#[tokio::test]
async fn the_expansion_signal_covers_the_expansion_the_quiet_rule_both_directions_and_the_prices() {
    let (app, browser) = setup().await;

    let html = get(&app, &browser, "/opportunities/expansion").await;
    let card = explainer(&html);
    let signal =
        &card[card.find("<h3>Signal</h3>").unwrap()..card.find("<h3>Shared figures</h3>").unwrap()];

    for text in [
        "at least twice the median",
        "UTC day",
        "dip",
        "spike",
        "more than 10% of its hours",
        "25th percentile",
        "75th percentile",
        "7 days",
        "1.20, 1.22, 1.25, 1.26, 1.28, 1.30, 1.31, 1.33, 1.35 and 1.40",
        "1.2525",
        "12,525.00",
        "12,520.00",
    ] {
        assert!(signal.contains(text), "{text} missing: {signal}");
    }
    assert!(
        !signal.contains("1.5 times"),
        "the old rule is gone: {signal}"
    );
    assert!(!signal.contains("fewer days"), "{signal}");
}

#[tokio::test]
async fn the_expansion_example_is_a_spike_and_its_playbook_has_both_directions() {
    let (app, browser) = setup().await;

    let html = get(&app, &browser, "/opportunities/expansion").await;
    let card = explainer(&html);
    let example = &card[card.find("<h3>Worked example</h3>").unwrap()
        ..card.find("<h3>Order playbook</h3>").unwrap()];
    let playbook = &card[card.find("<h3>Order playbook</h3>").unwrap()..];

    assert!(
        example.contains(r#"<th scope="row">Dir</th><td>Spike"#),
        "{example}"
    );
    for text in ["12,520.00", "17.55%", "11.87M"] {
        assert!(example.contains(text), "{text} missing");
    }
    assert!(
        playbook.contains("unless the table\u{2019}s own steps say otherwise"),
        "{playbook}"
    );
    assert!(playbook.contains("For a spike row"), "{playbook}");
    assert!(playbook.contains("For a dip row"), "{playbook}");
    assert!(
        playbook.contains("at B itself, not one tick above"),
        "{playbook}"
    );
    assert!(
        playbook.contains("cancel it when the window ends unfilled"),
        "{playbook}"
    );
}
