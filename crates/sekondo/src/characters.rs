//! Actions on the signed-in account's characters.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use tower_sessions::Session;

use crate::{account::CurrentAccount, app::AppState, pages};

fn is_htmx(headers: &HeaderMap) -> bool {
    headers.get("hx-request").is_some_and(|v| v == "true")
}

/// Unlinks a character after the user confirmed in the browser (`hx-confirm`). Removing the last
/// character deletes the account and signs the user out.
pub async fn unlink(
    State(state): State<AppState>,
    account: CurrentAccount,
    session: Session,
    headers: HeaderMap,
    Path(character_id): Path<i64>,
) -> Response {
    // Scoped to the signed-in account: another account's character looks like a missing one.
    let outcome = match state.repo.unlink_character(account.id, character_id).await {
        Ok(outcome) => outcome,
        Err(e) => return pages::internal_error(e),
    };
    if !outcome.deleted {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }

    if outcome.account_deleted {
        if let Err(e) = session.flush().await {
            return pages::internal_error(e);
        }
        if is_htmx(&headers) {
            // A plain redirect would be followed by XHR and swapped into the page.
            return (
                StatusCode::OK,
                [("hx-redirect", HeaderValue::from_static("/"))],
            )
                .into_response();
        }
        return Redirect::to("/").into_response();
    }
    // Empty body: htmx replaces the character's row with nothing.
    StatusCode::OK.into_response()
}
