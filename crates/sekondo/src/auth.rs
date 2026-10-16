//! SSO login routes.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use secrecy::{ExposeSecret, SecretString};
use subtle::ConstantTimeEq;
use tower_sessions::Session;

use crate::{
    account,
    app::AppState,
    csrf,
    decision::{Decision, ExistingCharacter, RejectReason, decide},
    oauth::{Intent, PendingFlow},
    pages,
    repo::CharacterUpsert,
    sso::VerifiedToken,
};

pub const PENDING_FLOW: &str = "pending_flow";

pub fn callback_url(state: &AppState) -> String {
    format!(
        "{}/auth/callback",
        state.config.public_base_url.as_str().trim_end_matches('/')
    )
}

async fn start_flow(state: &AppState, session: &Session, intent: Intent) -> Response {
    let flow = PendingFlow::new(intent);
    let url = flow.authorize_url(
        &state.config.eve_sso_base_url,
        &state.config.eve_client_id,
        &callback_url(state),
    );
    if let Err(e) = session.insert(PENDING_FLOW, &flow).await {
        tracing::error!("cannot store the pending SSO flow: {e}");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    Redirect::to(url.as_str()).into_response()
}

/// Starts the SSO flow for a login. Signed-in users go straight to their characters.
pub async fn login(State(state): State<AppState>, session: Session) -> Response {
    if account::current_account_id(&session).await.is_some() {
        return Redirect::to("/characters").into_response();
    }
    start_flow(&state, &session, Intent::Login).await
}

/// Starts the SSO flow that links another character to the signed-in account.
pub async fn add(State(state): State<AppState>, session: Session) -> Response {
    if account::current_account_id(&session).await.is_none() {
        return Redirect::to("/").into_response();
    }
    start_flow(&state, &session, Intent::AddCharacter).await
}

const FLASH: &str = "flash";

/// A one-time message shown on the next page that renders flashes.
pub async fn set_flash(session: &Session, message: &str) {
    let _ = session.insert(FLASH, message).await;
}

pub async fn take_flash(session: &Session) -> Option<String> {
    session.remove::<String>(FLASH).await.ok().flatten()
}

#[derive(serde::Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

async fn fail(session: &Session, status: StatusCode, title: &str, message: &str) -> Response {
    pages::error_page(session, status, title, message).await
}

/// Handles the redirect back from EVE SSO.
pub async fn callback(
    State(state): State<AppState>,
    session: Session,
    Query(query): Query<CallbackQuery>,
) -> Response {
    // Consuming the pending flow makes `state` single-use, whatever happens next.
    let flow = session
        .remove::<PendingFlow>(PENDING_FLOW)
        .await
        .ok()
        .flatten();

    if let Some(error) = &query.error {
        let detail = query.error_description.as_deref().unwrap_or(error);
        return fail(
            &session,
            StatusCode::BAD_REQUEST,
            "Login failed",
            &format!("EVE SSO reported: {detail}"),
        )
        .await;
    }
    let Some(flow) = flow else {
        return fail(
            &session,
            StatusCode::BAD_REQUEST,
            "Login failed",
            "This login attempt has expired or was already used. Please try again.",
        )
        .await;
    };
    let state_matches = query
        .state
        .as_deref()
        .is_some_and(|s| bool::from(s.as_bytes().ct_eq(flow.state.as_bytes())));
    if !state_matches {
        return fail(
            &session,
            StatusCode::BAD_REQUEST,
            "Login failed",
            "The login response did not match this browser session. Please try again.",
        )
        .await;
    }
    let Some(code) = query.code.as_deref() else {
        return fail(
            &session,
            StatusCode::BAD_REQUEST,
            "Login failed",
            "EVE SSO sent no authorization code.",
        )
        .await;
    };

    let tokens = match state
        .sso
        .exchange_code(code, &flow.code_verifier, &callback_url(&state))
        .await
    {
        Ok(tokens) => tokens,
        Err(e) => {
            tracing::warn!("code exchange failed: {e}");
            return fail(
                &session,
                StatusCode::BAD_GATEWAY,
                "Login failed",
                "EVE SSO did not accept the login. Please try again.",
            )
            .await;
        }
    };
    let verified = match state
        .sso
        .validate(tokens.access_token.expose_secret())
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("access token rejected: {e}");
            return fail(
                &session,
                StatusCode::BAD_REQUEST,
                "Login failed",
                "EVE SSO returned a token that could not be verified.",
            )
            .await;
        }
    };
    let missing = verified.missing_scopes();
    if !missing.is_empty() {
        return fail(
            &session,
            StatusCode::BAD_REQUEST,
            "Permissions missing",
            &format!(
                "Sekondo needs all of the requested permissions. Missing: {}.",
                missing.join(", ")
            ),
        )
        .await;
    }
    let Some(refresh_token) = tokens.refresh_token else {
        return fail(
            &session,
            StatusCode::BAD_GATEWAY,
            "Login failed",
            "EVE SSO sent no refresh token.",
        )
        .await;
    };

    match complete_login(
        &state,
        &session,
        flow.intent,
        verified,
        tokens.access_token,
        refresh_token,
    )
    .await
    {
        Ok(response) => response,
        Err(e) => pages::internal_error(e),
    }
}

async fn complete_login(
    state: &AppState,
    session: &Session,
    intent: Intent,
    verified: VerifiedToken,
    access_token: SecretString,
    refresh_token: SecretString,
) -> Result<Response, Box<dyn std::error::Error>> {
    let existing = state
        .repo
        .find_character(verified.character_id)
        .await?
        .map(|c| ExistingCharacter {
            account_id: c.account_id,
            owner_hash_matches: c.owner_hash == verified.owner_hash,
        });
    let session_account = account::current_account_id(session).await;
    let upsert = CharacterUpsert {
        character_id: verified.character_id,
        name: verified.name,
        owner_hash: verified.owner_hash,
        scopes: verified.scopes,
        refresh_token,
        access_token,
        access_expires_at: verified.expires_at,
    };

    let sign_in_as = match decide(intent, session_account, existing) {
        Decision::CreateAccount { remove_existing } => {
            if remove_existing {
                state.repo.delete_character(upsert.character_id).await?;
            }
            Some(state.repo.create_account_with_character(&upsert).await?)
        }
        Decision::ResumeAccount { account_id } => {
            state.repo.upsert_character(account_id, &upsert).await?;
            Some(account_id)
        }
        Decision::LinkToSessionAccount {
            account_id,
            remove_existing,
        } => {
            if remove_existing {
                state.repo.delete_character(upsert.character_id).await?;
            }
            state.repo.upsert_character(account_id, &upsert).await?;
            None
        }
        Decision::RefreshInSessionAccount { account_id } => {
            state.repo.upsert_character(account_id, &upsert).await?;
            None
        }
        Decision::Reject(RejectReason::BelongsToOtherAccount) => {
            set_flash(
                session,
                "That character belongs to another Sekondo account. Unlink it there first, then add it here.",
            )
            .await;
            None
        }
        Decision::Reject(RejectReason::NoSession) => {
            return Ok(fail(
                session,
                StatusCode::BAD_REQUEST,
                "Login failed",
                "You were signed out while logging in. Please log in again.",
            )
            .await);
        }
    };

    if let Some(account_id) = sign_in_as {
        // A new session ID on every sign-in, so a pre-login cookie can't be reused (fixation).
        session.cycle_id().await?;
        csrf::reset(session).await?;
        account::sign_in(session, account_id, state.clock.now()).await?;
    }
    Ok(Redirect::to("/characters").into_response())
}

/// Ends the session: the record is deleted on the server, so a replayed cookie authenticates nobody.
pub async fn logout(session: Session, headers: axum::http::HeaderMap) -> Response {
    if let Err(e) = session.flush().await {
        return pages::internal_error(e);
    }
    if headers.get("hx-request").is_some_and(|v| v == "true") {
        // A plain redirect would be followed by XHR and swapped into the page.
        return (StatusCode::OK, [("hx-redirect", "/")]).into_response();
    }
    Redirect::to("/").into_response()
}
