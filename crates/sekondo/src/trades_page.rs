//! The trades page and its fragments: the personal wallet transactions of the selected
//! characters with what the sales earned, and how fresh they are.

use askama::Template;
use axum::{
    extract::State,
    response::{IntoResponse, Redirect, Response},
};
use time::OffsetDateTime;
use tower_sessions::Session;
use uuid::Uuid;

use crate::{
    account::CurrentAccount,
    app::AppState,
    csrf, format,
    freshness::{FreshnessView, freshness_view, list_transactions_freshness},
    orders::FilterEntry,
    pages::{self, Nav, Page},
    range::Range,
    selection::{CharacterSelection, ViewQuery},
    trades::{self, TradeRow},
    wealth_poller::TRANSACTIONS_INTERVAL,
};

/// Shown where a sale has no margin.
const NO_DATA: &str = "\u{2014}";

pub struct RowView {
    pub transaction_id: i64,
    pub at: String,
    pub character: String,
    pub item: String,
    pub location: String,
    pub is_sale: bool,
    pub side: &'static str,
    pub quantity: String,
    pub price: String,
    pub price_exact: String,
    pub value: String,
    pub value_exact: String,
    /// The margin shortened, a dash for a sale without one, and empty for a purchase.
    pub margin: String,
    /// What hovering the margin adds: the exact value and the cost per unit, or how many units
    /// have a known cost. Empty when there is nothing to add.
    pub margin_title: String,
    /// The margin is negative: it is shown in the error style.
    pub margin_negative: bool,
}

pub struct TotalsView {
    pub bought: String,
    pub bought_exact: String,
    pub sold: String,
    pub sold_exact: String,
    pub margin: String,
    /// The exact margin sum, and how many sales it leaves out.
    pub margin_title: String,
}

pub struct RangeLink {
    pub label: &'static str,
    /// The page's URL with this range, for the address bar and without scripts.
    pub href: String,
    /// The table fragment's URL with this range.
    pub table_url: String,
    pub current: bool,
}

/// Everything the trades page and its fragments show for one request.
pub struct TradesView {
    pub characters: Vec<FilterEntry>,
    pub range: Range,
    pub ranges: Vec<RangeLink>,
    pub table_url: String,
    pub freshness_url: String,
    pub rows: Vec<RowView>,
    pub totals: TotalsView,
    pub freshness: Vec<FreshnessView>,
    /// How many of the selected characters are stale or need re-login.
    pub warnings: usize,
}

/// The view state a URL carries: the selection and the range, each only when it is not the
/// default.
struct ViewState {
    chars: Option<String>,
    range: Range,
}

impl ViewState {
    fn query(&self, with_range: bool) -> String {
        let mut pairs = url::form_urlencoded::Serializer::new(String::new());
        if let Some(chars) = &self.chars {
            pairs.append_pair("chars", chars);
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

    fn with_range(&self, range: Range) -> Self {
        Self {
            chars: self.chars.clone(),
            range,
        }
    }
}

/// Cents of a quantity at a unit price; it can exceed `i64`.
fn value_cents(row: &TradeRow) -> i128 {
    i128::from(row.quantity) * i128::from(row.unit_price_cents)
}

fn saturate(cents: i128) -> i64 {
    i64::try_from(cents).unwrap_or(if cents < 0 { i64::MIN } else { i64::MAX })
}

fn row_view(row: &TradeRow) -> RowView {
    let value = saturate(value_cents(row));
    let (margin, margin_title) = if !row.is_sale {
        (String::new(), String::new())
    } else {
        match (row.margin_cents, row.cost_per_unit_cents) {
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
                    Some(covered) if covered < row.quantity => {
                        format!("cost known for {covered} of {} units", row.quantity)
                    }
                    _ => String::new(),
                },
            ),
        }
    };
    RowView {
        transaction_id: row.transaction_id,
        at: format::eve_time(row.at),
        character: row.character_name.clone(),
        item: row
            .type_name
            .clone()
            .unwrap_or_else(|| format!("#{}", row.type_id)),
        location: row
            .location_name
            .clone()
            .unwrap_or_else(|| format!("Unknown location ({})", row.location_id)),
        is_sale: row.is_sale,
        side: if row.is_sale { "Sell" } else { "Buy" },
        quantity: format::thousands(row.quantity),
        price: format::isk_short(row.unit_price_cents),
        price_exact: format::isk_exact(row.unit_price_cents),
        value: format::isk_short(value),
        value_exact: format::isk_exact(value),
        margin,
        margin_title,
        margin_negative: row.is_sale && row.margin_cents.is_some_and(|m| m < 0),
    }
}

/// Summed over the listed rows, so the totals always match the table.
fn totals_view(rows: &[TradeRow]) -> TotalsView {
    let mut bought: i128 = 0;
    let mut sold: i128 = 0;
    let mut margin: i128 = 0;
    let mut left_out = 0usize;
    for row in rows {
        if row.is_sale {
            sold += value_cents(row);
            match row.margin_cents {
                Some(m) => margin += i128::from(m),
                None => left_out += 1,
            }
        } else {
            bought += value_cents(row);
        }
    }
    let (bought, sold, margin) = (saturate(bought), saturate(sold), saturate(margin));
    let mut margin_title = format::isk_exact(margin);
    if left_out > 0 {
        margin_title.push_str(&format!(
            "; {left_out} {} left out",
            if left_out == 1 { "sale" } else { "sales" }
        ));
    }
    TotalsView {
        bought: format::isk_short(bought),
        bought_exact: format::isk_exact(bought),
        sold: format::isk_short(sold),
        sold_exact: format::isk_exact(sold),
        margin: format::isk_short(margin),
        margin_title,
    }
}

async fn build_view(
    state: &AppState,
    account_id: Uuid,
    query: &ViewQuery,
) -> Result<Option<(TradesView, CharacterSelection)>, sqlx::Error> {
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
    let now: OffsetDateTime = state.clock.now();
    let view_state = ViewState {
        chars: selection.param(),
        range: Range::parse(query.range.as_deref()),
    };
    let listed = trades::list(
        &state.pool,
        account_id,
        &selection,
        now - view_state.range.span(),
        now,
    )
    .await?;
    let freshness: Vec<FreshnessView> =
        list_transactions_freshness(&state.pool, account_id, &selection)
            .await?
            .iter()
            .map(|r| freshness_view(r, now, TRANSACTIONS_INTERVAL))
            .collect();
    let warnings = freshness
        .iter()
        .filter(|f| f.stale || f.needs_relogin)
        .count();
    let view = TradesView {
        characters: characters
            .iter()
            .map(|c| FilterEntry {
                id: c.character_id,
                name: c.name.clone(),
                checked: selection.contains(c.character_id),
            })
            .collect(),
        range: view_state.range,
        ranges: Range::ALL
            .into_iter()
            .map(|range| {
                let target = view_state.with_range(range);
                RangeLink {
                    label: range.label(),
                    href: format!("/trades{}", target.query(true)),
                    table_url: format!("/trades/table{}", target.query(true)),
                    current: range == view_state.range,
                }
            })
            .collect(),
        table_url: format!("/trades/table{}", view_state.query(true)),
        freshness_url: format!("/trades/freshness{}", view_state.query(false)),
        rows: listed.iter().map(row_view).collect(),
        totals: totals_view(&listed),
        freshness,
        warnings,
    };
    Ok(Some((view, selection)))
}

#[derive(Template)]
#[template(path = "trades.html")]
struct TradesPage {
    csrf_token: String,
    nav: Option<Nav>,
    view: TradesView,
}

#[derive(Template)]
#[template(path = "trades_table_fragment.html")]
struct TableFragment {
    view: TradesView,
}

#[derive(Template)]
#[template(path = "trades_freshness.html")]
struct FreshnessFragment {
    view: TradesView,
}

async fn load(
    state: &AppState,
    account: CurrentAccount,
    session: &Session,
    query: &ViewQuery,
) -> Result<(TradesView, CharacterSelection), Box<Response>> {
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
    pages::render(&TradesPage {
        csrf_token,
        nav: Some(Nav::new(Page::Trades).with_chars_query(selection.query())),
        view,
    })
}

/// The table panel with its range control, for range switches and live refreshes.
pub async fn table(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    query: ViewQuery,
) -> Response {
    match load(&state, account, &session, &query).await {
        Ok((view, _)) => pages::render(&TableFragment { view }),
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
