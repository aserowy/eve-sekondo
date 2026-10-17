//! The wealth page and its fragments: what the selected characters are worth, over time and as an
//! inventory list, and how fresh the figures are.

use askama::Template;
use axum::{
    extract::State,
    response::{IntoResponse, Redirect, Response},
};
use time::Duration;
use tower_sessions::Session;
use uuid::Uuid;

use crate::{
    account::CurrentAccount,
    app::AppState,
    csrf, format,
    freshness::{WealthFreshnessView, list_wealth_freshness, wealth_freshness_view},
    names::STRUCTURE_ID_MIN,
    orders::{Dir, FilterEntry},
    pages::{self, Nav, Page},
    range::Range,
    selection::{CharacterSelection, ViewQuery},
    wealth::{self, Inventory, InventoryRow, InventorySort, InventorySortKey, WealthPoint},
    wealth_poller::ASSETS_INTERVAL,
};

/// Shown where a row has no price and no value.
const NO_DATA: &str = "\u{2014}";

pub struct HeaderView {
    pub label: &'static str,
    pub href: String,
    /// `ascending`, `descending` or empty: for `aria-sort`.
    pub aria_sort: &'static str,
    pub arrow: &'static str,
    /// Numbers are right-aligned.
    pub numeric: bool,
}

pub struct RowView {
    pub character: String,
    pub item: String,
    pub quantity: String,
    pub price: String,
    pub price_exact: String,
    pub value: String,
    pub value_exact: String,
    pub location: String,
}

/// The inventory table with its totals.
pub struct ItemsView {
    pub headers: Vec<HeaderView>,
    pub rows: Vec<RowView>,
    pub total: String,
    pub total_exact: String,
    pub unpriced: usize,
}

pub struct RangeLink {
    pub label: &'static str,
    /// The page's URL with this range, for the address bar and without scripts.
    pub href: String,
    /// The graph fragment's URL with this range.
    pub graph_url: String,
    pub current: bool,
}

/// The stacked graph's data for the canvas's `data-series` and what a screen reader hears.
pub struct StackChart {
    pub series: String,
    pub label: String,
}

pub struct GraphView {
    pub ranges: Vec<RangeLink>,
    /// `None` while no selected character has any recorded figure: the page says so instead.
    pub chart: Option<StackChart>,
}

/// Everything the wealth page and its fragments show for one request.
pub struct WealthView {
    pub characters: Vec<FilterEntry>,
    pub sort: InventorySort,
    pub range: Range,
    pub freshness_url: String,
    pub items_url: String,
    pub graph_url: String,
    pub freshness: Vec<WealthFreshnessView>,
    /// How many of the selected characters need re-login or have a stale wallet or assets.
    pub warnings: usize,
    pub items: ItemsView,
    pub graph: GraphView,
}

/// The view state a URL carries: the selection, the list's sorting and the graph's range, each
/// only when it is not the default.
struct ViewState {
    chars: Option<String>,
    sort: InventorySort,
    range: Range,
}

impl ViewState {
    fn query(&self, with_sort: bool, with_range: bool) -> String {
        let mut pairs = url::form_urlencoded::Serializer::new(String::new());
        if let Some(chars) = &self.chars {
            pairs.append_pair("chars", chars);
        }
        if with_sort && self.sort != InventorySort::default() {
            pairs.append_pair("sort", self.sort.key.name());
            pairs.append_pair("dir", self.sort.dir.name());
        }
        if with_range && self.range != Range::default() {
            pairs.append_pair("range", self.range.name());
        }
        // The commas stay readable in the URL, as in bookmarks people write by hand.
        let query = pairs.finish().replace("%2C", ",");
        if query.is_empty() {
            query
        } else {
            format!("?{query}")
        }
    }

    fn with_sort(&self, sort: InventorySort) -> Self {
        Self {
            chars: self.chars.clone(),
            sort,
            range: self.range,
        }
    }

    fn with_range(&self, range: Range) -> Self {
        Self {
            chars: self.chars.clone(),
            sort: self.sort,
            range,
        }
    }
}

/// The four series of the stack, bottom to top. The colours are chosen in `app.js`: sell and
/// escrow keep the dashboard's, and this order keeps every touching pair apart for colour-blind
/// readers (checked with the dataviz palette validator).
type Layer<'a> = (&'a str, fn(&WealthPoint) -> i64);
const LAYERS: [Layer; 4] = [
    ("Sell orders", |p| p.sell_cents),
    ("Buy escrow", |p| p.escrow_cents),
    ("Wallet", |p| p.wallet_cents),
    ("Inventory", |p| p.inventory_cents),
];

fn stack_chart(points: &[WealthPoint]) -> Option<StackChart> {
    if !wealth::has_history(points) {
        return None;
    }
    let last = points.last()?;
    let series = serde_json::json!({
        "times": points.iter().map(|p| p.at.unix_timestamp() * 1000).collect::<Vec<_>>(),
        "labels": points.iter().map(|p| format::eve_time(p.at)).collect::<Vec<_>>(),
        "totals": points.iter().map(|p| format::isk_exact(p.total_cents())).collect::<Vec<_>>(),
        "series": LAYERS.iter().map(|(name, value)| serde_json::json!({
            "name": name,
            "values": points.iter().map(value).collect::<Vec<_>>(),
            "exact": points.iter().map(|p| format::isk_exact(value(p))).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
    .to_string();
    Some(StackChart {
        series,
        label: format!(
            "Wealth over time; now {}: {} in the wallet, {} in sell orders, {} in escrow and {} in inventory",
            format::isk_exact(last.total_cents()),
            format::isk_exact(last.wallet_cents),
            format::isk_exact(last.sell_cents),
            format::isk_exact(last.escrow_cents),
            format::isk_exact(last.inventory_cents),
        ),
    })
}

fn graph_view(state: &ViewState, points: &[WealthPoint]) -> GraphView {
    GraphView {
        ranges: Range::ALL
            .into_iter()
            .map(|range| {
                let target = state.with_range(range);
                RangeLink {
                    label: range.label(),
                    href: format!("/wealth{}", target.query(true, true)),
                    graph_url: format!("/wealth/graph{}", target.query(false, true)),
                    current: range == state.range,
                }
            })
            .collect(),
        chart: stack_chart(points),
    }
}

fn headers(state: &ViewState) -> Vec<HeaderView> {
    let columns = [
        ("Character", InventorySortKey::Character, false),
        ("Item", InventorySortKey::Item, false),
        ("Quantity", InventorySortKey::Quantity, true),
        ("Unit price", InventorySortKey::Price, true),
        ("Value", InventorySortKey::Value, true),
        ("Location", InventorySortKey::Location, false),
    ];
    columns
        .into_iter()
        .map(|(label, key, numeric)| {
            let sorted_by_this = state.sort.key == key;
            let next = InventorySort {
                key,
                dir: if sorted_by_this {
                    state.sort.dir.reversed()
                } else {
                    Dir::Asc
                },
            };
            let (aria_sort, arrow) = match (sorted_by_this, state.sort.dir) {
                (false, _) => ("", ""),
                (true, Dir::Asc) => ("ascending", " \u{25B2}"),
                (true, Dir::Desc) => ("descending", " \u{25BC}"),
            };
            HeaderView {
                label,
                href: format!("/wealth{}", state.with_sort(next).query(true, true)),
                aria_sort,
                arrow,
                numeric,
            }
        })
        .collect()
}

fn row_view(row: &InventoryRow) -> RowView {
    let location = match (&row.location_name, row.location_known) {
        (Some(name), _) => name.clone(),
        // The cache knows the structure but ESI would not describe it.
        (None, true) if row.top_known && row.location_id >= STRUCTURE_ID_MIN => {
            format!("Unknown structure ({})", row.location_id)
        }
        (None, _) => format!("Unknown location ({})", row.location_id),
    };
    let amount = |cents: Option<i64>| match cents {
        Some(cents) => (format::isk_short(cents), format::isk_exact(cents)),
        None => (NO_DATA.to_owned(), String::new()),
    };
    let (price, price_exact) = amount(row.unit_price_cents);
    let (value, value_exact) = amount(row.value_cents);
    RowView {
        character: row.character_name.clone(),
        item: row
            .type_name
            .clone()
            .unwrap_or_else(|| format!("#{}", row.type_id)),
        quantity: row.quantity.to_string(),
        price,
        price_exact,
        value,
        value_exact,
        location,
    }
}

fn items_view(state: &ViewState, inventory: &Inventory) -> ItemsView {
    ItemsView {
        headers: headers(state),
        rows: inventory.rows.iter().map(row_view).collect(),
        total: format::isk_short(inventory.total_cents),
        total_exact: format::isk_exact(inventory.total_cents),
        unpriced: inventory.unpriced_rows,
    }
}

async fn build_view(
    state: &AppState,
    account_id: Uuid,
    query: &ViewQuery,
) -> Result<Option<(WealthView, CharacterSelection)>, sqlx::Error> {
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
    let now = state.clock.now();
    let order_interval: Duration = state.config.order_poll_interval;
    let freshness = list_wealth_freshness(&state.pool, account_id, &selection).await?;
    let view_state = ViewState {
        chars: selection.param(),
        sort: InventorySort::parse(query.sort.as_deref(), query.dir.as_deref()),
        range: Range::parse(query.range.as_deref()),
    };
    let inventory = wealth::inventory(&state.pool, account_id, &selection, view_state.sort).await?;
    let points =
        wealth::history(&state.pool, account_id, &selection, view_state.range, now).await?;

    let freshness: Vec<WealthFreshnessView> = freshness
        .iter()
        .map(|r| wealth_freshness_view(r, now, order_interval, ASSETS_INTERVAL))
        .collect();
    let warnings = freshness
        .iter()
        .filter(|f| f.needs_relogin || f.wallet.stale || f.assets.stale)
        .count();
    let view = WealthView {
        characters: characters
            .iter()
            .map(|c| FilterEntry {
                id: c.character_id,
                name: c.name.clone(),
                checked: selection.contains(c.character_id),
            })
            .collect(),
        sort: view_state.sort,
        range: view_state.range,
        freshness_url: format!("/wealth/freshness{}", view_state.query(false, false)),
        items_url: format!("/wealth/items{}", view_state.query(true, false)),
        graph_url: format!("/wealth/graph{}", view_state.query(false, true)),
        freshness,
        warnings,
        items: items_view(&view_state, &inventory),
        graph: graph_view(&view_state, &points),
    };
    Ok(Some((view, selection)))
}

#[derive(Template)]
#[template(path = "wealth.html")]
struct WealthPage {
    csrf_token: String,
    nav: Option<Nav>,
    view: WealthView,
}

#[derive(Template)]
#[template(path = "wealth_freshness.html")]
struct FreshnessFragment {
    view: WealthView,
}

#[derive(Template)]
#[template(path = "wealth_graph_fragment.html")]
struct GraphFragment {
    view: WealthView,
}

#[derive(Template)]
#[template(path = "wealth_items.html")]
struct ItemsFragment {
    view: WealthView,
}

async fn load(
    state: &AppState,
    account: CurrentAccount,
    session: &Session,
    query: &ViewQuery,
) -> Result<(WealthView, CharacterSelection), Box<Response>> {
    match build_view(state, account.id, query).await {
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
    pages::render(&WealthPage {
        csrf_token,
        nav: Some(Nav::new(Page::Wealth).with_chars_query(selection.query())),
        view,
    })
}

/// The freshness strip only, for live refreshes.
pub async fn freshness(
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

/// The inventory list only, for live refreshes.
pub async fn items(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    query: ViewQuery,
) -> Response {
    match load(&state, account, &session, &query).await {
        Ok((view, _)) => pages::render(&ItemsFragment { view }),
        Err(response) => *response,
    }
}

/// The graph and its range control only, for range switches and live refreshes.
pub async fn graph(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    query: ViewQuery,
) -> Response {
    match load(&state, account, &session, &query).await {
        Ok((view, _)) => pages::render(&GraphFragment { view }),
        Err(response) => *response,
    }
}
