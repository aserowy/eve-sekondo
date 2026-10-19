mod common;

use common::{AppBuilder, Browser, TestApp};
use sqlx::PgPool;

const ALICE: i64 = 90000001;

async fn setup(pool: PgPool) -> (TestApp, Browser) {
    let app = AppBuilder::new(pool).with_stub().spawn().await;
    let browser = app.browser();
    browser.sso_round_trip(&app, "/auth/login", ALICE).await;
    (app, browser)
}

async fn item(pool: &PgPool, type_id: i64, name: &str, best_sell: &str, volume: f64) {
    sqlx::query(
        "INSERT INTO station_item_stats
             (region_id, station_id, type_id, book_at, best_buy, best_sell, buy_near_orders,
              sell_near_orders, rho, indicators_at, volume_7d, volume_30d, sigma, returns)
         VALUES (10000002, 60003760, $1, now(), 100, $2::numeric, 1, 1, 0.5, now(), $3, $3, 0.02, 29)",
    )
    .bind(type_id)
    .bind(best_sell)
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

#[sqlx::test]
async fn an_unauthenticated_request_is_redirected_to_the_landing_page(pool: PgPool) {
    let app = AppBuilder::new(pool).with_stub().spawn().await;

    let res = app
        .browser()
        .get(&app.url("/opportunities/detail/classic/34"))
        .await;

    assert!(res.status().is_redirection());
    assert_eq!(Browser::location(&res), "/");
}

#[sqlx::test]
async fn an_unknown_table_is_not_found(pool: PgPool) {
    let (app, browser) = setup(pool).await;

    let res = detail(&app, &browser, "/opportunities/detail/bogus/34").await;

    assert_eq!(res.status(), 404);
}

#[sqlx::test]
async fn the_page_shows_the_fees_the_prices_and_how_the_figures_follow(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    item(&pool, 34, "Tritanium", "130.00", 100_000_000.0).await;

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

#[sqlx::test]
async fn the_page_names_the_character_whose_fees_are_used(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    item(&pool, 34, "Tritanium", "130.00", 100_000_000.0).await;
    for kind in ["skills", "standings"] {
        sqlx::query("INSERT INTO fee_sync (character_id, kind, next_poll_at, last_success_at) VALUES ($1, $2, now(), now())")
            .bind(ALICE)
            .bind(kind)
            .execute(&pool)
            .await
            .unwrap();
    }
    for skill in [16622_i64, 3446] {
        sqlx::query("INSERT INTO character_skills (character_id, skill_id, active_level) VALUES ($1, $2, 5)")
            .bind(ALICE)
            .bind(skill)
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO station_owners (station_id, owner_id, resolved_at) VALUES (60003760, 1000035, now())")
        .execute(&pool)
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

#[sqlx::test]
async fn an_item_that_dropped_out_is_still_shown_and_the_page_says_so(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    // Nothing trades any more: the item is in no table, but its figures are known.
    item(&pool, 34, "Tritanium", "130.00", 0.0).await;

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

#[sqlx::test]
async fn an_item_the_station_no_longer_trades_gets_a_page_that_says_so(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    sqlx::query("INSERT INTO names (id, category, name, resolved_at) VALUES (34, 'inventory_type', 'Tritanium', now())")
        .execute(&pool)
        .await
        .unwrap();

    let res = detail(&app, &browser, "/opportunities/detail/trend/34").await;

    assert_eq!(res.status(), 200);
    let html = res.text().await.unwrap();
    assert!(
        html.contains("Tritanium") && html.contains("No longer listed"),
        "{html}"
    );
}

#[sqlx::test]
async fn with_many_matches_every_item_is_listed_and_none_is_cut_off_by_a_row_limit(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    for i in 1..=60i64 {
        item(
            &pool,
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

#[sqlx::test]
async fn the_opportunities_page_has_no_floating_window_and_its_rows_open_pages(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    item(&pool, 34, "Tritanium", "130.00", 100_000_000.0).await;

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

#[sqlx::test]
async fn rows_carry_the_selection_and_sorting_to_the_detail_page(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    browser.sso_round_trip(&app, "/auth/add", 90000002).await;
    item(&pool, 34, "Tritanium", "130.00", 100_000_000.0).await;

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

#[sqlx::test]
async fn the_detail_page_redirects_a_signed_out_request_to_the_landing_page(pool: PgPool) {
    let app = AppBuilder::new(pool).with_stub().spawn().await;

    let res = app
        .browser()
        .get(&app.url("/opportunities/classic/34"))
        .await;

    assert!(res.status().is_redirection());
    assert_eq!(Browser::location(&res), "/");
}

#[sqlx::test]
async fn the_detail_page_of_an_unknown_table_is_not_found(pool: PgPool) {
    let (app, browser) = setup(pool).await;

    let res = detail(&app, &browser, "/opportunities/bogus/34").await;

    assert_eq!(res.status(), 404);
}

#[sqlx::test]
async fn the_detail_page_is_a_full_page_with_the_filter_box_the_sub_menu_and_the_figures(
    pool: PgPool,
) {
    let (app, browser) = setup(pool.clone()).await;
    browser.sso_round_trip(&app, "/auth/add", 90000002).await;
    item(&pool, 34, "Tritanium", "130.00", 100_000_000.0).await;

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

#[sqlx::test]
async fn the_detail_fragment_brings_the_sub_menu_and_the_context_text_out_of_band(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    item(&pool, 34, "Tritanium", "130.00", 100_000_000.0).await;

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
async fn history(pool: &PgPool, type_id: i64) {
    for back in 1..=25i32 {
        let average = if back == 1 { 119.4 } else { 152.0 };
        sqlx::query(
            "INSERT INTO market_history (region_id, type_id, day, average, highest, lowest, volume, order_count)
             VALUES (10000002, $1, current_date - $2, $3::float8::numeric, $3::float8::numeric + 1, $3::float8::numeric - 1, $4, 10)",
        )
        .bind(type_id)
        .bind(back)
        .bind(average)
        .bind(1000 + i64::from(back))
        .execute(pool)
        .await
        .unwrap();
    }
}

#[sqlx::test]
async fn the_mean_reversion_window_draws_the_average_with_its_bands_and_the_rsi(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    item(&pool, 34, "Tritanium", "130.00", 100_000_000.0).await;
    history(&pool, 34).await;

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

#[sqlx::test]
async fn the_trend_window_draws_the_averages_and_the_daily_volume(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    item(&pool, 34, "Tritanium", "130.00", 100_000_000.0).await;
    history(&pool, 34).await;

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

#[sqlx::test]
async fn the_classic_window_draws_prices_the_spread_and_the_depth_of_the_book(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    item(&pool, 34, "Tritanium", "130.00", 100_000_000.0).await;
    for (hours_back, sell) in [(5, 110.0_f64), (4, 120.0), (3, 130.0)] {
        sqlx::query(
            "INSERT INTO station_snapshots (recorded_at, region_id, station_id, type_id, best_buy,
                 best_sell, spread, buy_near_units, buy_near_orders, sell_near_units,
                 sell_near_orders, buy_units, sell_units, buy_taken, sell_taken)
             VALUES (date_trunc('hour', now()) - make_interval(hours => $1) + interval '5 minutes',
                     10000002, 60003760, 34, 100, $2::float8::numeric,
                     ($2::float8::numeric - 100) / 100, 800, 2, 200, 1, 900, 300, 0, 0)",
        )
        .bind(hours_back)
        .bind(sell)
        .execute(&pool)
        .await
        .unwrap();
    }
    for view in ["station_hourly", "station_daily"] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "CALL refresh_continuous_aggregate('{view}', now() - interval '40 days', now() + interval '2 hours')"
        )))
        .execute(&pool)
        .await
        .unwrap();
    }

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

#[sqlx::test]
async fn the_expansion_window_draws_the_spreads_the_past_peaks_and_the_next_occurrences(
    pool: PgPool,
) {
    let (app, browser) = setup(pool.clone()).await;
    item(&pool, 34, "Tritanium", "110.00", 100_000_000.0).await;
    // A daily window at 18:00 UTC, as the statistics hold it.
    sqlx::query(
        "UPDATE station_item_stats SET expansion_kind = 'hour', expansion_hour = 18, expansion_sell = 125.20",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Ten of the last fourteen days, the best sell peaked at 18:30 at 134 + the days back.
    for back in 1..=14i32 {
        for hour in 0..24i32 {
            let sell: f64 = if hour == 18 && back <= 10 {
                134.0 + f64::from(back)
            } else {
                105.0
            };
            sqlx::query(
                "INSERT INTO station_snapshots (recorded_at, region_id, station_id, type_id, best_buy,
                     best_sell, spread, buy_near_units, buy_near_orders, sell_near_units,
                     sell_near_orders, buy_units, sell_units, buy_taken, sell_taken)
                 VALUES (date_trunc('day', now()) - make_interval(days => $1) + make_interval(hours => $2, mins => 30),
                         10000002, 60003760, 34, 100, $3::float8::numeric,
                         ($3::float8::numeric - 100) / 100, 1, 1, 1, 1, 1, 1, 0, 0)",
            )
            .bind(back)
            .bind(hour)
            .bind(sell)
            .execute(&pool)
            .await
            .unwrap();
        }
    }
    for view in ["station_hourly", "station_daily"] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "CALL refresh_continuous_aggregate('{view}', now() - interval '40 days', now() + interval '2 hours')"
        )))
        .execute(&pool)
        .await
        .unwrap();
    }
    history(&pool, 34).await;

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

#[sqlx::test]
async fn a_window_without_chart_data_draws_no_charts_and_does_not_fail(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    item(&pool, 34, "Tritanium", "130.00", 100_000_000.0).await;

    for table in ["expansion", "classic", "reversion", "trend"] {
        let res = detail(&app, &browser, &format!("/opportunities/detail/{table}/34")).await;
        assert_eq!(res.status(), 200, "{table}");
        let html = res.text().await.unwrap();
        assert!(!html.contains("<canvas"), "{table}: {html}");
    }
}
