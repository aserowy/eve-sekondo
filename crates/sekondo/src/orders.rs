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
    csrf,
    fees::{self, FEE_RATES_SQL},
    format,
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
    Issued,
    TimeLeft,
}

impl SortKey {
    const ALL: [Self; 13] = [
        Self::Character,
        Self::Item,
        Self::Location,
        Self::Region,
        Self::Price,
        Self::Standing,
        Self::Gap,
        Self::UndercutVolume,
        Self::Volume,
        Self::Amount,
        Self::Margin,
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
            Self::Standing => "standing",
            Self::Gap => "gap",
            Self::UndercutVolume => "undercut-volume",
            Self::Volume => "volume",
            Self::Amount => "amount",
            Self::Margin => "margin",
            Self::Issued => "issued",
            Self::TimeLeft => "left",
        }
    }

    /// The SQL `ORDER BY` terms for a direction. Only these literals ever reach the query text.
    /// The standing and the gap put orders without a standing last in both directions, so the
    /// first term does not follow the direction.
    fn order_by(self, direction: &str) -> String {
        match self {
            // `undercut > 0` is false for "Top", so it comes first in ascending order.
            Self::Standing => format!(
                "(st.undercut IS NULL), (st.undercut > 0) {direction}, st.undercut {direction}"
            ),
            // A top order has no best price to differ from: it counts as no difference.
            Self::Gap => format!(
                "(st.undercut IS NULL), COALESCE(abs(st.best_price - o.price), 0) {direction}"
            ),
            Self::UndercutVolume => {
                format!("(st.undercut IS NULL), COALESCE(st.undercut_volume, 0) {direction}")
            }
            Self::Margin => {
                format!("({MARGIN_CENTS}) IS NULL, ({MARGIN_CENTS}) {direction}")
            }
            other => format!("{} {direction}", other.expression()),
        }
    }

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
            Self::Standing | Self::Gap | Self::UndercutVolume | Self::Margin => {
                unreachable!("ordered by `order_by`")
            }
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
}

impl OrderRow {
    pub fn expires_at(&self) -> OffsetDateTime {
        self.issued + time::Duration::days(i64::from(self.duration))
    }
}

/// Joins `st` to the current orders `o`: how many other public orders undercut each of them and
/// the best of those prices and their remaining volume, or NULL when there is no standing (design §5). The listing and the
/// change detection both use it, so a signal is sent exactly when what is shown changes.
///
/// A standing exists only when the order's pair was refreshed once, the order is not in a player
/// structure and, for a buy order, its own row in the market data tells its solar system.
/// A sell order is compared at its station. A buy order is compared at its station with the buy
/// orders that reach it: any order there, a `region` range, and a `solarsystem` or jump range
/// from the same solar system. The own range is not read.
pub(crate) const STANDING_JOIN: &str = "LEFT JOIN LATERAL (
        SELECT CASE WHEN a.available THEN c.undercut END AS undercut,
               CASE WHEN a.available THEN c.best_price END AS best_price,
               CASE WHEN a.available THEN c.undercut_volume END AS undercut_volume
        FROM (SELECT o.location_id < 100000000
                 AND EXISTS (SELECT 1 FROM market_sync s
                             WHERE s.region_id = o.region_id AND s.type_id = o.type_id
                               AND s.last_success_at IS NOT NULL)
                 AND (NOT o.is_buy_order
                      OR EXISTS (SELECT 1 FROM market_orders own WHERE own.order_id = o.order_id))
                 AS available) a,
        LATERAL (
            SELECT count(*) AS undercut,
                   CASE WHEN o.is_buy_order THEN max(m.price) ELSE min(m.price) END AS best_price,
                   sum(m.volume_remain)::bigint AS undercut_volume
            FROM market_orders m
            WHERE m.region_id = o.region_id AND m.type_id = o.type_id
              AND m.order_id <> o.order_id
              AND CASE WHEN o.is_buy_order THEN
                    m.is_buy_order AND m.price > o.price
                    AND (m.location_id = o.location_id
                         OR m.range = 'region'
                         OR (m.range <> 'station'
                             AND m.system_id = (SELECT own.system_id FROM market_orders own
                                                WHERE own.order_id = o.order_id)))
                  ELSE
                    NOT m.is_buy_order AND m.price < o.price AND m.location_id = o.location_id
                  END
        ) c
    ) st ON true";

/// The per-unit difference to the best price in cents, for an order that is undercut. Needs
/// `STANDING_JOIN` (`st`) and the current order (`o`).
pub(crate) const GAP_CENTS: &str =
    "CASE WHEN st.undercut > 0 THEN (abs(st.best_price - o.price) * 100)::bigint END";

/// The margin of a sell order in cents (design §6). Needs the cost row `sc`, the fee rates `f` and
/// the current order `o`. The cost per unit is the whole known cost over the total volume, so a
/// part that was sold already carries its share.
const MARGIN_CENTS: &str = "CASE WHEN NOT o.is_buy_order AND NOT o.is_corporation
          AND sc.covered_units = o.volume_total
          AND f.sales_tax IS NOT NULL AND f.broker_fee IS NOT NULL
     THEN (round(o.volume_remain * o.price * (1 - f.sales_tax / 100 - f.broker_fee / 100)
                 - o.volume_remain * sc.cost / o.volume_total, 2) * 100)::bigint
END";

/// The average cost per unit in cents, for an order whose cost covers its whole volume.
const COST_PER_UNIT_CENTS: &str =
    "CASE WHEN sc.covered_units = o.volume_total AND o.volume_total > 0
     THEN round(sc.cost * 100 / o.volume_total)::bigint
END";

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
                o.issued, o.duration,
                st.undercut,
                (st.best_price * 100)::bigint AS best_price_cents,
                {GAP_CENTS} AS gap_cents,
                st.undercut_volume,
                CASE WHEN NOT o.is_buy_order AND NOT o.is_corporation THEN sc.covered_units END
                    AS covered_units,
                {MARGIN_CENTS} AS margin_cents,
                {COST_PER_UNIT_CENTS} AS cost_per_unit_cents
         FROM current_orders o
         JOIN characters c ON c.character_id = o.character_id
         LEFT JOIN names tn ON tn.id = o.type_id
         LEFT JOIN names rn ON rn.id = o.region_id
         LEFT JOIN names ln ON ln.id = o.location_id
         {STANDING_JOIN}
         LEFT JOIN sell_order_costs sc ON sc.order_id = o.order_id
         LEFT JOIN ({FEE_RATES_SQL}) f
                ON f.character_id = o.character_id AND f.location_id = o.location_id
         WHERE c.account_id = $2 AND o.character_id = ANY($1) AND o.is_buy_order = $3
         ORDER BY {}, o.order_id",
        sort.key.order_by(direction)
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(selection.ids())
        .bind(account_id)
        .bind(side == Side::Buy)
        .fetch_all(pool)
        .await
}

// ---- view model ----

/// Shown where an order has no standing, gap and undercut volume.
const NO_DATA: &str = "\u{2014}";

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
    let mut columns = vec![
        ("Character", SortKey::Character),
        ("Item", SortKey::Item),
        ("Price", SortKey::Price),
        ("Standing", SortKey::Standing),
        ("Behind by", SortKey::Gap),
        ("Undercut volume", SortKey::UndercutVolume),
        ("Volume", SortKey::Volume),
        (amount, SortKey::Amount),
        ("Issued", SortKey::Issued),
        ("Time left", SortKey::TimeLeft),
        ("Location", SortKey::Location),
        ("Region", SortKey::Region),
    ];
    if side == Side::Sell {
        columns.insert(8, ("Margin", SortKey::Margin));
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
    let mut fee_locations =
        fees::locations_by_character(&state.pool, account_id, selection.ids()).await?;

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
            has_margin: true,
            headers: headers(&view_state, Side::Sell),
            rows: sell_rows
                .iter()
                .map(|r| row_view(r, Side::Sell, now))
                .collect(),
        },
        buy: TableView {
            has_margin: false,
            headers: headers(&view_state, Side::Buy),
            rows: buy_rows
                .iter()
                .map(|r| row_view(r, Side::Buy, now))
                .collect(),
        },
        freshness: freshness
            .iter()
            .map(|r| FreshnessView {
                locations: fee_locations.remove(&r.character_id).unwrap_or_default(),
                ..freshness_view(r, now, interval)
            })
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
