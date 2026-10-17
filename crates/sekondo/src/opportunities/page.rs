//! The Opportunities page and its fragments.

use askama::Template;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use tower_sessions::Session;
use uuid::Uuid;

use super::{
    REGION_ID, STATION_ID, STATION_NAME,
    analysis::{self, FeeSource, Fees},
    charts,
    settings::{Form, Settings},
    tables::{
        self, Context, Extra, Row, Sort, SortKey, Table, choose_station_fees, load_own_orders,
        load_stats,
    },
};
use crate::{
    account::CurrentAccount,
    app::AppState,
    csrf, format,
    orders::{Dir, FilterEntry},
    pages::{self, Nav, Page},
    selection::{CharacterSelection, ViewQuery},
    station_book::BOOK_INTERVAL,
};

/// The book is stale when its last success is older than this: twice the refresh interval.
pub fn book_stale_after() -> Duration {
    BOOK_INTERVAL * 2
}

/// What the station sync shows of itself.
pub struct FreshnessView {
    pub book_last: Option<String>,
    pub book_stale: bool,
    /// The book was never refreshed successfully.
    pub book_waiting: bool,
    pub book_error: Option<String>,
    pub history_last: Option<String>,
    pub history_error: Option<String>,
    pub own_days: i64,
    /// How many days of own snapshots the hourly patterns need.
    pub own_days_needed: usize,
    pub own_days_text: String,
}

#[derive(sqlx::FromRow)]
struct SyncRow {
    kind: String,
    last_success_at: Option<OffsetDateTime>,
    last_error: Option<String>,
}

/// Days with an own snapshot, counted on the item with the oldest snapshot: one item's rows are
/// few enough to count on every request, and it spans every day the others do.
async fn own_snapshot_days(pool: &PgPool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(DISTINCT date_trunc('day', recorded_at)) FROM station_snapshots
         WHERE region_id = $1 AND station_id = $2
           AND type_id = (SELECT type_id FROM station_snapshots
                          WHERE region_id = $1 AND station_id = $2
                          ORDER BY recorded_at LIMIT 1)",
    )
    .bind(REGION_ID)
    .bind(STATION_ID)
    .fetch_one(pool)
    .await
}

pub async fn freshness(pool: &PgPool, now: OffsetDateTime) -> Result<FreshnessView, sqlx::Error> {
    let rows: Vec<SyncRow> = sqlx::query_as(
        "SELECT kind, last_success_at, last_error FROM station_sync
         WHERE region_id = $1 AND station_id = $2",
    )
    .bind(REGION_ID)
    .bind(STATION_ID)
    .fetch_all(pool)
    .await?;
    let row = |kind: &str| rows.iter().find(|r| r.kind == kind);
    let book = row("book");
    let book_last = book.and_then(|r| r.last_success_at);
    let days = own_snapshot_days(pool).await?;
    Ok(FreshnessView {
        book_last: book_last.map(format::eve_time),
        book_stale: book_last.is_none_or(|last| now - last > book_stale_after()),
        book_waiting: book_last.is_none(),
        book_error: book.and_then(|r| r.last_error.clone()),
        history_last: row("history")
            .and_then(|r| r.last_success_at)
            .map(format::eve_time),
        history_error: row("history").and_then(|r| r.last_error.clone()),
        own_days: days,
        own_days_needed: analysis::MIN_HOURLY_DAYS,
        own_days_text: match days {
            1 => "1 day of own snapshots".to_owned(),
            n => format!("{n} days of own snapshots"),
        },
    })
}

pub struct HeaderView {
    pub label: &'static str,
    pub href: String,
    /// `ascending`, `descending` or empty: for `aria-sort`.
    pub aria_sort: &'static str,
    pub arrow: &'static str,
    pub numeric: bool,
}

pub struct CellView {
    pub col: &'static str,
    pub text: String,
    /// What hovering the cell adds; empty for nothing.
    pub title: String,
    pub numeric: bool,
}

pub struct RowView {
    pub type_id: i64,
    /// Where the detail window of the row loads from.
    pub detail_url: String,
    pub cells: Vec<CellView>,
    pub own_buy: bool,
    pub own_sell: bool,
}

pub struct TableView {
    pub id: &'static str,
    pub title: &'static str,
    pub headers: Vec<HeaderView>,
    pub rows: Vec<RowView>,
}

/// The filter bar: the stored (or just rejected) values, and the view state it carries along so
/// that saving keeps the selection and the sorting.
pub struct FiltersView {
    pub form: Form,
    pub error: Option<String>,
    pub chars: String,
    /// `(name, value)` of the hidden inputs: every table's sorting.
    pub sorts: Vec<(&'static str, String)>,
}

/// Everything the page and its fragments show for one request.
pub struct OpportunitiesView {
    pub filters: FiltersView,
    pub characters: Vec<FilterEntry>,
    pub station_name: &'static str,
    pub freshness_url: String,
    pub tables_url: String,
    pub freshness: FreshnessView,
    /// Which fees the analyses use.
    pub fees_note: String,
    pub tables: Vec<TableView>,
}

/// The view state a URL carries: the selection and one sorting per table.
struct ViewState {
    chars: Option<String>,
    sorts: [Sort; 4],
}

impl ViewState {
    fn from_query(selection: &CharacterSelection, query: &ViewQuery) -> Self {
        let pair = |table: Table| -> (Option<&str>, Option<&str>) {
            match table {
                Table::Expansion => (query.xs.as_deref(), query.xd.as_deref()),
                Table::Classic => (query.cs.as_deref(), query.cd.as_deref()),
                Table::Reversion => (query.ms.as_deref(), query.md.as_deref()),
                Table::Trend => (query.ts.as_deref(), query.td.as_deref()),
            }
        };
        let sorts = Table::ALL.map(|table| {
            let (key, dir) = pair(table);
            Sort::parse(table, key, dir)
        });
        Self {
            chars: selection.param(),
            sorts,
        }
    }

    fn index(table: Table) -> usize {
        Table::ALL
            .iter()
            .position(|t| *t == table)
            .expect("a table")
    }

    fn sort(&self, table: Table) -> Sort {
        self.sorts[Self::index(table)]
    }

    fn with_sort(&self, table: Table, sort: Sort) -> Self {
        let mut sorts = self.sorts;
        sorts[Self::index(table)] = sort;
        Self {
            chars: self.chars.clone(),
            sorts,
        }
    }

    /// The hidden inputs of the filter bar: every table's sorting, whatever it is.
    fn hidden_sorts(&self) -> Vec<(&'static str, String)> {
        let mut hidden = Vec::new();
        for table in Table::ALL {
            let sort = self.sort(table);
            let (key_param, dir_param) = table.sort_params();
            hidden.push((key_param, sort.key.name().to_owned()));
            hidden.push((
                dir_param,
                if sort.dir == Dir::Asc { "asc" } else { "desc" }.to_owned(),
            ));
        }
        hidden
    }

    /// `""` or `?chars=…&cs=…&cd=…`: only what differs from the defaults.
    fn query(&self) -> String {
        let mut pairs = url::form_urlencoded::Serializer::new(String::new());
        if let Some(chars) = &self.chars {
            // The commas stay readable in the URL, as in bookmarks people write by hand.
            pairs.append_pair("chars", chars);
        }
        for table in Table::ALL {
            let sort = self.sort(table);
            if sort != Sort::default() {
                let (key_param, dir_param) = table.sort_params();
                pairs.append_pair(key_param, sort.key.name());
                pairs.append_pair(dir_param, if sort.dir == Dir::Asc { "asc" } else { "desc" });
            }
        }
        let query = pairs.finish().replace("%2C", ",");
        if query.is_empty() {
            query
        } else {
            format!("?{query}")
        }
    }
}

/// A rejected filter form, shown again with the reason.
struct Rejected {
    form: Form,
    message: String,
}

/// What every request of the page works from: the account's characters and selection, the view
/// state of the URL, the settings, the fees, the own orders and the station's items.
struct Environment {
    characters: Vec<(i64, String)>,
    selection: CharacterSelection,
    view_state: ViewState,
    settings: Settings,
    fee_choice: tables::FeeChoice,
    own: tables::OwnOrders,
    stats: Vec<tables::Stats>,
    now: OffsetDateTime,
}

impl Environment {
    fn context(&self) -> Context<'_> {
        Context {
            settings: &self.settings,
            fees: self.fee_choice.fees,
            own: &self.own,
            now: self.now,
        }
    }

    fn character_name(&self, id: i64) -> String {
        self.characters
            .iter()
            .find(|(character, _)| *character == id)
            .map_or_else(|| id.to_string(), |(_, name)| name.clone())
    }
}

async fn environment(
    state: &AppState,
    account_id: Uuid,
    query: &ViewQuery,
) -> Result<Option<Environment>, sqlx::Error> {
    let characters = state
        .repo
        .list_characters(account_id)
        .await
        .map_err(|e| match e {
            crate::repo::RepoError::Db(e) => e,
            other => sqlx::Error::Protocol(other.to_string()),
        })?;
    if characters.is_empty() {
        return Ok(None);
    }
    let ids: Vec<i64> = characters.iter().map(|c| c.character_id).collect();
    let selection = query.selection(&ids);
    let view_state = ViewState::from_query(&selection, query);
    Ok(Some(Environment {
        settings: Settings::load(&state.pool, account_id).await?,
        fee_choice: choose_station_fees(&state.pool, account_id, selection.ids()).await?,
        own: load_own_orders(&state.pool, account_id, selection.ids()).await?,
        stats: load_stats(&state.pool).await?,
        now: state.clock.now(),
        characters: characters
            .iter()
            .map(|c| (c.character_id, c.name.clone()))
            .collect(),
        selection,
        view_state,
    }))
}

async fn build_view(
    state: &AppState,
    account_id: Uuid,
    query: &ViewQuery,
    rejected: Option<Rejected>,
) -> Result<Option<(OpportunitiesView, CharacterSelection)>, sqlx::Error> {
    let Some(env) = environment(state, account_id, query).await? else {
        return Ok(None);
    };
    let context = env.context();
    let page_query = env.view_state.query();
    let tables = Table::ALL
        .into_iter()
        .map(|table| {
            let sort = env.view_state.sort(table);
            TableView {
                id: table.id(),
                title: table.title(),
                headers: headers(&env.view_state, table),
                rows: tables::rows(table, &env.stats, &context, sort)
                    .iter()
                    .map(|row| row_view(table, row, &page_query))
                    .collect(),
            }
        })
        .collect();
    let (form, error) = match rejected {
        Some(Rejected { form, message }) => (form, Some(message)),
        None => (Form::of(&env.settings), None),
    };
    let view = OpportunitiesView {
        filters: FiltersView {
            form,
            error,
            chars: env.selection.param().unwrap_or_default(),
            sorts: env.view_state.hidden_sorts(),
        },
        characters: env
            .characters
            .iter()
            .map(|(id, name)| FilterEntry {
                id: *id,
                name: name.clone(),
                checked: env.selection.contains(*id),
            })
            .collect(),
        station_name: STATION_NAME,
        freshness_url: format!("/opportunities/freshness{}", env.selection.query()),
        tables_url: format!("/opportunities/tables{page_query}"),
        freshness: freshness(&state.pool, env.now).await?,
        fees_note: fees_note(&env.fee_choice, |id| env.character_name(id)),
        tables,
    };
    Ok(Some((view, env.selection.clone())))
}

fn fees_note(choice: &tables::FeeChoice, name: impl Fn(i64) -> String) -> String {
    let rates = |fees: Fees| {
        format!(
            "broker fee {}, sales tax {}",
            format::percent(fees.broker),
            format::percent(fees.tax)
        )
    };
    match (choice.source, choice.base_rates) {
        (FeeSource::Character(id), false) => {
            format!("Fees of {}: {}", name(id), rates(choice.fees))
        }
        _ => format!(
            "Fees: base rates ({}) until a character's skills and standings are known",
            rates(choice.fees)
        ),
    }
}

fn headers(state: &ViewState, table: Table) -> Vec<HeaderView> {
    let current = state.sort(table);
    table
        .keys()
        .into_iter()
        .map(|key| {
            let sorted_by_this = current.key == key;
            let next = Sort {
                key,
                dir: if sorted_by_this {
                    current.dir.reversed()
                } else if key.is_numeric() {
                    Dir::Desc
                } else {
                    Dir::Asc
                },
            };
            let (aria_sort, arrow) = match (sorted_by_this, current.dir) {
                (false, _) => ("", ""),
                (true, Dir::Asc) => ("ascending", " \u{25B2}"),
                (true, Dir::Desc) => ("descending", " \u{25BC}"),
            };
            HeaderView {
                label: key.label(),
                href: format!("/opportunities{}", state.with_sort(table, next).query()),
                aria_sort,
                arrow,
                numeric: key.is_numeric(),
            }
        })
        .collect()
}

fn isk(value: f64) -> String {
    format::isk_short((value * 100.0).round() as i64)
}

fn whole(value: f64) -> String {
    if value.is_finite() {
        format::thousands(value.round() as i64)
    } else {
        "\u{2014}".to_owned()
    }
}

fn row_view(table: Table, row: &Row, page_query: &str) -> RowView {
    let cell = |key: SortKey| {
        let (text, title) = match key {
            SortKey::Item => (row.name.clone(), String::new()),
            SortKey::Buy => (
                format::isk_short(row.best_buy_cents),
                format::isk_exact(row.best_buy_cents),
            ),
            SortKey::Sell => (
                format::isk_short(row.sell_cents),
                format::isk_exact(row.sell_cents),
            ),
            SortKey::Net => (isk(row.figures.net_per_unit), String::new()),
            SortKey::Margin => (format::percent(row.figures.margin), String::new()),
            SortKey::IskPerDay => (isk(row.figures.isk_per_day), String::new()),
            SortKey::V7 => (whole(row.v7), String::new()),
            SortKey::V30 => (whole(row.v30), String::new()),
            SortKey::Sigma => (format::percent(row.sigma), String::new()),
            SortKey::TMax => (format::days(row.figures.t_max), String::new()),
            SortKey::TFill => (format::days(row.figures.t_fill), String::new()),
            SortKey::MinVolume => (whole(row.figures.min_volume), String::new()),
            SortKey::Near => (
                format!("{} / {}", row.near_buy, row.near_sell),
                String::new(),
            ),
            SortKey::Window => (row.window_label(), String::new()),
            SortKey::NextStart => (
                match &row.extra {
                    Extra::Expansion { next_start, .. } => format::eve_time(*next_start),
                    _ => String::new(),
                },
                String::new(),
            ),
            SortKey::PercentB => (
                match &row.extra {
                    Extra::Reversion { percent_b, .. } => format!("{percent_b:.2}"),
                    _ => String::new(),
                },
                String::new(),
            ),
            SortKey::Rsi => (
                match &row.extra {
                    Extra::Reversion { rsi, .. } => format!("{rsi:.1}"),
                    _ => String::new(),
                },
                String::new(),
            ),
            SortKey::Sma5 => (
                match &row.extra {
                    Extra::Trend { sma5, .. } => isk(*sma5),
                    _ => String::new(),
                },
                String::new(),
            ),
            SortKey::Sma20 => (
                match &row.extra {
                    Extra::Trend { sma20, .. } => isk(*sma20),
                    _ => String::new(),
                },
                String::new(),
            ),
            SortKey::Cross => (
                match &row.extra {
                    Extra::Trend { cross_day, .. } => cross_day.to_string(),
                    _ => String::new(),
                },
                String::new(),
            ),
        };
        CellView {
            col: key.name(),
            text,
            title,
            numeric: key.is_numeric(),
        }
    };
    RowView {
        type_id: row.type_id,
        detail_url: format!(
            "/opportunities/detail/{}/{}{page_query}",
            table.id(),
            row.type_id
        ),
        cells: table.keys().into_iter().map(cell).collect(),
        own_buy: row.own_buy,
        own_sell: row.own_sell,
    }
}

#[derive(Template)]
#[template(path = "opportunities.html")]
struct OpportunitiesPage {
    csrf_token: String,
    nav: Option<Nav>,
    view: OpportunitiesView,
}

#[derive(Template)]
#[template(path = "opportunities_filters.html")]
struct FiltersFragment {
    view: OpportunitiesView,
}

#[derive(Template)]
#[template(path = "opportunities_freshness.html")]
struct FreshnessFragment {
    view: OpportunitiesView,
}

#[derive(Template)]
#[template(path = "opportunities_tables.html")]
struct TablesFragment {
    view: OpportunitiesView,
}

async fn load(
    state: &AppState,
    account: CurrentAccount,
    session: &Session,
    query: &ViewQuery,
) -> Result<(OpportunitiesView, CharacterSelection), Box<Response>> {
    load_with(state, account, session, query, None).await
}

async fn load_with(
    state: &AppState,
    account: CurrentAccount,
    session: &Session,
    query: &ViewQuery,
    rejected: Option<Rejected>,
) -> Result<(OpportunitiesView, CharacterSelection), Box<Response>> {
    match build_view(state, account.id, query, rejected).await {
        Ok(Some(loaded)) => Ok(loaded),
        Ok(None) => {
            // An account without characters cannot exist; the session outlived its account.
            let _ = session.flush().await;
            Err(Box::new(Redirect::to("/").into_response()))
        }
        Err(e) => Err(Box::new(pages::internal_error(e))),
    }
}

pub async fn page(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    query: ViewQuery,
) -> Response {
    let (view, selection) = match load(&state, account, &session, &query).await {
        Ok(loaded) => loaded,
        Err(response) => return *response,
    };
    let csrf_token = match csrf::token(&session).await {
        Ok(token) => token,
        Err(e) => return pages::internal_error(e),
    };
    pages::render(&OpportunitiesPage {
        csrf_token,
        nav: Some(Nav::new(Page::Opportunities).with_chars_query(selection.query())),
        view,
    })
}

/// The freshness section only, for live refreshes.
pub async fn freshness_fragment(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    query: ViewQuery,
) -> Response {
    match load(&state, account, &session, &query).await {
        Ok((view, _)) => pages::render(&FreshnessFragment { view }),
        Err(response) => *response,
    }
}

/// The four tables only, for live refreshes.
pub async fn tables(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    query: ViewQuery,
) -> Response {
    match load(&state, account, &session, &query).await {
        Ok((view, _)) => pages::render(&TablesFragment { view }),
        Err(response) => *response,
    }
}

/// Saves the filter bar. The form carries the view state (selection and sorting) as hidden
/// fields, so the answer is the tables of the view the user is looking at; a rejected form gets
/// the filter bar back with the reason and nothing is stored.
pub async fn save_settings(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    body: String,
) -> Response {
    let form = Form::from_pairs(url::form_urlencoded::parse(body.as_bytes()).into_owned());
    let query = ViewQuery::parse(Some(&body));
    match form.validate() {
        Ok(settings) => {
            if let Err(e) = settings.save(&state.pool, account.id).await {
                return pages::internal_error(e);
            }
            match load(&state, account, &session, &query).await {
                Ok((view, _)) => pages::render(&TablesFragment { view }),
                Err(response) => *response,
            }
        }
        Err(message) => {
            let rejected = Rejected { form, message };
            match load_with(&state, account, &session, &query, Some(rejected)).await {
                Ok((view, _)) => {
                    pages::render_with(StatusCode::UNPROCESSABLE_ENTITY, &FiltersFragment { view })
                }
                Err(response) => *response,
            }
        }
    }
}

// ---- the detail window ----

pub struct FigureView {
    pub key: &'static str,
    pub label: &'static str,
    pub value: String,
    /// How the figure follows from the others, with this item's numbers.
    pub how: String,
}

/// One chart of the window: its kind (how the script draws it), a title, and its data as JSON.
pub struct ChartView {
    pub kind: &'static str,
    pub title: &'static str,
    pub json: String,
}

pub struct DetailView {
    pub name: String,
    pub table_title: &'static str,
    pub charts: Vec<ChartView>,
    /// The starts of the next occurrences of the window of an expansion row.
    pub next_occurrences: Vec<String>,
    /// Why the table does not list the item (right now); `None` while it does.
    pub reason: Option<String>,
    pub fees_note: String,
    pub figures: Vec<FigureView>,
    /// Where this window's content loads from, so a live refresh can fetch it again.
    pub refresh_url: String,
}

fn figure(
    key: &'static str,
    label: &'static str,
    value: String,
    how: impl Into<String>,
) -> FigureView {
    FigureView {
        key,
        label,
        value,
        how: how.into(),
    }
}

fn detail_figures(
    table: Table,
    detail: &tables::Detail<'_>,
    context: &Context<'_>,
) -> Vec<FigureView> {
    let stats = detail.stats;
    let fees = context.fees;
    let mut figures = vec![
        figure(
            "buy",
            "Current best buy",
            format::isk_short(stats.best_buy_cents),
            "the highest buy order of the station book now",
        ),
        figure(
            "best-sell",
            "Current best sell",
            format::isk_short(stats.best_sell_cents),
            "the lowest sell order of the station book now",
        ),
    ];
    let Some(sell) = detail.sell_cents else {
        return figures;
    };
    let how_sell = match table {
        Table::Classic | Table::Trend => "one tick below the current best sell".to_owned(),
        Table::Reversion => {
            "the SMA 20 of the daily averages, rounded down to a valid price".to_owned()
        }
        Table::Expansion => format!(
            "one tick below the 25th percentile of the peaks of the window {}",
            detail
                .extra
                .as_ref()
                .map_or_else(String::new, |extra| match extra {
                    Extra::Expansion { window, .. } => window.label(),
                    _ => String::new(),
                })
        ),
    };
    figures.push(figure(
        "sell",
        "Sell at (S)",
        format::isk_short(sell),
        how_sell,
    ));
    if let Some(sigma) = stats.sigma {
        figures.push(figure(
            "sigma",
            "Volatility (\u{03C3})",
            format::percent(sigma),
            "sample standard deviation of the daily log returns of the average price over the last 30 days",
        ));
    }
    let Some(f) = &detail.figures else {
        return figures;
    };
    let buy = format::isk_short(stats.best_buy_cents);
    let sell_text = format::isk_short(sell);
    let shares = |n: i32| format!("1 / ({n} + 1)");
    let rho = stats.rho.unwrap_or(0.5);
    figures.extend([
        figure(
            "cost",
            "Cost per unit (c)",
            isk(f.cost),
            format!("B \u{00D7} (1 + b) = {buy} \u{00D7} {:.4}", 1.0 + fees.broker),
        ),
        figure(
            "revenue",
            "Revenue per unit (r)",
            isk(f.revenue),
            format!(
                "S \u{00D7} (1 \u{2212} b \u{2212} t) = {sell_text} \u{00D7} {:.5}",
                1.0 - fees.broker - fees.tax
            ),
        ),
        figure("net", "Net per unit", isk(f.net_per_unit), "r \u{2212} c"),
        figure("margin", "Net margin (m)", format::percent(f.margin), "(r \u{2212} c) / c"),
        figure(
            "units",
            "Order size (U)",
            format::thousands(f.units),
            format!(
                "\u{230A}capital / c\u{230B} = \u{230A}{} / {}\u{230B}",
                format::isk_short((context.settings.capital * 100.0).round() as i64),
                isk(f.cost)
            ),
        ),
        figure(
            "v7",
            "Volume per day, 7 days (V)",
            whole(stats.volume_7d.unwrap_or(0.0)),
            "traded volume per day, averaged over the last 7 days of history",
        ),
        figure(
            "v30",
            "Volume per day, 30 days",
            whole(stats.volume_30d.unwrap_or(0.0)),
            "traded volume per day, averaged over the last 30 days of history",
        ),
        figure(
            "rho",
            "Buy side ratio (\u{03C1})",
            format!("{rho:.2}"),
            "the share of the units taken from buy orders over 30 days of snapshots; 0.5 when none were taken",
        ),
        figure(
            "q-buy",
            "Daily capture, buy side",
            whole(f.q_buy),
            format!("{} \u{00D7} V \u{00D7} \u{03C1}", shares(stats.buy_near_orders)),
        ),
        figure(
            "q-sell",
            "Daily capture, sell side",
            whole(f.q_sell),
            format!(
                "{} \u{00D7} V \u{00D7} (1 \u{2212} \u{03C1})",
                shares(stats.sell_near_orders)
            ),
        ),
        figure("t-fill", "Fill time", format::days(f.t_fill), "U / q_buy + U / q_sell"),
        figure(
            "t-max",
            "Allowed holding time (T max)",
            format::days(f.t_max),
            "min(3, (m / 2\u{03C3})\u{00B2}) days",
        ),
        figure(
            "min-volume",
            "Minimum daily volume",
            whole(f.min_volume),
            "the volume at which the fill time equals T max",
        ),
        figure(
            "isk-day",
            "ISK/day",
            isk(f.isk_per_day),
            "(r \u{2212} c) \u{00D7} U / fill time",
        ),
    ]);
    figures
}

/// How far back the charts and the recurrence look, in days.
const CHART_DAYS: i64 = 30;

fn chart_view(kind: &'static str, title: &'static str, chart: &charts::Chart) -> Option<ChartView> {
    match serde_json::to_string(chart) {
        Ok(json) => Some(ChartView { kind, title, json }),
        Err(e) => {
            tracing::error!("cannot serialize the {kind} chart: {e}");
            None
        }
    }
}

async fn history_days(
    pool: &PgPool,
    type_id: i64,
    now: OffsetDateTime,
) -> Result<Vec<charts::Day>, sqlx::Error> {
    let rows: Vec<(time::Date, f64, f64)> = sqlx::query_as(
        "SELECT day, average::float8, volume::float8 FROM market_history
         WHERE region_id = $1 AND type_id = $2 AND day >= $3 ORDER BY day",
    )
    .bind(REGION_ID)
    .bind(type_id)
    .bind((now - time::Duration::days(CHART_DAYS)).date())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(date, average, volume)| charts::Day {
            date,
            average,
            volume,
        })
        .collect())
}

async fn price_points(
    pool: &PgPool,
    type_id: i64,
    now: OffsetDateTime,
) -> Result<Vec<charts::PricePoint>, sqlx::Error> {
    let rows: Vec<(OffsetDateTime, f64, f64, f64)> = sqlx::query_as(
        "SELECT bucket, best_buy_avg::float8, best_sell_avg::float8, spread_avg::float8
         FROM station_hourly
         WHERE region_id = $1 AND station_id = $2 AND type_id = $3 AND bucket >= $4
         ORDER BY bucket",
    )
    .bind(REGION_ID)
    .bind(STATION_ID)
    .bind(type_id)
    .bind(now - time::Duration::days(CHART_DAYS))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(at, best_buy, best_sell, spread)| charts::PricePoint {
            at,
            best_buy,
            best_sell,
            spread,
        })
        .collect())
}

async fn latest_depth(pool: &PgPool, type_id: i64) -> Result<Option<charts::Depth>, sqlx::Error> {
    let row: Option<(i64, i32, i64, i32)> = sqlx::query_as(
        "SELECT buy_near_units, buy_near_orders, sell_near_units, sell_near_orders
         FROM station_snapshots
         WHERE region_id = $1 AND station_id = $2 AND type_id = $3
         ORDER BY recorded_at DESC LIMIT 1",
    )
    .bind(REGION_ID)
    .bind(STATION_ID)
    .bind(type_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(
        |(buy_units, buy_orders, sell_units, sell_orders)| charts::Depth {
            buy_units,
            buy_orders,
            sell_units,
            sell_orders,
        },
    ))
}

async fn hour_points(
    pool: &PgPool,
    type_id: i64,
    now: OffsetDateTime,
) -> Result<Vec<analysis::HourPoint>, sqlx::Error> {
    let rows: Vec<(OffsetDateTime, f64, f64, i64)> = sqlx::query_as(
        "SELECT bucket, spread_avg::float8, spread_max::float8, round(best_sell_max * 100)::bigint
         FROM station_hourly
         WHERE region_id = $1 AND station_id = $2 AND type_id = $3 AND bucket >= $4
         ORDER BY bucket",
    )
    .bind(REGION_ID)
    .bind(STATION_ID)
    .bind(type_id)
    .bind(now - time::Duration::days(CHART_DAYS))
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(start, spread_avg, spread_max, sell_max_cents)| analysis::HourPoint {
                start,
                spread_avg,
                spread_max,
                sell_max_cents,
            },
        )
        .collect())
}

async fn day_points(
    pool: &PgPool,
    type_id: i64,
    now: OffsetDateTime,
) -> Result<Vec<analysis::DayPoint>, sqlx::Error> {
    let rows: Vec<(time::Date, f64, i64)> = sqlx::query_as(
        "SELECT day, spread::float8, round(high * 100)::bigint FROM station_daily_figures
         WHERE region_id = $1 AND type_id = $2 AND day >= $3 ORDER BY day",
    )
    .bind(REGION_ID)
    .bind(type_id)
    .bind((now - time::Duration::days(CHART_DAYS)).date())
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(day, spread, high_cents)| analysis::DayPoint {
            day,
            spread,
            high_cents,
        })
        .collect())
}

/// The charts of the window of one item in one table, from what is stored of it. A chart
/// without data is left out.
async fn detail_charts(
    pool: &PgPool,
    table: Table,
    stats: &tables::Stats,
    now: OffsetDateTime,
) -> Result<(Vec<ChartView>, Vec<String>), sqlx::Error> {
    let type_id = stats.type_id;
    let mut views = Vec::new();
    let mut next_occurrences = Vec::new();
    match table {
        Table::Trend => {
            let days = history_days(pool, type_id, now).await?;
            if !days.is_empty() {
                views.extend(chart_view(
                    "trend-price",
                    "Daily average with SMA 5 and SMA 20",
                    &charts::trend_price(&days),
                ));
                views.extend(chart_view(
                    "trend-volume",
                    "Daily volume",
                    &charts::trend_volume(&days),
                ));
            }
        }
        Table::Reversion => {
            let days = history_days(pool, type_id, now).await?;
            if !days.is_empty() {
                views.extend(chart_view(
                    "reversion-price",
                    "Daily average with SMA 20 and Bollinger bands",
                    &charts::reversion_price(&days),
                ));
                views.extend(chart_view("rsi", "RSI 14", &charts::rsi(&days)));
            }
        }
        Table::Classic => {
            let points = price_points(pool, type_id, now).await?;
            if !points.is_empty() {
                views.extend(chart_view(
                    "classic-prices",
                    "Best buy and best sell",
                    &charts::classic_prices(&points),
                ));
                views.extend(chart_view(
                    "classic-spread",
                    "Spread",
                    &charts::classic_spread(&points),
                ));
            }
            if let Some(depth) = latest_depth(pool, type_id).await? {
                views.extend(chart_view(
                    "depth",
                    "Units within 1% of the best price",
                    &charts::depth(&depth),
                ));
            }
        }
        Table::Expansion => {
            let hours = hour_points(pool, type_id, now).await?;
            let days = day_points(pool, type_id, now).await?;
            if !hours.is_empty() {
                views.extend(chart_view(
                    "spread-hours",
                    "Spread by hour of day (EVE time)",
                    &charts::spread_by_hour(&hours),
                ));
            }
            if !days.is_empty() {
                views.extend(chart_view(
                    "spread-weekdays",
                    "Spread by weekday",
                    &charts::spread_by_weekday(&days),
                ));
            }
            let windows = analysis::recurring_windows(&hours, &days, now);
            let window = stats
                .window()
                .and_then(|kind| windows.iter().find(|w| w.kind == kind))
                .or_else(|| windows.first());
            if let Some(window) = window {
                views.extend(chart_view(
                    "peaks",
                    "Past peaks of the window, with S",
                    &charts::peaks(window),
                ));
                next_occurrences = charts::next_occurrences(window.kind, now, 3);
            }
        }
    }
    Ok((views, next_occurrences))
}

#[derive(Template)]
#[template(path = "opportunities_detail.html")]
struct DetailFragment {
    view: DetailView,
}

/// The detail window of one item in one table: the fees, the shared figures with how they
/// follow, and whether the table lists the item right now. An item that dropped out is still
/// shown, from what is known of it.
pub async fn detail(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    Path((table_id, type_id)): Path<(String, i64)>,
    query: ViewQuery,
) -> Response {
    let Some(table) = Table::from_id(&table_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let env = match environment(&state, account.id, &query).await {
        Ok(Some(env)) => env,
        Ok(None) => {
            let _ = session.flush().await;
            return Redirect::to("/").into_response();
        }
        Err(e) => return pages::internal_error(e),
    };
    let context = env.context();
    let sort = env.view_state.sort(table);
    let refresh_url = format!(
        "/opportunities/detail/{}/{type_id}{}",
        table.id(),
        env.view_state.query()
    );
    let fees_note = fees_note(&env.fee_choice, |id| env.character_name(id));
    let view = match tables::detail(table, type_id, &env.stats, &context, sort) {
        Some(detail) => {
            let (charts, next_occurrences) =
                match detail_charts(&state.pool, table, detail.stats, env.now).await {
                    Ok(drawn) => drawn,
                    Err(e) => return pages::internal_error(e),
                };
            DetailView {
                name: detail.stats.display_name(),
                table_title: table.title(),
                charts,
                next_occurrences,
                reason: detail.reason.clone(),
                fees_note,
                figures: detail_figures(table, &detail, &context),
                refresh_url,
            }
        }
        None => {
            let name: Option<String> =
                match sqlx::query_scalar("SELECT name FROM names WHERE id = $1")
                    .bind(type_id)
                    .fetch_optional(&state.pool)
                    .await
                {
                    Ok(name) => name.flatten(),
                    Err(e) => return pages::internal_error(e),
                };
            DetailView {
                name: name.unwrap_or_else(|| format!("Type {type_id}")),
                table_title: table.title(),
                charts: Vec::new(),
                next_occurrences: Vec::new(),
                reason: Some("The station no longer trades it.".to_owned()),
                fees_note,
                figures: Vec::new(),
                refresh_url,
            }
        }
    };
    pages::render(&DetailFragment { view })
}
