//! The ESI client: the three routes the app uses, ESI's etiquette (a descriptive `User-Agent`, a
//! pinned compatibility date) and the error-limit and rate-limit gate.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration as StdDuration,
};

use reqwest::{StatusCode, header::HeaderMap};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer};
use time::{Duration, OffsetDateTime};
use url::Url;

use crate::{clock::Clock, config::Config};

/// Pins ESI's behaviour: route changes only reach the app when this is raised on purpose.
pub const COMPATIBILITY_DATE: &str = "2025-09-30";

/// Stop sending requests when ESI reports fewer remaining errors than this.
const ERROR_LIMIT_MARGIN: i64 = 20;
/// Used when ESI blocks us without telling for how long.
const DEFAULT_BLOCK: Duration = Duration::seconds(60);
const REQUEST_TIMEOUT: StdDuration = StdDuration::from_secs(30);

pub const ROUTE_ORDERS: &str = "orders";
pub const ROUTE_NAMES: &str = "names";
pub const ROUTE_STRUCTURES: &str = "structures";
/// Public market orders, in a rate-limit group of their own.
pub const ROUTE_MARKET: &str = "market";
/// The region-wide order book, page by page. Shares the market rate-limit group, which the gate
/// learns from the answers' headers.
pub const ROUTE_REGION_BOOK: &str = "region-book";
/// The market history of one item; error-limited, not rate-limited.
pub const ROUTE_HISTORY: &str = "history";
pub const ROUTE_WALLET: &str = "wallet";
/// Shares the wallet's rate-limit group, which the gate learns from the answers' headers.
pub const ROUTE_TRANSACTIONS: &str = "transactions";
pub const ROUTE_ASSETS: &str = "assets";
pub const ROUTE_PRICES: &str = "prices";
pub const ROUTE_SKILLS: &str = "skills";
pub const ROUTE_STANDINGS: &str = "standings";
pub const ROUTE_STATIONS: &str = "stations";

#[derive(Debug, thiserror::Error)]
pub enum EsiError {
    /// Network trouble, a timeout or a server error: nothing changed, try again later.
    #[error("temporary ESI failure: {0}")]
    Temporary(String),
    #[error("ESI refused access")]
    Forbidden,
    #[error("ESI does not know the requested resource")]
    NotFound,
    /// The gate is blocking requests (or ESI just asked for a pause) until the given time.
    #[error("ESI requests are paused until {until}")]
    Deferred { until: OffsetDateTime },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EsiOrder {
    pub order_id: i64,
    pub type_id: i64,
    pub region_id: i64,
    pub location_id: i64,
    /// ESI leaves the flag out of sell orders.
    #[serde(default)]
    pub is_buy_order: bool,
    #[serde(default)]
    pub is_corporation: bool,
    /// ISK in cents: EVE prices have at most two decimals, so rounding is lossless.
    #[serde(rename = "price", deserialize_with = "cents")]
    pub price_cents: i64,
    pub volume_remain: i64,
    pub volume_total: i64,
    /// ESI only sends it for buy orders; a sell order has no minimum.
    #[serde(default = "no_minimum")]
    pub min_volume: i64,
    pub range: String,
    #[serde(rename = "escrow", default, deserialize_with = "optional_cents")]
    pub escrow_cents: Option<i64>,
    #[serde(with = "time::serde::rfc3339")]
    pub issued: OffsetDateTime,
    pub duration: i32,
}

/// One public market order, with only what the comparison of standings reads.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EsiMarketOrder {
    pub order_id: i64,
    pub type_id: i64,
    pub location_id: i64,
    pub system_id: i64,
    pub is_buy_order: bool,
    /// ISK in cents, as for the character's own orders.
    #[serde(rename = "price", deserialize_with = "cents")]
    pub price_cents: i64,
    pub range: String,
    pub volume_remain: i64,
}

/// One day of an item's market history in a region: what the opportunity analysis reads.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EsiHistoryDay {
    /// `YYYY-MM-DD`
    pub date: String,
    #[serde(rename = "average", deserialize_with = "cents")]
    pub average_cents: i64,
    #[serde(rename = "highest", deserialize_with = "cents")]
    pub highest_cents: i64,
    #[serde(rename = "lowest", deserialize_with = "cents")]
    pub lowest_cents: i64,
    pub order_count: i64,
    pub volume: i64,
}

/// One asset of a character, with only what the valuation and the list read.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EsiAsset {
    pub item_id: i64,
    pub type_id: i64,
    pub location_id: i64,
    pub location_type: String,
    pub location_flag: String,
    pub quantity: i64,
    pub is_singleton: bool,
    /// ESI leaves the flag out of everything that is not a blueprint copy.
    #[serde(default)]
    pub is_blueprint_copy: bool,
}

/// One wallet transaction of a character, with only what the purchase history reads.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EsiTransaction {
    pub transaction_id: i64,
    #[serde(rename = "date", with = "time::serde::rfc3339")]
    pub bought_at: OffsetDateTime,
    pub type_id: i64,
    pub location_id: i64,
    pub quantity: i64,
    /// ISK in cents, as for orders.
    #[serde(rename = "unit_price", deserialize_with = "cents")]
    pub unit_price_cents: i64,
    pub is_buy: bool,
    pub is_personal: bool,
}

/// ESI's market price of one item. An item nobody trades has no average.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EsiPrice {
    pub type_id: i64,
    /// ISK in cents, as for orders.
    #[serde(rename = "average_price", default, deserialize_with = "optional_cents")]
    pub average_price_cents: Option<i64>,
}

/// The skills of a character. ESI also sends totals, which are not read.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EsiSkills {
    pub skills: Vec<EsiSkill>,
}

/// One trained skill. The active level is the one that applies: it differs from the trained level
/// for Alpha clones and under skill limits.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EsiSkill {
    pub skill_id: i64,
    pub active_skill_level: i16,
}

/// A standing of a character towards an agent, an NPC corporation or a faction.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EsiStanding {
    pub from_id: i64,
    /// `agent`, `npc_corp` or `faction`.
    pub from_type: String,
    /// The decimal text of the reported number: it goes to a `numeric` column as it is, so no
    /// `f64` arithmetic touches it.
    #[serde(deserialize_with = "decimal_text")]
    pub standing: String,
}

/// An NPC station, with only its owner corporation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EsiStation {
    pub owner: i64,
}

fn decimal_text<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    // `Display` of an `f64` is the shortest text that reads back as the same number, and never
    // uses an exponent.
    Ok(f64::deserialize(d)?.to_string())
}

fn no_minimum() -> i64 {
    1
}

fn cents<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    Ok((f64::deserialize(d)? * 100.0).round() as i64)
}

fn optional_cents<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    Ok(Option::<f64>::deserialize(d)?.map(|v| (v * 100.0).round() as i64))
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EsiName {
    pub category: String,
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct EsiStructure {
    pub name: String,
}

#[derive(Debug)]
pub struct EsiResponse<T> {
    pub body: T,
    /// When ESI's cache for this answer runs out, on the local clock.
    pub expires_at: Option<OffsetDateTime>,
    /// ESI's own `Expires` (Unix seconds): the same for every answer of one cache generation,
    /// whatever the local clock says.
    pub server_expires: Option<i64>,
}

/// One page of a region's whole order book.
#[derive(Debug, Clone)]
pub struct EsiBookPage {
    pub orders: Vec<EsiMarketOrder>,
    /// How many pages the book has (`X-Pages`).
    pub pages: u32,
    /// When ESI's cache for this page runs out, on the local clock.
    pub expires_at: Option<OffsetDateTime>,
    /// ESI's own `Expires` (Unix seconds): pages of one cache generation share it.
    pub generation: Option<i64>,
}

pub struct EsiClient {
    http: reqwest::Client,
    base_url: Url,
    user_agent: String,
    clock: Arc<dyn Clock>,
    gate: EsiGate,
}

impl EsiClient {
    pub fn new(http: reqwest::Client, config: &Config, clock: Arc<dyn Clock>) -> Self {
        Self {
            http,
            base_url: config.esi_base_url.clone(),
            user_agent: user_agent(&config.public_base_url, config.esi_contact.as_deref()),
            gate: EsiGate::new(clock.clone()),
            clock,
        }
    }

    pub fn gate(&self) -> &EsiGate {
        &self.gate
    }

    pub async fn orders(
        &self,
        character_id: i64,
        token: &SecretString,
    ) -> Result<EsiResponse<Vec<EsiOrder>>, EsiError> {
        let request = self
            .http
            .get(self.url(&format!("characters/{character_id}/orders")))
            .bearer_auth(token.expose_secret());
        self.fetch(ROUTE_ORDERS, request).await
    }

    /// The wallet balance in cents. ESI answers with a bare number.
    pub async fn wallet(
        &self,
        character_id: i64,
        token: &SecretString,
    ) -> Result<EsiResponse<i64>, EsiError> {
        let request = self
            .http
            .get(self.url(&format!("characters/{character_id}/wallet")))
            .bearer_auth(token.expose_secret());
        let response = self.fetch::<f64>(ROUTE_WALLET, request).await?;
        Ok(EsiResponse {
            body: (response.body * 100.0).round() as i64,
            expires_at: response.expires_at,
            server_expires: response.server_expires,
        })
    }

    /// One answer of the character's wallet transactions, newest first. With `from_id`, the
    /// transactions up to and including that one (verified against real ESI). Reading further
    /// back is the caller's job.
    pub async fn wallet_transactions(
        &self,
        character_id: i64,
        token: &SecretString,
        from_id: Option<i64>,
    ) -> Result<EsiResponse<Vec<EsiTransaction>>, EsiError> {
        let mut url = self.url(&format!("characters/{character_id}/wallet/transactions"));
        if let Some(from_id) = from_id {
            url.query_pairs_mut()
                .append_pair("from_id", &from_id.to_string());
        }
        let request = self.http.get(url).bearer_auth(token.expose_secret());
        self.fetch(ROUTE_TRANSACTIONS, request).await
    }

    /// All assets of a character, every page read one after another. The expiry is the first
    /// page's. One failing page fails the whole answer.
    pub async fn assets(
        &self,
        character_id: i64,
        token: &SecretString,
    ) -> Result<EsiResponse<Vec<EsiAsset>>, EsiError> {
        let page_request = |page: u32| {
            let mut url = self.url(&format!("characters/{character_id}/assets"));
            url.query_pairs_mut().append_pair("page", &page.to_string());
            self.http.get(url).bearer_auth(token.expose_secret())
        };
        let (first, pages) = self
            .fetch_paged::<Vec<EsiAsset>>(ROUTE_ASSETS, page_request(1))
            .await?;
        let mut assets = first.body;
        for page in 2..=pages.unwrap_or(1) {
            let (next, _) = self
                .fetch_paged::<Vec<EsiAsset>>(ROUTE_ASSETS, page_request(page))
                .await?;
            assets.extend(next.body);
        }
        Ok(EsiResponse {
            body: assets,
            expires_at: first.expires_at,
            server_expires: first.server_expires,
        })
    }

    /// The skills of a character.
    pub async fn skills(
        &self,
        character_id: i64,
        token: &SecretString,
    ) -> Result<EsiResponse<EsiSkills>, EsiError> {
        let request = self
            .http
            .get(self.url(&format!("characters/{character_id}/skills")))
            .bearer_auth(token.expose_secret());
        self.fetch(ROUTE_SKILLS, request).await
    }

    /// The standings of a character towards agents, NPC corporations and factions.
    pub async fn standings(
        &self,
        character_id: i64,
        token: &SecretString,
    ) -> Result<EsiResponse<Vec<EsiStanding>>, EsiError> {
        let request = self
            .http
            .get(self.url(&format!("characters/{character_id}/standings")))
            .bearer_auth(token.expose_secret());
        self.fetch(ROUTE_STANDINGS, request).await
    }

    /// An NPC station. Public.
    pub async fn station(&self, station_id: i64) -> Result<EsiStation, EsiError> {
        let request = self
            .http
            .get(self.url(&format!("universe/stations/{station_id}")));
        Ok(self.fetch(ROUTE_STATIONS, request).await?.body)
    }

    /// ESI's market price of every item. Public.
    pub async fn prices(&self) -> Result<EsiResponse<Vec<EsiPrice>>, EsiError> {
        self.fetch(ROUTE_PRICES, self.http.get(self.url("markets/prices")))
            .await
    }

    /// Resolves up to 1000 IDs. One unknown ID fails the whole request with `NotFound`.
    pub async fn names(&self, ids: &[i64]) -> Result<Vec<EsiName>, EsiError> {
        let request = self.http.post(self.url("universe/names")).json(ids);
        Ok(self.fetch(ROUTE_NAMES, request).await?.body)
    }

    pub async fn structure(
        &self,
        structure_id: i64,
        token: &SecretString,
    ) -> Result<EsiStructure, EsiError> {
        let request = self
            .http
            .get(self.url(&format!("universe/structures/{structure_id}")))
            .bearer_auth(token.expose_secret());
        Ok(self.fetch(ROUTE_STRUCTURES, request).await?.body)
    }

    /// All public market orders of a region for one item, every page read one after another. The
    /// expiry is the first page's. One failing page fails the whole answer. Needs no token.
    pub async fn market_orders(
        &self,
        region_id: i64,
        type_id: i64,
    ) -> Result<EsiResponse<Vec<EsiMarketOrder>>, EsiError> {
        let page_request = |page: u32| {
            let mut url = self.url(&format!("markets/{region_id}/orders"));
            url.query_pairs_mut()
                .append_pair("order_type", "all")
                .append_pair("type_id", &type_id.to_string())
                .append_pair("page", &page.to_string());
            self.http.get(url)
        };
        let (first, pages) = self
            .fetch_paged::<Vec<EsiMarketOrder>>(ROUTE_MARKET, page_request(1))
            .await?;
        let mut orders = first.body;
        for page in 2..=pages.unwrap_or(1) {
            let (next, _) = self
                .fetch_paged::<Vec<EsiMarketOrder>>(ROUTE_MARKET, page_request(page))
                .await?;
            orders.extend(next.body);
        }
        Ok(EsiResponse {
            body: orders,
            expires_at: first.expires_at,
            server_expires: first.server_expires,
        })
    }

    /// One page of every public market order of a region, whatever the item. Needs no token.
    pub async fn region_orders_page(
        &self,
        region_id: i64,
        page: u32,
    ) -> Result<EsiBookPage, EsiError> {
        let mut url = self.url(&format!("markets/{region_id}/orders"));
        url.query_pairs_mut().append_pair("page", &page.to_string());
        let (response, pages) = self
            .fetch_paged::<Vec<EsiMarketOrder>>(ROUTE_REGION_BOOK, self.http.get(url))
            .await?;
        Ok(EsiBookPage {
            orders: response.body,
            pages: pages.unwrap_or(1),
            expires_at: response.expires_at,
            generation: response.server_expires,
        })
    }

    /// The daily market history of one item in a region, oldest first. A type nobody traded is an
    /// empty list; an unknown type is `NotFound`. Needs no token.
    pub async fn market_history(
        &self,
        region_id: i64,
        type_id: i64,
    ) -> Result<EsiResponse<Vec<EsiHistoryDay>>, EsiError> {
        let mut url = self.url(&format!("markets/{region_id}/history"));
        url.query_pairs_mut()
            .append_pair("type_id", &type_id.to_string());
        self.fetch(ROUTE_HISTORY, self.http.get(url)).await
    }

    fn url(&self, path: &str) -> Url {
        self.base_url.join(path).expect("a valid ESI path")
    }

    async fn fetch<T: serde::de::DeserializeOwned>(
        &self,
        route: &'static str,
        request: reqwest::RequestBuilder,
    ) -> Result<EsiResponse<T>, EsiError> {
        Ok(self.fetch_paged(route, request).await?.0)
    }

    /// Like `fetch`, and also the page count from `X-Pages` when the answer has one.
    async fn fetch_paged<T: serde::de::DeserializeOwned>(
        &self,
        route: &'static str,
        request: reqwest::RequestBuilder,
    ) -> Result<(EsiResponse<T>, Option<u32>), EsiError> {
        self.gate
            .check(route)
            .map_err(|until| EsiError::Deferred { until })?;
        let response = request
            .header(reqwest::header::USER_AGENT, &self.user_agent)
            .header("X-Compatibility-Date", COMPATIBILITY_DATE)
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|e| EsiError::Temporary(e.without_url().to_string()))?;
        let status = response.status();
        self.gate.record(route, status, response.headers());
        match status {
            s if s.is_success() => {
                let expires_at = expires_at(response.headers(), self.clock.now());
                let server_expires = server_expires(response.headers());
                let pages =
                    header_int(response.headers(), "x-pages").and_then(|p| u32::try_from(p).ok());
                // Read as text first: reqwest's own decode error does not say which field failed.
                let text = response
                    .text()
                    .await
                    .map_err(|e| EsiError::Temporary(format!("unreadable ESI answer: {e}")))?;
                let body = serde_json::from_str(&text)
                    .map_err(|e| EsiError::Temporary(format!("unreadable ESI answer: {e}")))?;
                Ok((
                    EsiResponse {
                        body,
                        expires_at,
                        server_expires,
                    },
                    pages,
                ))
            }
            StatusCode::FORBIDDEN => Err(EsiError::Forbidden),
            StatusCode::NOT_FOUND => Err(EsiError::NotFound),
            // 420 and 429 have just armed the gate, so there is a time to wait for.
            StatusCode::TOO_MANY_REQUESTS => Err(EsiError::Deferred {
                until: self
                    .gate
                    .blocked_until(route)
                    .unwrap_or_else(|| self.clock.now() + DEFAULT_BLOCK),
            }),
            s if s.as_u16() == 420 => Err(EsiError::Deferred {
                until: self
                    .gate
                    .blocked_until(route)
                    .unwrap_or_else(|| self.clock.now() + DEFAULT_BLOCK),
            }),
            s => Err(EsiError::Temporary(format!("HTTP {s}"))),
        }
    }
}

/// `Sekondo/<version> (+<public url>[; <contact>])`
pub fn user_agent(public_base_url: &Url, contact: Option<&str>) -> String {
    let mut agent = format!(
        "Sekondo/{} (+{}",
        env!("CARGO_PKG_VERSION"),
        public_base_url.as_str().trim_end_matches('/')
    );
    if let Some(contact) = contact {
        // Header values are visible ASCII only.
        let contact: String = contact
            .chars()
            .filter(|c| c.is_ascii_graphic() || *c == ' ')
            .collect();
        if !contact.trim().is_empty() {
            agent.push_str("; ");
            agent.push_str(contact.trim());
        }
    }
    agent.push(')');
    agent
}

/// ESI's `Expires` as it is, in Unix seconds.
fn server_expires(headers: &HeaderMap) -> Option<i64> {
    let expires = httpdate::parse_http_date(headers.get("expires")?.to_str().ok()?).ok()?;
    let seconds = expires
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    i64::try_from(seconds).ok()
}

/// ESI's `Expires` on the local clock: the server's `Expires` minus its own `Date`, added to now,
/// so a skewed local clock does not matter.
fn expires_at(headers: &HeaderMap, now: OffsetDateTime) -> Option<OffsetDateTime> {
    let http_date = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| httpdate::parse_http_date(v).ok())
    };
    let expires = http_date("expires")?;
    let unix = |t: std::time::SystemTime| {
        t.duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64)
    };
    match http_date("date") {
        Some(date) => Some(now + Duration::seconds((unix(expires) - unix(date)).max(0))),
        None => OffsetDateTime::from_unix_timestamp(unix(expires)).ok(),
    }
}

/// Remembers when ESI asked us to pause, globally (error limit) or per rate-limit group.
pub struct EsiGate {
    clock: Arc<dyn Clock>,
    state: Mutex<GateState>,
}

#[derive(Default)]
struct GateState {
    global_until: Option<OffsetDateTime>,
    /// Rate-limit group (or, until ESI names one, the route) to the end of its block.
    groups: HashMap<String, OffsetDateTime>,
    /// The group each route was last seen in.
    route_groups: HashMap<&'static str, String>,
}

/// What a single response tells the gate.
#[derive(Debug, Default, PartialEq, Eq)]
struct Limits {
    global_until: Option<OffsetDateTime>,
    group: Option<(String, Option<OffsetDateTime>)>,
}

fn header_text<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

fn header_int(headers: &HeaderMap, name: &str) -> Option<i64> {
    header_text(headers, name).and_then(|v| v.trim().parse().ok())
}

/// Header parsing, kept free of state so it can be tested on its own.
fn read_limits(
    route: &'static str,
    status: StatusCode,
    headers: &HeaderMap,
    now: OffsetDateTime,
) -> Limits {
    let mut limits = Limits::default();

    let reset = header_int(headers, "x-esi-error-limit-reset").map(Duration::seconds);
    let remain = header_int(headers, "x-esi-error-limit-remain");
    if status.as_u16() == 420 || remain.is_some_and(|r| r < ERROR_LIMIT_MARGIN) {
        limits.global_until = Some(now + reset.unwrap_or(DEFAULT_BLOCK));
    }

    let group = header_text(headers, "x-ratelimit-group")
        .map(str::to_owned)
        .unwrap_or_else(|| route.to_owned());
    if status == StatusCode::TOO_MANY_REQUESTS {
        let wait = header_int(headers, "retry-after")
            .map(Duration::seconds)
            .unwrap_or(DEFAULT_BLOCK);
        limits.group = Some((group, Some(now + wait)));
    } else if header_int(headers, "x-ratelimit-remaining") == Some(0) {
        limits.group = Some((group, Some(now + Duration::seconds(1))));
    } else if header_text(headers, "x-ratelimit-group").is_some() {
        limits.group = Some((group, None));
    }
    limits
}

impl EsiGate {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            state: Mutex::new(GateState::default()),
        }
    }

    /// `Err(until)` while requests to `route` must not be sent.
    pub fn check(&self, route: &'static str) -> Result<(), OffsetDateTime> {
        match self.blocked_until(route) {
            Some(until) => Err(until),
            None => Ok(()),
        }
    }

    /// The end of the block that applies to `route` right now, if any.
    pub fn blocked_until(&self, route: &'static str) -> Option<OffsetDateTime> {
        let now = self.clock.now();
        let state = self.state.lock().unwrap();
        let group = state.route_groups.get(route).map_or(route, String::as_str);
        [
            state.global_until,
            state.groups.get(group).copied(),
            state.groups.get(route).copied(),
        ]
        .into_iter()
        .flatten()
        .filter(|until| *until > now)
        .max()
    }

    /// Learns from a response's status and headers.
    pub fn record(&self, route: &'static str, status: StatusCode, headers: &HeaderMap) {
        let limits = read_limits(route, status, headers, self.clock.now());
        let mut state = self.state.lock().unwrap();
        if let Some(until) = limits.global_until {
            state.global_until = state.global_until.max(Some(until));
        }
        if let Some((group, until)) = limits.group {
            state.route_groups.insert(route, group.clone());
            if let Some(until) = until {
                let entry = state.groups.entry(group).or_insert(until);
                *entry = (*entry).max(until);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use reqwest::header::HeaderValue;
    use time::macros::datetime;

    use super::*;

    #[derive(Debug)]
    struct FixedClock(Mutex<OffsetDateTime>);

    impl Clock for FixedClock {
        fn now(&self) -> OffsetDateTime {
            *self.0.lock().unwrap()
        }
    }

    const NOW: OffsetDateTime = datetime!(2026-10-16 12:00:00 UTC);

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                reqwest::header::HeaderName::try_from(*name).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    fn limits(status: u16, pairs: &[(&str, &str)]) -> Limits {
        read_limits(
            ROUTE_ORDERS,
            StatusCode::from_u16(status).unwrap(),
            &headers(pairs),
            NOW,
        )
    }

    #[test]
    fn an_error_limit_below_the_margin_blocks_everything_until_the_reset() {
        let l = limits(
            200,
            &[
                ("x-esi-error-limit-remain", "19"),
                ("x-esi-error-limit-reset", "30"),
            ],
        );
        assert_eq!(l.global_until, Some(NOW + Duration::seconds(30)));
    }

    #[test]
    fn an_error_limit_at_the_margin_does_not_block() {
        let l = limits(
            200,
            &[
                ("x-esi-error-limit-remain", "20"),
                ("x-esi-error-limit-reset", "30"),
            ],
        );
        assert_eq!(l.global_until, None);
    }

    #[test]
    fn status_420_blocks_globally_even_without_headers() {
        assert_eq!(
            limits(420, &[("x-esi-error-limit-reset", "45")]).global_until,
            Some(NOW + Duration::seconds(45))
        );
        assert_eq!(limits(420, &[]).global_until, Some(NOW + DEFAULT_BLOCK));
    }

    #[test]
    fn status_429_blocks_the_group_for_retry_after() {
        let l = limits(
            429,
            &[("retry-after", "60"), ("x-ratelimit-group", "char-market")],
        );
        assert_eq!(l.global_until, None);
        assert_eq!(
            l.group,
            Some(("char-market".to_owned(), Some(NOW + Duration::seconds(60))))
        );
    }

    #[test]
    fn status_429_without_a_group_blocks_the_route() {
        let l = limits(429, &[("retry-after", "5")]);
        assert_eq!(
            l.group,
            Some((ROUTE_ORDERS.to_owned(), Some(NOW + Duration::seconds(5))))
        );
    }

    #[test]
    fn no_remaining_requests_without_a_429_blocks_the_group_for_a_second() {
        let l = limits(
            200,
            &[
                ("x-ratelimit-group", "char-market"),
                ("x-ratelimit-remaining", "0"),
            ],
        );
        assert_eq!(
            l.group,
            Some(("char-market".to_owned(), Some(NOW + Duration::seconds(1))))
        );
    }

    #[test]
    fn a_healthy_response_only_teaches_the_group_name() {
        let l = limits(
            200,
            &[
                ("x-ratelimit-group", "char-market"),
                ("x-ratelimit-remaining", "1199"),
                ("x-esi-error-limit-remain", "100"),
            ],
        );
        assert_eq!(l.global_until, None);
        assert_eq!(l.group, Some(("char-market".to_owned(), None)));
    }

    #[test]
    fn expires_is_measured_against_the_servers_date() {
        let h = headers(&[
            ("date", "Fri, 16 Oct 2026 10:00:00 GMT"),
            ("expires", "Fri, 16 Oct 2026 10:20:00 GMT"),
        ]);
        // The local clock is an hour ahead of ESI's.
        assert_eq!(expires_at(&h, NOW), Some(NOW + Duration::minutes(20)));
        assert_eq!(expires_at(&headers(&[]), NOW), None);
    }

    fn gate() -> (EsiGate, Arc<FixedClock>) {
        let clock = Arc::new(FixedClock(Mutex::new(NOW)));
        (EsiGate::new(clock.clone()), clock)
    }

    #[test]
    fn the_gate_defers_until_the_block_has_passed() {
        let (gate, clock) = gate();
        assert_eq!(gate.check(ROUTE_ORDERS), Ok(()));

        gate.record(
            ROUTE_ORDERS,
            StatusCode::from_u16(420).unwrap(),
            &headers(&[("x-esi-error-limit-reset", "30")]),
        );

        let until = NOW + Duration::seconds(30);
        assert_eq!(gate.check(ROUTE_ORDERS), Err(until));
        assert_eq!(
            gate.check(ROUTE_NAMES),
            Err(until),
            "the error limit is global"
        );

        *clock.0.lock().unwrap() = until;
        assert_eq!(gate.check(ROUTE_ORDERS), Ok(()));
    }

    #[test]
    fn a_rate_limit_only_blocks_its_own_group() {
        let (gate, _) = gate();
        gate.record(
            ROUTE_ORDERS,
            StatusCode::TOO_MANY_REQUESTS,
            &headers(&[("retry-after", "60"), ("x-ratelimit-group", "char-market")]),
        );
        assert_eq!(gate.check(ROUTE_ORDERS), Err(NOW + Duration::seconds(60)));
        assert_eq!(gate.check(ROUTE_NAMES), Ok(()));
    }
}
