mod common;

use common::{AppBuilder, Browser, TestApp};
use sqlx::PgPool;

const ALICE: i64 = 90000001;
const JITA_44: i64 = 60003760;

async fn setup(pool: PgPool) -> (TestApp, Browser) {
    let app = AppBuilder::new(pool).with_stub().spawn().await;
    let browser = app.browser();
    browser.sso_round_trip(&app, "/auth/login", ALICE).await;
    (app, browser)
}

async fn get(app: &TestApp, browser: &Browser, path: &str) -> String {
    let res = browser.get(&app.url(path)).await;
    assert_eq!(res.status(), 200, "{path}");
    res.text().await.unwrap()
}

/// The page with HTML-escaped ampersands turned back, so URLs can be compared as written.
fn unescaped(html: &str) -> String {
    html.replace("&#38;", "&").replace("&amp;", "&")
}

/// One item's statistics. Defaults describe an item that passes every rule: a calm price with a
/// large volume, no competitors within 1%, and history for 30 days.
struct Item {
    type_id: i64,
    name: Option<&'static str>,
    best_buy: &'static str,
    best_sell: &'static str,
    buy_near: i32,
    sell_near: i32,
    rho: f64,
    volume_7d: f64,
    volume_30d: f64,
    sigma: Option<f64>,
    cross_day: Option<&'static str>,
    volume_rising: Option<bool>,
    sma5: Option<f64>,
    sma20: Option<f64>,
    percent_b: Option<f64>,
    rsi14: Option<f64>,
    expansion_kind: Option<&'static str>,
    expansion_hour: Option<i16>,
    expansion_sell: Option<&'static str>,
}

impl Item {
    fn new(type_id: i64) -> Self {
        Self {
            type_id,
            name: Some("Tritanium"),
            best_buy: "100.00",
            best_sell: "130.00",
            buy_near: 0,
            sell_near: 0,
            rho: 0.5,
            volume_7d: 100_000_000.0,
            volume_30d: 90_000_000.0,
            sigma: Some(0.02),
            cross_day: None,
            volume_rising: None,
            sma5: None,
            sma20: None,
            percent_b: None,
            rsi14: None,
            expansion_kind: None,
            expansion_hour: None,
            expansion_sell: None,
        }
    }

    async fn insert(&self, pool: &PgPool) {
        sqlx::query(
            "INSERT INTO station_item_stats
                 (region_id, station_id, type_id, book_at, best_buy, best_sell, buy_near_orders,
                  sell_near_orders, rho, indicators_at, volume_7d, volume_30d, sigma, returns,
                  sma5, sma20, cross_day, volume_rising, percent_b, rsi14, expansion_at,
                  expansion_kind, expansion_hour, expansion_sell)
             VALUES (10000002, 60003760, $1, now(), $2::numeric, $3::numeric, $4, $5, $6, now(),
                     $7, $8, $9, 29, $10, $11, $12::date, $13, $14, $15, now(),
                     $16, $17, $18::numeric)",
        )
        .bind(self.type_id)
        .bind(self.best_buy)
        .bind(self.best_sell)
        .bind(self.buy_near)
        .bind(self.sell_near)
        .bind(self.rho)
        .bind(self.volume_7d)
        .bind(self.volume_30d)
        .bind(self.sigma)
        .bind(self.sma5)
        .bind(self.sma20)
        .bind(self.cross_day)
        .bind(self.volume_rising)
        .bind(self.percent_b)
        .bind(self.rsi14)
        .bind(self.expansion_kind)
        .bind(self.expansion_hour)
        .bind(self.expansion_sell)
        .execute(pool)
        .await
        .unwrap();
        if let Some(name) = self.name {
            sqlx::query(
                "INSERT INTO names (id, category, name, resolved_at) VALUES ($1, 'inventory_type', $2, now())
                 ON CONFLICT (id) DO NOTHING",
            )
            .bind(self.type_id)
            .bind(name)
            .execute(pool)
            .await
            .unwrap();
        }
    }
}

/// The table of one kind (`classic`, `trend`, ...) of the fragment.
fn table<'a>(html: &'a str, id: &str) -> &'a str {
    let start = html
        .find(&format!(r#"<section id="{id}""#))
        .unwrap_or_else(|| panic!("no {id} section"));
    let rest = &html[start..];
    &rest[..rest.find("</section>").unwrap_or(rest.len())]
}

/// The text of a cell of one row.
fn cell(section: &str, type_id: i64, col: &str) -> String {
    let row_start = section
        .find(&format!(r#"data-type-id="{type_id}""#))
        .unwrap_or_else(|| panic!("no row for {type_id} in {section}"));
    let row = &section[row_start..];
    let row = &row[..row.find("</tr>").unwrap()];
    let cell_start = row
        .find(&format!(r#"data-col="{col}""#))
        .unwrap_or_else(|| panic!("no {col} cell in {row}"));
    let cell = &row[cell_start..];
    let cell = cell.split_once('>').unwrap().1;
    let text = cell.split("</td>").next().unwrap();
    // Strip tags.
    let mut plain = String::new();
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => plain.push(c),
            _ => {}
        }
    }
    plain.trim().to_owned()
}

fn rows(section: &str) -> Vec<i64> {
    section
        .split(r#"data-type-id=""#)
        .skip(1)
        .map(|part| part.split('"').next().unwrap().parse().unwrap())
        .collect()
}

#[sqlx::test]
async fn a_classic_row_shows_the_current_best_buy_the_sell_price_and_the_shared_figures(
    pool: PgPool,
) {
    let (app, browser) = setup(pool.clone()).await;
    Item::new(34).insert(&pool).await;

    let html = get(&app, &browser, "/opportunities/tables").await;
    let classic = table(&html, "classic");

    assert_eq!(rows(classic), [34]);
    assert_eq!(cell(classic, 34, "item"), "Tritanium");
    assert_eq!(cell(classic, 34, "buy"), "100.00");
    // One tick below the best sell of 130.00.
    assert_eq!(cell(classic, 34, "sell"), "129.90");
    // Base rates: c = 103.00, r = 129.90 * (1 - 0.03 - 0.075) = 116.2605, net 13.2605 (m 12.87%).
    assert_eq!(cell(classic, 34, "net"), "13.26");
    assert_eq!(cell(classic, 34, "margin"), "12.87%");
    assert_eq!(cell(classic, 34, "v7"), "100,000,000");
    assert_eq!(cell(classic, 34, "v30"), "90,000,000");
    assert_eq!(cell(classic, 34, "sigma"), "2.00%");
    assert_eq!(cell(classic, 34, "near"), "0 / 0");
    assert!(
        cell(classic, 34, "isk-day").ends_with('m'),
        "{}",
        cell(classic, 34, "isk-day")
    );
    assert!(!cell(classic, 34, "t-max").is_empty());
    assert!(!cell(classic, 34, "t-fill").is_empty());
    assert!(!cell(classic, 34, "min-volume").is_empty());
}

#[sqlx::test]
async fn the_columns_say_that_the_buy_price_is_current_and_not_a_suggestion(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    Item::new(34).insert(&pool).await;

    let html = get(&app, &browser, "/opportunities/tables").await;

    assert!(html.contains("Current best buy"), "{html}");
    assert!(!html.to_lowercase().contains("suggested buy"));
}

#[sqlx::test]
async fn an_item_without_a_name_is_shown_as_its_type_id(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    Item {
        name: None,
        ..Item::new(12345)
    }
    .insert(&pool)
    .await;

    let html = get(&app, &browser, "/opportunities/tables").await;

    assert_eq!(cell(table(&html, "classic"), 12345, "item"), "Type 12345");
}

#[sqlx::test]
async fn items_that_fail_a_hard_rule_are_in_no_table(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    Item::new(1).insert(&pool).await; // passes
    Item {
        best_sell: "100.00",
        ..Item::new(2)
    }
    .insert(&pool)
    .await; // no margin
    Item {
        sigma: None,
        ..Item::new(3)
    }
    .insert(&pool)
    .await; // not enough history
    Item {
        volume_7d: 0.0,
        ..Item::new(4)
    }
    .insert(&pool)
    .await; // no volume
    // Fills in far more than three days: a tiny volume and many competitors.
    Item {
        volume_7d: 100_000.0,
        buy_near: 5,
        sell_near: 5,
        ..Item::new(5)
    }
    .insert(&pool)
    .await;

    let html = get(&app, &browser, "/opportunities/tables").await;

    assert_eq!(rows(table(&html, "classic")), [1]);
}

#[sqlx::test]
async fn more_than_fifty_matches_show_the_fifty_with_the_highest_isk_per_day(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    for i in 1..=80i64 {
        // A larger volume fills faster and so earns more per day.
        Item {
            volume_7d: 10_000_000.0 * i as f64,
            ..Item::new(1000 + i)
        }
        .insert(&pool)
        .await;
    }

    let html = get(&app, &browser, "/opportunities/tables").await;
    let shown = rows(table(&html, "classic"));

    assert_eq!(shown.len(), 50);
    assert_eq!(shown[0], 1080, "highest ISK/day first");
    assert!(shown.contains(&1031) && !shown.contains(&1030));
}

#[sqlx::test]
async fn each_table_has_its_own_sorting_in_the_url_and_the_headers_carry_all_of_them(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    // Both items are in the trend table too, so every table has its headers.
    let crossed = |item: Item| Item {
        cross_day: Some("2026-10-16"),
        volume_rising: Some(true),
        sma5: Some(106.0),
        sma20: Some(105.0),
        ..item
    };
    crossed(Item {
        name: Some("Pyerite"),
        volume_7d: 200_000_000.0,
        ..Item::new(35)
    })
    .insert(&pool)
    .await;
    crossed(Item {
        name: Some("Tritanium"),
        volume_7d: 100_000_000.0,
        ..Item::new(34)
    })
    .insert(&pool)
    .await;

    let default = get(&app, &browser, "/opportunities/tables").await;
    assert_eq!(
        rows(table(&default, "classic")),
        [35, 34],
        "ISK/day, highest first"
    );

    let by_item = get(
        &app,
        &browser,
        "/opportunities/tables?cs=item&cd=asc&ts=sigma&td=desc",
    )
    .await;
    assert_eq!(
        rows(table(&by_item, "classic")),
        [35, 34],
        "Pyerite before Tritanium"
    );
    let by_item_desc = get(&app, &browser, "/opportunities/tables?cs=item&cd=desc").await;
    assert_eq!(rows(table(&by_item_desc, "classic")), [34, 35]);

    let html = unescaped(&by_item);
    // A classic header link keeps the trend sorting; a trend header link keeps the classic one.
    assert!(
        table(&html, "classic").contains("ts=sigma") && table(&html, "classic").contains("td=desc"),
        "{html}"
    );
    assert!(table(&html, "trend").contains("cs=item") && table(&html, "trend").contains("cd=asc"));
    // Clicking the sorted column again reverses it.
    assert!(table(&html, "classic").contains("cs=item&cd=desc"));
}

#[sqlx::test]
async fn an_unknown_sort_key_falls_back_to_isk_per_day(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    Item {
        name: Some("Pyerite"),
        volume_7d: 200_000_000.0,
        ..Item::new(35)
    }
    .insert(&pool)
    .await;
    Item::new(34).insert(&pool).await;

    let html = get(
        &app,
        &browser,
        "/opportunities/tables?cs=%3Bdrop&cd=sideways",
    )
    .await;

    assert_eq!(rows(table(&html, "classic")), [35, 34]);
}

#[sqlx::test]
async fn a_row_is_marked_when_a_selected_character_has_an_order_of_that_side_at_the_station(
    pool: PgPool,
) {
    let (app, browser) = setup(pool.clone()).await;
    for id in [34, 35, 36] {
        Item::new(id).insert(&pool).await;
    }
    let order = |order_id: i64, type_id: i64, buy: bool, location: i64| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO current_orders (order_id, character_id, type_id, region_id, location_id,
                     is_buy_order, is_corporation, price, volume_remain, volume_total, min_volume,
                     range, issued, duration)
                 VALUES ($1, $2, $3, 10000002, $4, $5, false, 10, 1, 1, 1, 'station', now(), 90)",
            )
            .bind(order_id)
            .bind(ALICE)
            .bind(type_id)
            .bind(location)
            .bind(buy)
            .execute(&pool)
            .await
            .unwrap();
        }
    };
    order(1, 34, false, JITA_44).await;
    order(2, 35, true, JITA_44).await;
    // Elsewhere does not count.
    order(3, 36, false, 60008494).await;

    let html = get(&app, &browser, "/opportunities/tables").await;
    let classic = table(&html, "classic");

    assert!(
        cell(classic, 34, "item").contains("own sell"),
        "{}",
        cell(classic, 34, "item")
    );
    assert!(cell(classic, 35, "item").contains("own buy"));
    assert!(!cell(classic, 36, "item").contains("own"));
}

#[sqlx::test]
async fn the_trend_table_lists_a_fresh_crossing_with_rising_volume_only(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    let yesterday = (time::OffsetDateTime::now_utc() - time::Duration::days(1))
        .date()
        .to_string();
    let yesterday: &'static str = Box::leak(yesterday.into_boxed_str());
    Item {
        cross_day: Some(yesterday),
        volume_rising: Some(true),
        sma5: Some(106.0),
        sma20: Some(105.0),
        ..Item::new(34)
    }
    .insert(&pool)
    .await;
    // Crossed, but the volume is not rising.
    Item {
        cross_day: Some(yesterday),
        volume_rising: Some(false),
        sma5: Some(106.0),
        sma20: Some(105.0),
        ..Item::new(35)
    }
    .insert(&pool)
    .await;
    Item::new(36).insert(&pool).await;

    let html = get(&app, &browser, "/opportunities/tables").await;
    let trend = table(&html, "trend");

    assert_eq!(rows(trend), [34]);
    assert_eq!(cell(trend, 34, "sma5"), "106.00");
    assert_eq!(cell(trend, 34, "sma20"), "105.00");
    assert_eq!(cell(trend, 34, "cross"), yesterday);
    assert_eq!(
        cell(trend, 34, "sell"),
        "129.90",
        "one tick below the best sell"
    );
    assert_eq!(rows(table(&html, "classic")).len(), 3);
}

#[sqlx::test]
async fn the_tables_say_which_fees_they_use(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;

    let html = get(&app, &browser, "/opportunities/tables").await;

    assert!(html.contains("base rates"), "{html}");
}

#[sqlx::test]
async fn the_tables_use_the_fees_of_the_selected_character_with_the_lowest_margin(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    Item::new(34).insert(&pool).await;
    // Skills and standings polled; the owner of Jita 4-4 is known: b = 3 - 0.3 * 5 = 1.5%,
    // t = 7.5 * (1 - 0.11 * 5) = 3.375%.
    for kind in ["skills", "standings"] {
        sqlx::query(
            "INSERT INTO fee_sync (character_id, kind, next_poll_at, last_success_at)
             VALUES ($1, $2, now(), now())",
        )
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

    let html = get(&app, &browser, "/opportunities/tables").await;
    let classic = table(&html, "classic");

    assert!(
        html.contains("Alice Alpha"),
        "the page names the character: {html}"
    );
    assert!(!html.contains("base rates"));
    // c = 100 * 1.015 = 101.50, r = 129.90 * (1 - 0.015 - 0.03375) = 123.567375, net 22.07.
    assert_eq!(cell(classic, 34, "net"), "22.07");
}

// ---- mean reversion ----

#[sqlx::test]
async fn an_oversold_item_is_listed_with_the_sma_20_rounded_down_as_its_sell_price(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    Item {
        best_buy: "120.00",
        best_sell: "160.00",
        sma20: Some(150.37),
        percent_b: Some(-0.3),
        rsi14: Some(45.0),
        ..Item::new(34)
    }
    .insert(&pool)
    .await;

    let html = get(&app, &browser, "/opportunities/tables").await;
    let reversion = table(&html, "reversion");

    assert_eq!(rows(reversion), [34]);
    assert_eq!(cell(reversion, 34, "sell"), "150.30");
    assert_eq!(cell(reversion, 34, "pb"), "-0.30");
    assert_eq!(cell(reversion, 34, "rsi"), "45.0");
}

#[sqlx::test]
async fn an_item_inside_its_bands_is_not_listed_but_a_low_rsi_alone_is_enough(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    let base = |id| Item {
        best_buy: "120.00",
        best_sell: "160.00",
        sma20: Some(150.37),
        ..Item::new(id)
    };
    // Within the band and RSI 45: not listed.
    Item {
        percent_b: Some(0.4),
        rsi14: Some(45.0),
        ..base(1)
    }
    .insert(&pool)
    .await;
    // Within the band, but RSI 25: listed.
    Item {
        percent_b: Some(0.2),
        rsi14: Some(25.0),
        ..base(2)
    }
    .insert(&pool)
    .await;
    // Oversold, but the SMA 20 is not above the current best buy: nothing to earn.
    Item {
        best_buy: "150.40",
        best_sell: "190.00",
        percent_b: Some(-0.5),
        rsi14: Some(20.0),
        ..base(3)
    }
    .insert(&pool)
    .await;
    // Fewer than 20 history days: no SMA 20, so not listed.
    Item {
        sma20: None,
        percent_b: None,
        rsi14: Some(10.0),
        ..base(4)
    }
    .insert(&pool)
    .await;

    let html = get(&app, &browser, "/opportunities/tables").await;

    assert_eq!(rows(table(&html, "reversion")), [2]);
}

// ---- recurring expansion ----

#[sqlx::test]
async fn a_recurring_window_is_listed_with_its_name_its_next_start_and_its_sell_price(
    pool: PgPool,
) {
    use sekondo::opportunities::analysis::WindowKind;
    let (app, browser) = setup(pool.clone()).await;
    Item {
        best_buy: "100.00",
        best_sell: "110.00",
        expansion_kind: Some("hour"),
        expansion_hour: Some(18),
        expansion_sell: Some("125.20"),
        ..Item::new(34)
    }
    .insert(&pool)
    .await;
    // No window: not in this table, but in the classic one.
    Item::new(35).insert(&pool).await;

    let html = get(&app, &browser, "/opportunities/tables").await;
    let expansion = table(&html, "expansion");

    assert_eq!(rows(expansion), [34]);
    assert_eq!(cell(expansion, 34, "window"), "daily 18:00");
    assert_eq!(cell(expansion, 34, "sell"), "125.20");
    let next = WindowKind::HourOfDay { hour: 18 }.next_start(time::OffsetDateTime::now_utc());
    assert_eq!(cell(expansion, 34, "next"), sekondo::format::eve_time(next));
    // Buying at 100 and selling at 109.90 earns nothing after the base fees, so item 34 is not in
    // the classic table; item 35 is.
    assert_eq!(rows(table(&html, "classic")), [35]);
}

async fn snapshot_days(pool: &PgPool, days: i64) {
    for back in 1..=days {
        sqlx::query(
            "INSERT INTO station_snapshots (recorded_at, region_id, station_id, type_id, best_buy,
                 best_sell, spread, buy_near_units, buy_near_orders, sell_near_units,
                 sell_near_orders, buy_units, sell_units, buy_taken, sell_taken)
             VALUES (now() - make_interval(days => $1) - interval '1 hour', 10000002, 60003760,
                     34, 100, 110, 0.1, 1, 1, 1, 1, 1, 1, 0, 0)",
        )
        .bind(back as i32)
        .execute(pool)
        .await
        .unwrap();
    }
}

#[sqlx::test]
async fn the_expansion_table_says_that_hourly_patterns_are_still_being_collected(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    snapshot_days(&pool, 3).await;

    let html = get(&app, &browser, "/opportunities/tables").await;

    assert!(
        table(&html, "expansion").contains("Collecting hourly data (3/7 days)"),
        "{html}"
    );
    assert!(!table(&html, "classic").contains("Collecting hourly data"));
}

#[sqlx::test]
async fn the_collecting_note_goes_away_after_seven_days(pool: PgPool) {
    let (app, browser) = setup(pool.clone()).await;
    snapshot_days(&pool, 7).await;

    let html = get(&app, &browser, "/opportunities/tables").await;

    assert!(!html.contains("Collecting hourly data"), "{html}");
}
