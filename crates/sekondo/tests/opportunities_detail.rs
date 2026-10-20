mod common;

use common::{AppBuilder, Browser, TestApp};
use sekondo::store::{SeedHistoryRow, SeedSnapshotRow};
use time::{Duration, OffsetDateTime};

const ALICE: i64 = 90000001;

async fn setup() -> (TestApp, Browser) {
    let app = AppBuilder::new().with_stub().spawn().await;
    let browser = app.browser();
    browser.sso_round_trip(&app, "/auth/login", ALICE).await;
    (app, browser)
}

async fn item(app: &TestApp, type_id: i64, name: &str, best_sell: &str, volume: f64) {
    let now = OffsetDateTime::now_utc();
    app.stores
        .station
        .seed_item_stats(
            10000002,
            60003760,
            type_id,
            &sekondo::store::ItemStatsRecord {
                book_at: Some(now),
                best_buy: Some("100".to_owned()),
                best_sell: Some(best_sell.to_owned()),
                buy_near_orders: Some(1),
                sell_near_orders: Some(1),
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
    name_of(app, type_id, name).await;
}

async fn name_of(app: &TestApp, type_id: i64, name: &str) {
    app.stores
        .market
        .store_names(
            &[sekondo::store::NameRow {
                id: type_id,
                category: "inventory_type".to_owned(),
                name: Some(name.to_owned()),
            }],
            OffsetDateTime::now_utc(),
            None,
        )
        .await
        .unwrap();
}

/// A snapshot of item 34 with the best sell at `sell` and the depth the charts read.
fn snapshot(
    recorded_at: OffsetDateTime,
    sell: f64,
    depth: (i64, i32, i64, i32, i64, i64),
) -> SeedSnapshotRow {
    SeedSnapshotRow {
        recorded_at,
        type_id: 34,
        best_buy: 100.0,
        best_sell: sell,
        buy_near_units: depth.0,
        buy_near_orders: depth.1,
        sell_near_units: depth.2,
        sell_near_orders: depth.3,
        buy_units: depth.4,
        sell_units: depth.5,
        buy_taken: 0,
        sell_taken: 0,
    }
}

/// The hourly and daily rollups over everything seeded so far.
async fn roll_up(app: &TestApp) {
    let now = OffsetDateTime::now_utc();
    app.stores
        .station
        .refresh_rollups(now - Duration::days(40), now + Duration::hours(2))
        .await
        .unwrap();
}

async fn detail(app: &TestApp, browser: &Browser, path: &str) -> reqwest::Response {
    browser.get(&app.url(path)).await
}

/// The text of the figure `key` of a detail fragment.
fn figure(html: &str, key: &str) -> String {
    let start = html
        .find(&format!(r#"data-figure="{key}""#))
        .unwrap_or_else(|| panic!("no {key} figure in {html}"));
    let row = &html[start..];
    let row = &row[..row.find("</tr>").unwrap()];
    let value = row
        .split_once(r#"data-value"#)
        .unwrap_or_else(|| panic!("no value cell in {row}"))
        .1;
    let value = value.split_once('>').unwrap().1;
    value.split("</td>").next().unwrap().trim().to_owned()
}

#[tokio::test]
async fn an_unauthenticated_request_is_redirected_to_the_landing_page() {
    let app = AppBuilder::new().with_stub().spawn().await;

    let res = app
        .browser()
        .get(&app.url("/opportunities/detail/classic/34"))
        .await;

    assert!(res.status().is_redirection());
    assert_eq!(Browser::location(&res), "/");
}

#[tokio::test]
async fn an_unknown_table_is_not_found() {
    let (app, browser) = setup().await;

    let res = detail(&app, &browser, "/opportunities/detail/bogus/34").await;

    assert_eq!(res.status(), 404);
}

#[tokio::test]
async fn the_page_shows_the_fees_the_prices_and_how_the_figures_follow() {
    let (app, browser) = setup().await;
    item(&app, 34, "Tritanium", "130.00", 100_000_000.0).await;

    let res = detail(&app, &browser, "/opportunities/detail/classic/34").await;

    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();
    assert!(!html.contains("<html"), "a fragment");
    assert!(
        html.contains("Tritanium") && html.contains("Classic spread trade"),
        "{html}"
    );
    assert!(
        html.contains("base rates"),
        "the fees used are named: {html}"
    );
    assert!(!html.contains("No longer listed"));
    assert_eq!(figure(&html, "buy"), "100.00");
    assert_eq!(figure(&html, "sell"), "129.90");
    // Base rates: c = 103.00, r = 116.2605, m = 12.87%.
    assert_eq!(figure(&html, "cost"), "103.00");
    assert_eq!(figure(&html, "revenue"), "116.26");
    assert_eq!(figure(&html, "margin"), "12.87%");
    assert_eq!(figure(&html, "sigma"), "2.00%");
    assert_eq!(figure(&html, "t-max"), "3.0 d");
    assert!(!figure(&html, "t-fill").is_empty());
    assert_eq!(figure(&html, "units"), "970,873");
    // Each figure says how it was worked out.
    assert!(html.contains("B \u{00D7} (1 + b)"), "{html}");
    assert!(
        html.contains("S \u{00D7} (1 \u{2212} b \u{2212} t)"),
        "{html}"
    );
}

#[tokio::test]
async fn the_page_names_the_character_whose_fees_are_used() {
    let (app, browser) = setup().await;
    item(&app, 34, "Tritanium", "130.00", 100_000_000.0).await;
    let now = OffsetDateTime::now_utc();
    let fees = &app.stores.fees;
    fees.store_skills(ALICE, &[16622, 3446], &[5, 5], now, now, None)
        .await
        .unwrap();
    fees.store_standings(ALICE, &[], now, now, None)
        .await
        .unwrap();
    fees.insert_station_owner(60003760, 1000035, now)
        .await
        .unwrap();

    let html = detail(&app, &browser, "/opportunities/detail/classic/34")
        .await
        .text()
        .await
        .unwrap();

    assert!(
        html.contains("Alice Alpha") && html.contains("broker fee 1.50%"),
        "{html}"
    );
    assert_eq!(figure(&html, "cost"), "101.50");
}

#[tokio::test]
async fn an_item_that_dropped_out_is_still_shown_and_the_page_says_so() {
    let (app, browser) = setup().await;
    // Nothing trades any more: the item is in no table, but its figures are known.
    item(&app, 34, "Tritanium", "130.00", 0.0).await;

    let html = detail(&app, &browser, "/opportunities/detail/classic/34")
        .await
        .text()
        .await
        .unwrap();

    assert!(html.contains("No longer listed"), "{html}");
    assert_eq!(figure(&html, "buy"), "100.00");
    assert_eq!(figure(&html, "sell"), "129.90");
    assert_eq!(figure(&html, "sigma"), "2.00%");
}

#[tokio::test]
async fn an_item_the_station_no_longer_trades_gets_a_page_that_says_so() {
    let (app, browser) = setup().await;
    name_of(&app, 34, "Tritanium").await;

    let res = detail(&app, &browser, "/opportunities/detail/trend/34").await;

    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();
    assert!(
        html.contains("Tritanium") && html.contains("No longer listed"),
        "{html}"
    );
}

#[tokio::test]
async fn with_many_matches_every_item_is_listed_and_none_is_cut_off_by_a_row_limit() {
    let (app, browser) = setup().await;
    for i in 1..=60i64 {
        item(
            &app,
            1000 + i,
            &format!("Item {i}"),
            "130.00",
            10_000_000.0 * i as f64,
        )
        .await;
    }

    let weak = detail(&app, &browser, "/opportunities/detail/classic/1001")
        .await
        .text()
        .await
        .unwrap();
    let strong = detail(&app, &browser, "/opportunities/detail/classic/1060")
        .await
        .text()
        .await
        .unwrap();

    // 60 items pass the table's rules: the table pages through all of them.
    assert!(!weak.contains("No longer listed"), "{weak}");
    assert!(!strong.contains("No longer listed"));
}

#[tokio::test]
async fn the_opportunities_page_has_no_floating_window_and_its_rows_open_pages() {
    let (app, browser) = setup().await;
    item(&app, 34, "Tritanium", "130.00", 100_000_000.0).await;

    let page = browser
        .get(&app.url("/opportunities/classic"))
        .await
        .text()
        .await
        .unwrap();

    // No window floats over the page; the only <aside> is the sidebar.
    assert_eq!(page.matches("<aside").count(), 1, "{page}");
    assert!(!page.contains(r#"role="dialog""#), "{page}");
    // A row opens the detail page of its own table and item, and its item is a real link.
    assert!(
        page.contains(r#"data-href="/opportunities/classic/34""#),
        "{page}"
    );
    assert!(
        page.contains(r#"<a href="/opportunities/classic/34">Tritanium</a>"#),
        "{page}"
    );
}

#[tokio::test]
async fn rows_carry_the_selection_and_sorting_to_the_detail_page() {
    let (app, browser) = setup().await;
    browser.sso_round_trip(&app, "/auth/add", 90000002).await;
    item(&app, 34, "Tritanium", "130.00", 100_000_000.0).await;

    let page = browser
        .get(&app.url("/opportunities/classic?chars=90000002&cs=item&cd=asc"))
        .await
        .text()
        .await
        .unwrap()
        .replace("&#38;", "&")
        .replace("&amp;", "&");

    assert!(
        page.contains(r#"data-href="/opportunities/classic/34?chars=90000002&cs=item&cd=asc""#),
        "{page}"
    );
}

// ---- the detail page ----

#[tokio::test]
async fn the_detail_page_redirects_a_signed_out_request_to_the_landing_page() {
    let app = AppBuilder::new().with_stub().spawn().await;

    let res = app
        .browser()
        .get(&app.url("/opportunities/classic/34"))
        .await;

    assert!(res.status().is_redirection());
    assert_eq!(Browser::location(&res), "/");
}

#[tokio::test]
async fn the_detail_page_of_an_unknown_table_is_not_found() {
    let (app, browser) = setup().await;

    let res = detail(&app, &browser, "/opportunities/bogus/34").await;

    assert_eq!(res.status(), 404);
}

#[tokio::test]
async fn the_detail_page_is_a_full_page_with_the_filter_box_the_sub_menu_and_the_figures() {
    let (app, browser) = setup().await;
    browser.sso_round_trip(&app, "/auth/add", 90000002).await;
    item(&app, 34, "Tritanium", "130.00", 100_000_000.0).await;

    let html = detail(
        &app,
        &browser,
        "/opportunities/classic/34?chars=90000002&cs=item&cd=asc",
    )
    .await
    .text()
    .await
    .unwrap()
    .replace("&#38;", "&")
    .replace("&amp;", "&");

    assert!(html.contains("<html"), "a page");
    assert!(
        html.contains("<h1>Tritanium</h1>"),
        "the item names the page"
    );
    assert!(html.contains(r#"id="filter-box""#) && html.contains(r#"id="character-filter""#));
    assert!(html.contains(r#"id="filter-context""#) && html.contains("base rates"));
    // The sub-menu leads back with the selection and sorting, then to the rows of the page.
    assert!(
        html.contains(
            r#"<a href="/opportunities/classic?chars=90000002&cs=item&cd=asc">&larr; Classic spread trade</a>"#
        ),
        "{html}"
    );
    assert!(
        html.contains(r##"<a href="#figures">Figures</a>"##),
        "{html}"
    );
    assert!(html.contains(r#"id="figures""#) && html.contains("Tritanium"));
    // The navigation marks Opportunities as the current page.
    let current = html.split_once(r#"aria-current="page""#).unwrap().0;
    assert!(
        current
            .rsplit_once("<a ")
            .unwrap()
            .1
            .contains("/opportunities"),
        "{html}"
    );
    assert_eq!(figure(&html, "sell"), "129.90");
}

#[tokio::test]
async fn the_detail_fragment_brings_the_sub_menu_and_the_context_text_out_of_band() {
    let (app, browser) = setup().await;
    item(&app, 34, "Tritanium", "130.00", 100_000_000.0).await;

    let html = detail(&app, &browser, "/opportunities/detail/classic/34")
        .await
        .text()
        .await
        .unwrap();

    assert!(!html.contains("<html"), "a fragment");
    assert!(
        html.contains(r#"id="section-nav""#) && html.contains(r#"id="filter-context""#),
        "{html}"
    );
    assert_eq!(html.matches(r#"hx-swap-oob="true""#).count(), 2, "{html}");
}

// ---- the charts of the page ----

/// The JSON a canvas of this kind carries.
fn chart(html: &str, kind: &str) -> serde_json::Value {
    let tag = html
        .split_once(&format!(r#"data-chart="{kind}""#))
        .unwrap_or_else(|| panic!("no {kind} chart in {html}"))
        .1;
    let attr = tag
        .split_once(r#"data-series=""#)
        .unwrap()
        .1
        .split('"')
        .next()
        .unwrap();
    let json = attr
        .replace("&#34;", "\"")
        .replace("&#39;", "'")
        .replace("&#60;", "<")
        .replace("&#62;", ">")
        .replace("&#38;", "&");
    serde_json::from_str(&json).unwrap_or_else(|e| panic!("{e}: {json}"))
}

fn has_chart(html: &str, kind: &str) -> bool {
    html.contains(&format!(r#"data-chart="{kind}""#))
}

fn series_values(chart: &serde_json::Value, series: usize) -> Vec<serde_json::Value> {
    chart["series"][series]["values"]
        .as_array()
        .unwrap()
        .clone()
}

/// 25 days of history for an item: 19 days at 152 and a last day at 119.40, the volume rising.
async fn history(app: &TestApp, type_id: i64) {
    let today = OffsetDateTime::now_utc().date();
    let rows: Vec<_> = (1..=25i64)
        .map(|back| {
            let average = if back == 1 { 119.4 } else { 152.0 };
            SeedHistoryRow {
                type_id,
                day: today - Duration::days(back),
                average,
                highest: average + 1.0,
                lowest: average - 1.0,
                volume: 1000 + back,
                order_count: 10,
            }
        })
        .collect();
    app.stores
        .station
        .seed_history(10000002, &rows)
        .await
        .unwrap();
}

#[tokio::test]
async fn the_mean_reversion_window_draws_the_average_with_its_bands_and_the_rsi() {
    let (app, browser) = setup().await;
    item(&app, 34, "Tritanium", "130.00", 100_000_000.0).await;
    history(&app, 34).await;

    let html = detail(&app, &browser, "/opportunities/detail/reversion/34")
        .await
        .text()
        .await
        .unwrap();

    let price = chart(&html, "reversion-price");
    assert_eq!(price["series"].as_array().unwrap().len(), 4);
    assert_eq!(price["series"][1]["name"], "SMA 20");
    assert_eq!(price["labels"].as_array().unwrap().len(), 25);
    let last_sma = series_values(&price, 1)[24].as_f64().unwrap();
    // The last twenty days: 19 at 152 and 119.40 -> 150.37 ISK.
    assert!((last_sma - 15_037.0).abs() < 1e-6, "{last_sma}");
    assert_eq!(series_values(&price, 1)[0], serde_json::Value::Null);
    let rsi = chart(&html, "rsi");
    assert_eq!(rsi["series"][0]["name"], "RSI 14");
    assert!(
        series_values(&rsi, 0)[24].as_f64().unwrap() < 30.0,
        "a fall of 30 ISK in one day"
    );
}

#[tokio::test]
async fn the_trend_window_draws_the_averages_and_the_daily_volume() {
    let (app, browser) = setup().await;
    item(&app, 34, "Tritanium", "130.00", 100_000_000.0).await;
    history(&app, 34).await;

    let html = detail(&app, &browser, "/opportunities/detail/trend/34")
        .await
        .text()
        .await
        .unwrap();

    let price = chart(&html, "trend-price");
    assert_eq!(
        price["series"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["Average", "SMA 5", "SMA 20"]
    );
    let volume = chart(&html, "trend-volume");
    assert_eq!(series_values(&volume, 0).len(), 25);
    assert_eq!(series_values(&volume, 0)[24], 1001.0, "yesterday's volume");
    // Not the other tables' charts.
    assert!(!has_chart(&html, "rsi") && !has_chart(&html, "classic-prices"));
}

#[tokio::test]
async fn the_classic_window_draws_prices_the_spread_and_the_depth_of_the_book() {
    let (app, browser) = setup().await;
    item(&app, 34, "Tritanium", "130.00", 100_000_000.0).await;
    let hour = OffsetDateTime::now_utc()
        .replace_minute(0)
        .unwrap()
        .replace_second(0)
        .unwrap()
        .replace_nanosecond(0)
        .unwrap();
    let rows: Vec<_> = [(5, 110.0_f64), (4, 120.0), (3, 130.0)]
        .into_iter()
        .map(|(back, sell)| {
            snapshot(
                hour - Duration::hours(back) + Duration::minutes(5),
                sell,
                (800, 2, 200, 1, 900, 300),
            )
        })
        .collect();
    app.stores
        .station
        .seed_snapshots(10000002, 60003760, &rows)
        .await
        .unwrap();
    roll_up(&app).await;

    let html = detail(&app, &browser, "/opportunities/detail/classic/34")
        .await
        .text()
        .await
        .unwrap();

    let prices = chart(&html, "classic-prices");
    assert_eq!(series_values(&prices, 1), [11_000.0, 12_000.0, 13_000.0]);
    assert_eq!(series_values(&prices, 0), [10_000.0, 10_000.0, 10_000.0]);
    let spread = chart(&html, "classic-spread");
    assert_eq!(series_values(&spread, 0), [10.0, 20.0, 30.0]);
    // The latest snapshot: 800 buy units in 2 orders and 200 sell units in 1 order within 1%.
    let depth = chart(&html, "depth");
    assert_eq!(
        depth["labels"],
        serde_json::json!(["Buy (2 orders)", "Sell (1 order)"])
    );
    assert_eq!(series_values(&depth, 0), [800.0, 200.0]);
}

#[tokio::test]
async fn the_expansion_window_draws_the_spreads_the_past_peaks_and_the_next_occurrences() {
    let (app, browser) = setup().await;
    item(&app, 34, "Tritanium", "110.00", 100_000_000.0).await;
    let mut stats = app
        .stores
        .station
        .item_stats_row(10000002, 60003760, 34)
        .await
        .unwrap()
        .unwrap();
    stats.expansion_kind = Some("hour".to_owned());
    stats.expansion_hour = Some(18);
    stats.expansion_sell = Some("125.20".to_owned());
    app.stores
        .station
        .seed_item_stats(10000002, 60003760, 34, &stats)
        .await
        .unwrap();
    // Ten of the last fourteen days, the best sell peaked at 18:30 at 134 + the days back.
    let midnight = OffsetDateTime::now_utc().replace_time(time::Time::MIDNIGHT);
    let mut rows = Vec::new();
    for back in 1..=14i64 {
        for hour in 0..24i64 {
            let sell: f64 = if hour == 18 && back <= 10 {
                134.0 + back as f64
            } else {
                105.0
            };
            rows.push(snapshot(
                midnight - Duration::days(back) + Duration::hours(hour) + Duration::minutes(30),
                sell,
                (1, 1, 1, 1, 1, 1),
            ));
        }
    }
    app.stores
        .station
        .seed_snapshots(10000002, 60003760, &rows)
        .await
        .unwrap();
    roll_up(&app).await;
    history(&app, 34).await;

    let html = detail(&app, &browser, "/opportunities/detail/expansion/34")
        .await
        .text()
        .await
        .unwrap();

    let hours = chart(&html, "spread-hours");
    assert_eq!(hours["labels"].as_array().unwrap().len(), 24);
    let at_18 = series_values(&hours, 0)[18].as_f64().unwrap();
    let at_noon = series_values(&hours, 0)[12].as_f64().unwrap();
    assert!(
        at_18 > at_noon * 3.0,
        "the evening spread is wide: {at_18} against {at_noon}"
    );
    assert_eq!(
        chart(&html, "spread-weekdays")["labels"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
    let peaks = chart(&html, "peaks");
    assert_eq!(
        series_values(&peaks, 0).len(),
        10,
        "one peak per expanded day"
    );
    // The peaks are 135 to 144 ISK: their 25th percentile is 137.25, and S is one tick below it.
    assert!(
        series_values(&peaks, 1)
            .iter()
            .all(|s| s == &serde_json::json!(13_720.0)),
        "S marked: {peaks}"
    );
    assert!(html.contains("Next occurrences"), "{html}");
    assert!(html.contains(" 18:00"), "{html}");
}

#[tokio::test]
async fn a_window_without_chart_data_draws_no_charts_and_does_not_fail() {
    let (app, browser) = setup().await;
    item(&app, 34, "Tritanium", "130.00", 100_000_000.0).await;

    for table in ["expansion", "classic", "reversion", "trend"] {
        let res = detail(&app, &browser, &format!("/opportunities/detail/{table}/34")).await;
        assert_eq!(res.status(), 200, "{table}");
        let html = res.text().await.unwrap();
        assert!(!html.contains("<canvas"), "{table}: {html}");
    }
}
