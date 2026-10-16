//! Routes that exist only in builds with the `test-hooks` feature (the Playwright suite). They
//! are compiled out of everything else, so production has no way to reach them.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::{
    account::CurrentAccount, app::AppState, pages, poller::PollOutcome, tokens::TokenError,
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
    match state.poller.poll_character(character_id).await {
        PollOutcome::Updated => "updated",
        PollOutcome::NeedsRelogin => "needs-relogin",
        PollOutcome::Failed => "failed",
        PollOutcome::Deferred => "deferred",
        PollOutcome::Dropped => "dropped",
    }
    .into_response()
}
