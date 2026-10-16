//! CSRF protection for every state-changing request, in two independent layers:
//! a per-session synchronizer token, and an Origin/Referer plus Fetch-Metadata check.

use axum::{
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use subtle::ConstantTimeEq;
use tower_sessions::Session;
use url::Url;

use crate::app::AppState;

const SESSION_KEY: &str = "csrf_token";
pub const HEADER: &str = "x-csrf-token";
pub const FORM_FIELD: &str = "csrf_token";
const MAX_FORM_BYTES: usize = 64 * 1024;

/// The session's CSRF token, created on first use (256 random bits).
pub async fn token(session: &Session) -> Result<String, tower_sessions::session::Error> {
    if let Some(token) = session.get::<String>(SESSION_KEY).await? {
        return Ok(token);
    }
    let bytes: [u8; 32] = rand::random();
    let token = URL_SAFE_NO_PAD.encode(bytes);
    session.insert(SESSION_KEY, &token).await?;
    Ok(token)
}

fn forbidden(reason: &str) -> Response {
    tracing::warn!("csrf: rejected request: {reason}");
    (StatusCode::FORBIDDEN, "forbidden").into_response()
}

fn is_safe(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn origin_of(value: &str) -> Option<String> {
    Url::parse(value)
        .ok()
        .map(|u| u.origin().ascii_serialization())
}

fn origin_matches(headers: &HeaderMap, public_base_url: &Url) -> bool {
    let expected = public_base_url.origin().ascii_serialization();
    let get = |name| headers.get(name).and_then(|v| v.to_str().ok());
    match get(header::ORIGIN).or_else(|| get(header::REFERER)) {
        Some(value) => origin_of(value).is_some_and(|o| o == expected),
        None => false,
    }
}

fn form_token(body: &[u8]) -> Option<String> {
    url::form_urlencoded::parse(body)
        .find(|(k, _)| k == FORM_FIELD)
        .map(|(_, v)| v.into_owned())
}

fn is_form(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/x-www-form-urlencoded"))
}

pub async fn protect(
    State(state): State<AppState>,
    session: Session,
    request: Request,
    next: Next,
) -> Response {
    if is_safe(request.method()) {
        return next.run(request).await;
    }

    if request
        .headers()
        .get("sec-fetch-site")
        .is_some_and(|v| v.as_bytes() == b"cross-site")
    {
        return forbidden("cross-site fetch metadata");
    }
    if !origin_matches(request.headers(), &state.config.public_base_url) {
        return forbidden("origin does not match");
    }

    let Ok(Some(expected)) = session.get::<String>(SESSION_KEY).await else {
        return forbidden("no session token");
    };

    let (parts, body) = request.into_parts();
    let (supplied, body) = match parts.headers.get(HEADER).and_then(|v| v.to_str().ok()) {
        Some(token) => (Some(token.to_owned()), body),
        None if is_form(&parts.headers) => match to_bytes(body, MAX_FORM_BYTES).await {
            Ok(bytes) => (form_token(&bytes), Body::from(bytes)),
            Err(_) => return forbidden("unreadable form body"),
        },
        None => (None, body),
    };

    let valid = supplied.is_some_and(|s| bool::from(s.as_bytes().ct_eq(expected.as_bytes())));
    if !valid {
        return forbidden("token missing or wrong");
    }
    next.run(Request::from_parts(parts, body)).await
}

/// Drops the session's token so the next page gets a fresh one (done when a user signs in).
pub async fn reset(session: &Session) -> Result<(), tower_sessions::session::Error> {
    session.remove::<String>(SESSION_KEY).await?;
    Ok(())
}
