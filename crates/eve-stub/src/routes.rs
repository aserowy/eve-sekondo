use std::{
    collections::HashMap,
    time::{Duration, SystemTime},
};

use axum::{
    Form, Json, Router,
    extract::{Path, Query, State},
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

use crate::state::{
    ACCESS_TOKEN_SECONDS, ASSETS_PAGE_SIZE, EsiFailure, Inner, MARKET_PAGE_SIZE, MarketFixture,
    NAMES, PendingCode, RefreshRecord, STRUCTURES, Shared, now_secs, random_token,
};

macro_rules! method_router {
    ("GET", $handler:expr) => {
        get($handler)
    };
    ("POST", $handler:expr) => {
        post($handler)
    };
}

/// Declares the routes that stand in for EVE (not the `/_stub/` controls) once, as the router and
/// as the `ROUTES` list the contract coverage test reads, so the two cannot differ.
macro_rules! eve_routes {
    ($(($method:tt, $path:literal, $handler:expr)),* $(,)?) => {
        /// Every EVE route the stub serves: method and path pattern.
        pub const ROUTES: &[(&str, &str)] = &[$(($method, $path)),*];

        fn eve_router() -> Router<Shared> {
            Router::new()$(.route($path, method_router!($method, $handler)))*
        }
    };
}

eve_routes! {
    ("GET", "/oauth/jwks", jwks),
    ("GET", "/v2/oauth/authorize", authorize),
    ("GET", "/v2/oauth/authorize/select", select),
    ("POST", "/v2/oauth/token", token),
    ("GET", "/characters/{id}/orders", character_orders),
    ("POST", "/universe/names", universe_names),
    ("GET", "/universe/structures/{id}", universe_structure),
    ("GET", "/markets/{region_id}/orders", market_orders),
    ("GET", "/characters/{id}/wallet", character_wallet),
    ("GET", "/characters/{id}/assets", character_assets),
    ("GET", "/markets/prices", market_prices),
}

pub fn router(shared: Shared) -> Router {
    let controls = Router::new()
        .route("/revoke", post(revoke))
        .route("/owner-hash", post(owner_hash))
        .route("/scopes", post(scopes))
        .route("/fail-next-token", post(fail_next_token))
        .route("/orders", post(set_orders))
        .route("/market-orders", post(set_market_orders))
        .route("/wallet", post(set_wallet))
        .route("/assets", post(set_assets))
        .route("/prices", post(set_prices))
        .route("/esi-expires", post(set_esi_expires))
        .route("/structure-access", post(set_structure_access))
        .route("/esi-fail-next", post(esi_fail_next))
        .route("/esi-delay", post(set_esi_delay))
        .route("/stats", get(stats))
        .route("/rotate-key", post(rotate_key))
        .route("/reset", post(reset))
        .layer(middleware::from_fn_with_state(
            shared.clone(),
            require_control_token,
        ));

    eve_router().nest("/_stub", controls).with_state(shared)
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
    let calls = |route: &str| inner.esi_calls.get(route).copied().unwrap_or(0);
    Json(json!({
        "token_calls": inner.token_calls,
        "jwks_calls": inner.jwks_calls,
        "orders_calls": calls("orders"),
        "names_calls": calls("names"),
        "structures_calls": calls("structures"),
        "market_calls": calls("market"),
        "wallet_calls": calls("wallet"),
        "assets_calls": calls("assets"),
        "prices_calls": calls("prices"),
    }))
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

const ORDERS_SCOPE: &str = "esi-markets.read_character_orders.v1";
const STRUCTURES_SCOPE: &str = "esi-universe.read_structures.v1";
const WALLET_SCOPE: &str = "esi-wallet.read_character_wallet.v1";
const ASSETS_SCOPE: &str = "esi-assets.read_assets.v1";

/// Counts the call and, while a failure is armed, answers it instead of the route.
fn begin_esi_call(
    inner: &mut Inner,
    route: &'static str,
    request_headers: &HeaderMap,
) -> Option<Response> {
    inner.count_esi_call(route, request_headers);
    let failure = inner.esi_failure.as_mut()?;
    if failure.route.as_deref().is_some_and(|r| r != route) {
        return None;
    }
    failure.remaining -= 1;
    let failure = failure.clone();
    if failure.remaining == 0 {
        inner.esi_failure = None;
    }
    let mut headers = HeaderMap::new();
    for (name, value) in &failure.headers {
        if let (Ok(name), Ok(value)) = (
            header::HeaderName::try_from(name.as_str()),
            value.parse::<header::HeaderValue>(),
        ) {
            headers.insert(name, value);
        }
    }
    let status = StatusCode::from_u16(failure.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    Some((status, headers, "stub: injected ESI failure").into_response())
}

/// The headers every ESI answer carries: cache expiry and the error and rate limit state.
fn esi_headers(inner: &Inner) -> HeaderMap {
    let now = SystemTime::now();
    let mut headers = HeaderMap::new();
    let mut put = |name: &'static str, value: String| {
        headers.insert(name, value.parse().expect("header value"));
    };
    put("date", httpdate::fmt_http_date(now));
    put(
        "expires",
        httpdate::fmt_http_date(now + Duration::from_secs(inner.esi_expires_secs)),
    );
    put("x-esi-error-limit-remain", "100".to_owned());
    put("x-esi-error-limit-reset", "60".to_owned());
    headers
}

/// An ESI error answer: the date and error-limit headers, and ESI's `{"error": ...}` body.
fn esi_error(inner: &Inner, status: StatusCode, message: &str) -> Response {
    let mut headers = esi_headers(inner);
    headers.remove("expires");
    (status, headers, Json(json!({"error": message}))).into_response()
}

/// The stub-issued, unexpired token behind a bearer header.
#[allow(clippy::result_large_err)] // a test double: the error is a ready-made HTTP response
fn authorize_bearer(inner: &Inner, headers: &HeaderMap) -> Result<(i64, Vec<String>), Response> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| StatusCode::UNAUTHORIZED.into_response())?;
    let issued = inner
        .issued_tokens
        .get(token)
        .filter(|t| t.expires_at > now_secs() as i64)
        .ok_or_else(|| StatusCode::UNAUTHORIZED.into_response())?;
    Ok((issued.character_id, issued.scopes.clone()))
}

async fn character_orders(
    State(shared): State<Shared>,
    Path(character_id): Path<i64>,
    headers: HeaderMap,
) -> Response {
    // The lock is released before any waiting.
    let answer = {
        let mut inner = shared.lock().unwrap();
        answer_orders(&mut inner, character_id, &headers)
    };
    match answer {
        Err(response) => response,
        Ok((response_headers, orders, delay_ms)) => {
            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
            (response_headers, Json(orders)).into_response()
        }
    }
}

#[allow(clippy::result_large_err)] // a test double: the error is a ready-made HTTP response
fn answer_orders(
    inner: &mut Inner,
    character_id: i64,
    headers: &HeaderMap,
) -> Result<(HeaderMap, Vec<Value>, u64), Response> {
    if let Some(response) = begin_esi_call(inner, "orders", headers) {
        return Err(response);
    }
    let (token_character, scopes) = authorize_bearer(inner, headers)?;
    if token_character != character_id || !scopes.iter().any(|s| s == ORDERS_SCOPE) {
        return Err(StatusCode::FORBIDDEN.into_response());
    }
    let orders = inner.orders.get(&character_id).cloned().unwrap_or_default();
    Ok((esi_headers(inner), orders, inner.esi_delay_ms))
}

#[derive(Deserialize)]
struct OrdersBody {
    character_id: i64,
    orders: Vec<Value>,
}

/// Replaces what ESI returns as a character's orders.
async fn set_orders(State(shared): State<Shared>, Json(body): Json<OrdersBody>) -> StatusCode {
    shared
        .lock()
        .unwrap()
        .orders
        .insert(body.character_id, body.orders);
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct ExpiresBody {
    seconds: u64,
}

async fn set_esi_expires(
    State(shared): State<Shared>,
    Json(body): Json<ExpiresBody>,
) -> StatusCode {
    shared.lock().unwrap().esi_expires_secs = body.seconds;
    StatusCode::NO_CONTENT
}

const MAX_NAMES_PER_REQUEST: usize = 1000;

async fn universe_names(
    State(shared): State<Shared>,
    headers: HeaderMap,
    Json(ids): Json<Vec<i64>>,
) -> Response {
    let mut inner = shared.lock().unwrap();
    if let Some(response) = begin_esi_call(&mut inner, "names", &headers) {
        return response;
    }
    inner.names_requests.push(ids.clone());
    if ids.is_empty() || ids.len() > MAX_NAMES_PER_REQUEST {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let mut found = Vec::new();
    for id in &ids {
        let Some((_, category, name)) = NAMES.iter().find(|(known, _, _)| known == id) else {
            return (
                StatusCode::NOT_FOUND,
                esi_headers(&inner),
                Json(json!({"error": "Invalid IDs in the request"})),
            )
                .into_response();
        };
        found.push(json!({"category": category, "id": id, "name": name}));
    }
    (esi_headers(&inner), Json(found)).into_response()
}

async fn universe_structure(
    State(shared): State<Shared>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Response {
    let mut inner = shared.lock().unwrap();
    if let Some(response) = begin_esi_call(&mut inner, "structures", &headers) {
        return response;
    }
    let (_, scopes) = match authorize_bearer(&inner, &headers) {
        Ok(granted) => granted,
        Err(response) => return response,
    };
    if !scopes.iter().any(|s| s == STRUCTURES_SCOPE) || inner.structures_denied.contains(&id) {
        return esi_error(&inner, StatusCode::FORBIDDEN, "Forbidden");
    }
    let Some((_, name)) = STRUCTURES.iter().find(|(known, _)| *known == id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    (
        esi_headers(&inner),
        Json(json!({"name": name, "owner_id": 98000001, "solar_system_id": 30000142, "type_id": 35834})),
    )
        .into_response()
}

#[derive(Deserialize)]
struct StructureAccessBody {
    structure_id: i64,
    allowed: bool,
}

async fn set_structure_access(
    State(shared): State<Shared>,
    Json(body): Json<StructureAccessBody>,
) -> StatusCode {
    let mut inner = shared.lock().unwrap();
    if body.allowed {
        inner.structures_denied.remove(&body.structure_id);
    } else {
        inner.structures_denied.insert(body.structure_id);
    }
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct EsiFailBody {
    route: Option<String>,
    count: Option<u32>,
    status: u16,
    #[serde(default)]
    headers: HashMap<String, String>,
}

/// The next `count` ESI calls (any route) answer with `status` and the given headers.
async fn esi_fail_next(State(shared): State<Shared>, Json(body): Json<EsiFailBody>) -> StatusCode {
    shared.lock().unwrap().esi_failure = Some(EsiFailure {
        route: body.route,
        remaining: body.count.unwrap_or(1).max(1),
        status: body.status,
        headers: body.headers.into_iter().collect(),
    });
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct DelayBody {
    millis: u64,
}

/// Makes the orders route answer slowly, so tests can act while a poll is in flight.
async fn set_esi_delay(State(shared): State<Shared>, Json(body): Json<DelayBody>) -> StatusCode {
    shared.lock().unwrap().esi_delay_ms = body.millis;
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct MarketQuery {
    type_id: Option<i64>,
    order_type: Option<String>,
    page: Option<usize>,
}

/// The public market orders of a region for one item, paged like ESI's. Needs no token.
async fn market_orders(
    State(shared): State<Shared>,
    Path(region_id): Path<i64>,
    Query(query): Query<MarketQuery>,
    headers: HeaderMap,
) -> Response {
    let mut inner = shared.lock().unwrap();
    if let Some(response) = begin_esi_call(&mut inner, "market", &headers) {
        return response;
    }
    let (orders, page_size) = inner
        .market_orders
        .get(&region_id)
        .map_or((&[][..], MARKET_PAGE_SIZE), |f| {
            (&f.orders[..], f.page_size)
        });
    let wanted: Vec<&Value> = orders
        .iter()
        .filter(|o| query.type_id.is_none_or(|t| o["type_id"] == t))
        .filter(|o| match query.order_type.as_deref() {
            Some("buy") => o["is_buy_order"] == true,
            Some("sell") => o["is_buy_order"] == false,
            _ => true,
        })
        .collect();
    let pages = wanted.len().div_ceil(page_size).max(1);
    let page = query.page.unwrap_or(1).max(1);
    let slice: Vec<&Value> = wanted
        .into_iter()
        .skip((page - 1) * page_size)
        .take(page_size)
        .collect();
    let mut response_headers = esi_headers(&inner);
    // Like real ESI on this route: rate-limit headers instead of the error-limit ones.
    response_headers.remove("x-esi-error-limit-remain");
    response_headers.remove("x-esi-error-limit-reset");
    for (name, value) in [
        ("x-ratelimit-group", "market-order"),
        ("x-ratelimit-limit", "12000/15m"),
        ("x-ratelimit-remaining", "11999"),
        ("x-ratelimit-used", "1"),
    ] {
        response_headers.insert(name, value.parse().expect("header value"));
    }
    response_headers.insert("x-pages", pages.to_string().parse().expect("header value"));
    (response_headers, Json(slice)).into_response()
}

#[derive(Deserialize)]
struct MarketOrdersBody {
    region_id: i64,
    orders: Vec<Value>,
    page_size: Option<usize>,
}

/// Replaces what ESI returns as the public market orders of a region.
async fn set_market_orders(
    State(shared): State<Shared>,
    Json(body): Json<MarketOrdersBody>,
) -> StatusCode {
    shared.lock().unwrap().market_orders.insert(
        body.region_id,
        MarketFixture {
            orders: body.orders,
            page_size: body.page_size.unwrap_or(MARKET_PAGE_SIZE).max(1),
        },
    );
    StatusCode::NO_CONTENT
}

/// Like real ESI on the character wallet and asset routes (verified): the rate-limit headers of
/// the route's group instead of the error-limit ones.
fn char_route_headers(inner: &Inner, group: &'static str, limit: &'static str) -> HeaderMap {
    let mut headers = esi_headers(inner);
    headers.remove("x-esi-error-limit-remain");
    headers.remove("x-esi-error-limit-reset");
    for (name, value) in [
        ("x-ratelimit-group", group),
        ("x-ratelimit-limit", limit),
        ("x-ratelimit-remaining", "148"),
        ("x-ratelimit-used", "2"),
    ] {
        headers.insert(name, value.parse().expect("header value"));
    }
    headers
}

/// The bearer's character, when the token is the character's own and carries `scope`; otherwise
/// the answer to send instead (401 without a token, 403 with the wrong one).
#[allow(clippy::result_large_err)] // a test double: the error is a ready-made HTTP response
fn authorize_character(
    inner: &Inner,
    headers: &HeaderMap,
    character_id: i64,
    scope: &str,
    mut forbidden_headers: HeaderMap,
) -> Result<(), Response> {
    let (token_character, scopes) = authorize_bearer(inner, headers)?;
    if token_character != character_id || !scopes.iter().any(|s| s == scope) {
        forbidden_headers.remove("expires");
        return Err((
            StatusCode::FORBIDDEN,
            forbidden_headers,
            Json(json!({"error": "Forbidden"})),
        )
            .into_response());
    }
    Ok(())
}

/// The wallet balance of a character: a bare JSON number.
async fn character_wallet(
    State(shared): State<Shared>,
    Path(character_id): Path<i64>,
    headers: HeaderMap,
) -> Response {
    let mut inner = shared.lock().unwrap();
    if let Some(response) = begin_esi_call(&mut inner, "wallet", &headers) {
        return response;
    }
    let response_headers = char_route_headers(&inner, "char-wallet", "150/15m");
    if let Err(response) = authorize_character(
        &inner,
        &headers,
        character_id,
        WALLET_SCOPE,
        response_headers.clone(),
    ) {
        return response;
    }
    let balance = inner.wallets.get(&character_id).copied().unwrap_or(0.0);
    (response_headers, Json(balance)).into_response()
}

#[derive(Deserialize)]
struct PageQuery {
    page: Option<usize>,
}

/// The assets of a character, paged like ESI's.
async fn character_assets(
    State(shared): State<Shared>,
    Path(character_id): Path<i64>,
    Query(query): Query<PageQuery>,
    headers: HeaderMap,
) -> Response {
    let mut inner = shared.lock().unwrap();
    if let Some(response) = begin_esi_call(&mut inner, "assets", &headers) {
        return response;
    }
    let mut response_headers = char_route_headers(&inner, "char-asset", "1800/15m");
    if let Err(response) = authorize_character(
        &inner,
        &headers,
        character_id,
        ASSETS_SCOPE,
        response_headers.clone(),
    ) {
        return response;
    }
    let (assets, page_size) = inner
        .assets
        .get(&character_id)
        .map_or((&[][..], ASSETS_PAGE_SIZE), |f| {
            (&f.orders[..], f.page_size)
        });
    let pages = assets.len().div_ceil(page_size).max(1);
    let page = query.page.unwrap_or(1).max(1);
    let slice: Vec<&Value> = assets
        .iter()
        .skip((page - 1) * page_size)
        .take(page_size)
        .collect();
    response_headers.insert("x-pages", pages.to_string().parse().expect("header value"));
    (response_headers, Json(slice)).into_response()
}

/// ESI's market prices. Needs no token.
async fn market_prices(State(shared): State<Shared>, headers: HeaderMap) -> Response {
    let mut inner = shared.lock().unwrap();
    if let Some(response) = begin_esi_call(&mut inner, "prices", &headers) {
        return response;
    }
    (esi_headers(&inner), Json(inner.prices.clone())).into_response()
}

#[derive(Deserialize)]
struct WalletBody {
    character_id: i64,
    balance: f64,
}

/// Replaces what ESI returns as a character's wallet balance.
async fn set_wallet(State(shared): State<Shared>, Json(body): Json<WalletBody>) -> StatusCode {
    shared
        .lock()
        .unwrap()
        .wallets
        .insert(body.character_id, body.balance);
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct AssetsBody {
    character_id: i64,
    assets: Vec<Value>,
    page_size: Option<usize>,
}

/// Replaces what ESI returns as a character's assets.
async fn set_assets(State(shared): State<Shared>, Json(body): Json<AssetsBody>) -> StatusCode {
    shared.lock().unwrap().assets.insert(
        body.character_id,
        MarketFixture {
            orders: body.assets,
            page_size: body.page_size.unwrap_or(ASSETS_PAGE_SIZE).max(1),
        },
    );
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct PricesBody {
    prices: Vec<Value>,
}

/// Replaces what ESI returns as the market prices.
async fn set_prices(State(shared): State<Shared>, Json(body): Json<PricesBody>) -> StatusCode {
    shared.lock().unwrap().prices = body.prices;
    StatusCode::NO_CONTENT
}
