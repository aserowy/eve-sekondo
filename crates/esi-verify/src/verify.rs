//! The verification: login in a browser, then every verifiable variant against real EVE.

use std::{collections::BTreeSet, fmt, io::Write};

use esi_contract::{Verifiable, ViolationKind, check, contract, contracts, gate::Record};
use reqwest::header::{HeaderMap, USER_AGENT};
use secrecy::{ExposeSecret, SecretString};
use sekondo::{
    esi::{
        COMPATIBILITY_DATE, EsiAsset, EsiHistoryDay, EsiMarketOrder, EsiName, EsiOrder, EsiPrice,
        EsiSkills, EsiStanding, EsiStation, EsiStructure, EsiTransaction, user_agent,
    },
    oauth::{Intent, PendingFlow},
    poller::STRUCTURE_ID_MIN,
    sso::{SsoClient, decode_token_body},
    tls,
};
use serde_json::{Map, Value};
use url::Url;

use crate::{browser::Browser, callback, scrub::scrub_decode_error, settings::Settings};

#[derive(Debug)]
pub struct VerifyError(pub String);

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for VerifyError {}

impl From<String> for VerifyError {
    fn from(message: String) -> Self {
        Self(message)
    }
}

type Result<T> = std::result::Result<T, VerifyError>;

fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(VerifyError(message.into()))
}

/// What the caller supplies besides the settings.
pub struct Hooks<'a> {
    /// Proves the stub conforms to the contracts before anything is verified, so a recorded
    /// fingerprint always means "stub conforms and real EVE conforms".
    pub stub_conforms: &'a dyn Fn() -> std::result::Result<(), String>,
}

/// An answer, as far as the verification looks at it. Never printed.
struct Answer {
    status: u16,
    headers: HeaderMap,
    text: String,
    json: Option<Value>,
}

impl Answer {
    async fn of(response: reqwest::Response) -> Result<Self> {
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let text = response
            .text()
            .await
            .map_err(|e| VerifyError(format!("unreadable answer: {}", e.without_url())))?;
        Ok(Self {
            status,
            headers,
            json: serde_json::from_str(&text).ok(),
            text,
        })
    }
}

struct Run<'a> {
    out: &'a mut dyn Write,
    verified: BTreeSet<String>,
    problems: Vec<String>,
}

impl Run<'_> {
    fn say(&mut self, line: impl AsRef<str>) {
        let _ = writeln!(self.out, "{}", line.as_ref());
    }

    /// Checks an answer against `call/variant`; records the variant when it passes.
    fn check(&mut self, call: &str, variant: &str, answer: &Answer) {
        let limits: Vec<String> = answer
            .headers
            .iter()
            .filter(|(name, _)| {
                name.as_str().starts_with("x-ratelimit-") || name.as_str() == "retry-after"
            })
            .map(|(name, value)| format!("{name}={}", value.to_str().unwrap_or("?")))
            .collect();
        if !limits.is_empty() {
            self.say(format!("note  {call}/{variant}: {}", limits.join(" ")));
        }
        self.check_parts(
            call,
            variant,
            Some(answer.status),
            &answer.headers,
            answer.json.as_ref(),
        );
    }

    fn check_parts(
        &mut self,
        call: &str,
        variant: &str,
        status: Option<u16>,
        headers: &HeaderMap,
        body: Option<&Value>,
    ) {
        let violations = check(&contract(call), variant, status, headers, body);
        if violations.is_empty() {
            self.say(format!("ok    {call}/{variant}"));
            self.verified.insert(format!("{call}/{variant}"));
            return;
        }
        for v in violations {
            let message = match v.kind {
                ViolationKind::NothingToCheck => format!(
                    "{call}/{variant}: the verification character needs {} order ({})",
                    if variant.starts_with("buy") {
                        "a buy"
                    } else {
                        "a sell"
                    },
                    v.problem
                ),
                _ => v.to_string(),
            };
            self.say(format!("FAIL  {message}"));
            self.problems.push(message);
        }
    }

    fn problem(&mut self, message: String) {
        self.say(format!("FAIL  {message}"));
        self.problems.push(message);
    }
}

fn query_object(pairs: impl Iterator<Item = (String, String)>) -> Value {
    Value::Object(
        pairs
            .map(|(k, v)| (k, Value::String(v)))
            .collect::<Map<_, _>>(),
    )
}

/// 420 and 429 mean the budget is used up: stop, do not make it worse.
fn stop_when_limited(call: &str, answer: &Answer) -> Result<()> {
    if matches!(answer.status, 420 | 429) {
        return fail(format!(
            "ESI answered {} to {call}: the error or rate limit is used up; wait and run again",
            answer.status
        ));
    }
    Ok(())
}

pub async fn run(
    settings: &Settings,
    browser: &dyn Browser,
    hooks: &Hooks<'_>,
    out: &mut dyn Write,
) -> Result<Record> {
    (hooks.stub_conforms)().map_err(|e| {
        VerifyError(format!(
            "the stub does not conform to the contracts, so it cannot be verified: {e}"
        ))
    })?;
    browser.ensure_available()?;

    let mut run = Run {
        out,
        verified: BTreeSet::new(),
        problems: Vec::new(),
    };
    let http = tls::http_client(settings.extra_ca_path.as_deref()).map_err(|e| e.to_string())?;
    let sso = SsoClient::new(
        http.clone(),
        settings.sso_base_url.clone(),
        settings.client_id.clone(),
        settings.client_secret.clone(),
        std::time::Duration::from_secs(30),
    );

    let (access_token, refresh_token) = login(settings, browser, &mut run, &sso).await?;
    let refreshed = verify_sso(
        &mut run,
        &sso,
        &http,
        settings,
        &access_token,
        refresh_token,
    )
    .await?;
    let token = refreshed.unwrap_or(access_token);
    verify_esi(settings, &mut run, &http, &token).await?;

    let expected: BTreeSet<String> = contracts()
        .iter()
        .flat_map(|c| {
            c.variants
                .iter()
                .filter(|v| v.verifiable == Verifiable::Yes)
                .map(|v| format!("{}/{}", c.name, v.name))
        })
        .collect();
    for missing in expected.difference(&run.verified.clone()) {
        if run.problems.is_empty() {
            run.problem(format!(
                "{missing}: the verification did not reach this variant"
            ));
        }
    }
    if !run.problems.is_empty() {
        let count = run.problems.len();
        return fail(format!(
            "{count} problem(s); the record of the verified stub was not changed"
        ));
    }

    let record = Record {
        fingerprint: esi_contract::fingerprint(&settings.workspace_root)
            .map_err(|e| format!("cannot fingerprint the stub: {e}"))?,
        verified_at: time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|e| e.to_string())?,
        variants: run.verified.iter().cloned().collect(),
    };
    esi_contract::gate::write_record(&settings.workspace_root, &record)
        .map_err(|e| format!("cannot write the record: {e}"))?;
    run.say(format!(
        "verified {} variants; recorded the stub's fingerprint",
        record.variants.len()
    ));
    Ok(record)
}

/// The browser login. Returns the verification character's access token and refresh token.
async fn login(
    settings: &Settings,
    browser: &dyn Browser,
    run: &mut Run<'_>,
    sso: &SsoClient,
) -> Result<(SecretString, Option<SecretString>)> {
    let addr = settings.callback_addr()?;
    let flow = PendingFlow::new(Intent::Login);
    let redirect_uri = settings.callback_url();
    let authorize: Url =
        flow.authorize_url(&settings.sso_base_url, &settings.client_id, &redirect_uri);

    run.check_parts(
        "authorization",
        "request",
        None,
        &HeaderMap::new(),
        Some(&query_object(
            authorize
                .query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned())),
        )),
    );

    let server = callback::listen(addr, &settings.tls_cert_path, &settings.tls_key_path).await?;
    run.say(format!(
        "Log in with the verification character at:\n{authorize}"
    ));
    browser.open(&authorize)?;
    let query = server.wait(settings.login_timeout).await?;

    if query.contains_key("error") {
        return fail("the login ended with an error from SSO; no code was exchanged");
    }
    if query.get("state") != Some(&flow.state) {
        return fail(
            "the login callback carried a different state than the one sent; no code was exchanged",
        );
    }
    let query_json = query_object(query.clone().into_iter());
    run.check_parts(
        "authorization",
        "callback",
        None,
        &HeaderMap::new(),
        Some(&query_json),
    );
    let Some(code) = query.get("code") else {
        return fail("the login callback carried no code");
    };

    let answer = Answer::of(
        sso.exchange_code_response(code, &flow.code_verifier, &redirect_uri)
            .await
            .map_err(|e| VerifyError(format!("token request failed: {e}")))?,
    )
    .await?;
    run.check("token", "authorization-code", &answer);
    let tokens = token_set(&answer, "authorization-code")?;
    Ok((tokens.access_token, tokens.refresh_token))
}

fn token_set(answer: &Answer, variant: &str) -> Result<sekondo::sso::TokenSet> {
    if answer.status != 200 {
        return fail(format!("token/{variant}: SSO answered {}", answer.status));
    }
    decode_token_body(&answer.text).map_err(|e| {
        VerifyError(format!(
            "token/{variant}: the app cannot decode the answer: {}",
            scrub_decode_error(&e.to_string())
        ))
    })
}

/// JWKS, the access token's claims and one refresh. Returns the refreshed access token.
async fn verify_sso(
    run: &mut Run<'_>,
    sso: &SsoClient,
    http: &reqwest::Client,
    settings: &Settings,
    access_token: &SecretString,
    refresh_token: Option<SecretString>,
) -> Result<Option<SecretString>> {
    let jwks_url = settings
        .sso_base_url
        .join("oauth/jwks")
        .map_err(|e| e.to_string())?;
    let answer = Answer::of(
        http.get(jwks_url)
            .send()
            .await
            .map_err(|e| VerifyError(format!("jwks request failed: {}", e.without_url())))?,
    )
    .await?;
    run.check("jwks", "rsa-keys", &answer);

    let claims = esi_contract::jwt_payload(access_token.expose_secret());
    run.check_parts(
        "token",
        "access-token-claims",
        None,
        &HeaderMap::new(),
        claims.as_ref(),
    );
    match sso.validate(access_token.expose_secret()).await {
        Ok(verified) => {
            let missing = verified.missing_scopes();
            if !missing.is_empty() {
                run.problem(format!(
                    "the verification character did not grant the scopes {}",
                    missing.join(", ")
                ));
            }
        }
        Err(e) => run.problem(format!("the app cannot validate the access token: {e}")),
    }

    let Some(refresh_token) = refresh_token else {
        run.problem(
            "the code exchange gave no refresh token to verify the refresh with".to_owned(),
        );
        return Ok(None);
    };
    let answer = Answer::of(
        sso.refresh_response(&refresh_token)
            .await
            .map_err(|e| VerifyError(format!("refresh request failed: {e}")))?,
    )
    .await?;
    run.check("token", "refresh", &answer);
    Ok(token_set(&answer, "refresh").ok().map(|t| t.access_token))
}

/// The region of the whole-book and history checks: The Forge, home of Jita 4-4.
const FORGE: i64 = 10000002;

async fn esi_request(
    settings: &Settings,
    request: reqwest::RequestBuilder,
    call: &str,
) -> Result<Answer> {
    let answer = Answer::of(
        request
            .header(
                USER_AGENT,
                user_agent(&settings.public_base_url, settings.contact.as_deref()),
            )
            .header("X-Compatibility-Date", COMPATIBILITY_DATE)
            .send()
            .await
            .map_err(|e| VerifyError(format!("{call} request failed: {}", e.without_url())))?,
    )
    .await?;
    stop_when_limited(call, &answer)?;
    Ok(answer)
}

fn decode<T: serde::de::DeserializeOwned>(
    run: &mut Run<'_>,
    call: &str,
    answer: &Answer,
) -> Option<T> {
    match serde_json::from_str(&answer.text) {
        Ok(value) => Some(value),
        Err(e) => {
            run.problem(format!(
                "{call}: the app cannot decode the answer: {}",
                scrub_decode_error(&e.to_string())
            ));
            None
        }
    }
}

async fn verify_esi(
    settings: &Settings,
    run: &mut Run<'_>,
    http: &reqwest::Client,
    token: &SecretString,
) -> Result<()> {
    let esi = |path: &str| {
        settings
            .esi_base_url
            .join(path)
            .map_err(|e| VerifyError(e.to_string()))
    };
    let me = sekondo_character_id(token)?;

    let answer = esi_request(
        settings,
        http.get(esi(&format!("characters/{me}/orders"))?)
            .bearer_auth(token.expose_secret()),
        "orders",
    )
    .await?;
    run.check("orders", "sell-orders", &answer);
    run.check("orders", "buy-orders", &answer);
    let orders: Vec<EsiOrder> = decode(run, "orders", &answer).unwrap_or_default();

    let mut ids: BTreeSet<i64> = [34, 10_000_002, 60_003_760].into();
    ids.extend(
        orders
            .iter()
            .flat_map(|o| [o.type_id, o.region_id, o.location_id])
            .filter(|id| *id < STRUCTURE_ID_MIN),
    );
    let ids: Vec<i64> = ids.into_iter().collect();
    let answer = esi_request(
        settings,
        http.post(esi("universe/names")?).json(&ids),
        "names",
    )
    .await?;
    run.check("names", "found", &answer);
    let _: Option<Vec<EsiName>> = decode(run, "names", &answer);

    let answer = esi_request(
        settings,
        http.post(esi("universe/names")?).json(&[1_i64]),
        "names",
    )
    .await?;
    run.check("names", "unknown-id", &answer);

    for (id, variant) in [
        (settings.structure_id, "found"),
        (settings.forbidden_structure_id, "forbidden"),
    ] {
        let answer = esi_request(
            settings,
            http.get(esi(&format!("universe/structures/{id}"))?)
                .bearer_auth(token.expose_secret()),
            "structures",
        )
        .await?;
        run.check("structures", variant, &answer);
        if variant == "found" {
            let _: Option<EsiStructure> = decode(run, "structures", &answer);
        }
    }

    // The public market of the pair of the first sell order and of the first buy order, which
    // lists the character's own order too.
    for own in [
        orders.iter().find(|o| !o.is_buy_order),
        orders.iter().find(|o| o.is_buy_order),
    ]
    .into_iter()
    .flatten()
    {
        let mut url = esi(&format!("markets/{}/orders", own.region_id))?;
        url.query_pairs_mut()
            .append_pair("order_type", "all")
            .append_pair("type_id", &own.type_id.to_string())
            .append_pair("page", "1");
        let answer = esi_request(settings, http.get(url), "market-orders").await?;
        run.check("market-orders", "orders", &answer);
        if let Some(market) = decode::<Vec<EsiMarketOrder>>(run, "market-orders", &answer)
            && market.is_empty()
        {
            run.problem(
                "market-orders/orders: the answer has no orders, but the character's own order for the item is on the market".to_owned(),
            );
        }
    }

    // The first page of The Forge's whole book, and the history of the item of the market check.
    let mut url = esi(&format!("markets/{FORGE}/orders"))?;
    url.query_pairs_mut().append_pair("page", "1");
    let answer = esi_request(settings, http.get(url), "region-orders").await?;
    run.check("region-orders", "orders", &answer);
    decode::<Vec<EsiMarketOrder>>(run, "region-orders", &answer);

    if let Some(own) = orders.iter().find(|o| !o.is_buy_order) {
        let mut url = esi(&format!("markets/{FORGE}/history"))?;
        url.query_pairs_mut()
            .append_pair("type_id", &own.type_id.to_string());
        let answer = esi_request(settings, http.get(url), "market-history").await?;
        run.check("market-history", "history", &answer);
        decode::<Vec<EsiHistoryDay>>(run, "market-history", &answer);
    }

    verify_wealth(settings, run, http, token, me).await?;
    verify_fees(settings, run, http, token, me, &orders).await
}

/// The wallet, the assets and the market prices. Nothing of the answers is printed: not a
/// balance, not an item.
async fn verify_wealth(
    settings: &Settings,
    run: &mut Run<'_>,
    http: &reqwest::Client,
    token: &SecretString,
    me: i64,
) -> Result<()> {
    let esi = |path: &str| {
        settings
            .esi_base_url
            .join(path)
            .map_err(|e| VerifyError(e.to_string()))
    };

    let answer = esi_request(
        settings,
        http.get(esi(&format!("characters/{me}/wallet"))?)
            .bearer_auth(token.expose_secret()),
        "wallet",
    )
    .await?;
    run.check("wallet", "balance", &answer);
    let _: Option<f64> = decode(run, "wallet", &answer);

    let answer = esi_request(
        settings,
        http.get(esi(&format!("characters/{me}/assets"))?)
            .bearer_auth(token.expose_secret()),
        "assets",
    )
    .await?;
    run.check("assets", "items", &answer);
    if let Some(assets) = decode::<Vec<EsiAsset>>(run, "assets", &answer)
        && assets.is_empty()
    {
        run.problem(
            "assets/items: an asset is needed: the verification character has none, so the answer cannot be checked".to_owned(),
        );
    }

    verify_transactions(settings, run, http, token, me).await?;

    // Another character's data is refused whatever the scopes are.
    let stranger = me + 1;
    for (call, path) in [
        ("wallet", "wallet"),
        ("wallet-transactions", "wallet/transactions"),
        ("assets", "assets"),
    ] {
        let answer = esi_request(
            settings,
            http.get(esi(&format!("characters/{stranger}/{path}"))?)
                .bearer_auth(token.expose_secret()),
            call,
        )
        .await?;
        run.check(call, "forbidden", &answer);
    }

    let answer = esi_request(settings, http.get(esi("markets/prices")?), "prices").await?;
    run.check("prices", "list", &answer);
    let _: Option<Vec<EsiPrice>> = decode(run, "prices", &answer);
    Ok(())
}

/// The newest wallet transactions, then one page older than the oldest of them. Notes how many
/// each answer carried and whether `from_id` includes the transaction it names, which the poll's
/// paging depends on. No transaction is printed.
async fn verify_transactions(
    settings: &Settings,
    run: &mut Run<'_>,
    http: &reqwest::Client,
    token: &SecretString,
    me: i64,
) -> Result<()> {
    let url = settings
        .esi_base_url
        .join(&format!("characters/{me}/wallet/transactions"))
        .map_err(|e| VerifyError(e.to_string()))?;
    let answer = esi_request(
        settings,
        http.get(url.clone()).bearer_auth(token.expose_secret()),
        "wallet-transactions",
    )
    .await?;
    run.check("wallet-transactions", "newest", &answer);
    let Some(newest) = decode::<Vec<EsiTransaction>>(run, "wallet-transactions", &answer) else {
        return Ok(());
    };
    if !newest.iter().any(|t| t.is_buy && t.is_personal) {
        run.problem(
            "wallet-transactions/newest: a purchase is needed: the verification character has none in its wallet transactions, so the answer cannot be checked".to_owned(),
        );
    }
    let Some(oldest) = newest.iter().map(|t| t.transaction_id).min() else {
        return Ok(());
    };

    let mut older_url = url;
    older_url
        .query_pairs_mut()
        .append_pair("from_id", &oldest.to_string());
    let answer = esi_request(
        settings,
        http.get(older_url).bearer_auth(token.expose_secret()),
        "wallet-transactions",
    )
    .await?;
    run.check("wallet-transactions", "from-id", &answer);
    if let Some(older) = decode::<Vec<EsiTransaction>>(run, "wallet-transactions", &answer) {
        let inclusive = older.iter().any(|t| t.transaction_id == oldest);
        run.say(format!(
            "note  wallet-transactions: newest {}, from_id page {}, from_id inclusive: {}",
            newest.len(),
            older.len(),
            if inclusive { "yes" } else { "no" },
        ));
    }
    Ok(())
}

/// Where the character is: the variant is whatever it turns out to be (a station, a structure or
/// open space). The location itself is never printed. A 403 for the character's own location means
/// the verification character did not grant the optional location scope.
async fn verify_location(
    settings: &Settings,
    run: &mut Run<'_>,
    http: &reqwest::Client,
    token: &SecretString,
    me: i64,
) -> Result<()> {
    let location = |id: i64| {
        settings
            .esi_base_url
            .join(&format!("characters/{id}/location"))
            .map_err(|e| VerifyError(e.to_string()))
    };

    let answer = esi_request(
        settings,
        http.get(location(me)?).bearer_auth(token.expose_secret()),
        "location",
    )
    .await?;
    if answer.status == 403 {
        run.problem(
            "character-location: the verification character did not grant the location scope, so the answer cannot be checked".to_owned(),
        );
    } else {
        let variant = match &answer.json {
            Some(body) if body.get("station_id").is_some() => "station",
            Some(body) if body.get("structure_id").is_some() => "structure",
            _ => "in-space",
        };
        run.check("character-location", variant, &answer);
    }

    // Another character's location is refused whatever the scopes are.
    let answer = esi_request(
        settings,
        http.get(location(me + 1)?)
            .bearer_auth(token.expose_secret()),
        "location",
    )
    .await?;
    run.check("character-location", "forbidden", &answer);
    Ok(())
}

/// A station well known to be NPC-owned, for characters whose orders are all in structures.
const FALLBACK_STATION_ID: i64 = 60_003_760;

/// The skills, the standings and the station of an NPC order. Neither a skill level nor a standing
/// is printed.
async fn verify_fees(
    settings: &Settings,
    run: &mut Run<'_>,
    http: &reqwest::Client,
    token: &SecretString,
    me: i64,
    orders: &[EsiOrder],
) -> Result<()> {
    let esi = |path: &str| {
        settings
            .esi_base_url
            .join(path)
            .map_err(|e| VerifyError(e.to_string()))
    };

    let answer = esi_request(
        settings,
        http.get(esi(&format!("characters/{me}/skills"))?)
            .bearer_auth(token.expose_secret()),
        "skills",
    )
    .await?;
    run.check("skills", "levels", &answer);
    let _: Option<EsiSkills> = decode(run, "skills", &answer);

    let answer = esi_request(
        settings,
        http.get(esi(&format!("characters/{me}/standings"))?)
            .bearer_auth(token.expose_secret()),
        "standings",
    )
    .await?;
    run.check("standings", "list", &answer);
    if let Some(standings) = decode::<Vec<EsiStanding>>(run, "standings", &answer)
        && !standings
            .iter()
            .any(|s| matches!(s.from_type.as_str(), "npc_corp" | "faction"))
    {
        run.problem(
            "standings/list: a standing is needed: the verification character has no NPC corporation or faction standing, so the answer cannot be checked".to_owned(),
        );
    }

    // Another character's data is refused whatever the scopes are.
    let stranger = me + 1;
    for call in ["skills", "standings"] {
        let answer = esi_request(
            settings,
            http.get(esi(&format!("characters/{stranger}/{call}"))?)
                .bearer_auth(token.expose_secret()),
            call,
        )
        .await?;
        run.check(call, "forbidden", &answer);
    }

    verify_location(settings, run, http, token, me).await?;

    let station_id = orders
        .iter()
        .map(|o| o.location_id)
        .find(|id| *id < STRUCTURE_ID_MIN)
        .unwrap_or(FALLBACK_STATION_ID);
    let answer = esi_request(
        settings,
        http.get(esi(&format!("universe/stations/{station_id}"))?),
        "stations",
    )
    .await?;
    run.check("stations", "station", &answer);
    let _: Option<EsiStation> = decode(run, "stations", &answer);
    Ok(())
}

/// The character id in the token's subject (`CHARACTER:EVE:<id>`).
fn sekondo_character_id(token: &SecretString) -> Result<i64> {
    esi_contract::jwt_payload(token.expose_secret())
        .and_then(|claims| {
            claims["sub"]
                .as_str()
                .and_then(|s| s.strip_prefix("CHARACTER:EVE:"))
                .and_then(|id| id.parse().ok())
        })
        .ok_or_else(|| VerifyError("the access token has no character subject".to_owned()))
}
