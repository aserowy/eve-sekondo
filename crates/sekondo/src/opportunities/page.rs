//! The Opportunities page and its fragments.

use askama::Template;
use axum::{
    extract::{Path, State},
    http::{StatusCode, Uri},
    response::{IntoResponse, Redirect, Response},
};
use time::{Duration, OffsetDateTime};
use tower_sessions::Session;
use uuid::Uuid;

use super::{
    REGION_ID, STATION_ID, STATION_NAME,
    analysis::{self, FeeSource},
    charts,
    legend::{self, LegendEntry, Term},
    settings::{FieldError, Form, Settings},
    tables::{
        self, Context, Extra, Row, Sort, SortKey, Table, choose_station_fees, load_own_orders,
    },
};
use crate::{
    account::CurrentAccount,
    app::AppState,
    csrf, format,
    orders::{Dir, FilterEntry},
    pages::{self, Nav, Page},
    pagination::{Link, PageView, Paging, paginate},
    selection::{CharacterSelection, ViewQuery},
    station_book::BOOK_INTERVAL,
    store::{OpportunityStore, StoreError},
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

pub async fn freshness(
    store: &dyn OpportunityStore,
    now: OffsetDateTime,
) -> Result<FreshnessView, StoreError> {
    let rows = store.station_sync(REGION_ID, STATION_ID).await?;
    let row = |kind: &str| rows.iter().find(|r| r.kind == kind);
    let book = row("book");
    let book_last = book.and_then(|r| r.last_success_at);
    let days = store.own_snapshot_days(REGION_ID, STATION_ID).await?;
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
    /// The full name, shown on hover; the Item header has none.
    pub title: &'static str,
    /// The page that the sorting leads to, for the address bar.
    pub href: String,
    /// The content fragment that htmx fetches for it.
    pub content_href: String,
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
    /// The detail page of the row's item in its table.
    pub href: String,
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

/// The filter box: the stored (or just rejected) values, and the view state it carries along so
/// that saving keeps the selection and the sorting.
pub struct FiltersView {
    pub form: Form,
    pub error: Option<FieldError>,
    pub chars: String,
    /// `(name, value)` of the hidden inputs: every table's sorting.
    pub sorts: Vec<(&'static str, String)>,
    /// The table of the page that saves, and its page size: saving shows page 1 of it.
    pub table: &'static str,
    pub per_page: u32,
}

impl FiltersView {
    /// The message for the input called `name`, when that is the field the form was rejected for.
    pub fn error_for(&self, name: &str) -> Option<&str> {
        self.error
            .as_ref()
            .filter(|error| error.field.name() == name)
            .map(|error| error.message.as_str())
    }
}

/// One link of the page sub-menu: a table's page.
pub struct SubMenuLink {
    pub label: &'static str,
    pub href: String,
    pub current: bool,
}

/// Everything the page and its fragments show for one request.
pub struct OpportunitiesView {
    pub table_id: &'static str,
    pub filters: FiltersView,
    pub characters: Vec<FilterEntry>,
    pub station_name: &'static str,
    /// The path of the table's page, without a query: the character filter builds its own.
    pub page_path: String,
    pub freshness_url: String,
    /// The table's content fragment with the whole view state, for live refreshes.
    pub content_url: String,
    pub content_path: String,
    pub sub_menu: Vec<SubMenuLink>,
    pub freshness: FreshnessView,
    pub fees: FeeGroupView,
    pub table: TableView,
    pub pager: PageView,
    /// The legend of the explainer card: the table's columns.
    pub legend: Vec<LegendEntry>,
}

/// The view state a URL carries: the selection, one sorting per table and the paging of the
/// table shown.
struct ViewState {
    chars: Option<String>,
    sorts: [Sort; 4],
    paging: Paging,
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
            paging: query.paging(),
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

    /// Another sorting of one table, from the first page.
    fn with_sort(&self, table: Table, sort: Sort) -> Self {
        let mut sorts = self.sorts;
        sorts[Self::index(table)] = sort;
        Self {
            chars: self.chars.clone(),
            sorts,
            paging: self.paging.first(),
        }
    }

    fn with_paging(&self, paging: Paging) -> Self {
        Self {
            chars: self.chars.clone(),
            sorts: self.sorts,
            paging,
        }
    }

    /// The hidden inputs of the filter box: every table's sorting, whatever it is.
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

    /// `""` or `?chars=…&cs=…&cd=…&page=…`: only what differs from the defaults.
    fn query(&self, with_paging: bool) -> String {
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
        let mut query = pairs.finish().replace("%2C", ",");
        if with_paging {
            let paging = self.paging.query();
            if !paging.is_empty() {
                if !query.is_empty() {
                    query.push('&');
                }
                query.push_str(&paging);
            }
        }
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
    error: FieldError,
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
}

async fn environment(
    state: &AppState,
    account_id: Uuid,
    query: &ViewQuery,
) -> Result<Option<Environment>, StoreError> {
    let characters = state
        .repo
        .list_characters(account_id)
        .await
        .map_err(|e| match e {
            crate::repo::RepoError::Db(e) => e,
            other => StoreError::Backend(other.to_string()),
        })?;
    if characters.is_empty() {
        return Ok(None);
    }
    let ids: Vec<i64> = characters.iter().map(|c| c.character_id).collect();
    let selection = query.selection(&ids);
    let view_state = ViewState::from_query(&selection, query);
    Ok(Some(Environment {
        settings: Settings::load(&*state.stores.opportunities, account_id).await?,
        fee_choice: choose_station_fees(
            &*state.stores.fees,
            account_id,
            selection.ids(),
            state
                .stores
                .opportunities
                .load_fee_character(account_id)
                .await?,
        )
        .await?,
        own: load_own_orders(&*state.stores.opportunities, account_id, selection.ids()).await?,
        stats: state
            .stores
            .opportunities
            .load_stats(REGION_ID, STATION_ID)
            .await?,
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
    table: Table,
    query: &ViewQuery,
    rejected: Option<Rejected>,
) -> Result<Option<(OpportunitiesView, CharacterSelection)>, StoreError> {
    let Some(mut env) = environment(state, account_id, query).await? else {
        return Ok(None);
    };
    let context = env.context();
    let sort = env.view_state.sort(table);
    // Every row that passes the rules and the filters, sorted; the page shows a slice of them.
    let all_rows = tables::rows(table, &env.stats, &context, sort);
    let link_to = |paging: Paging| {
        let query = env.view_state.with_paging(paging).query(true);
        Link {
            href: format!("/opportunities/{}{query}", table.id()),
            content: format!("/opportunities/{}/content{query}", table.id()),
        }
    };
    let (page_rows, pager) = paginate(&all_rows, env.view_state.paging, link_to);
    // What the URLs below carry is the page actually shown, after clamping.
    env.view_state.paging = pager.paging;
    let detail_query = env.view_state.query(false);
    let page_query = env.view_state.query(true);
    let table_view = TableView {
        id: table.id(),
        title: table.title(),
        headers: headers(&env.view_state, table),
        rows: page_rows
            .iter()
            .map(|row| row_view(table, row, &detail_query))
            .collect(),
    };
    let (form, error) = match rejected {
        Some(Rejected { form, error }) => (form, Some(error)),
        None => (Form::of(&env.settings), None),
    };
    let view = OpportunitiesView {
        table_id: table.id(),
        filters: FiltersView {
            form,
            error,
            chars: env.selection.param().unwrap_or_default(),
            sorts: env.view_state.hidden_sorts(),
            table: table.id(),
            per_page: env.view_state.paging.per_page,
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
        page_path: format!("/opportunities/{}", table.id()),
        freshness_url: format!("/opportunities/freshness{}", env.selection.query()),
        content_url: format!("/opportunities/{}/content{page_query}", table.id()),
        content_path: format!("/opportunities/{}/content", table.id()),
        sub_menu: Table::ALL
            .map(|link| SubMenuLink {
                label: link.title(),
                href: format!("/opportunities/{}{detail_query}", link.id()),
                current: link == table,
            })
            .into(),
        freshness: freshness(&*state.stores.opportunities, env.now).await?,
        fees: fee_group(&env, table, None),
        table: table_view,
        pager,
        legend: legend::entries(&legend::table_terms(table)),
    };
    Ok(Some((view, env.selection.clone())))
}

/// One radio button of the Fees group.
pub struct FeeRadio {
    pub id: i64,
    pub name: String,
    pub checked: bool,
}

/// The Fees group of the filter section: a radio button per selected character with the fee
/// character checked, the rates in use, and what the form posts along.
pub struct FeeGroupView {
    pub radios: Vec<FeeRadio>,
    pub note: String,
    /// The table of the page that saves, and the item of a detail page.
    pub table: &'static str,
    pub type_id: Option<i64>,
}

fn fee_group(env: &Environment, table: Table, type_id: Option<i64>) -> FeeGroupView {
    let choice = &env.fee_choice;
    let rates = format!(
        "broker fee {}, sales tax {}",
        format::percent(choice.fees.broker),
        format::percent(choice.fees.tax)
    );
    let note = match (choice.source, choice.base_rates) {
        (FeeSource::Character(_), false) => {
            let mut note = rates;
            note[..1].make_ascii_uppercase();
            note
        }
        (FeeSource::Character(_), true) | (FeeSource::Base, _) => format!("Base rates: {rates}"),
    };
    FeeGroupView {
        radios: env
            .characters
            .iter()
            .filter(|(id, _)| env.selection.contains(*id))
            .map(|(id, name)| FeeRadio {
                id: *id,
                name: name.clone(),
                checked: choice.source == FeeSource::Character(*id),
            })
            .collect(),
        note,
        table: table.id(),
        type_id,
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
            let query = state.with_sort(table, next).query(true);
            HeaderView {
                label: key.label(),
                title: if key == SortKey::Item {
                    ""
                } else {
                    key.term().name()
                },
                href: format!("/opportunities/{}{query}", table.id()),
                content_href: format!("/opportunities/{}/content{query}", table.id()),
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
        href: format!("/opportunities/{}/{}{page_query}", table.id(), row.type_id),
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

/// The filter form alone, with the rejected field marked: the answer to a rejected filter form.
#[derive(Template)]
#[template(path = "opportunities_filter_error_fragment.html")]
struct FilterErrorFragment {
    view: OpportunitiesView,
}

#[derive(Template)]
#[template(path = "opportunities_freshness.html")]
struct FreshnessFragment {
    view: OpportunitiesView,
}

/// The table with its pagination, and out of band the filter box's context text.
#[derive(Template)]
#[template(path = "opportunities_content_fragment.html")]
struct ContentFragment {
    view: OpportunitiesView,
}

async fn load(
    state: &AppState,
    account: CurrentAccount,
    session: &Session,
    table: Table,
    query: &ViewQuery,
) -> Result<(OpportunitiesView, CharacterSelection), Box<Response>> {
    load_with(state, account, session, table, query, None).await
}

async fn load_with(
    state: &AppState,
    account: CurrentAccount,
    session: &Session,
    table: Table,
    query: &ViewQuery,
    rejected: Option<Rejected>,
) -> Result<(OpportunitiesView, CharacterSelection), Box<Response>> {
    match build_view(state, account.id, table, query, rejected).await {
        Ok(Some(loaded)) => Ok(loaded),
        Ok(None) => {
            // An account without characters cannot exist; the session outlived its account.
            let _ = session.flush().await;
            Err(Box::new(Redirect::to("/").into_response()))
        }
        Err(e) => Err(Box::new(pages::internal_error(e))),
    }
}

/// `/opportunities` is the first table's page, with whatever the old link carried.
pub async fn index(uri: Uri) -> Response {
    let target = match uri.query() {
        Some(query) => format!("/opportunities/{}?{query}", Table::Expansion.id()),
        None => format!("/opportunities/{}", Table::Expansion.id()),
    };
    Redirect::to(&target).into_response()
}

fn table_of(id: &str) -> Result<Table, Box<Response>> {
    Table::from_id(id).ok_or_else(|| Box::new(StatusCode::NOT_FOUND.into_response()))
}

/// The page of one table.
pub async fn table_page(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    Path(table_id): Path<String>,
    query: ViewQuery,
) -> Response {
    let table = match table_of(&table_id) {
        Ok(table) => table,
        Err(response) => return *response,
    };
    let (view, selection) = match load(&state, account, &session, table, &query).await {
        Ok(loaded) => loaded,
        Err(response) => return *response,
    };
    let csrf_token = match csrf::token(&session).await {
        Ok(token) => token,
        Err(e) => return pages::internal_error(e),
    };
    pages::render(&OpportunitiesPage {
        csrf_token,
        nav: Some(Nav::new(Page::Opportunities(table)).with_chars_query(selection.query())),
        view,
    })
}

/// The table and its pagination only, for live refreshes and the page's own controls.
pub async fn content(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    Path(table_id): Path<String>,
    query: ViewQuery,
) -> Response {
    let table = match table_of(&table_id) {
        Ok(table) => table,
        Err(response) => return *response,
    };
    match load(&state, account, &session, table, &query).await {
        Ok((view, _)) => pages::render(&ContentFragment { view }),
        Err(response) => *response,
    }
}

/// The freshness section only, for live refreshes.
pub async fn freshness_fragment(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    query: ViewQuery,
) -> Response {
    match load(&state, account, &session, Table::Expansion, &query).await {
        Ok((view, _)) => pages::render(&FreshnessFragment { view }),
        Err(response) => *response,
    }
}

/// Saves the filter box. The form carries the view state (table, selection and sorting) as hidden
/// fields, so the answer is page 1 of the table the user is looking at; a rejected form gets the
/// reason for the filter box's error text and nothing is stored.
pub async fn save_settings(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    body: String,
) -> Response {
    let pairs: Vec<(String, String)> = url::form_urlencoded::parse(body.as_bytes())
        .into_owned()
        .collect();
    let form = Form::from_pairs(pairs.clone());
    // Saving starts over on the first page.
    let query = ViewQuery::parse(Some(&body));
    let query = ViewQuery {
        page: None,
        ..query
    };
    let table = pairs
        .iter()
        .find(|(name, _)| name == "table")
        .and_then(|(_, id)| Table::from_id(id))
        .unwrap_or(Table::Expansion);
    match form.validate() {
        Ok(settings) => {
            if let Err(e) = settings
                .save(&*state.stores.opportunities, account.id)
                .await
            {
                return pages::internal_error(e);
            }
            match load(&state, account, &session, table, &query).await {
                Ok((view, _)) => {
                    // The address bar goes back to the first page of what the user was reading.
                    let url = view.content_url.replacen("/content", "", 1);
                    let mut response = pages::render(&ContentFragment { view });
                    if let Ok(value) = url.parse() {
                        response.headers_mut().insert("HX-Replace-Url", value);
                    }
                    response
                }
                Err(response) => *response,
            }
        }
        Err(error) => {
            let rejected = Rejected { form, error };
            match load_with(&state, account, &session, table, &query, Some(rejected)).await {
                Ok((view, _)) => pages::render_with(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    &FilterErrorFragment { view },
                ),
                Err(response) => *response,
            }
        }
    }
}

/// Saves the fee character the Fees group chose. The form carries the view state like the filter
/// form: the answer is page 1 of the table the user is looking at, or, with an item, that item's
/// detail fragment. A character that is not linked to the account is rejected and nothing is stored.
pub async fn save_fee_character(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    body: String,
) -> Response {
    let pairs: Vec<(String, String)> = url::form_urlencoded::parse(body.as_bytes())
        .into_owned()
        .collect();
    let field = |name: &str| {
        pairs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    // Saving starts over on the first page.
    let query = ViewQuery {
        page: None,
        ..ViewQuery::parse(Some(&body))
    };
    let characters = match state.repo.list_characters(account.id).await {
        Ok(characters) => characters,
        Err(e) => return pages::internal_error(e),
    };
    let Some(character) = field("fee_character")
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|id| characters.iter().any(|c| c.character_id == *id))
    else {
        return StatusCode::UNPROCESSABLE_ENTITY.into_response();
    };
    if let Err(e) = state
        .stores
        .opportunities
        .save_fee_character(account.id, Some(character))
        .await
    {
        return pages::internal_error(e);
    }
    let table_id = field("table").unwrap_or(Table::Expansion.id());
    if let Some(type_id) = field("type_id").and_then(|value| value.parse::<i64>().ok()) {
        return match build_detail(&state, account, &session, table_id, type_id, &query).await {
            Ok((view, _)) => pages::render(&DetailFragment { view }),
            Err(response) => *response,
        };
    }
    let table = Table::from_id(table_id).unwrap_or(Table::Expansion);
    match load(&state, account, &session, table, &query).await {
        Ok((view, _)) => {
            // The address bar goes back to the first page of what the user was reading.
            let url = view.content_url.replacen("/content", "", 1);
            let mut response = pages::render(&ContentFragment { view });
            if let Ok(value) = url.parse() {
                response.headers_mut().insert("HX-Replace-Url", value);
            }
            response
        }
        Err(response) => *response,
    }
}

// ---- the detail page ----

/// One KPI card of the figures section: the term names it, the detail line works it out with
/// this item's numbers.
pub struct FigureView {
    pub key: &'static str,
    pub term: Term,
    pub value: String,
    pub detail: Option<String>,
}

/// A labelled group of figure cards.
pub struct FigureGroup {
    pub title: &'static str,
    pub figures: Vec<FigureView>,
}

/// One chart row of the page: its kind (how the script draws it), a title, and its data as JSON.
pub struct ChartView {
    pub kind: &'static str,
    pub title: &'static str,
    pub json: String,
}

/// One link of the detail page's sub-menu: the id of a row and its label.
pub struct SectionView {
    pub id: String,
    pub label: String,
}

pub struct DetailView {
    pub name: String,
    pub table_title: &'static str,
    pub station_name: &'static str,
    pub characters: Vec<FilterEntry>,
    /// `(name, value)` of the hidden inputs of the character filter: every table's sorting.
    pub sorts: Vec<(&'static str, String)>,
    /// The opportunities page with this view's selection and sorting.
    pub back_url: String,
    /// The detail page itself, which the character filter loads again.
    pub page_url: String,
    pub sections: Vec<SectionView>,
    pub charts: Vec<ChartView>,
    /// The starts of the next occurrences of the window of an expansion row.
    pub next_occurrences: Vec<String>,
    /// Why the table does not list the item (right now); `None` while it does.
    pub reason: Option<String>,
    pub fees: FeeGroupView,
    pub figures: Vec<FigureGroup>,
    /// The legend card: the terms of the figures, in group order.
    pub legend: Vec<LegendEntry>,
    /// Where this page's content loads from, so a live refresh can fetch it again.
    pub refresh_url: String,
    /// The same without a query: a live refresh asks with the address bar's query.
    pub refresh_path: String,
}

fn figure(key: &'static str, term: Term, value: String, detail: Option<String>) -> FigureView {
    FigureView {
        key,
        term,
        value,
        detail,
    }
}

/// How the table derives S, with its input.
fn sell_detail(table: Table, detail: &tables::Detail<'_>) -> Option<String> {
    let stats = detail.stats;
    match table {
        Table::Classic | Table::Trend => Some(format!(
            "1 tick below {}",
            format::isk_short(stats.best_sell_cents)
        )),
        Table::Reversion => stats
            .sma20
            .map(|sma20| format!("SMA 20 {} rounded down", isk(sma20))),
        Table::Expansion => match &detail.extra {
            Some(Extra::Expansion { window, .. }) => {
                Some(format!("p25 of peaks of {}", window.label()))
            }
            _ => None,
        },
    }
}

/// The figures of the detail page in their groups. A figure that cannot be computed for the item is
/// left out, and so is a group left empty.
fn detail_figures(
    table: Table,
    detail: &tables::Detail<'_>,
    context: &Context<'_>,
) -> Vec<FigureGroup> {
    let stats = detail.stats;
    let fees = context.fees;
    let group = |title, figures| FigureGroup { title, figures };
    let mut prices = vec![
        figure(
            "buy",
            Term::BestBuy,
            format::isk_short(stats.best_buy_cents),
            None,
        ),
        figure(
            "best-sell",
            Term::BestSell,
            format::isk_short(stats.best_sell_cents),
            None,
        ),
    ];
    if let Some(sell) = detail.sell_cents {
        prices.push(figure(
            "sell",
            Term::Sell,
            format::isk_short(sell),
            sell_detail(table, detail),
        ));
    }
    let mut margin = Vec::new();
    let mut size = Vec::new();
    let mut timing = Vec::new();
    let mut result = Vec::new();
    if let Some(sigma) = stats.sigma {
        timing.push(figure("sigma", Term::Sigma, format::percent(sigma), None));
    }
    if let (Some(sell), Some(f)) = (detail.sell_cents, &detail.figures) {
        let rho = stats.rho.unwrap_or(0.5);
        let volume = stats.volume_7d.unwrap_or(0.0);
        let (c, r, net) = (isk(f.cost), isk(f.revenue), isk(f.net_per_unit));
        let units = format::thousands(f.units);
        margin = vec![
            figure(
                "cost",
                Term::Cost,
                c.clone(),
                Some(format!(
                    "{} \u{00D7} {:.4}",
                    format::isk_short(stats.best_buy_cents),
                    1.0 + fees.broker
                )),
            ),
            figure(
                "revenue",
                Term::Revenue,
                r.clone(),
                Some(format!(
                    "{} \u{00D7} {:.5}",
                    format::isk_short(sell),
                    1.0 - fees.broker - fees.tax
                )),
            ),
            figure(
                "net",
                Term::Net,
                net.clone(),
                Some(format!("{r} \u{2212} {c}")),
            ),
            figure(
                "margin",
                Term::Margin,
                format::percent(f.margin),
                Some(format!("{net} / {c}")),
            ),
        ];
        let capture = |n: i32, share: f64| {
            format!("1/({n} + 1) \u{00D7} {} \u{00D7} {share:.2}", whole(volume))
        };
        size = vec![
            figure(
                "units",
                Term::Units,
                units.clone(),
                Some(format!(
                    "\u{230A}{} / {c}\u{230B}",
                    format::isk_short((context.settings.capital * 100.0).round() as i64)
                )),
            ),
            figure("v7", Term::V7, whole(volume), None),
            figure(
                "v30",
                Term::V30,
                whole(stats.volume_30d.unwrap_or(0.0)),
                None,
            ),
            figure("rho", Term::Rho, format!("{rho:.2}"), None),
            figure(
                "q-buy",
                Term::QBuy,
                whole(f.q_buy),
                Some(capture(stats.buy_near_orders, rho)),
            ),
            figure(
                "q-sell",
                Term::QSell,
                whole(f.q_sell),
                Some(capture(stats.sell_near_orders, 1.0 - rho)),
            ),
        ];
        timing.extend([
            figure(
                "t-fill",
                Term::TFill,
                format::days(f.t_fill),
                Some(format!(
                    "{units} / {} + {units} / {}",
                    whole(f.q_buy),
                    whole(f.q_sell)
                )),
            ),
            figure(
                "t-max",
                Term::TMax,
                format::days(f.t_max),
                stats.sigma.map(|sigma| {
                    format!(
                        "min(3, ({} / {})\u{00B2})",
                        format::percent(f.margin),
                        format::percent(2.0 * sigma)
                    )
                }),
            ),
            figure("min-volume", Term::MinVolume, whole(f.min_volume), None),
        ]);
        result.push(figure(
            "isk-day",
            Term::IskPerDay,
            isk(f.isk_per_day),
            Some(format!(
                "{net} \u{00D7} {units} / {}",
                format::days(f.t_fill)
            )),
        ));
    }
    [
        group(
            "Fees",
            vec![
                figure(
                    "broker",
                    Term::BrokerFee,
                    format::percent(fees.broker),
                    None,
                ),
                figure("tax", Term::SalesTax, format::percent(fees.tax), None),
            ],
        ),
        group("Prices", prices),
        group("Margin", margin),
        group("Size & liquidity", size),
        group("Timing & risk", timing),
        group("Result", result),
    ]
    .into_iter()
    .filter(|g| !g.figures.is_empty())
    .collect()
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
    store: &dyn OpportunityStore,
    type_id: i64,
    now: OffsetDateTime,
) -> Result<Vec<charts::Day>, StoreError> {
    let rows = store
        .history_days(
            REGION_ID,
            type_id,
            (now - time::Duration::days(CHART_DAYS)).date(),
        )
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
    store: &dyn OpportunityStore,
    type_id: i64,
    now: OffsetDateTime,
) -> Result<Vec<charts::PricePoint>, StoreError> {
    let rows = store
        .price_points(
            REGION_ID,
            STATION_ID,
            type_id,
            now - time::Duration::days(CHART_DAYS),
        )
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

async fn latest_depth(
    store: &dyn OpportunityStore,
    type_id: i64,
) -> Result<Option<charts::Depth>, StoreError> {
    let row = store.latest_depth(REGION_ID, STATION_ID, type_id).await?;
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
    store: &dyn OpportunityStore,
    type_id: i64,
    now: OffsetDateTime,
) -> Result<Vec<analysis::HourPoint>, StoreError> {
    let rows = store
        .item_hour_points(
            REGION_ID,
            STATION_ID,
            type_id,
            now - time::Duration::days(CHART_DAYS),
        )
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
    store: &dyn OpportunityStore,
    type_id: i64,
    now: OffsetDateTime,
) -> Result<Vec<analysis::DayPoint>, StoreError> {
    let rows = store
        .item_day_points(
            REGION_ID,
            type_id,
            (now - time::Duration::days(CHART_DAYS)).date(),
        )
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
    store: &dyn OpportunityStore,
    table: Table,
    stats: &tables::Stats,
    now: OffsetDateTime,
) -> Result<(Vec<ChartView>, Vec<String>), StoreError> {
    let type_id = stats.type_id;
    let mut views = Vec::new();
    let mut next_occurrences = Vec::new();
    match table {
        Table::Trend => {
            let days = history_days(store, type_id, now).await?;
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
            let days = history_days(store, type_id, now).await?;
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
            let points = price_points(store, type_id, now).await?;
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
            if let Some(depth) = latest_depth(store, type_id).await? {
                views.extend(chart_view(
                    "depth",
                    "Units within 1% of the best price",
                    &charts::depth(&depth),
                ));
            }
        }
        Table::Expansion => {
            let hours = hour_points(store, type_id, now).await?;
            let days = day_points(store, type_id, now).await?;
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

/// The rows of the detail page, and out of band its sub-menu and context text.
#[derive(Template)]
#[template(path = "opportunities_detail_fragment.html")]
struct DetailFragment {
    view: DetailView,
}

#[derive(Template)]
#[template(path = "opportunity_detail.html")]
struct DetailPage {
    csrf_token: String,
    nav: Option<Nav>,
    view: DetailView,
}

/// What the detail page and its fragment show of one item in one table: the fees, the shared
/// figures with how they follow, the charts, and whether the table lists the item right now. An
/// item that dropped out is still shown, from what is known of it.
async fn build_detail(
    state: &AppState,
    account: CurrentAccount,
    session: &Session,
    table_id: &str,
    type_id: i64,
    query: &ViewQuery,
) -> Result<(DetailView, CharacterSelection), Box<Response>> {
    let Some(table) = Table::from_id(table_id) else {
        return Err(Box::new(StatusCode::NOT_FOUND.into_response()));
    };
    let env = match environment(state, account.id, query).await {
        Ok(Some(env)) => env,
        Ok(None) => {
            let _ = session.flush().await;
            return Err(Box::new(Redirect::to("/").into_response()));
        }
        Err(e) => return Err(Box::new(pages::internal_error(e))),
    };
    let context = env.context();
    let sort = env.view_state.sort(table);
    let page_query = env.view_state.query(false);
    let refresh_path = format!("/opportunities/detail/{}/{type_id}", table.id());
    let refresh_url = format!("{refresh_path}{page_query}");
    let fees = fee_group(&env, table, Some(type_id));
    let (name, charts, next_occurrences, reason, figures) =
        match tables::detail(table, type_id, &env.stats, &context, sort) {
            Some(detail) => {
                let (charts, next_occurrences) =
                    match detail_charts(&*state.stores.opportunities, table, detail.stats, env.now)
                        .await
                    {
                        Ok(drawn) => drawn,
                        Err(e) => return Err(Box::new(pages::internal_error(e))),
                    };
                (
                    detail.stats.display_name(),
                    charts,
                    next_occurrences,
                    detail.reason.clone(),
                    detail_figures(table, &detail, &context),
                )
            }
            None => {
                let name: Option<String> = match state.stores.market.name_of(type_id).await {
                    Ok(name) => name,
                    Err(e) => return Err(Box::new(pages::internal_error(e))),
                };
                (
                    name.unwrap_or_else(|| format!("Type {type_id}")),
                    Vec::new(),
                    Vec::new(),
                    Some("The station no longer trades it.".to_owned()),
                    Vec::new(),
                )
            }
        };
    let mut sections = vec![
        SectionView {
            id: "figures".to_owned(),
            label: "Figures".to_owned(),
        },
        SectionView {
            id: "legend".to_owned(),
            label: "Legend".to_owned(),
        },
    ];
    sections.extend(charts.iter().map(|chart| SectionView {
        id: chart.kind.to_owned(),
        label: chart.title.to_owned(),
    }));
    if !next_occurrences.is_empty() {
        sections.push(SectionView {
            id: "next".to_owned(),
            label: "Next occurrences".to_owned(),
        });
    }
    let view = DetailView {
        name,
        table_title: table.title(),
        station_name: STATION_NAME,
        characters: env
            .characters
            .iter()
            .map(|(id, name)| FilterEntry {
                id: *id,
                name: name.clone(),
                checked: env.selection.contains(*id),
            })
            .collect(),
        sorts: env.view_state.hidden_sorts(),
        back_url: format!("/opportunities/{}{page_query}", table.id()),
        page_url: format!("/opportunities/{}/{type_id}", table.id()),
        sections,
        charts,
        next_occurrences,
        reason,
        fees,
        figures,
        legend: legend::entries(&legend::detail_terms()),
        refresh_url,
        refresh_path,
    };
    Ok((view, env.selection.clone()))
}

/// The rows of the detail page only, for live refreshes (and the sub-menu and context text out of
/// band).
pub async fn detail(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    Path((table_id, type_id)): Path<(String, i64)>,
    query: ViewQuery,
) -> Response {
    match build_detail(&state, account, &session, &table_id, type_id, &query).await {
        Ok((view, _)) => pages::render(&DetailFragment { view }),
        Err(response) => *response,
    }
}

/// The detail page of one item in one table.
pub async fn detail_page(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    Path((table_id, type_id)): Path<(String, i64)>,
    query: ViewQuery,
) -> Response {
    let (view, selection) =
        match build_detail(&state, account, &session, &table_id, type_id, &query).await {
            Ok(loaded) => loaded,
            Err(response) => return *response,
        };
    // `build_detail` has answered "not found" for an unknown table, so this one is known.
    let table = Table::from_id(&table_id).unwrap_or(Table::Expansion);
    let csrf_token = match csrf::token(&session).await {
        Ok(token) => token,
        Err(e) => return pages::internal_error(e),
    };
    pages::render(&DetailPage {
        csrf_token,
        nav: Some(Nav::new(Page::Opportunities(table)).with_chars_query(selection.query())),
        view,
    })
}
