//! Routes that exist only in builds with the `test-hooks` feature (the Playwright suite). They
//! are compiled out of everything else, so production has no way to reach them.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::{
    account::CurrentAccount, app::AppState, market::RefreshOutcome, pages, poller::PollOutcome,
    tokens::TokenError, wealth_poller::Kind,
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
    if let Err(e) = state.repo.expire_access_token(character_id).await {
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
