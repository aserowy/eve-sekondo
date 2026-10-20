//! The orders overview: reading current orders for an account's selected characters, and the page
//! and fragments that show them.

use askama::Template;
use axum::{
    extract::State,
    http::Uri,
    response::{IntoResponse, Redirect, Response},
};
use time::OffsetDateTime;
use tower_sessions::Session;
use uuid::Uuid;

use crate::{
    account::CurrentAccount,
    app::AppState,
    csrf, fees, format,
    freshness::{FreshnessView, freshness_view},
    kpi::{Kpi, KpiLink},
    pages::{self, Nav, Page},
    pagination::{Link, PageView, Paging, paginate},
    selection::{CharacterSelection, ViewQuery},
    store::StoreError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Sell,
    Buy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Item,
    Price,
    /// "Top" first, then the number of undercutting orders; no market data last.
    Standing,
    /// The difference to the best price; no market data last.
    Gap,
    /// The remaining volume of the undercutting orders: a top order counts as 0, no market data
    /// last.
    UndercutVolume,
    Volume,
    /// The order value of a sell order, the escrow of a buy order.
    Amount,
    /// The margin of a sell order; orders without one last.
    Margin,
    TimeLeft,
}

impl SortKey {
    const ALL: [Self; 9] = [
        Self::Item,
        Self::Price,
        Self::Standing,
        Self::Gap,
        Self::UndercutVolume,
        Self::Volume,
        Self::Amount,
        Self::Margin,
        Self::TimeLeft,
    ];

    /// The name used in `sort` and `bsort`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Item => "item",
            Self::Price => "price",
            Self::Standing => "standing",
            Self::Gap => "gap",
            Self::UndercutVolume => "undercut-volume",
            Self::Volume => "volume",
            Self::Amount => "amount",
            Self::Margin => "margin",
            Self::TimeLeft => "left",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Asc,
    Desc,
}

impl Dir {
    pub fn name(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }

    pub fn reversed(self) -> Self {
        match self {
            Self::Asc => Self::Desc,
            Self::Desc => Self::Asc,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub key: SortKey,
    pub dir: Dir,
}

impl Default for Sort {
    /// Item name, ascending.
    fn default() -> Self {
        Self {
            key: SortKey::Item,
            dir: Dir::Asc,
        }
    }
}

impl Sort {
    /// Unknown or missing values fall back to the default instead of failing the request: an
    /// unknown column gives the whole default sorting, an unknown direction ascending.
    pub fn parse(key: Option<&str>, dir: Option<&str>) -> Self {
        let Some(key) = key.and_then(|k| {
            SortKey::ALL
                .into_iter()
                .find(|candidate| candidate.name() == k)
        }) else {
            return Self::default();
        };
        let dir = match dir {
            Some("desc") => Dir::Desc,
            _ => Dir::Asc,
        };
        Self { key, dir }
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OrderRow {
    pub order_id: i64,
    pub character_id: i64,
    pub character_name: String,
    pub type_id: i64,
    pub type_name: Option<String>,
    pub region_id: i64,
    pub region_name: Option<String>,
    pub location_id: i64,
    pub location_name: Option<String>,
    /// There is a row for the location in the name cache (its name may be unresolvable).
    pub location_known: bool,
    pub is_corporation: bool,
    pub price_cents: i64,
    pub volume_remain: i64,
    pub volume_total: i64,
    /// Price times remaining volume.
    pub value_cents: i64,
    pub escrow_cents: Option<i64>,
    pub issued: OffsetDateTime,
    pub duration: i32,
    /// How many compared orders have a strictly better price; `None` without market data.
    pub undercut: Option<i64>,
    /// The best price among the orders that undercut this one (none for a top order).
    pub best_price_cents: Option<i64>,
    /// Per unit, the difference to the best price; `None` for a top order or without market data.
    pub gap_cents: Option<i64>,
    /// The remaining volume of the orders that undercut this one; `None` for a top order or
    /// without market data.
    pub undercut_volume: Option<i64>,
    /// How many of the order's units have a known purchase cost; `None` when the order is not a
    /// remembered personal sell order.
    pub covered_units: Option<i64>,
    /// What the remaining volume earns after the character's fees and the purchase cost, in
    /// cents; `None` unless the cost covers the whole volume and the fees are known.
    pub margin_cents: Option<i64>,
    /// The average purchase cost per unit, in cents, when it covers the whole volume.
    pub cost_per_unit_cents: Option<i64>,
    /// The known purchase cost of the remaining volume, in cents, when it covers the whole
    /// volume: what the margin was worked out against.
    pub cost_cents: Option<i64>,
}

impl OrderRow {
    pub fn expires_at(&self) -> OffsetDateTime {
        self.issued + time::Duration::days(i64::from(self.duration))
    }
}

// ---- KPIs ----

/// What the count and value cards of one side add up: every order of the side, not only the ones
/// on the shown page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideTotals {
    pub count: i64,
    /// The value of the sell orders, the escrow of the buy orders.
    pub total_cents: i64,
    /// The part of the total that belongs to corporation orders.
    pub corp_cents: i64,
}

impl SideTotals {
    pub fn of(rows: &[OrderRow], side: Side) -> Self {
        let amount = |row: &OrderRow| match side {
            Side::Sell => row.value_cents,
            Side::Buy => row.escrow_cents.unwrap_or(0),
        };
        Self {
            count: rows.len() as i64,
            total_cents: rows.iter().map(amount).sum(),
            corp_cents: rows
                .iter()
                .filter(|row| row.is_corporation)
                .map(amount)
                .sum(),
        }
    }
}

/// The margins of the sell orders that have one, against the known cost they were worked out on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetMargin {
    pub margin_cents: i64,
    pub cost_cents: i64,
    /// How many orders the sum covers, and how many it leaves out for lack of a margin.
    pub counted: usize,
    pub left_out: usize,
}

impl NetMargin {
    pub fn of(rows: &[OrderRow]) -> Self {
        let with_margin = rows.iter().filter(|row| row.margin_cents.is_some());
        Self {
            margin_cents: with_margin.clone().filter_map(|row| row.margin_cents).sum(),
            cost_cents: with_margin.clone().filter_map(|row| row.cost_cents).sum(),
            counted: with_margin.count(),
            left_out: rows.iter().filter(|row| row.margin_cents.is_none()).count(),
        }
    }

    /// The sum as a percentage of the known cost, rounded half up to two decimals; `None` when no
    /// order has a margin.
    pub fn percent(&self) -> Option<String> {
        (self.counted > 0)
            .then(|| format::percent_of_hundredths(self.margin_cents, self.cost_cents))
    }
}

fn order_kpis(side: Side, rows: &[OrderRow]) -> Vec<Kpi> {
    let totals = SideTotals::of(rows, side);
    let (count_name, total_name, prefix) = match side {
        Side::Sell => ("Sell orders", "Sell value", "kpi-sell"),
        Side::Buy => ("Buy orders", "Buy escrow", "kpi-buy"),
    };
    let mut total = Kpi::new(
        total_name,
        format::isk_short(totals.total_cents),
        format::isk_exact(totals.total_cents),
    )
    .with_id(format!("{prefix}-total"));
    if totals.corp_cents != 0 {
        total = total
            .with_detail(format!(
                "of which {} corp",
                format::isk_short(totals.corp_cents)
            ))
            .with_detail_title(format::isk_exact(totals.corp_cents));
    }
    let mut kpis = vec![
        Kpi::new(
            count_name,
            totals.count.to_string(),
            format::thousands(totals.count),
        )
        .with_id(format!("{prefix}-count")),
        total,
    ];
    if side == Side::Sell {
        let net = NetMargin::of(rows);
        let kpi = match net.percent() {
            None => Kpi::new("Net margin", NO_DATA, ""),
            Some(percent) => {
                let mut exact = format::isk_exact(net.margin_cents);
                if net.left_out > 0 {
                    exact.push_str(&format!(
                        "; {} left out",
                        match net.left_out {
                            1 => "1 order".to_owned(),
                            n => format!("{n} orders"),
                        }
                    ));
                }
                let kpi = Kpi::new("Net margin", format::isk_short(net.margin_cents), exact)
                    .with_detail(format!("{percent}%"));
                if net.margin_cents < 0 {
                    kpi.as_error()
                } else {
                    kpi
                }
            }
        };
        kpis.push(kpi.with_id("kpi-net-margin"));
    }
    kpis
}

/// Three cards per selected character: its sales tax, broker fee and minimum margin at its fee
/// location.
fn fee_kpis(cards: &[fees::FeeCard]) -> Vec<Kpi> {
    cards
        .iter()
        .flat_map(|card| {
            let detail = format!("{} \u{b7} {}", card.character, card.location.name);
            [
                ("Sales tax", "sales-tax", &card.location.sales_tax),
                ("Broker fee", "broker-fee", &card.location.broker_fee),
                ("Min. margin", "min-margin", &card.location.min_margin),
            ]
            .map(|(name, key, figure)| {
                let kpi = Kpi::new(
                    name,
                    figure.text.clone(),
                    figure.title.clone().unwrap_or_default(),
                )
                .with_id(format!("fee-{}-{key}", card.character_id))
                .with_detail(detail.clone());
                if card.needs_location_scope {
                    kpi.with_link(KpiLink::log_in_again())
                } else {
                    kpi
                }
            })
        })
        .collect()
}

// ---- view model ----

/// Shown where an order has no standing, gap and undercut volume.
const NO_DATA: &str = "\u{2014}";

pub struct HeaderView {
    pub label: &'static str,
    /// The page that the sorting leads to, for the address bar.
    pub href: String,
    /// The content fragment that htmx fetches for it.
    pub content_href: String,
    /// `ascending`, `descending` or empty: for `aria-sort`.
    pub aria_sort: &'static str,
    pub arrow: &'static str,
}

pub struct RowView {
    pub order_id: i64,
    pub character: String,
    pub item: String,
    pub location: String,
    pub region: String,
    pub is_corporation: bool,
    pub price: String,
    pub price_exact: String,
    /// `Top`, `Undercut N` or a dash while there is no market data.
    pub standing: String,
    /// `top` or `undercut` for the badge; empty without a standing.
    pub standing_class: &'static str,
    /// The difference to the best price and its percentage; empty for a top order.
    pub gap: String,
    pub gap_exact: String,
    /// The undercut volume with separators; empty for a top order.
    pub undercut_volume: String,
    pub volume: String,
    pub amount: String,
    pub amount_exact: String,
    /// The margin shortened, or a dash; only the sell table has the column.
    pub margin: String,
    /// What hovering the margin adds: the exact value and the cost per unit, or how many units
    /// have a known cost. Empty when there is nothing to add.
    pub margin_title: String,
    /// The margin is negative: it is shown in the error style.
    pub margin_negative: bool,
    pub issued: String,
    pub left: String,
}

pub struct TableView {
    /// The sell table has a margin column, the buy table has not.
    pub has_margin: bool,
    pub headers: Vec<HeaderView>,
    pub rows: Vec<RowView>,
}

pub struct FilterEntry {
    pub id: i64,
    pub name: String,
    pub checked: bool,
}

/// A link of the sub-menu: the other side's page.
pub struct SideLink {
    pub label: &'static str,
    pub href: String,
    pub current: bool,
}

/// Everything the orders pages and their fragments show for one request.
pub struct OrdersView {
    pub side: Side,
    /// The path of the page, without a query: the filter form builds its own.
    pub path: String,
    pub characters: Vec<FilterEntry>,
    /// The page itself, carrying the whole view state, as the address bar shows it.
    pub page_url: String,
    /// The content fragment of the page, for live refreshes.
    pub content_url: String,
    /// The same without a query: live refreshes ask with the address bar's query.
    pub content_path: String,
    pub freshness_url: String,
    pub sub_menu: Vec<SideLink>,
    pub sell_sort: Sort,
    pub buy_sort: Sort,
    /// The id of the table's section: `sell-orders` or `buy-orders`.
    pub table_id: &'static str,
    /// "No sell orders." or "No buy orders.", for an empty table.
    pub empty: &'static str,
    pub kpis: Vec<Kpi>,
    pub table: TableView,
    pub pager: PageView,
    pub freshness: Vec<FreshnessView>,
}

/// The view state a URL carries: the selection, one sorting per table and the paging of the table
/// shown.
struct ViewState {
    chars: Option<String>,
    sell: Sort,
    buy: Sort,
    paging: Paging,
}

impl ViewState {
    /// The query string, `""` or `?…`, with the selection and the sortings, and the paging unless
    /// `without_paging`.
    fn query_with(&self, with_paging: bool) -> String {
        let mut pairs = url::form_urlencoded::Serializer::new(String::new());
        if let Some(chars) = &self.chars {
            // The commas stay readable in the URL, as in bookmarks people write by hand.
            pairs.append_pair("chars", chars);
        }
        let default = Sort::default();
        if self.sell != default {
            pairs.append_pair("sort", self.sell.key.name());
            pairs.append_pair("dir", self.sell.dir.name());
        }
        if self.buy != default {
            pairs.append_pair("bsort", self.buy.key.name());
            pairs.append_pair("bdir", self.buy.dir.name());
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

    fn query(&self) -> String {
        self.query_with(true)
    }

    /// The same view sorted another way, from the first page.
    fn with_sort(&self, side: Side, sort: Sort) -> Self {
        Self {
            chars: self.chars.clone(),
            sell: if side == Side::Sell { sort } else { self.sell },
            buy: if side == Side::Buy { sort } else { self.buy },
            paging: self.paging.first(),
        }
    }

    fn with_paging(&self, paging: Paging) -> Self {
        Self {
            chars: self.chars.clone(),
            sell: self.sell,
            buy: self.buy,
            paging,
        }
    }
}

impl Side {
    fn slug(self) -> &'static str {
        match self {
            Side::Sell => "sell",
            Side::Buy => "buy",
        }
    }
}

fn page_path(side: Side) -> String {
    format!("/orders/{}", side.slug())
}

fn content_path(side: Side) -> String {
    format!("/orders/{}/content", side.slug())
}

fn headers(state: &ViewState, side: Side) -> Vec<HeaderView> {
    let current = if side == Side::Sell {
        state.sell
    } else {
        state.buy
    };
    let amount = if side == Side::Sell {
        "Value"
    } else {
        "Escrow"
    };
    let mut columns = vec![
        ("Item", SortKey::Item),
        ("Price", SortKey::Price),
        ("Standing", SortKey::Standing),
        ("Behind by", SortKey::Gap),
        ("Undercut volume", SortKey::UndercutVolume),
        ("Volume", SortKey::Volume),
        (amount, SortKey::Amount),
        ("Time left", SortKey::TimeLeft),
    ];
    if side == Side::Sell {
        columns.insert(7, ("Margin", SortKey::Margin));
    }
    columns
        .into_iter()
        .map(|(label, key)| {
            let sorted_by_this = current.key == key;
            let next = Sort {
                key,
                dir: if sorted_by_this {
                    current.dir.reversed()
                } else {
                    Dir::Asc
                },
            };
            let (aria_sort, arrow) = match (sorted_by_this, current.dir) {
                (false, _) => ("", ""),
                (true, Dir::Asc) => ("ascending", " \u{25B2}"),
                (true, Dir::Desc) => ("descending", " \u{25BC}"),
            };
            let query = state.with_sort(side, next).query();
            HeaderView {
                label,
                href: format!("{}{query}", page_path(side)),
                content_href: format!("{}{query}", content_path(side)),
                aria_sort,
                arrow,
            }
        })
        .collect()
}

fn row_view(row: &OrderRow, side: Side, now: OffsetDateTime) -> RowView {
    let placeholder = |id: i64| format!("#{id}");
    let location = match (&row.location_name, row.location_known) {
        (Some(name), _) => name.clone(),
        // The cache knows the structure but ESI would not describe it.
        (None, true) if row.location_id >= 100_000_000 => {
            format!("Unknown structure ({})", row.location_id)
        }
        (None, _) => placeholder(row.location_id),
    };
    let amount_cents = match side {
        Side::Sell => row.value_cents,
        Side::Buy => row.escrow_cents.unwrap_or(0),
    };
    let (standing, standing_class, gap, gap_exact) = match (row.undercut, row.gap_cents) {
        (None, _) => (NO_DATA.to_owned(), "", NO_DATA.to_owned(), String::new()),
        (Some(0), _) => ("Top".to_owned(), "top", String::new(), String::new()),
        (Some(count), gap_cents) => {
            let gap_cents = gap_cents.unwrap_or(0);
            let best = row.best_price_cents.unwrap_or(0);
            (
                format!("Undercut {count}"),
                "undercut",
                format!(
                    "{} ({}%)",
                    format::isk_short(gap_cents),
                    format::percent_of(gap_cents, best)
                ),
                format::isk_exact(gap_cents),
            )
        }
    };
    let undercut_volume = match (row.undercut, row.undercut_volume) {
        (None, _) => NO_DATA.to_owned(),
        (Some(_), volume) => volume.map(format::thousands).unwrap_or_default(),
    };
    let (margin, margin_title) = match (row.margin_cents, row.cost_per_unit_cents) {
        (Some(margin), cost) => (
            format::isk_short(margin),
            match cost {
                Some(cost) => format!(
                    "{}; cost per unit {}",
                    format::isk_exact(margin),
                    format::isk_exact(cost)
                ),
                None => format::isk_exact(margin),
            },
        ),
        (None, _) => (
            NO_DATA.to_owned(),
            match row.covered_units {
                Some(covered) if covered < row.volume_total => {
                    format!("cost known for {covered} of {} units", row.volume_total)
                }
                _ => String::new(),
            },
        ),
    };
    RowView {
        order_id: row.order_id,
        character: row.character_name.clone(),
        item: row
            .type_name
            .clone()
            .unwrap_or_else(|| placeholder(row.type_id)),
        location,
        region: row
            .region_name
            .clone()
            .unwrap_or_else(|| placeholder(row.region_id)),
        is_corporation: row.is_corporation,
        price: format::isk_short(row.price_cents),
        price_exact: format::isk_exact(row.price_cents),
        standing,
        standing_class,
        gap,
        gap_exact,
        undercut_volume,
        volume: format!("{} / {}", row.volume_remain, row.volume_total),
        amount: format::isk_short(amount_cents),
        amount_exact: format::isk_exact(amount_cents),
        margin,
        margin_title,
        margin_negative: row.margin_cents.is_some_and(|m| m < 0),
        issued: format::eve_time(row.issued),
        left: format::time_left(row.expires_at(), now),
    }
}

async fn build_view(
    state: &AppState,
    account_id: Uuid,
    side: Side,
    query: &ViewQuery,
) -> Result<Option<(OrdersView, CharacterSelection)>, StoreError> {
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
    let mut view_state = ViewState {
        chars: selection.param(),
        sell: Sort::parse(query.sort.as_deref(), query.dir.as_deref()),
        buy: Sort::parse(query.bsort.as_deref(), query.bdir.as_deref()),
        paging: query.paging(),
    };
    let now = state.clock.now();
    let interval = state.config.order_poll_interval;

    let sort = if side == Side::Sell {
        view_state.sell
    } else {
        view_state.buy
    };
    let rows = state
        .stores
        .orders
        .list_orders(account_id, selection.ids(), side, sort)
        .await?;
    let freshness = state
        .stores
        .orders
        .list_freshness(account_id, selection.ids())
        .await?;
    let cards = fees::fee_cards(&state.stores, account_id, selection.ids()).await?;

    // The KPIs cover every order of the side; the table shows the requested page of them.
    let mut kpis = order_kpis(side, &rows);
    kpis.extend(fee_kpis(&cards));
    let link_to = |paging: Paging| {
        let query = view_state.with_paging(paging).query();
        Link {
            href: format!("{}{query}", page_path(side)),
            content: format!("{}{query}", content_path(side)),
        }
    };
    let (page_rows, pager) = paginate(&rows, view_state.paging, link_to);
    // What the URLs below carry is the page actually shown, after clamping.
    view_state.paging = pager.paging;

    let freshness: Vec<FreshnessView> = freshness
        .iter()
        .map(|r| freshness_view(r, now, interval))
        .collect();
    let table = TableView {
        has_margin: side == Side::Sell,
        headers: headers(&view_state, side),
        rows: page_rows.iter().map(|r| row_view(r, side, now)).collect(),
    };
    let menu_query = view_state.query_with(false);
    let page_query = view_state.query();
    let view = OrdersView {
        side,
        path: page_path(side),
        characters: characters
            .iter()
            .map(|c| FilterEntry {
                id: c.character_id,
                name: c.name.clone(),
                checked: selection.contains(c.character_id),
            })
            .collect(),
        page_url: format!("{}{page_query}", page_path(side)),
        content_url: format!("{}{page_query}", content_path(side)),
        content_path: content_path(side),
        freshness_url: format!("/orders/freshness{page_query}"),
        sub_menu: [("Sell", Side::Sell), ("Buy", Side::Buy)]
            .map(|(label, link_side)| SideLink {
                label,
                href: format!("{}{menu_query}", page_path(link_side)),
                current: link_side == side,
            })
            .into(),
        sell_sort: view_state.sell,
        buy_sort: view_state.buy,
        table_id: if side == Side::Sell {
            "sell-orders"
        } else {
            "buy-orders"
        },
        empty: if side == Side::Sell {
            "No sell orders."
        } else {
            "No buy orders."
        },
        kpis,
        table,
        pager,
        freshness,
    };
    Ok(Some((view, selection)))
}

// ---- templates and handlers ----

#[derive(Template)]
#[template(path = "orders.html")]
struct OrdersPage {
    csrf_token: String,
    nav: Option<Nav>,
    view: OrdersView,
}

#[derive(Template)]
#[template(path = "orders_content_fragment.html")]
struct ContentFragment {
    view: OrdersView,
}

#[derive(Template)]
#[template(path = "orders_freshness.html")]
struct FreshnessFragment {
    view: OrdersView,
}

async fn load(
    state: &AppState,
    account: CurrentAccount,
    session: &Session,
    side: Side,
    query: &ViewQuery,
) -> Result<(OrdersView, CharacterSelection), Box<Response>> {
    match build_view(state, account.id, side, query).await {
        Ok(Some(loaded)) => Ok(loaded),
        Ok(None) => {
            // An account without characters cannot exist; the session outlived its account.
            let _ = session.flush().await;
            Err(Box::new(Redirect::to("/").into_response()))
        }
        Err(e) => Err(Box::new(pages::internal_error(e))),
    }
}

/// `/orders` is the sell orders page, with whatever the old link carried.
pub async fn index(uri: Uri) -> Response {
    let target = match uri.query() {
        Some(query) => format!("/orders/sell?{query}"),
        None => "/orders/sell".to_owned(),
    };
    Redirect::to(&target).into_response()
}

async fn page(
    state: AppState,
    account: CurrentAccount,
    session: Session,
    side: Side,
    query: ViewQuery,
) -> Response {
    let (view, selection) = match load(&state, account, &session, side, &query).await {
        Ok(loaded) => loaded,
        Err(response) => return *response,
    };
    let csrf_token = match csrf::token(&session).await {
        Ok(token) => token,
        Err(e) => return pages::internal_error(e),
    };
    let current = match side {
        Side::Sell => Page::OrdersSell,
        Side::Buy => Page::OrdersBuy,
    };
    pages::render(&OrdersPage {
        csrf_token,
        nav: Some(Nav::new(current).with_chars_query(selection.query())),
        view,
    })
}

pub async fn sell_page(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    query: ViewQuery,
) -> Response {
    page(state, account, session, Side::Sell, query).await
}

pub async fn buy_page(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    query: ViewQuery,
) -> Response {
    page(state, account, session, Side::Buy, query).await
}

async fn content(
    state: AppState,
    account: CurrentAccount,
    session: Session,
    side: Side,
    query: ViewQuery,
) -> Response {
    match load(&state, account, &session, side, &query).await {
        Ok((view, _)) => pages::render(&ContentFragment { view }),
        Err(response) => *response,
    }
}

/// The KPIs, the table and its pagination of the sell orders, for live refreshes and for the
/// page's own controls.
pub async fn sell_content(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    query: ViewQuery,
) -> Response {
    content(state, account, session, Side::Sell, query).await
}

/// The same for the buy orders.
pub async fn buy_content(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    query: ViewQuery,
) -> Response {
    content(state, account, session, Side::Buy, query).await
}

/// The freshness section only, for live refreshes. The same for both sides.
pub async fn freshness(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    query: ViewQuery,
) -> Response {
    match load(&state, account, &session, Side::Sell, &query).await {
        Ok((view, _)) => pages::render(&FreshnessFragment { view }),
        Err(response) => *response,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(margin: Option<i64>, cost: Option<i64>, value: i64, corp: bool) -> OrderRow {
        OrderRow {
            order_id: 1,
            character_id: 1,
            character_name: "Alice".to_owned(),
            type_id: 34,
            type_name: None,
            region_id: 10000002,
            region_name: None,
            location_id: 60003760,
            location_name: None,
            location_known: false,
            is_corporation: corp,
            price_cents: 100,
            volume_remain: 1,
            volume_total: 1,
            value_cents: value,
            escrow_cents: Some(value),
            issued: OffsetDateTime::UNIX_EPOCH,
            duration: 90,
            undercut: None,
            best_price_cents: None,
            gap_cents: None,
            undercut_volume: None,
            covered_units: None,
            margin_cents: margin,
            cost_per_unit_cents: None,
            cost_cents: cost,
        }
    }

    #[test]
    fn the_net_margin_sums_margins_and_costs_and_counts_the_orders_left_out() {
        // 1,500,000 and 500,000 ISK of margin on 8,000,000 ISK of known cost, and one without.
        let rows = [
            order(Some(150_000_000), Some(500_000_000), 0, false),
            order(Some(50_000_000), Some(300_000_000), 0, false),
            order(None, None, 0, false),
        ];

        let net = NetMargin::of(&rows);

        assert_eq!(net.margin_cents, 200_000_000);
        assert_eq!(net.cost_cents, 800_000_000);
        assert_eq!((net.counted, net.left_out), (2, 1));
        assert_eq!(net.percent().as_deref(), Some("25.00"));
    }

    #[test]
    fn a_negative_net_margin_has_a_negative_percentage() {
        let rows = [order(Some(-150_000), Some(10_000_000), 0, false)];

        let net = NetMargin::of(&rows);

        assert_eq!(net.margin_cents, -150_000);
        assert_eq!(net.percent().as_deref(), Some("-1.50"));
    }

    #[test]
    fn without_any_margin_there_is_no_net_margin() {
        let rows = [order(None, None, 0, false), order(None, None, 0, true)];

        let net = NetMargin::of(&rows);

        assert_eq!((net.counted, net.left_out), (0, 2));
        assert_eq!(net.percent(), None);
        assert_eq!(NetMargin::of(&[]).percent(), None);
    }

    #[test]
    fn the_side_totals_count_every_order_and_split_off_the_corporation_share() {
        let rows = [
            order(None, None, 1_000, false),
            order(None, None, 2_500, true),
            order(None, None, 500, false),
        ];

        let sell = SideTotals::of(&rows, Side::Sell);
        assert_eq!(
            (sell.count, sell.total_cents, sell.corp_cents),
            (3, 4_000, 2_500)
        );

        // A buy order counts its escrow (a missing one as nothing).
        let mut no_escrow = order(None, None, 900, false);
        no_escrow.escrow_cents = None;
        let buy = SideTotals::of(&[order(None, None, 700, true), no_escrow], Side::Buy);
        assert_eq!((buy.count, buy.total_cents, buy.corp_cents), (2, 700, 700));
    }

    #[test]
    fn sorting_defaults_to_item_name_ascending() {
        let default = Sort::parse(None, None);
        assert_eq!(default.key, SortKey::Item);
        assert_eq!(default.dir, Dir::Asc);
    }

    #[test]
    fn known_columns_and_directions_are_accepted() {
        let s = Sort::parse(Some("price"), Some("desc"));
        assert_eq!((s.key, s.dir), (SortKey::Price, Dir::Desc));
        let s = Sort::parse(Some("left"), Some("asc"));
        assert_eq!(s.key, SortKey::TimeLeft);
    }

    #[test]
    fn every_key_round_trips_through_its_name() {
        for key in SortKey::ALL {
            assert_eq!(Sort::parse(Some(key.name()), None).key, key);
        }
    }

    #[test]
    fn unknown_values_fall_back_to_the_default() {
        for key in ["", "price; DROP TABLE orders", "o.price", "PRICE", "name"] {
            assert_eq!(
                Sort::parse(Some(key), Some("sideways")),
                Sort::default(),
                "{key}"
            );
        }
    }
}
