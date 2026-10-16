//! The dashboard: what the selected characters have listed and locked in escrow, per character
//! and in total.

use askama::Template;
use axum::{
    extract::State,
    response::{IntoResponse, Redirect, Response},
};
use sqlx::{PgPool, Row, postgres::PgRow};
use time::{Duration, OffsetDateTime};
use tower_sessions::Session;
use uuid::Uuid;

use crate::{
    account::CurrentAccount,
    app::AppState,
    csrf, format,
    freshness::{FreshnessRecord, FreshnessView, freshness_view},
    orders::{FilterEntry, Side},
    pages::{self, Nav, Page},
    selection::{CharacterSelection, ViewQuery},
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

impl sqlx::FromRow<'_, PgRow> for CharacterFigures {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            freshness: FreshnessRecord::from_row(row)?,
            sell_cents: row.try_get("sell_cents")?,
            escrow_cents: row.try_get("escrow_cents")?,
            sell_corp_cents: row.try_get("sell_corp_cents")?,
            escrow_corp_cents: row.try_get("escrow_corp_cents")?,
            sell_count: row.try_get("sell_count")?,
            buy_count: row.try_get("buy_count")?,
        })
    }
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

/// One row per selected character of the account, in link order, including characters without
/// orders. Sums are converted to cents once, after summing.
pub async fn character_figures(
    pool: &PgPool,
    account_id: Uuid,
    selection: &CharacterSelection,
) -> Result<Vec<CharacterFigures>, sqlx::Error> {
    sqlx::query_as(
        "SELECT c.character_id, c.name, c.needs_relogin, s.last_success_at, s.next_poll_at,
                (COALESCE(SUM(o.price * o.volume_remain) FILTER (WHERE NOT o.is_buy_order), 0) * 100)::bigint
                    AS sell_cents,
                (COALESCE(SUM(COALESCE(o.escrow, 0)) FILTER (WHERE o.is_buy_order), 0) * 100)::bigint
                    AS escrow_cents,
                (COALESCE(SUM(o.price * o.volume_remain) FILTER (WHERE NOT o.is_buy_order AND o.is_corporation), 0) * 100)::bigint
                    AS sell_corp_cents,
                (COALESCE(SUM(COALESCE(o.escrow, 0)) FILTER (WHERE o.is_buy_order AND o.is_corporation), 0) * 100)::bigint
                    AS escrow_corp_cents,
                COUNT(o.order_id) FILTER (WHERE NOT o.is_buy_order) AS sell_count,
                COUNT(o.order_id) FILTER (WHERE o.is_buy_order) AS buy_count
         FROM characters c
         LEFT JOIN order_sync s ON s.character_id = c.character_id
         LEFT JOIN current_orders o ON o.character_id = c.character_id
         WHERE c.account_id = $1 AND c.character_id = ANY($2)
         GROUP BY c.character_id, c.name, c.needs_relogin, s.last_success_at, s.next_poll_at, c.created_at
         ORDER BY c.created_at, c.character_id",
    )
    .bind(account_id)
    .bind(selection.ids())
    .fetch_all(pool)
    .await
}

/// How many items the top-items graphs show.
const TOP_ITEMS: i64 = 10;

#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct TopItem {
    pub type_id: i64,
    /// The item's name, or `#<type id>` while it is not resolved.
    pub label: String,
    pub cents: i64,
}

/// The items with the most ISK in the selected characters' sell orders (their value) or buy
/// orders (their escrow), summed over characters and locations. Highest first, ties by name.
pub async fn top_items(
    pool: &PgPool,
    account_id: Uuid,
    selection: &CharacterSelection,
    side: Side,
) -> Result<Vec<TopItem>, sqlx::Error> {
    let amount = match side {
        Side::Sell => "o.price * o.volume_remain",
        Side::Buy => "COALESCE(o.escrow, 0)",
    };
    let sql = format!(
        "SELECT o.type_id,
                COALESCE(n.name, '#' || o.type_id) AS label,
                (SUM({amount}) * 100)::bigint AS cents
         FROM current_orders o
         JOIN characters c ON c.character_id = o.character_id
         LEFT JOIN names n ON n.id = o.type_id
         WHERE c.account_id = $1 AND o.character_id = ANY($2) AND o.is_buy_order = $3
         GROUP BY o.type_id, n.name
         ORDER BY SUM({amount}) DESC, lower(COALESCE(n.name, '#' || o.type_id)), o.type_id
         LIMIT {TOP_ITEMS}"
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(account_id)
        .bind(selection.ids())
        .bind(side == Side::Buy)
        .fetch_all(pool)
        .await
}

/// The time range of the over-time graphs, as the `range` query parameter names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Range {
    Day,
    #[default]
    Week,
    Month,
    Quarter,
}

impl Range {
    pub const ALL: [Range; 4] = [Range::Day, Range::Week, Range::Month, Range::Quarter];

    /// Never fails: a missing or unknown value is the default.
    pub fn parse(raw: Option<&str>) -> Self {
        Self::ALL
            .into_iter()
            .find(|range| Some(range.name()) == raw)
            .unwrap_or_default()
    }

    pub fn name(self) -> &'static str {
        match self {
            Range::Day => "24h",
            Range::Week => "7d",
            Range::Month => "30d",
            Range::Quarter => "90d",
        }
    }

    pub fn span(self) -> Duration {
        match self {
            Range::Day => Duration::hours(24),
            Range::Week => Duration::days(7),
            Range::Month => Duration::days(30),
            Range::Quarter => Duration::days(90),
        }
    }

    /// The width of one point of the graphs: 48 to 120 points per range.
    pub fn bucket(self) -> Duration {
        match self {
            Range::Day => Duration::minutes(30),
            Range::Week => Duration::hours(2),
            Range::Month => Duration::hours(6),
            Range::Quarter => Duration::days(1),
        }
    }
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

/// The series over `range` ending at `now`, one point per bucket: its timestamp is the bucket's
/// end (the last one clamped to `now`), its figures are those of each selected character's most
/// recent poll at or before that time, summed. A character's last poll before the range carries
/// in, and one that has not been polled yet contributes nothing.
pub async fn history(
    pool: &PgPool,
    account_id: Uuid,
    selection: &CharacterSelection,
    range: Range,
    now: OffsetDateTime,
) -> Result<Vec<HistoryPoint>, sqlx::Error> {
    let seconds = |d: Duration| d.as_seconds_f64();
    sqlx::query_as(
        "WITH width AS (SELECT make_interval(secs => $4) AS w, make_interval(secs => $5) AS span),
              ends AS (
                  SELECT g AS at
                  FROM width, generate_series(time_bucket(w, $3 - span) + w, $3, w) AS g
                  WHERE g < $3
                  UNION ALL
                  SELECT $3
              ),
              chars AS (
                  SELECT character_id FROM characters
                  WHERE account_id = $1 AND character_id = ANY($2)
              )
         SELECT e.at,
                (COALESCE(SUM(t.sell_value), 0) * 100)::bigint AS sell_cents,
                (COALESCE(SUM(t.escrow), 0) * 100)::bigint AS escrow_cents,
                COALESCE(SUM(t.sell_count), 0)::bigint AS sell_count,
                COALESCE(SUM(t.buy_count), 0)::bigint AS buy_count,
                COUNT(t.character_id) AS contributors
         FROM ends e
         CROSS JOIN chars c
         LEFT JOIN LATERAL (
             SELECT p.character_id, p.sell_value, p.escrow, p.sell_count, p.buy_count
             FROM order_poll_totals p
             WHERE p.character_id = c.character_id AND p.polled_at <= e.at
             ORDER BY p.polled_at DESC
             LIMIT 1
         ) t ON true
         GROUP BY e.at
         ORDER BY e.at",
    )
    .bind(account_id)
    .bind(selection.ids())
    .bind(now)
    .bind(seconds(range.bucket()))
    .bind(seconds(range.span()))
    .fetch_all(pool)
    .await
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
    pub sell_count: i64,
    pub buy_count: i64,
    pub rows: Vec<BreakdownRow>,
    /// The per-character split graph's data, as JSON for the canvas's `data-series`.
    pub split_series: String,
    pub split_label: String,
    /// `None` when there are no orders of that side: the page says so instead of drawing.
    pub top_sell: Option<TopChart>,
    pub top_buy: Option<TopChart>,
    /// The over-time graphs; `None` while no selected character has been polled.
    pub history: Option<HistoryCharts>,
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

impl Range {
    fn label(self) -> &'static str {
        match self {
            Range::Day => "24 hours",
            Range::Week => "7 days",
            Range::Month => "30 days",
            Range::Quarter => "90 days",
        }
    }
}

pub struct HistoryCharts {
    pub value: TopChart,
    pub count: TopChart,
}

/// A line of an over-time graph: its name, how to read it from a point, how to print its value.
type Line<'a> = (&'a str, fn(&HistoryPoint) -> i64, fn(i64) -> String);

fn line_series(points: &[HistoryPoint], lines: [Line; 2]) -> String {
    serde_json::json!({
        "times": points.iter().map(|p| p.at.unix_timestamp() * 1000).collect::<Vec<_>>(),
        "labels": points.iter().map(|p| format::eve_time(p.at)).collect::<Vec<_>>(),
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
    (top_sell, top_buy): (&[TopItem], &[TopItem]),
    history: &[HistoryPoint],
    range: Range,
    selection: &CharacterSelection,
    now: OffsetDateTime,
    interval: Duration,
) -> SummaryView {
    let totals = Totals::of(figures);
    SummaryView {
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
        sell_count: totals.sell_count,
        buy_count: totals.buy_count,
        history: history_charts(history),
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
    let figures = character_figures(&state.pool, account.id, &selection)
        .await
        .map_err(|e| Box::new(pages::internal_error(e)))?;
    let top_sell = top_items(&state.pool, account.id, &selection, Side::Sell)
        .await
        .map_err(|e| Box::new(pages::internal_error(e)))?;
    let top_buy = top_items(&state.pool, account.id, &selection, Side::Buy)
        .await
        .map_err(|e| Box::new(pages::internal_error(e)))?;
    let now = state.clock.now();
    let range = Range::parse(query.range.as_deref());
    let history = history(&state.pool, account.id, &selection, range, now)
        .await
        .map_err(|e| Box::new(pages::internal_error(e)))?;
    let view = summary_view(
        &figures,
        (&top_sell, &top_buy),
        &history,
        range,
        &selection,
        now,
        state.config.order_poll_interval,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_range_value_selects_its_range() {
        assert_eq!(Range::parse(Some("24h")), Range::Day);
        assert_eq!(Range::parse(Some("7d")), Range::Week);
        assert_eq!(Range::parse(Some("30d")), Range::Month);
        assert_eq!(Range::parse(Some("90d")), Range::Quarter);
    }

    #[test]
    fn a_missing_or_unknown_range_falls_back_to_seven_days() {
        assert_eq!(Range::parse(None), Range::Week);
        assert_eq!(Range::parse(Some("1y")), Range::Week);
        assert_eq!(Range::parse(Some("")), Range::Week);
        assert_eq!(Range::parse(Some("7D")), Range::Week);
    }

    #[test]
    fn names_round_trip_through_parse() {
        for range in Range::ALL {
            assert_eq!(Range::parse(Some(range.name())), range);
        }
    }

    #[test]
    fn spans_and_bucket_widths_follow_the_design() {
        let table = [
            (Range::Day, Duration::hours(24), Duration::minutes(30)),
            (Range::Week, Duration::days(7), Duration::hours(2)),
            (Range::Month, Duration::days(30), Duration::hours(6)),
            (Range::Quarter, Duration::days(90), Duration::days(1)),
        ];
        for (range, span, bucket) in table {
            assert_eq!((range.span(), range.bucket()), (span, bucket), "{range:?}");
        }
    }
}
