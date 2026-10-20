//! Routes that exist only in builds with the `test-hooks` feature (the Playwright suite). They
//! are compiled out of everything else, so production has no way to reach them.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use serde::Deserialize;

use crate::{
    account::CurrentAccount,
    app::AppState,
    market::RefreshOutcome,
    opportunities::{REGION_ID, STATION_ID, stats},
    pages,
    poller::PollOutcome,
    store::{SeedHistoryRow, SeedSnapshotRow, StoreError},
    tokens::TokenError,
    wealth_poller::Kind,
};

/// Forces a token refresh for one of the signed-in account's characters, as the background
/// poller will do later. The stored access token is expired first so SSO is really contacted.
pub async fn refresh(
    State(state): State<AppState>,
    account: CurrentAccount,
    Path(character_id): Path<i64>,
) -> Response {
    match state.repo.find_character(character_id).await {
        Ok(Some(c)) if c.account_id == account.id => {}
        Ok(_) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return pages::internal_error(e),
    }
    if let Err(e) = state
        .repo
        .expire_access_token(character_id, state.clock.now() - time::Duration::hours(1))
        .await
    {
        return pages::internal_error(e);
    }
    match state.tokens.access_token(character_id).await {
        Ok(_) => "ok".into_response(),
        Err(TokenError::NeedsRelogin) => "needs-relogin".into_response(),
        Err(TokenError::Temporary(_)) => {
            (StatusCode::SERVICE_UNAVAILABLE, "temporary").into_response()
        }
        Err(TokenError::UnknownCharacter) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Polls one of the signed-in account's characters right now, ignoring its schedule (but not the
/// ESI gate), so browser tests do not have to wait for the scheduler.
pub async fn poll(
    State(state): State<AppState>,
    account: CurrentAccount,
    Path(character_id): Path<i64>,
) -> Response {
    match state.repo.find_character(character_id).await {
        Ok(Some(c)) if c.account_id == account.id => {}
        Ok(_) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return pages::internal_error(e),
    }
    outcome_text(state.poller.poll_character(character_id).await).into_response()
}

fn outcome_text(outcome: PollOutcome) -> &'static str {
    match outcome {
        PollOutcome::Updated => "updated",
        PollOutcome::NeedsRelogin => "needs-relogin",
        PollOutcome::Failed => "failed",
        PollOutcome::Deferred => "deferred",
        PollOutcome::Dropped => "dropped",
        PollOutcome::Skipped => "skipped",
    }
}

/// Polls the wallet or the assets of one of the signed-in account's characters right now,
/// ignoring the schedule (but not the ESI gate).
pub async fn wealth_poll(
    State(state): State<AppState>,
    account: CurrentAccount,
    Path((character_id, kind)): Path<(i64, String)>,
) -> Response {
    let Some(kind) = Kind::ALL.into_iter().find(|k| k.as_str() == kind) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match state.repo.find_character(character_id).await {
        Ok(Some(c)) if c.account_id == account.id => {}
        Ok(_) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return pages::internal_error(e),
    }
    outcome_text(state.wealth.poll(character_id, kind).await).into_response()
}

/// Refreshes ESI's market prices right now, whatever the schedule says (but not the ESI gate).
pub async fn price_refresh(State(state): State<AppState>, _account: CurrentAccount) -> Response {
    match state.prices.refresh().await {
        RefreshOutcome::Updated => "updated",
        RefreshOutcome::Failed => "failed",
        RefreshOutcome::Deferred => "deferred",
    }
    .into_response()
}

/// Refreshes every market pair the signed-in account's current orders use, right now and
/// whatever `next_poll_at` says (but not the ESI gate). Answers how many pairs it refreshed.
pub async fn market_poll(State(state): State<AppState>, account: CurrentAccount) -> Response {
    match state.market.refresh_account_pairs(account.id).await {
        Ok(refreshed) => refreshed.to_string().into_response(),
        Err(e) => pages::internal_error(e),
    }
}

/// Polls the skills and the standings of every character of the signed-in account right now and
/// looks up the owners of its stations, whatever the schedule says (but not the ESI gate).
pub async fn fee_poll(State(state): State<AppState>, account: CurrentAccount) -> Response {
    match state.fees.refresh_account(account.id).await {
        Ok(()) => "ok".into_response(),
        Err(e) => pages::internal_error(e),
    }
}

/// Refreshes the station book right now, whatever the schedule says (but not the ESI gate).
pub async fn station_book(State(state): State<AppState>, _account: CurrentAccount) -> Response {
    match state.station_book.refresh().await {
        RefreshOutcome::Updated => "updated",
        RefreshOutcome::Failed => "failed",
        RefreshOutcome::Deferred => "deferred",
    }
    .into_response()
}

/// Asks for the history of every due station item until none is due. Answers how many it asked.
pub async fn station_history(State(state): State<AppState>, _account: CurrentAccount) -> Response {
    state
        .station_history
        .tick()
        .await
        .to_string()
        .into_response()
}

/// Rolls the station snapshots up into the hourly and daily aggregates now, and recomputes the
/// expansion fields when the hourly rollup advanced.
pub async fn station_rollups(State(state): State<AppState>, _account: CurrentAccount) -> Response {
    let now = state.clock.now();
    let from = now - time::Duration::days(40);
    let to = now + time::Duration::hours(2);
    if let Err(e) = state.stores.station.refresh_rollups(from, to).await {
        return pages::internal_error(e);
    }
    match stats::refresh_expansions_if_advanced(&*state.stores.station, now).await {
        Ok(_) => "ok".into_response(),
        Err(e) => pages::internal_error(e),
    }
}

/// One synthetic snapshot of an item some minutes ago. Whatever the body leaves out gets a
/// plausible value.
#[derive(Deserialize)]
pub struct SeedSnapshot {
    type_id: i64,
    minutes_back: i64,
    best_buy: f64,
    best_sell: f64,
    #[serde(default = "default_near_units")]
    buy_near_units: i64,
    #[serde(default = "default_near_orders")]
    buy_near_orders: i32,
    #[serde(default = "default_near_units")]
    sell_near_units: i64,
    #[serde(default = "default_near_orders")]
    sell_near_orders: i32,
    #[serde(default = "default_total_units")]
    buy_units: i64,
    #[serde(default = "default_total_units")]
    sell_units: i64,
    #[serde(default)]
    buy_taken: i64,
    #[serde(default)]
    sell_taken: i64,
}

fn default_near_units() -> i64 {
    100
}

fn default_near_orders() -> i32 {
    2
}

fn default_total_units() -> i64 {
    1000
}

/// One synthetic history day of an item.
#[derive(Deserialize)]
pub struct SeedHistory {
    type_id: i64,
    days_back: i64,
    average: f64,
    highest: f64,
    lowest: f64,
    volume: i64,
    #[serde(default = "default_order_count")]
    order_count: i64,
}

fn default_order_count() -> i64 {
    10
}

#[derive(Deserialize, Default)]
pub struct Seed {
    #[serde(default)]
    snapshots: Vec<SeedSnapshot>,
    #[serde(default)]
    history: Vec<SeedHistory>,
}

/// Inserts synthetic station snapshots and history for past times, because a browser test cannot
/// wait days for hourly patterns, and recomputes the indicator fields from the history.
pub async fn station_seed(
    State(state): State<AppState>,
    _account: CurrentAccount,
    body: Option<axum::Json<Seed>>,
) -> Response {
    let seed = body.map(|axum::Json(seed)| seed).unwrap_or_default();
    let now = state.clock.now();
    match insert_seed(&state, &seed, now).await {
        Ok(()) => format!(
            "{} snapshots, {} history days",
            seed.snapshots.len(),
            seed.history.len()
        )
        .into_response(),
        Err(e) => pages::internal_error(e),
    }
}

async fn insert_seed(
    state: &AppState,
    seed: &Seed,
    now: time::OffsetDateTime,
) -> Result<(), StoreError> {
    let snapshots: Vec<SeedSnapshotRow> = seed
        .snapshots
        .iter()
        .map(|s| SeedSnapshotRow {
            recorded_at: now - time::Duration::minutes(s.minutes_back),
            type_id: s.type_id,
            best_buy: s.best_buy,
            best_sell: s.best_sell,
            buy_near_units: s.buy_near_units,
            buy_near_orders: s.buy_near_orders,
            sell_near_units: s.sell_near_units,
            sell_near_orders: s.sell_near_orders,
            buy_units: s.buy_units,
            sell_units: s.sell_units,
            buy_taken: s.buy_taken,
            sell_taken: s.sell_taken,
        })
        .collect();
    state
        .stores
        .station
        .seed_snapshots(REGION_ID, STATION_ID, &snapshots)
        .await?;
    let history: Vec<SeedHistoryRow> = seed
        .history
        .iter()
        .map(|h| SeedHistoryRow {
            type_id: h.type_id,
            day: (now - time::Duration::days(h.days_back)).date(),
            average: h.average,
            highest: h.highest,
            lowest: h.lowest,
            volume: h.volume,
            order_count: h.order_count,
        })
        .collect();
    state
        .stores
        .station
        .seed_history(REGION_ID, &history)
        .await?;
    if !seed.history.is_empty() {
        stats::refresh_indicators(&*state.stores.station, now).await?;
    }
    Ok(())
}
