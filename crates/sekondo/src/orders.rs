//! The orders overview: reading current orders for an account's selected characters, and the page
//! and fragments that show them.

use askama::Template;
use axum::{
    extract::State,
    response::{IntoResponse, Redirect, Response},
};
use sqlx::PgPool;
use time::OffsetDateTime;
use tower_sessions::Session;
use uuid::Uuid;

use crate::{
    account::CurrentAccount,
    app::AppState,
    csrf, format,
    freshness::{FreshnessView, freshness_view, list_freshness},
    pages::{self, Nav, Page},
    selection::{CharacterSelection, ViewQuery},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Sell,
    Buy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Character,
    Item,
    Location,
    Region,
    Price,
    Volume,
    /// The order value of a sell order, the escrow of a buy order.
    Amount,
    Issued,
    TimeLeft,
}

impl SortKey {
    const ALL: [Self; 9] = [
        Self::Character,
        Self::Item,
        Self::Location,
        Self::Region,
        Self::Price,
        Self::Volume,
        Self::Amount,
        Self::Issued,
        Self::TimeLeft,
    ];

    /// The name used in `sort` and `bsort`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Character => "character",
            Self::Item => "item",
            Self::Location => "location",
            Self::Region => "region",
            Self::Price => "price",
            Self::Volume => "volume",
            Self::Amount => "amount",
            Self::Issued => "issued",
            Self::TimeLeft => "left",
        }
    }

    /// The SQL expression to sort by. Only these literals ever reach the query text.
    fn expression(self) -> &'static str {
        match self {
            Self::Character => "lower(c.name)",
            Self::Item => "lower(COALESCE(tn.name, '#' || o.type_id))",
            Self::Location => "lower(COALESCE(ln.name, '#' || o.location_id))",
            Self::Region => "lower(COALESCE(rn.name, '#' || o.region_id))",
            Self::Price => "o.price",
            Self::Volume => "o.volume_remain",
            Self::Amount => {
                "CASE WHEN o.is_buy_order THEN COALESCE(o.escrow, 0) ELSE o.price * o.volume_remain END"
            }
            Self::Issued => "o.issued",
            Self::TimeLeft => "o.issued + o.duration * interval '1 day'",
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
}

impl OrderRow {
    pub fn expires_at(&self) -> OffsetDateTime {
        self.issued + time::Duration::days(i64::from(self.duration))
    }
}

/// The current orders of the selected characters, restricted to the account in the query itself
/// so a selection can never reach another account's data.
pub async fn list_orders(
    pool: &PgPool,
    account_id: Uuid,
    selection: &CharacterSelection,
    side: Side,
    sort: Sort,
) -> Result<Vec<OrderRow>, sqlx::Error> {
    let direction = match sort.dir {
        Dir::Asc => "ASC",
        Dir::Desc => "DESC",
    };
    let sql = format!(
        "SELECT o.order_id, o.character_id, c.name AS character_name,
                o.type_id, tn.name AS type_name,
                o.region_id, rn.name AS region_name,
                o.location_id, ln.name AS location_name, (ln.id IS NOT NULL) AS location_known,
                o.is_corporation,
                (o.price * 100)::bigint AS price_cents,
                o.volume_remain, o.volume_total,
                (o.price * o.volume_remain * 100)::bigint AS value_cents,
                (o.escrow * 100)::bigint AS escrow_cents,
                o.issued, o.duration
         FROM current_orders o
         JOIN characters c ON c.character_id = o.character_id
         LEFT JOIN names tn ON tn.id = o.type_id
         LEFT JOIN names rn ON rn.id = o.region_id
         LEFT JOIN names ln ON ln.id = o.location_id
         WHERE c.account_id = $1 AND o.character_id = ANY($2) AND o.is_buy_order = $3
         ORDER BY {} {direction}, o.order_id",
        sort.key.expression()
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(account_id)
        .bind(selection.ids())
        .bind(side == Side::Buy)
        .fetch_all(pool)
        .await
}

// ---- view model ----

pub struct HeaderView {
    pub label: &'static str,
    pub href: String,
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
    pub volume: String,
    pub amount: String,
    pub amount_exact: String,
    pub issued: String,
    pub left: String,
}

pub struct TableView {
    pub headers: Vec<HeaderView>,
    pub rows: Vec<RowView>,
}

pub struct FilterEntry {
    pub id: i64,
    pub name: String,
    pub checked: bool,
}

/// Everything the orders page and its fragments show for one request.
pub struct OrdersView {
    pub characters: Vec<FilterEntry>,
    /// The URL of the page itself, carrying the whole view state.
    pub tables_url: String,
    pub freshness_url: String,
    pub sell_sort: Sort,
    pub buy_sort: Sort,
    pub sell: TableView,
    pub buy: TableView,
    pub freshness: Vec<FreshnessView>,
}

/// The view state a URL carries: the selection and one sorting per table.
struct ViewState {
    chars: Option<String>,
    sell: Sort,
    buy: Sort,
}

impl ViewState {
    fn query(&self) -> String {
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
        let query = pairs.finish().replace("%2C", ",");
        if query.is_empty() {
            query
        } else {
            format!("?{query}")
        }
    }

    fn with_sort(&self, side: Side, sort: Sort) -> Self {
        Self {
            chars: self.chars.clone(),
            sell: if side == Side::Sell { sort } else { self.sell },
            buy: if side == Side::Buy { sort } else { self.buy },
        }
    }
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
    let columns = [
        ("Character", SortKey::Character),
        ("Item", SortKey::Item),
        ("Location", SortKey::Location),
        ("Region", SortKey::Region),
        ("Price", SortKey::Price),
        ("Volume", SortKey::Volume),
        (amount, SortKey::Amount),
        ("Issued", SortKey::Issued),
        ("Time left", SortKey::TimeLeft),
    ];
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
            HeaderView {
                label,
                href: format!("/orders{}", state.with_sort(side, next).query()),
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
        volume: format!("{} / {}", row.volume_remain, row.volume_total),
        amount: format::isk_short(amount_cents),
        amount_exact: format::isk_exact(amount_cents),
        issued: format::eve_time(row.issued),
        left: format::time_left(row.expires_at(), now),
    }
}

async fn build_view(
    state: &AppState,
    account_id: Uuid,
    query: &ViewQuery,
) -> Result<Option<(OrdersView, CharacterSelection)>, sqlx::Error> {
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
    let view_state = ViewState {
        chars: selection.param(),
        sell: Sort::parse(query.sort.as_deref(), query.dir.as_deref()),
        buy: Sort::parse(query.bsort.as_deref(), query.bdir.as_deref()),
    };
    let now = state.clock.now();
    let interval = state.config.order_poll_interval;

    let sell_rows = list_orders(
        &state.pool,
        account_id,
        &selection,
        Side::Sell,
        view_state.sell,
    )
    .await?;
    let buy_rows = list_orders(
        &state.pool,
        account_id,
        &selection,
        Side::Buy,
        view_state.buy,
    )
    .await?;
    let freshness = list_freshness(&state.pool, account_id, &selection).await?;

    let page_query = view_state.query();
    let view = OrdersView {
        characters: characters
            .iter()
            .map(|c| FilterEntry {
                id: c.character_id,
                name: c.name.clone(),
                checked: selection.contains(c.character_id),
            })
            .collect(),
        tables_url: format!("/orders/tables{page_query}"),
        freshness_url: format!("/orders/freshness{page_query}"),
        sell_sort: view_state.sell,
        buy_sort: view_state.buy,
        sell: TableView {
            headers: headers(&view_state, Side::Sell),
            rows: sell_rows
                .iter()
                .map(|r| row_view(r, Side::Sell, now))
                .collect(),
        },
        buy: TableView {
            headers: headers(&view_state, Side::Buy),
            rows: buy_rows
                .iter()
                .map(|r| row_view(r, Side::Buy, now))
                .collect(),
        },
        freshness: freshness
            .iter()
            .map(|r| freshness_view(r, now, interval))
            .collect(),
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
#[template(path = "orders_tables.html")]
struct TablesFragment {
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
    query: &ViewQuery,
) -> Result<(OrdersView, CharacterSelection), Box<Response>> {
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
    pages::render(&OrdersPage {
        csrf_token,
        nav: Some(Nav::new(Page::Orders).with_chars_query(selection.query())),
        view,
    })
}

/// The sell and buy tables only, for live refreshes.
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

#[cfg(test)]
mod tests {
    use super::*;

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
