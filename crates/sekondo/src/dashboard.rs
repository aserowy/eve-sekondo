//! The dashboard: what the selected characters have listed and locked in escrow, per character
//! and in total.

use askama::Template;
use axum::{
    extract::State,
    response::{IntoResponse, Redirect, Response},
};
use time::{Duration, OffsetDateTime};
use tower_sessions::Session;

use crate::{
    account::CurrentAccount,
    app::AppState,
    csrf, format,
    freshness::{FreshnessRecord, FreshnessView, freshness_view},
    kpi::{Kpi, KpiLink},
    orders::{FilterEntry, Side},
    pages::{self, Nav, Page},
    range::Range,
    selection::{CharacterSelection, ViewQuery},
    wealth,
};

/// One character's current figures, in cents, with what the freshness rule needs.
#[derive(Debug, Clone)]
pub struct CharacterFigures {
    pub freshness: FreshnessRecord,
    pub sell_cents: i64,
    pub escrow_cents: i64,
    pub sell_corp_cents: i64,
    pub escrow_corp_cents: i64,
    pub sell_count: i64,
    pub buy_count: i64,
}

/// The sums of the rows, so the rows add up to the totals by construction.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Totals {
    pub sell_cents: i64,
    pub escrow_cents: i64,
    pub sell_corp_cents: i64,
    pub escrow_corp_cents: i64,
    pub sell_count: i64,
    pub buy_count: i64,
}

impl Totals {
    pub fn of(rows: &[CharacterFigures]) -> Self {
        rows.iter().fold(Self::default(), |totals, row| Self {
            sell_cents: totals.sell_cents + row.sell_cents,
            escrow_cents: totals.escrow_cents + row.escrow_cents,
            sell_corp_cents: totals.sell_corp_cents + row.sell_corp_cents,
            escrow_corp_cents: totals.escrow_corp_cents + row.escrow_corp_cents,
            sell_count: totals.sell_count + row.sell_count,
            buy_count: totals.buy_count + row.buy_count,
        })
    }
}

/// How many items the top-items graphs show.
pub const TOP_ITEMS: i64 = 10;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct TopItem {
    pub type_id: i64,
    /// The item's name, or `#<type id>` while it is not resolved.
    pub label: String,
    pub cents: i64,
}

/// One point of the over-time graphs: the figures in force at `at`, summed over the selection.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct HistoryPoint {
    pub at: OffsetDateTime,
    pub sell_cents: i64,
    pub escrow_cents: i64,
    pub sell_count: i64,
    pub buy_count: i64,
    /// How many selected characters had a poll at or before `at`.
    pub contributors: i64,
}

/// Whether any selected character has been polled by the end of the series.
pub fn has_history(points: &[HistoryPoint]) -> bool {
    points.last().is_some_and(|point| point.contributors > 0)
}

// ---- view model ----

/// An ISK amount as shown: shortened, with the exact value for the hover.
pub struct Amount {
    pub short: String,
    pub exact: String,
}

impl Amount {
    fn of(cents: i64) -> Self {
        Self {
            short: format::isk_short(cents),
            exact: format::isk_exact(cents),
        }
    }
}

/// A total with the part of it that comes from corporation orders, when there is any.
pub struct AmountTile {
    pub total: Amount,
    pub corp: Option<Amount>,
}

impl AmountTile {
    fn of(total_cents: i64, corp_cents: i64) -> Self {
        Self {
            total: Amount::of(total_cents),
            corp: (corp_cents != 0).then(|| Amount::of(corp_cents)),
        }
    }
}

/// The wealth tile: the current total and where it leads.
pub struct WealthTile {
    pub total: Amount,
    /// The wealth page with the selection and never the range, which is a page's own.
    pub href: String,
}

pub struct BreakdownRow {
    pub id: i64,
    pub sell: Amount,
    pub escrow: Amount,
    pub sell_count: i64,
    pub buy_count: i64,
    pub freshness: FreshnessView,
}

/// Everything the tiles and the breakdown show for one request.
pub struct SummaryView {
    /// Where the summary refreshes itself from, carrying the whole view state.
    pub summary_url: String,
    pub sell: AmountTile,
    pub escrow: AmountTile,
    pub wealth: WealthTile,
    pub sell_count: i64,
    pub buy_count: i64,
    /// The tiles as KPI cards: sell value, buy escrow, wealth, sell orders, buy orders.
    pub tiles: Vec<Kpi>,
    pub rows: Vec<BreakdownRow>,
    /// The per-character split graph's data, as JSON for the canvas's `data-series`.
    pub split_series: String,
    pub split_label: String,
    /// `None` when there are no orders of that side: the page says so instead of drawing.
    pub top_sell: Option<TopChart>,
    pub top_buy: Option<TopChart>,
    /// The over-time graphs; `None` while no selected character has been polled.
    pub history: Option<HistoryCharts>,
    pub wealth_history: Option<TopChart>,
    pub ranges: Vec<RangeLink>,
}

pub struct RangeLink {
    pub label: &'static str,
    pub href: String,
    pub current: bool,
}

/// `""` or `?chars=…&range=…`: the view state a URL carries, with the defaults left out.
fn view_query(selection: &CharacterSelection, range: Range) -> String {
    let mut pairs = url::form_urlencoded::Serializer::new(String::new());
    if let Some(chars) = selection.param() {
        pairs.append_pair("chars", &chars);
    }
    if range != Range::default() {
        pairs.append_pair("range", range.name());
    }
    // The commas stay readable in the URL, as in bookmarks people write by hand.
    let query = pairs.finish().replace("%2C", ",");
    if query.is_empty() {
        query
    } else {
        format!("?{query}")
    }
}

pub struct HistoryCharts {
    pub value: TopChart,
    pub count: TopChart,
}

/// A line of an over-time graph: its name, how to read it from a point, how to print its value.
type Line<'a, P> = (&'a str, fn(&P) -> i64, fn(i64) -> String);

fn line_series<P, const N: usize>(
    points: &[P],
    at: fn(&P) -> OffsetDateTime,
    lines: [Line<P>; N],
) -> String {
    serde_json::json!({
        "times": points.iter().map(|p| at(p).unix_timestamp() * 1000).collect::<Vec<_>>(),
        "labels": points.iter().map(|p| format::eve_time(at(p))).collect::<Vec<_>>(),
        "series": lines.iter().map(|(name, value, exact)| serde_json::json!({
            "name": name,
            "values": points.iter().map(value).collect::<Vec<_>>(),
            "exact": points.iter().map(|p| exact(value(p))).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
    .to_string()
}

fn orders_text(count: i64, noun: &str) -> String {
    format!("{count} {noun} order{}", if count == 1 { "" } else { "s" })
}

fn history_charts(points: &[HistoryPoint]) -> Option<HistoryCharts> {
    if !has_history(points) {
        return None;
    }
    let last = points.last()?;
    Some(HistoryCharts {
        value: TopChart {
            series: line_series(
                points,
                |p| p.at,
                [
                    ("Sell value", |p| p.sell_cents, format::isk_exact),
                    ("Buy escrow", |p| p.escrow_cents, format::isk_exact),
                ],
            ),
            label: format!(
                "Sell value and buy escrow over time; now {} in sell orders and {} in escrow",
                format::isk_exact(last.sell_cents),
                format::isk_exact(last.escrow_cents)
            ),
        },
        count: TopChart {
            series: line_series(
                points,
                |p| p.at,
                [
                    ("Sell orders", |p| p.sell_count, |n| n.to_string()),
                    ("Buy orders", |p| p.buy_count, |n| n.to_string()),
                ],
            ),
            label: format!(
                "Orders over time; now {} and {}",
                orders_text(last.sell_count, "sell"),
                orders_text(last.buy_count, "buy")
            ),
        },
    })
}

/// The total wealth over time as one line: the top edge of the wealth page's stack, from the
/// same query. `None` while no selected character has a recorded figure.
fn wealth_chart(points: &[wealth::WealthPoint]) -> Option<TopChart> {
    if !wealth::has_history(points) {
        return None;
    }
    let last = points.last()?;
    Some(TopChart {
        series: line_series(
            points,
            |p| p.at,
            [("Wealth", |p| p.total_cents(), format::isk_exact)],
        ),
        label: format!(
            "Wealth over time; now {}",
            format::isk_exact(last.total_cents())
        ),
    })
}

/// A top-items graph: its data for `data-series` and what a screen reader hears.
pub struct TopChart {
    pub series: String,
    pub label: String,
}

fn top_chart(items: &[TopItem], series_name: &str) -> Option<TopChart> {
    let best = items.first()?;
    Some(TopChart {
        series: serde_json::json!({
            "labels": items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
            "series": [bar_series(series_name, items.iter().map(|i| i.cents))],
        })
        .to_string(),
        label: format!(
            "{series_name} of the top {} items; the highest is {} with {}",
            items.len(),
            best.label,
            format::isk_exact(best.cents)
        ),
    })
}

/// Cents and their exact form per label, for one bar series.
fn bar_series(name: &str, cents: impl Iterator<Item = i64> + Clone) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "cents": cents.clone().collect::<Vec<_>>(),
        "exact": cents.map(format::isk_exact).collect::<Vec<_>>(),
    })
}

fn split_series(figures: &[CharacterFigures]) -> String {
    serde_json::json!({
        "labels": figures.iter().map(|r| r.freshness.name.as_str()).collect::<Vec<_>>(),
        "series": [
            bar_series("Sell value", figures.iter().map(|r| r.sell_cents)),
            bar_series("Buy escrow", figures.iter().map(|r| r.escrow_cents)),
        ],
    })
    .to_string()
}

fn summary_view(
    figures: &[CharacterFigures],
    (wealth, wealth_points): (&wealth::Current, &[wealth::WealthPoint]),
    (top_sell, top_buy): (&[TopItem], &[TopItem]),
    history: &[HistoryPoint],
    range: Range,
    selection: &CharacterSelection,
    (now, interval): (OffsetDateTime, Duration),
) -> SummaryView {
    let totals = Totals::of(figures);
    let wealth_href = format!("/wealth{}", selection.query());
    let amount_card = |name: &str, id: &str, total: i64, corp: i64| {
        let card = Kpi::new(name, format::isk_short(total), format::isk_exact(total)).with_id(id);
        if corp != 0 {
            card.with_detail(format!("of which {} corp", format::isk_short(corp)))
                .with_detail_title(format::isk_exact(corp))
        } else {
            card
        }
    };
    let count_card = |name: &str, id: &str, count: i64| {
        Kpi::new(name, count.to_string(), format::thousands(count)).with_id(id)
    };
    let tiles = vec![
        amount_card(
            "Sell value",
            "tile-sell-value",
            totals.sell_cents,
            totals.sell_corp_cents,
        ),
        amount_card(
            "Buy escrow",
            "tile-escrow",
            totals.escrow_cents,
            totals.escrow_corp_cents,
        ),
        Kpi::new(
            "Wealth",
            format::isk_short(wealth.total_cents()),
            format::isk_exact(wealth.total_cents()),
        )
        .with_id("tile-wealth")
        .with_link(KpiLink::new(wealth_href, "Wealth")),
        count_card("Sell orders", "tile-sell-count", totals.sell_count),
        count_card("Buy orders", "tile-buy-count", totals.buy_count),
    ];
    SummaryView {
        tiles,
        summary_url: format!("/dashboard/summary{}", view_query(selection, range)),
        ranges: Range::ALL
            .into_iter()
            .map(|candidate| RangeLink {
                label: candidate.label(),
                href: format!("/dashboard{}", view_query(selection, candidate)),
                current: candidate == range,
            })
            .collect(),
        sell: AmountTile::of(totals.sell_cents, totals.sell_corp_cents),
        escrow: AmountTile::of(totals.escrow_cents, totals.escrow_corp_cents),
        wealth: WealthTile {
            total: Amount::of(wealth.total_cents()),
            href: format!("/wealth{}", selection.query()),
        },
        sell_count: totals.sell_count,
        buy_count: totals.buy_count,
        history: history_charts(history),
        wealth_history: wealth_chart(wealth_points),
        top_sell: top_chart(top_sell, "Sell value"),
        top_buy: top_chart(top_buy, "Buy escrow"),
        split_series: split_series(figures),
        split_label: format!("Sell value and buy escrow of {} characters", figures.len()),
        rows: figures
            .iter()
            .map(|row| BreakdownRow {
                id: row.freshness.character_id,
                sell: Amount::of(row.sell_cents),
                escrow: Amount::of(row.escrow_cents),
                sell_count: row.sell_count,
                buy_count: row.buy_count,
                freshness: freshness_view(&row.freshness, now, interval),
            })
            .collect(),
    }
}

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardPage {
    csrf_token: String,
    nav: Option<Nav>,
    view: SummaryView,
    characters: Vec<FilterEntry>,
    range_name: &'static str,
}

#[derive(Template)]
#[template(path = "dashboard_summary.html")]
struct SummaryFragment {
    view: SummaryView,
}

/// What a request shows: the summary, and the filter that produced it.
struct Loaded {
    view: SummaryView,
    selection: CharacterSelection,
    characters: Vec<FilterEntry>,
    range: Range,
}

async fn load(
    state: &AppState,
    account: &CurrentAccount,
    session: &Session,
    query: &ViewQuery,
) -> Result<Loaded, Box<Response>> {
    let characters = state
        .repo
        .list_characters(account.id)
        .await
        .map_err(|e| Box::new(pages::internal_error(e)))?;
    if characters.is_empty() {
        // An account without characters cannot exist; the session outlived its account.
        let _ = session.flush().await;
        return Err(Box::new(Redirect::to("/").into_response()));
    }
    let ids: Vec<i64> = characters.iter().map(|c| c.character_id).collect();
    let selection = query.selection(&ids);
    let figures = state
        .stores
        .orders
        .character_figures(account.id, selection.ids())
        .await
        .map_err(|e| Box::new(pages::internal_error(e)))?;
    let top_sell = state
        .stores
        .orders
        .top_items(account.id, selection.ids(), Side::Sell, TOP_ITEMS)
        .await
        .map_err(|e| Box::new(pages::internal_error(e)))?;
    let top_buy = state
        .stores
        .orders
        .top_items(account.id, selection.ids(), Side::Buy, TOP_ITEMS)
        .await
        .map_err(|e| Box::new(pages::internal_error(e)))?;
    let now = state.clock.now();
    let range = Range::parse(query.range.as_deref());
    let history = state
        .stores
        .orders
        .history(account.id, selection.ids(), range, now)
        .await
        .map_err(|e| Box::new(pages::internal_error(e)))?;
    let wealth_points = state
        .stores
        .wealth
        .history(account.id, selection.ids(), range, now)
        .await
        .map_err(|e| Box::new(pages::internal_error(e)))?;
    let wealth = state
        .stores
        .wealth
        .current(account.id, selection.ids())
        .await
        .map_err(|e| Box::new(pages::internal_error(e)))?;
    let view = summary_view(
        &figures,
        (&wealth, &wealth_points),
        (&top_sell, &top_buy),
        &history,
        range,
        &selection,
        (now, state.config.order_poll_interval),
    );
    let characters = characters
        .iter()
        .map(|c| FilterEntry {
            id: c.character_id,
            name: c.name.clone(),
            checked: selection.contains(c.character_id),
        })
        .collect();
    Ok(Loaded {
        view,
        selection,
        characters,
        range,
    })
}

pub async fn page(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    query: ViewQuery,
) -> Response {
    let Loaded {
        view,
        selection,
        characters,
        range,
    } = match load(&state, &account, &session, &query).await {
        Ok(loaded) => loaded,
        Err(response) => return *response,
    };
    let csrf_token = match csrf::token(&session).await {
        Ok(token) => token,
        Err(e) => return pages::internal_error(e),
    };
    pages::render(&DashboardPage {
        csrf_token,
        nav: Some(Nav::new(Page::Dashboard).with_chars_query(selection.query())),
        view,
        characters,
        range_name: range.name(),
    })
}

/// The tiles and the breakdown only, for live refreshes.
pub async fn summary(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    query: ViewQuery,
) -> Response {
    match load(&state, &account, &session, &query).await {
        Ok(loaded) => pages::render(&SummaryFragment { view: loaded.view }),
        Err(response) => *response,
    }
}
