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

/// The card of the figure `key` of a detail fragment.
fn card<'a>(html: &'a str, key: &str) -> &'a str {
    let start = html
        .find(&format!(r#"data-figure="{key}""#))
        .unwrap_or_else(|| panic!("no {key} figure in {html}"));
    let card = &html[start..];
    &card[..card.find("</article>").unwrap()]
}

/// The value of the figure `key` of a detail fragment.
fn figure(html: &str, key: &str) -> String {
    let card = card(html, key);
    let value = card
        .split_once("data-value")
        .unwrap_or_else(|| panic!("no value in {card}"))
        .1;
    let value = value.split_once('>').unwrap().1;
    value.split("</p>").next().unwrap().trim().to_owned()
}

/// The detail line of the figure `key`, if it has one.
fn detail_line(html: &str, key: &str) -> Option<String> {
    let card = card(html, key);
    let (_, rest) = card.split_once(r#"class="detail""#)?;
    let line = rest.split_once('>').unwrap().1;
    Some(line.split("</small>").next().unwrap().trim().to_owned())
}

/// The titles of the figure groups, in order.
fn groups(html: &str) -> Vec<String> {
    let figures = &html[html.find(r#"id="figures""#).unwrap()..];
    let figures = &figures[..figures.find("</section>").unwrap()];
    figures
        .split("<h3>")
        .skip(1)
        .map(|h| h[..h.find("</h3>").unwrap()].replace("&#38;", "&"))
        .collect()
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
async fn the_page_shows_the_fees_the_prices_and_the_figures() {
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
        html.contains("Base rates"),
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
    // The fees are figures of their own.
    assert_eq!(figure(&html, "broker"), "3.00%");
    assert_eq!(figure(&html, "tax"), "7.50%");
}

#[tokio::test]
async fn the_figures_are_grouped_cards_with_this_items_numbers() {
    let (app, browser) = setup().await;
    item(&app, 34, "Tritanium", "130.00", 100_000_000.0).await;

    let html = detail(&app, &browser, "/opportunities/detail/classic/34")
        .await
        .text()
        .await
        .unwrap();

    assert_eq!(
        groups(&html),
        [
            "Fees",
            "Prices",
            "Margin",
            "Size & liquidity",
            "Timing & risk",
            "Result"
        ]
    );
    assert!(!html.contains("table class=\"data figures\""), "{html}");
    // Base rates: b = 3%, t = 7.5%.
    assert!(
        card(&html, "cost").contains(r#"<abbr title="Cost per unit">c</abbr>"#),
        "{html}"
    );
    assert_eq!(
        detail_line(&html, "cost").unwrap(),
        "100.00 \u{00D7} 1.0300"
    );
    assert_eq!(
        detail_line(&html, "revenue").unwrap(),
        "129.90 \u{00D7} 0.89500"
    );
    assert_eq!(detail_line(&html, "net").unwrap(), "116.26 \u{2212} 103.00");
    assert_eq!(detail_line(&html, "margin").unwrap(), "13.26 / 103.00");
    assert_eq!(detail_line(&html, "sell").unwrap(), "1 tick below 130.00");
    assert_eq!(
        detail_line(&html, "t-max").unwrap(),
        "min(3, (12.87% / 4.00%)\u{00B2})"
    );
    assert!(
        detail_line(&html, "q-buy")
            .unwrap()
            .starts_with("1/(1 + 1) \u{00D7} ")
    );
    assert!(
        detail_line(&html, "isk-day")
            .unwrap()
            .starts_with("13.26 \u{00D7} 970,873 / ")
    );
    for key in [
        "broker",
        "tax",
        "buy",
        "best-sell",
        "v7",
        "v30",
        "rho",
        "sigma",
    ] {
        assert_eq!(detail_line(&html, key), None, "{key} has a detail line");
    }
    // The formulas are in the legend, not on the cards.
    let figures = &html[html.find(r#"id="figures""#).unwrap()..];
    let figures = &figures[..figures.find("</section>").unwrap()];
    assert!(!figures.contains("B \u{00D7} (1 + b)"), "{figures}");
    assert!(!figures.contains("(r \u{2212} c) / c"), "{figures}");
}

#[tokio::test]
async fn the_page_checks_the_character_whose_fees_are_used() {
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
        html.contains(r#"name="fee_character" value="90000001" checked"#)
            && html.contains("Broker fee 1.50%"),
        "{html}"
    );
    assert_eq!(figure(&html, "cost"), "101.50");
    assert_eq!(figure(&html, "broker"), "1.50%");
    assert_eq!(
        detail_line(&html, "cost").unwrap(),
        "100.00 \u{00D7} 1.0150"
    );
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
    assert!(html.contains(r#"<form id="fee-filter""#) && html.contains("Base rates"));
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
async fn the_detail_fragment_brings_the_sub_menu_and_the_fees_group_out_of_band() {
    let (app, browser) = setup().await;
    item(&app, 34, "Tritanium", "130.00", 100_000_000.0).await;

    let html = detail(&app, &browser, "/opportunities/detail/classic/34")
        .await
        .text()
        .await
        .unwrap();

    assert!(!html.contains("<html"), "a fragment");
    assert!(
        html.contains(r#"id="section-nav""#) && html.contains(r#"<form id="fee-filter""#),
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

/// Computes the expansion windows from the rollups, as the hourly refresh does.
async fn expansions(app: &TestApp) {
    sekondo::opportunities::stats::refresh_expansions_if_advanced(
        &*app.stores.station,
        OffsetDateTime::now_utc(),
    )
    .await
    .unwrap();
}

/// Item 34 on the last 14 days, one snapshot at :30 of every hour: the best buy at 100.00 and the
/// best sell at 105.00, except for the hours `jump` returns a `(best buy, best sell)` for.
async fn hourly_days(app: &TestApp, jump: impl Fn(i64, i64) -> Option<(f64, f64)>) {
    let midnight = OffsetDateTime::now_utc().replace_time(time::Time::MIDNIGHT);
    let mut rows = Vec::new();
    for back in 1..=14i64 {
        for hour in 0..24i64 {
            let (buy, sell) = jump(back, hour).unwrap_or((100.0, 105.0));
            rows.push(SeedSnapshotRow {
                best_buy: buy,
                ..snapshot(
                    midnight - Duration::days(back) + Duration::hours(hour) + Duration::minutes(30),
                    sell,
                    (1, 1, 1, 1, 1, 1),
                )
            });
        }
    }
    app.stores
        .station
        .seed_snapshots(10000002, 60003760, &rows)
        .await
        .unwrap();
    roll_up(app).await;
    expansions(app).await;
}

#[tokio::test]
async fn the_expansion_page_draws_the_spread_by_hour_and_weekday_the_past_peaks_and_lists_the_windows()
 {
    let (app, browser) = setup().await;
    item(&app, 34, "Tritanium", "110.00", 100_000_000.0).await;
    // On ten of the last fourteen days the best sell peaked at 18:30 at 134 + the days back.
    hourly_days(&app, |back, hour| {
        (hour == 18 && back <= 10).then(|| (100.0, 134.0 + back as f64))
    })
    .await;
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
    // Yesterday is not complete before 01:00 today, so nine peaks count: 136 to 144 ISK, whose
    // 25th percentile is 138.00, and S is one tick below it.
    let peaks = chart(&html, "peaks");
    assert_eq!(
        series_values(&peaks, 0).len(),
        9,
        "one peak per expanded day"
    );
    assert_eq!(peaks["series"][0]["name"], "Peak");
    assert!(
        series_values(&peaks, 1)
            .iter()
            .all(|s| s == &serde_json::json!(13_790.0)),
        "S marked: {peaks}"
    );
    let section = &html[html
        .find(r#"<section id="windows">"#)
        .expect("a windows section")..];
    assert!(section.contains("daily 18:00"), "{section}");
    assert!(section.contains("Spike"), "{section}");
    assert!(
        section.contains("9/13"),
        "occurrences against covered days: {section}"
    );
    assert!(section.contains(r#"class="chosen""#), "{section}");
    assert_eq!(
        section_nav(&html),
        [
            "\u{2190} Recurring spread expansion",
            "Figures",
            "Legend",
            "Spread by hour of day (EVE time)",
            "Spread by weekday",
            "Past peaks of the window, with S",
            "Recurring windows",
        ]
    );
}

#[tokio::test]
async fn the_expansion_page_lists_both_windows_of_an_item_marks_the_dip_and_draws_its_troughs() {
    let (app, browser) = setup().await;
    item(&app, 34, "Tritanium", "110.00", 100_000_000.0).await;
    // Every day: a spike at 18:30 (best sell 160.00) and a dip at 11:30 (best buy 60.00).
    hourly_days(&app, |_, hour| match hour {
        18 => Some((100.0, 160.0)),
        11 => Some((60.0, 105.0)),
        _ => None,
    })
    .await;
    history(&app, 34).await;

    let html = detail(&app, &browser, "/opportunities/detail/expansion/34")
        .await
        .text()
        .await
        .unwrap();

    let section = &html[html
        .find(r#"<section id="windows">"#)
        .expect("a windows section")..];
    assert!(section.contains("daily 11:00") && section.contains("daily 18:00"));
    assert!(section.contains("Dip") && section.contains("Spike"));
    // Buying at 60.01 beats buying at 100.00, so the dip is the row's window and the only marked one.
    assert_eq!(section.matches(r#"class="chosen""#).count(), 1, "{section}");
    let chosen = &section[section.find(r#"class="chosen""#).unwrap()..];
    assert!(chosen[..chosen.find("</tr>").unwrap()].contains("daily 11:00"));
    // The troughs, with B marked over them.
    let troughs = chart(&html, "peaks");
    assert_eq!(troughs["series"][0]["name"], "Trough");
    assert_eq!(troughs["series"][1]["name"], "B");
    assert!(
        series_values(&troughs, 0)
            .iter()
            .all(|t| t == &serde_json::json!(6_000.0)),
        "{troughs}"
    );
    assert!(
        series_values(&troughs, 1)
            .iter()
            .all(|b| b == &serde_json::json!(6_001.0)),
        "{troughs}"
    );
    assert!(
        html.contains("Past troughs of the window, with B"),
        "{html}"
    );
    // B is the predicted buy price, so the current best buy has its own card.
    assert_eq!(figure(&html, "buy"), "60.01");
    assert_eq!(figure(&html, "best-buy"), "100.00");
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
async fn the_legend_is_a_table_of_label_name_meaning_and_formula() {
    let (app, browser) = setup().await;
    item(&app, 34, "Tritanium", "130.00", 100_000_000.0).await;

    let html = detail(&app, &browser, "/opportunities/detail/classic/34")
        .await
        .text()
        .await
        .unwrap();
    let card = &html[html.find(r#"<article id="legend""#).expect("a legend card")..];

    let rows = legend_rows(card);
    let row = |label: &str| {
        rows.iter()
            .find(|row| row[0] == label)
            .unwrap_or_else(|| panic!("no row {label}: {rows:?}"))
            .clone()
    };
    let c = row("c");
    assert_eq!(c.len(), 4);
    assert_eq!(c[1], "Cost per unit");
    assert_eq!(c[2], "What one unit costs, broker fee included.");
    assert_eq!(c[3], "<code>B \u{00D7} (1 + b)</code>");
    assert_eq!(
        row("T_max")[3],
        "<code>min(3, (m / 2\u{03C3})\u{00B2}) days</code>"
    );
    let b = row("B");
    assert_eq!(
        b[1],
        "Current best buy, not a suggested price, except in a dip row of the recurring spread \
         expansion table, where it is the predicted buy price"
    );
    assert_eq!(b[3], "", "no formula, an empty cell");
}

/// The link texts of the detail page's sub-menu.
fn section_nav(html: &str) -> Vec<String> {
    let nav = &html[html.find(r#"id="section-nav""#).expect("a sub-menu")..];
    let nav = &nav[..nav.find("</nav>").unwrap()];
    nav.split("<a ")
        .skip(1)
        .map(|a| {
            let text = a.split_once('>').unwrap().1;
            text[..text.find("</a>").unwrap()]
                .replace("&larr;", "\u{2190}")
                .trim()
                .to_owned()
        })
        .collect()
}

#[tokio::test]
async fn a_closed_legend_card_sits_between_the_figures_and_the_charts() {
    let (app, browser) = setup().await;
    item(&app, 34, "Tritanium", "130.00", 100_000_000.0).await;

    let html = detail(&app, &browser, "/opportunities/detail/trend/34")
        .await
        .text()
        .await
        .unwrap();

    let card_at = html
        .find(r#"<article id="legend" hx-preserve="true""#)
        .unwrap_or_else(|| panic!("no legend card: {html}"));
    assert!(html.find(r#"id="figures""#).unwrap() < card_at);
    if let Some(chart) = html.find(r#"class="graph chart-row""#) {
        assert!(card_at < chart);
    }
    let card = &html[card_at..];
    let card = &card[..card.find("</article>").unwrap()];
    assert!(card.contains("<details>") && card.contains("<summary>Legend</summary>"));
    assert_eq!(
        legend_labels(card),
        [
            "b",
            "t",
            "B",
            "Best buy",
            "Best sell",
            "S",
            "c",
            "r",
            "Net",
            "m",
            "U",
            "V7",
            "V30",
            "\u{03C1}",
            "q_buy",
            "q_sell",
            "\u{03C3}",
            "T_fill",
            "T_max",
            "V_min",
            "ISK/d",
        ]
    );
    assert!(
        card.contains("min(3, (m / 2\u{03C3})\u{00B2}) days"),
        "{card}"
    );
    assert!(card.contains("Cost per unit"), "{card}");
    for part in ["Signal", "Worked example", "Order playbook"] {
        assert!(!card.contains(part), "{part} in the legend card");
    }
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

#[tokio::test]
async fn the_detail_page_names_the_maximum_margin_when_it_hides_the_item() {
    let (app, browser) = setup().await;
    item(&app, 34, "Tritanium", "250.00", 100_000_000.0).await; // m 117%, above the default 80%

    let html = detail(&app, &browser, "/opportunities/detail/classic/34")
        .await
        .text()
        .await
        .unwrap();

    assert!(html.contains("No longer listed"), "{html}");
    assert!(
        html.contains("Its net margin is above your maximum margin."),
        "{html}"
    );
}
