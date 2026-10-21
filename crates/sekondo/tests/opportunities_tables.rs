mod common;

use common::{AppBuilder, Browser, TestApp};
use sekondo::{
    opportunities::analysis::{Direction, WindowKind},
    store::WindowRow,
};

const ALICE: i64 = 90000001;
const JITA_44: i64 = 60003760;

async fn setup() -> (TestApp, Browser) {
    let app = AppBuilder::new().with_stub().spawn().await;
    let browser = app.browser();
    browser.sso_round_trip(&app, "/auth/login", ALICE).await;
    (app, browser)
}

async fn get_one(app: &TestApp, browser: &Browser, path: &str) -> String {
    let res = browser.get(&app.url(path)).await;
    assert_eq!(res.status(), 200, "{path}");
    res.text().await.unwrap()
}

/// A page by its path. `/opportunities/tables` and `/opportunities/tables?…` stand for the four
/// tables, the content of each table's page one after the other with the same query, so that a
/// test can look at the rows of every table.
async fn get(app: &TestApp, browser: &Browser, path: &str) -> String {
    match path.strip_prefix("/opportunities/tables") {
        Some(query) if query.is_empty() || query.starts_with('?') => {
            let mut all = String::new();
            for table in ["expansion", "classic", "reversion", "trend"] {
                all += &get_one(
                    app,
                    browser,
                    &format!("/opportunities/{table}/content{query}"),
                )
                .await;
            }
            all
        }
        _ => get_one(app, browser, path).await,
    }
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
    /// Recurring windows of the expansion table: kind, direction, S (spike) or B (dip) in cents
    /// and V_win.
    windows: Vec<(WindowKind, Direction, i64, f64)>,
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
            windows: Vec::new(),
        }
    }

    async fn insert(&self, app: &TestApp) {
        let now = time::OffsetDateTime::now_utc();
        let cross_day = self.cross_day.map(|day| {
            time::Date::parse(
                day,
                time::macros::format_description!("[year]-[month]-[day]"),
            )
            .unwrap()
        });
        app.stores
            .station
            .seed_item_stats(
                10000002,
                JITA_44,
                self.type_id,
                &sekondo::store::ItemStatsRecord {
                    book_at: Some(now),
                    best_buy: Some(self.best_buy.to_owned()),
                    best_sell: Some(self.best_sell.to_owned()),
                    buy_near_orders: Some(self.buy_near),
                    sell_near_orders: Some(self.sell_near),
                    rho: Some(self.rho),
                    indicators_at: Some(now),
                    volume_7d: Some(self.volume_7d),
                    volume_30d: Some(self.volume_30d),
                    sigma: self.sigma,
                    returns: Some(29),
                    sma5: self.sma5,
                    sma20: self.sma20,
                    cross_day,
                    volume_rising: self.volume_rising,
                    percent_b: self.percent_b,
                    rsi14: self.rsi14,
                    expansion_at: Some(now),
                },
            )
            .await
            .unwrap();
        // The windows of every item inserted so far, and this one's.
        let mut windows = app.stores.station.windows(10000002, JITA_44).await.unwrap();
        windows.extend(
            self.windows
                .iter()
                .map(|(kind, direction, price_cents, v_win)| WindowRow {
                    type_id: self.type_id,
                    kind: *kind,
                    direction: *direction,
                    price_cents: *price_cents,
                    occurrences: 14,
                    covered: 20,
                    v_win: *v_win,
                }),
        );
        app.stores
            .station
            .replace_windows(10000002, JITA_44, now, &windows)
            .await
            .unwrap();
        if let Some(name) = self.name {
            app.stores
                .market
                .store_names(
                    &[sekondo::store::NameRow {
                        id: self.type_id,
                        category: "inventory_type".to_owned(),
                        name: Some(name.to_owned()),
                    }],
                    now,
                    None,
                )
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

#[tokio::test]
async fn a_classic_row_shows_the_current_best_buy_the_sell_price_and_the_shared_figures() {
    let (app, browser) = setup().await;
    Item::new(34).insert(&app).await;

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

#[tokio::test]
async fn the_columns_say_that_the_buy_price_is_current_and_not_a_suggestion() {
    let (app, browser) = setup().await;
    Item::new(34).insert(&app).await;

    let html = get(&app, &browser, "/opportunities/tables").await;

    assert!(
        html.contains(
            r#"<abbr title="Current best buy, not a suggested price, except in a dip row of the recurring spread expansion table, where it is the predicted buy price">B</abbr>"#
        ),
        "{html}"
    );
    assert!(!html.to_lowercase().contains("suggested buy"));
}

#[tokio::test]
async fn the_headers_are_short_labels_with_their_full_names_on_hover() {
    let (app, browser) = setup().await;
    Item {
        cross_day: Some("2026-10-16"),
        volume_rising: Some(true),
        sma5: Some(106.0),
        sma20: Some(105.0),
        ..Item::new(34)
    }
    .insert(&app)
    .await;

    let html = get_one(&app, &browser, "/opportunities/trend").await;
    let head = &html[html.find("<thead>").unwrap()..html.find("</thead>").unwrap()];

    let labels: Vec<&str> = head
        .split("</th>")
        .filter_map(|th| th.split_once("<a ")?.1.split_once('>'))
        .map(|(_, rest)| rest)
        .collect();
    assert_eq!(labels.len(), 16, "{head}");
    assert!(
        labels[0].starts_with("Item"),
        "Item is no abbreviation: {}",
        labels[0]
    );
    for (label, short) in labels[1..].iter().zip([
        "B", "S", "Net", "m", "ISK/d", "SMA5", "SMA20", "Cross", "V7", "V30", "\u{03C3}", "T_max",
        "T_fill", "V_min", "n b/s",
    ]) {
        assert!(label.starts_with("<abbr title=\""), "{label}");
        assert!(
            label.contains(&format!("\">{short}</abbr>")),
            "{short}: {label}"
        );
    }
    assert!(
        head.contains(r#"<abbr title="Allowed holding time">T_max</abbr>"#),
        "{head}"
    );
}

#[tokio::test]
async fn the_expansion_headers_end_with_window_direction_window_volume_and_next_start() {
    let (app, browser) = setup().await;
    Item {
        windows: vec![(DAILY_18, Direction::Spike, 12_520, 4_000.0)],
        ..Item::new(34)
    }
    .insert(&app)
    .await;

    let html = get_one(&app, &browser, "/opportunities/expansion").await;
    let head = &html[html.find("<thead>").unwrap()..html.find("</thead>").unwrap()];

    let shorts: Vec<&str> = head
        .split("</abbr>")
        .filter_map(|part| part.rsplit_once('>').map(|(_, short)| short))
        .filter(|short| !short.trim().is_empty())
        .collect();
    assert_eq!(
        shorts,
        [
            "B",
            "S",
            "Net",
            "m",
            "ISK/d",
            "V7",
            "V30",
            "\u{03C3}",
            "T_max",
            "T_fill",
            "V_min",
            "n b/s",
            "Window",
            "Dir",
            "V_win",
            "Next (UTC)"
        ],
        "{head}"
    );
    assert!(head.contains("direction"), "{head}");
    assert!(head.contains("window-volume"), "{head}");
}

#[tokio::test]
async fn an_item_without_a_name_is_shown_as_its_type_id() {
    let (app, browser) = setup().await;
    Item {
        name: None,
        ..Item::new(12345)
    }
    .insert(&app)
    .await;

    let html = get(&app, &browser, "/opportunities/tables").await;

    assert_eq!(cell(table(&html, "classic"), 12345, "item"), "Type 12345");
}

#[tokio::test]
async fn items_that_fail_a_hard_rule_are_in_no_table() {
    let (app, browser) = setup().await;
    Item::new(1).insert(&app).await; // passes
    Item {
        best_sell: "100.00",
        ..Item::new(2)
    }
    .insert(&app)
    .await; // no margin
    Item {
        sigma: None,
        ..Item::new(3)
    }
    .insert(&app)
    .await; // not enough history
    Item {
        volume_7d: 0.0,
        ..Item::new(4)
    }
    .insert(&app)
    .await; // no volume
    // Fills in far more than three days: a tiny volume and many competitors.
    Item {
        volume_7d: 100_000.0,
        buy_near: 5,
        sell_near: 5,
        ..Item::new(5)
    }
    .insert(&app)
    .await;

    let html = get(&app, &browser, "/opportunities/tables").await;

    assert_eq!(rows(table(&html, "classic")), [1]);
}

#[tokio::test]
async fn more_than_fifty_matches_page_through_all_of_them_by_isk_per_day() {
    let (app, browser) = setup().await;
    for i in 1..=80i64 {
        // A larger volume fills faster and so earns more per day.
        Item {
            volume_7d: 10_000_000.0 * i as f64,
            ..Item::new(1000 + i)
        }
        .insert(&app)
        .await;
    }

    let first = get_one(&app, &browser, "/opportunities/classic/content").await;
    let second = get_one(&app, &browser, "/opportunities/classic/content?page=2").await;
    let first = rows(table(&first, "classic"));
    let second = rows(table(&second, "classic"));

    assert_eq!(first.len(), 50);
    assert_eq!(first[0], 1080, "highest ISK/day first");
    assert!(first.contains(&1031) && !first.contains(&1030));
    assert_eq!(second.len(), 30, "the other 30");
    assert_eq!(second[0], 1030);
    assert_eq!(second.last(), Some(&1001));
}

#[tokio::test]
async fn the_page_is_applied_after_sorting_and_clamped_and_carried_by_the_controls() {
    let (app, browser) = setup().await;
    for i in 1..=80i64 {
        Item {
            volume_7d: 10_000_000.0 * i as f64,
            ..Item::new(1000 + i)
        }
        .insert(&app)
        .await;
    }

    let by_item = get_one(
        &app,
        &browser,
        "/opportunities/classic/content?cs=item&cd=asc&page=2",
    )
    .await;
    let past = get_one(&app, &browser, "/opportunities/classic/content?page=9").await;

    // All items are named alike, so the tie falls to the item number: page 2 is the last 30.
    assert_eq!(rows(table(&by_item, "classic")).len(), 30);
    assert!(past.contains("Page 2 of 2"), "{past}");
    let links = unescaped(&by_item);
    let previous = links
        .split("<a ")
        .find(|a| a.contains(r#"rel="prev""#))
        .unwrap();
    assert!(
        previous.contains(r#"href="/opportunities/classic?cs=item&cd=asc""#),
        "{previous}"
    );
    // A sorting starts over on the first page.
    let sort_link = links
        .split("<a ")
        .find(|a| a.contains("cs=item&cd=desc"))
        .unwrap();
    assert!(!sort_link.contains("page="), "{sort_link}");
}

#[tokio::test]
async fn each_table_has_its_own_sorting_in_the_url_and_the_headers_carry_all_of_them() {
    let (app, browser) = setup().await;
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
    .insert(&app)
    .await;
    crossed(Item {
        name: Some("Tritanium"),
        volume_7d: 100_000_000.0,
        ..Item::new(34)
    })
    .insert(&app)
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

#[tokio::test]
async fn an_unknown_sort_key_falls_back_to_isk_per_day() {
    let (app, browser) = setup().await;
    Item {
        name: Some("Pyerite"),
        volume_7d: 200_000_000.0,
        ..Item::new(35)
    }
    .insert(&app)
    .await;
    Item::new(34).insert(&app).await;

    let html = get(
        &app,
        &browser,
        "/opportunities/tables?cs=%3Bdrop&cd=sideways",
    )
    .await;

    assert_eq!(rows(table(&html, "classic")), [35, 34]);
}

#[tokio::test]
async fn a_row_is_marked_when_a_selected_character_has_an_order_of_that_side_at_the_station() {
    let (app, browser) = setup().await;
    for id in [34, 35, 36] {
        Item::new(id).insert(&app).await;
    }
    let now = time::OffsetDateTime::now_utc();
    let order = |order_id: i64, type_id: i64, is_buy_order: bool, location_id: i64| {
        sekondo::esi::EsiOrder {
            order_id,
            type_id,
            region_id: 10000002,
            location_id,
            is_buy_order,
            is_corporation: false,
            price_cents: 1000,
            volume_remain: 1,
            volume_total: 1,
            min_volume: 1,
            range: "station".to_owned(),
            escrow_cents: None,
            issued: now,
            duration: 90,
        }
    };
    let orders = [
        order(1, 34, false, JITA_44),
        order(2, 35, true, JITA_44),
        // Elsewhere does not count.
        order(3, 36, false, 60008494),
    ];
    app.stores
        .orders
        .replace_orders(ALICE, now, &orders, now + time::Duration::hours(1), None)
        .await
        .unwrap();

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

#[tokio::test]
async fn the_trend_table_lists_a_fresh_crossing_with_rising_volume_only() {
    let (app, browser) = setup().await;
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
    .insert(&app)
    .await;
    // Crossed, but the volume is not rising.
    Item {
        cross_day: Some(yesterday),
        volume_rising: Some(false),
        sma5: Some(106.0),
        sma20: Some(105.0),
        ..Item::new(35)
    }
    .insert(&app)
    .await;
    Item::new(36).insert(&app).await;

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

#[tokio::test]
async fn the_tables_say_which_fees_they_use() {
    let (app, browser) = setup().await;

    let html = get(&app, &browser, "/opportunities/tables").await;

    assert!(html.contains(r#"<form id="fee-filter""#), "{html}");
    assert!(html.contains("Base rates"), "{html}");
}

#[tokio::test]
async fn the_tables_use_the_fees_of_the_selected_character_with_the_lowest_margin() {
    let (app, browser) = setup().await;
    Item::new(34).insert(&app).await;
    // Skills and standings polled; the owner of Jita 4-4 is known: b = 3 - 0.3 * 5 = 1.5%,
    // t = 7.5 * (1 - 0.11 * 5) = 3.375%.
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

    let html = get(&app, &browser, "/opportunities/tables").await;
    let classic = table(&html, "classic");

    assert!(
        html.contains(r#"<input type="radio" name="fee_character" value="90000001" checked>"#),
        "the character is the fee character: {html}"
    );
    assert!(!html.contains("Base rates"));
    // c = 100 * 1.015 = 101.50, r = 129.90 * (1 - 0.015 - 0.03375) = 123.567375, net 22.07.
    assert_eq!(cell(classic, 34, "net"), "22.07");
}

// ---- mean reversion ----

#[tokio::test]
async fn an_oversold_item_is_listed_with_the_sma_20_rounded_down_as_its_sell_price() {
    let (app, browser) = setup().await;
    Item {
        best_buy: "120.00",
        best_sell: "160.00",
        sma20: Some(150.37),
        percent_b: Some(-0.3),
        rsi14: Some(45.0),
        ..Item::new(34)
    }
    .insert(&app)
    .await;

    let html = get(&app, &browser, "/opportunities/tables").await;
    let reversion = table(&html, "reversion");

    assert_eq!(rows(reversion), [34]);
    assert_eq!(cell(reversion, 34, "sell"), "150.30");
    assert_eq!(cell(reversion, 34, "pb"), "-0.30");
    assert_eq!(cell(reversion, 34, "rsi"), "45.0");
}

#[tokio::test]
async fn an_item_inside_its_bands_is_not_listed_but_a_low_rsi_alone_is_enough() {
    let (app, browser) = setup().await;
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
    .insert(&app)
    .await;
    // Within the band, but RSI 25: listed.
    Item {
        percent_b: Some(0.2),
        rsi14: Some(25.0),
        ..base(2)
    }
    .insert(&app)
    .await;
    // Oversold, but the SMA 20 is not above the current best buy: nothing to earn.
    Item {
        best_buy: "150.40",
        best_sell: "190.00",
        percent_b: Some(-0.5),
        rsi14: Some(20.0),
        ..base(3)
    }
    .insert(&app)
    .await;
    // Fewer than 20 history days: no SMA 20, so not listed.
    Item {
        sma20: None,
        percent_b: None,
        rsi14: Some(10.0),
        ..base(4)
    }
    .insert(&app)
    .await;

    let html = get(&app, &browser, "/opportunities/tables").await;

    assert_eq!(rows(table(&html, "reversion")), [2]);
}

// ---- recurring expansion ----

const DAILY_18: WindowKind = WindowKind::HourOfDay { hour: 18 };
const TUESDAY_11: WindowKind = WindowKind::WeekdayHour {
    weekday: time::Weekday::Tuesday,
    hour: 11,
};

#[tokio::test]
async fn a_spike_window_is_listed_with_its_direction_its_volume_its_next_start_and_its_sell_price()
{
    let (app, browser) = setup().await;
    Item {
        best_buy: "100.00",
        best_sell: "110.00",
        windows: vec![(DAILY_18, Direction::Spike, 12_520, 4_000.0)],
        ..Item::new(34)
    }
    .insert(&app)
    .await;
    // No window: not in this table, but in the classic one.
    Item::new(35).insert(&app).await;

    let html = get(&app, &browser, "/opportunities/tables").await;
    let expansion = table(&html, "expansion");

    assert_eq!(rows(expansion), [34]);
    assert_eq!(cell(expansion, 34, "window"), "daily 18:00");
    assert_eq!(cell(expansion, 34, "direction"), "Spike");
    assert_eq!(cell(expansion, 34, "window-volume"), "4,000");
    assert_eq!(cell(expansion, 34, "buy"), "100.00");
    assert_eq!(cell(expansion, 34, "sell"), "125.20");
    let next = DAILY_18.next_start(time::OffsetDateTime::now_utc());
    assert_eq!(cell(expansion, 34, "next"), sekondo::format::eve_time(next));
    // Buying at 100 and selling at 109.90 earns nothing after the base fees, so item 34 is not in
    // the classic table; item 35 is.
    assert_eq!(rows(table(&html, "classic")), [35]);
}

#[tokio::test]
async fn a_dip_row_buys_at_the_predicted_price_and_sells_one_tick_below_the_best_sell() {
    let (app, browser) = setup().await;
    Item {
        best_buy: "100.00",
        best_sell: "110.00",
        windows: vec![(TUESDAY_11, Direction::Dip, 8_210, 900.0)],
        ..Item::new(34)
    }
    .insert(&app)
    .await;

    let html = get(&app, &browser, "/opportunities/tables").await;
    let expansion = table(&html, "expansion");

    assert_eq!(rows(expansion), [34]);
    assert_eq!(cell(expansion, 34, "window"), "weekly Tue 11:00");
    assert_eq!(cell(expansion, 34, "direction"), "Dip");
    assert_eq!(cell(expansion, 34, "buy"), "82.10");
    assert_eq!(cell(expansion, 34, "sell"), "109.90");
    // c = 82.10 x 1.03 = 84.563 and r = 109.90 x 0.895 = 98.3605, so the net margin is 13.7975.
    assert_eq!(cell(expansion, 34, "net"), "13.80");
    assert_eq!(cell(expansion, 34, "margin"), "16.32%");
}

#[tokio::test]
async fn the_window_with_the_highest_margin_is_the_one_in_the_row() {
    let (app, browser) = setup().await;
    // The spike gives m = 8.8% (buy 100.00, sell 125.20); the dip 16.3% (buy 82.10, sell 109.90).
    Item {
        best_buy: "100.00",
        best_sell: "110.00",
        windows: vec![
            (DAILY_18, Direction::Spike, 12_520, 4_000.0),
            (TUESDAY_11, Direction::Dip, 8_210, 900.0),
        ],
        ..Item::new(34)
    }
    .insert(&app)
    .await;

    let html = get(&app, &browser, "/opportunities/tables").await;
    let expansion = table(&html, "expansion");

    assert_eq!(rows(expansion), [34], "one row per item");
    assert_eq!(cell(expansion, 34, "direction"), "Dip");
    assert_eq!(cell(expansion, 34, "window"), "weekly Tue 11:00");
}

#[tokio::test]
async fn a_dip_whose_buy_price_is_not_below_the_sell_price_is_not_listed() {
    let (app, browser) = setup().await;
    // B 150.00 against S 109.90: no margin.
    Item {
        best_buy: "100.00",
        best_sell: "110.00",
        windows: vec![(TUESDAY_11, Direction::Dip, 15_000, 900.0)],
        ..Item::new(34)
    }
    .insert(&app)
    .await;

    let html = get(&app, &browser, "/opportunities/tables").await;

    assert_eq!(rows(table(&html, "expansion")), Vec::<i64>::new());
}

#[tokio::test]
async fn on_equal_margins_the_window_that_starts_next_is_the_one_in_the_row() {
    let (app, browser) = setup().await;
    // Two spikes with the same S: the daily window starts before the weekly one or at most with
    // it, so the row is the daily one.
    Item {
        best_buy: "100.00",
        best_sell: "110.00",
        windows: vec![
            (TUESDAY_11, Direction::Spike, 12_520, 1.0),
            (DAILY_18, Direction::Spike, 12_520, 2.0),
        ],
        ..Item::new(34)
    }
    .insert(&app)
    .await;
    let now = time::OffsetDateTime::now_utc();
    let expected = if DAILY_18.next_start(now) <= TUESDAY_11.next_start(now) {
        "daily 18:00"
    } else {
        "weekly Tue 11:00"
    };

    let html = get(&app, &browser, "/opportunities/tables").await;

    assert_eq!(cell(table(&html, "expansion"), 34, "window"), expected);
}

async fn snapshot_days(app: &TestApp, days: i64) {
    let rows: Vec<_> = (1..=days)
        .map(|back| sekondo::store::SeedSnapshotRow {
            recorded_at: time::OffsetDateTime::now_utc()
                - time::Duration::days(back)
                - time::Duration::hours(1),
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
        .seed_snapshots(10000002, JITA_44, &rows)
        .await
        .unwrap();
}

#[tokio::test]
async fn no_table_says_that_hourly_data_is_being_collected() {
    let (app, browser) = setup().await;
    snapshot_days(&app, 3).await;

    let html = get(&app, &browser, "/opportunities/tables").await;

    assert!(!html.contains("Collecting hourly data"), "{html}");
    assert!(
        table(&html, "expansion").contains("No opportunities."),
        "an empty table, without a note: {html}"
    );
}
