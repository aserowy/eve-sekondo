use std::collections::HashMap;

use axum::{
    Form, Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use url::Url;

use crate::state::{ACCESS_TOKEN_SECONDS, PendingCode, RefreshRecord, Shared, random_token};

pub fn router(shared: Shared) -> Router {
    let controls = Router::new()
        .route("/revoke", post(revoke))
        .route("/owner-hash", post(owner_hash))
        .route("/scopes", post(scopes))
        .route("/fail-next-token", post(fail_next_token))
        .route("/stats", get(stats))
        .route("/rotate-key", post(rotate_key))
        .route("/reset", post(reset))
        .layer(middleware::from_fn_with_state(
            shared.clone(),
            require_control_token,
        ));

    Router::new()
        .route("/oauth/jwks", get(jwks))
        .route("/v2/oauth/authorize", get(authorize))
        .route("/v2/oauth/authorize/select", get(select))
        .route("/v2/oauth/token", post(token))
        .nest("/_stub", controls)
        .with_state(shared)
}

async fn jwks(State(shared): State<Shared>) -> Json<Value> {
    let mut inner = shared.lock().unwrap();
    inner.jwks_calls += 1;
    Json(json!({
        "keys": [{
            "kty": "RSA",
            "alg": "RS256",
            "use": "sig",
            "kid": inner.kid,
            "n": inner.jwk_n,
            "e": inner.jwk_e,
        }]
    }))
}

fn bad_request(message: &str) -> Response {
    (StatusCode::BAD_REQUEST, message.to_owned()).into_response()
}

fn oauth_error(status: StatusCode, error: &str, description: &str) -> Response {
    (
        status,
        Json(json!({"error": error, "error_description": description})),
    )
        .into_response()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Checks the authorize parameters; returns the requested scopes.
#[allow(clippy::result_large_err)] // a test double: the error is a ready-made HTTP response
fn validate_authorize(
    shared: &Shared,
    q: &HashMap<String, String>,
) -> Result<Vec<String>, Response> {
    let get = |k: &str| q.get(k).filter(|v| !v.is_empty());
    if get("response_type").map(String::as_str) != Some("code") {
        return Err(bad_request("response_type must be code"));
    }
    if get("client_id") != Some(&shared.lock().unwrap().config.client_id) {
        return Err(bad_request("unknown client_id"));
    }
    if get("redirect_uri").is_none() {
        return Err(bad_request("redirect_uri is required"));
    }
    if get("state").is_none() {
        return Err(bad_request("state is required"));
    }
    if get("code_challenge").is_none()
        || get("code_challenge_method").map(String::as_str) != Some("S256")
    {
        return Err(bad_request(
            "PKCE with code_challenge_method=S256 is required",
        ));
    }
    Ok(get("scope")
        .map(|s| s.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default())
}

/// The character picker, standing in for EVE's login and character selection page.
async fn authorize(
    State(shared): State<Shared>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    if let Err(response) = validate_authorize(&shared, &q) {
        return response;
    }
    let mut items = String::new();
    for c in &shared.lock().unwrap().characters {
        let mut link =
            Url::parse("https://stub.invalid/v2/oauth/authorize/select").expect("static url");
        link.query_pairs_mut()
            .append_pair("character_id", &c.id.to_string())
            .extend_pairs(q.iter());
        let href = format!("{}?{}", link.path(), link.query().unwrap_or_default());
        items.push_str(&format!(
            r#"<li><a href="{}">{}</a></li>"#,
            esc(&href),
            esc(&c.name)
        ));
    }
    Html(format!(
        "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>EVE stub login</title></head>\
         <body><h1>Choose a character</h1><ul>{items}</ul></body></html>"
    ))
    .into_response()
}

async fn select(
    State(shared): State<Shared>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let requested = match validate_authorize(&shared, &q) {
        Ok(scopes) => scopes,
        Err(response) => return response,
    };
    let Some(character_id) = q.get("character_id").and_then(|v| v.parse::<i64>().ok()) else {
        return bad_request("character_id is required");
    };
    let code = random_token();
    {
        let mut inner = shared.lock().unwrap();
        let Some(character) = inner.character(character_id) else {
            return bad_request("unknown character");
        };
        let scopes = requested
            .into_iter()
            .filter(|s| character.scopes.contains(s))
            .collect();
        inner.codes.insert(
            code.clone(),
            PendingCode {
                character_id,
                code_challenge: q["code_challenge"].clone(),
                redirect_uri: q["redirect_uri"].clone(),
                scopes,
            },
        );
    }
    let Ok(mut target) = Url::parse(&q["redirect_uri"]) else {
        return bad_request("redirect_uri is not a URL");
    };
    target
        .query_pairs_mut()
        .append_pair("code", &code)
        .append_pair("state", &q["state"]);
    (StatusCode::FOUND, [(header::LOCATION, target.to_string())]).into_response()
}

/// Client credentials from a Basic header, or from the form body.
fn client_credentials(
    headers: &HeaderMap,
    form: &HashMap<String, String>,
) -> Option<(String, String)> {
    if let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        let encoded = value.strip_prefix("Basic ")?;
        let decoded = String::from_utf8(STANDARD.decode(encoded).ok()?).ok()?;
        let (id, secret) = decoded.split_once(':')?;
        return Some((id.to_owned(), secret.to_owned()));
    }
    Some((
        form.get("client_id")?.clone(),
        form.get("client_secret")?.clone(),
    ))
}

async fn token(
    State(shared): State<Shared>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let mut inner = shared.lock().unwrap();
    inner.token_calls += 1;
    if inner.fail_next_token > 0 {
        inner.fail_next_token -= 1;
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "stub: token endpoint unavailable",
        )
            .into_response();
    }

    let authenticated = client_credentials(&headers, &form).is_some_and(|(id, secret)| {
        id == inner.config.client_id && secret == inner.config.client_secret
    });
    if !authenticated {
        return oauth_error(
            StatusCode::UNAUTHORIZED,
            "invalid_client",
            "Client authentication failed.",
        );
    }

    let invalid_grant = |why: &str| oauth_error(StatusCode::BAD_REQUEST, "invalid_grant", why);
    let (character_id, scopes) = match form.get("grant_type").map(String::as_str) {
        Some("authorization_code") => {
            let (Some(code), Some(verifier)) = (form.get("code"), form.get("code_verifier")) else {
                return bad_request("code and code_verifier are required");
            };
            let Some(pending) = inner.codes.remove(code) else {
                return invalid_grant("The authorization code is invalid.");
            };
            if form.get("redirect_uri") != Some(&pending.redirect_uri) {
                return invalid_grant("redirect_uri does not match.");
            }
            if URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) != pending.code_challenge
            {
                return invalid_grant("PKCE verification failed.");
            }
            (pending.character_id, pending.scopes)
        }
        Some("refresh_token") => {
            let Some(old) = form.get("refresh_token") else {
                return bad_request("refresh_token is required");
            };
            let Some(record) = inner.refresh_tokens.remove(old) else {
                return invalid_grant("The refresh token is invalid.");
            };
            (record.character_id, record.scopes)
        }
        _ => {
            return oauth_error(
                StatusCode::BAD_REQUEST,
                "unsupported_grant_type",
                "Unsupported grant type.",
            );
        }
    };

    let options = crate::TokenOptions {
        scopes: Some(scopes.clone()),
        ..inner.token_options.clone()
    };
    let Some(access_token) = inner.issue_token(character_id, &options) else {
        return invalid_grant("The character no longer exists.");
    };
    let refresh_token = random_token();
    inner.refresh_tokens.insert(
        refresh_token.clone(),
        RefreshRecord {
            character_id,
            scopes,
        },
    );
    Json(json!({
        "access_token": access_token,
        "token_type": "Bearer",
        "expires_in": ACCESS_TOKEN_SECONDS,
        "refresh_token": refresh_token,
    }))
    .into_response()
}

async fn require_control_token(
    State(shared): State<Shared>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    let expected = format!("Bearer {}", shared.lock().unwrap().config.control_token);
    let supplied = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    if supplied != Some(expected.as_str()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    next.run(request).await
}

#[derive(Deserialize)]
struct CharacterRef {
    character_id: i64,
}

async fn revoke(State(shared): State<Shared>, Json(body): Json<CharacterRef>) -> StatusCode {
    shared
        .lock()
        .unwrap()
        .refresh_tokens
        .retain(|_, r| r.character_id != body.character_id);
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct OwnerHashBody {
    character_id: i64,
    owner_hash: String,
}

async fn owner_hash(State(shared): State<Shared>, Json(body): Json<OwnerHashBody>) -> StatusCode {
    match shared
        .lock()
        .unwrap()
        .characters
        .iter_mut()
        .find(|c| c.id == body.character_id)
    {
        Some(c) => {
            c.owner_hash = body.owner_hash;
            StatusCode::NO_CONTENT
        }
        None => StatusCode::NOT_FOUND,
    }
}

#[derive(Deserialize)]
struct ScopesBody {
    character_id: i64,
    scopes: Vec<String>,
}

/// Reduces what the character grants from now on, including on already issued refresh tokens.
async fn scopes(State(shared): State<Shared>, Json(body): Json<ScopesBody>) -> StatusCode {
    let mut inner = shared.lock().unwrap();
    let Some(c) = inner
        .characters
        .iter_mut()
        .find(|c| c.id == body.character_id)
    else {
        return StatusCode::NOT_FOUND;
    };
    c.scopes = body.scopes.clone();
    for record in inner
        .refresh_tokens
        .values_mut()
        .filter(|r| r.character_id == body.character_id)
    {
        record.scopes.retain(|s| body.scopes.contains(s));
    }
    StatusCode::NO_CONTENT
}

#[derive(Deserialize, Default)]
struct FailBody {
    count: Option<u32>,
}

async fn fail_next_token(State(shared): State<Shared>, body: Option<Json<FailBody>>) -> StatusCode {
    let count = body.and_then(|Json(b)| b.count).unwrap_or(1);
    shared.lock().unwrap().fail_next_token = count;
    StatusCode::NO_CONTENT
}

async fn stats(State(shared): State<Shared>) -> Json<Value> {
    let inner = shared.lock().unwrap();
    Json(json!({"token_calls": inner.token_calls, "jwks_calls": inner.jwks_calls}))
}

async fn rotate_key(State(shared): State<Shared>) -> StatusCode {
    match shared.lock().unwrap().rotate_key() {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn reset(State(shared): State<Shared>) -> StatusCode {
    shared.lock().unwrap().reset();
    StatusCode::NO_CONTENT
}
