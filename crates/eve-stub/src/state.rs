use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use aws_lc_rs::{
    rand::SystemRandom,
    rsa::{KeyPair, KeySize, PublicKeyComponents},
    signature::KeyPair as _,
    signature::RSA_PKCS1_SHA256,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Serialize;
use serde_json::{Value, json};

use crate::StubError;

pub type Shared = Arc<Mutex<Inner>>;

pub const KEY_ID: &str = "JWT-Signature-Key";
pub const ACCESS_TOKEN_SECONDS: u64 = 1199;

#[derive(Clone, Debug)]
pub struct Character {
    pub id: i64,
    pub name: String,
    pub owner_hash: String,
    /// The scopes this character is willing to grant.
    pub scopes: Vec<String>,
}

pub fn default_characters() -> Vec<Character> {
    let scopes: Vec<String> = [
        "esi-markets.read_character_orders.v1",
        "esi-universe.read_structures.v1",
        "esi-wallet.read_character_wallet.v1",
        "esi-assets.read_assets.v1",
        "esi-skills.read_skills.v1",
        "esi-characters.read_standings.v1",
    ]
    .map(str::to_owned)
    .into();
    [
        (90000001, "Alice Alpha"),
        (90000002, "Bob Bravo"),
        (90000003, "Carol Charlie"),
    ]
    .into_iter()
    .map(|(id, name)| Character {
        id,
        name: name.to_owned(),
        owner_hash: format!("owner-hash-{id}"),
        scopes: scopes.clone(),
    })
    .collect()
}

#[derive(Clone, Debug)]
pub struct StubConfig {
    pub client_id: String,
    pub client_secret: String,
    pub control_token: String,
    pub characters: Vec<Character>,
    /// Accepted for `iss`: with the `https://` prefix (current EVE behaviour) or without (legacy).
    pub issuer_with_scheme: bool,
    /// The URL clients reach the stub under, when that is not `https://localhost:<port>`. Used for `iss`.
    pub base_url: Option<String>,
}

impl StubConfig {
    pub fn new(client_id: &str, client_secret: &str, control_token: &str) -> Self {
        Self {
            client_id: client_id.to_owned(),
            client_secret: client_secret.to_owned(),
            control_token: control_token.to_owned(),
            characters: default_characters(),
            issuer_with_scheme: true,
            base_url: None,
        }
    }
}

pub struct PendingCode {
    pub character_id: i64,
    pub code_challenge: String,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
}

pub struct RefreshRecord {
    pub character_id: i64,
    pub scopes: Vec<String>,
}

/// What the stub remembers about an access token it issued, so ESI routes can authorise it.
pub struct IssuedToken {
    pub character_id: i64,
    pub scopes: Vec<String>,
    pub expires_at: i64,
}

pub const ESI_EXPIRES_SECONDS: u64 = 1200;

/// The next ESI calls answer with this instead of their normal response.
#[derive(Clone, Debug)]
pub struct EsiFailure {
    /// Only calls to this route fail (`orders`, `names`, `structures`, `market`, `wallet`,
    /// `assets`, `prices`, `skills`, `standings`, `stations`); `None`: any route.
    pub route: Option<String>,
    pub remaining: u32,
    pub status: u16,
    pub headers: Vec<(String, String)>,
}

/// What an ESI request told the stub about the client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EsiRequestRecord {
    pub route: &'static str,
    pub user_agent: Option<String>,
    pub compatibility_date: Option<String>,
}

pub const NAMES: [(i64, &str, &str); 8] = [
    (34, "inventory_type", "Tritanium"),
    (35, "inventory_type", "Pyerite"),
    (36, "inventory_type", "Mexallon"),
    (10000002, "region", "The Forge"),
    (10000043, "region", "Domain"),
    (
        60003760,
        "station",
        "Jita IV - Moon 4 - Caldari Navy Assembly Plant",
    ),
    (
        60008494,
        "station",
        "Amarr VIII (Oris) - Emperor Family Academy",
    ),
    (
        60011866,
        "station",
        "Dodixie IX - Moon 20 - Federation Navy Assembly Plant",
    ),
];

pub const STRUCTURES: [(i64, &str); 2] = [
    (1000000000001, "Alpha Keepstar"),
    (1000000000002, "Bravo Fortizar"),
];

pub struct Inner {
    pub config: StubConfig,
    pub issuer: String,
    pub characters: Vec<Character>,
    pub codes: HashMap<String, PendingCode>,
    pub refresh_tokens: HashMap<String, RefreshRecord>,
    pub fail_next_token: u32,
    pub token_calls: u64,
    pub jwks_calls: u64,
    pub issued_tokens: HashMap<String, IssuedToken>,
    /// Fixture orders per character, as ESI's JSON.
    pub orders: HashMap<i64, Vec<Value>>,
    /// Fixture public market orders per region, as ESI's JSON.
    pub market_orders: HashMap<i64, MarketFixture>,
    /// Fixture wallet balance per character.
    pub wallets: HashMap<i64, f64>,
    /// Fixture assets per character, as ESI's JSON.
    pub assets: HashMap<i64, MarketFixture>,
    /// Fixture skill list per character, as ESI's JSON objects.
    pub skills: HashMap<i64, Vec<Value>>,
    /// Fixture standings per character, as ESI's JSON objects.
    pub standings: HashMap<i64, Vec<Value>>,
    /// Owner corporation per NPC station.
    pub station_owners: HashMap<i64, i64>,
    /// Fixture market prices, as ESI's JSON.
    pub prices: Vec<Value>,
    /// Seconds ahead that ESI's `Expires` header points.
    pub esi_expires_secs: u64,
    /// Calls per ESI route (`orders`), counting every request, failed or not.
    pub esi_calls: HashMap<&'static str, u64>,
    /// The IDs of every `/universe/names` request, in order.
    pub names_requests: Vec<Vec<i64>>,
    /// The ID of every `/universe/stations/{id}` request, in order.
    pub station_requests: Vec<i64>,
    pub esi_requests: Vec<EsiRequestRecord>,
    pub structures_denied: HashSet<i64>,
    pub esi_failure: Option<EsiFailure>,
    /// How long the orders route waits before answering.
    pub esi_delay_ms: u64,
    /// Applied to every access token the token endpoint issues (tests make them odd on purpose).
    pub token_options: TokenOptions,
    key_generation: u32,
    pub kid: String,
    signing_key: KeyPair,
    pub jwk_n: String,
    pub jwk_e: String,
}

/// A list of ESI's JSON objects and how many fit on a page: the public market orders of one
/// region, or the assets of one character.
#[derive(Clone, Debug)]
pub struct MarketFixture {
    pub orders: Vec<Value>,
    pub page_size: usize,
}

/// ESI's page size for the region market.
pub const MARKET_PAGE_SIZE: usize = 1000;

/// ESI's page size for the assets of a character.
pub const ASSETS_PAGE_SIZE: usize = 1000;

/// Overrides for issuing deliberately odd tokens in tests.
#[derive(Clone, Debug, Default)]
pub struct TokenOptions {
    /// Seconds until expiry (negative: already expired). Default: the normal 1199 seconds.
    pub expires_in_secs: Option<i64>,
    pub audience: Option<Vec<String>>,
    pub issuer: Option<String>,
    /// The scopes to put in `scp`; default: the character's current scopes.
    pub scopes: Option<Vec<String>>,
    /// Omit the `scp` claim entirely.
    pub omit_scopes: bool,
}

#[derive(Serialize)]
struct Claims<'a> {
    iss: &'a str,
    sub: String,
    aud: Vec<String>,
    azp: &'a str,
    name: &'a str,
    owner: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    scp: Option<serde_json::Value>,
    exp: i64,
    iat: i64,
    jti: String,
    tenant: &'static str,
    region: &'static str,
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs()
}

pub fn random_token() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}

fn generate_key() -> Result<(KeyPair, String, String), StubError> {
    let key = KeyPair::generate(KeySize::Rsa2048).map_err(|_| StubError::KeyGeneration)?;
    let components: PublicKeyComponents<Vec<u8>> = key.public_key().into();
    Ok((
        key,
        URL_SAFE_NO_PAD.encode(components.n),
        URL_SAFE_NO_PAD.encode(components.e),
    ))
}

impl Inner {
    pub fn new(config: StubConfig, base_url: &str) -> Result<Self, StubError> {
        let (key, n, e) = generate_key()?;
        let issuer = if config.issuer_with_scheme {
            base_url.to_owned()
        } else {
            base_url.trim_start_matches("https://").to_owned()
        };
        Ok(Self {
            characters: config.characters.clone(),
            config,
            issuer,
            codes: HashMap::new(),
            refresh_tokens: HashMap::new(),
            fail_next_token: 0,
            token_calls: 0,
            jwks_calls: 0,
            issued_tokens: HashMap::new(),
            orders: default_orders(),
            market_orders: HashMap::new(),
            wallets: default_wallets(),
            assets: default_assets(),
            skills: default_skills(),
            standings: default_standings(),
            station_owners: default_station_owners(),
            prices: default_prices(),
            esi_expires_secs: ESI_EXPIRES_SECONDS,
            esi_calls: HashMap::new(),
            names_requests: Vec::new(),
            station_requests: Vec::new(),
            esi_requests: Vec::new(),
            structures_denied: HashSet::new(),
            esi_failure: None,
            esi_delay_ms: 0,
            token_options: TokenOptions::default(),
            key_generation: 0,
            kid: KEY_ID.to_owned(),
            signing_key: key,
            jwk_n: n,
            jwk_e: e,
        })
    }

    /// Replaces the signing key (and its `kid`), like a real key rotation.
    pub fn rotate_key(&mut self) -> Result<(), StubError> {
        let (key, n, e) = generate_key()?;
        self.key_generation += 1;
        self.kid = format!("rotated-key-{}", self.key_generation);
        self.signing_key = key;
        self.jwk_n = n;
        self.jwk_e = e;
        Ok(())
    }

    /// Replaces the signing key but keeps the `kid`, which is what a constant-kid issuer does.
    pub fn rotate_key_keeping_kid(&mut self) -> Result<(), StubError> {
        let (key, n, e) = generate_key()?;
        self.signing_key = key;
        self.jwk_n = n;
        self.jwk_e = e;
        Ok(())
    }

    pub fn character(&self, id: i64) -> Option<&Character> {
        self.characters.iter().find(|c| c.id == id)
    }

    /// `scopes` overrides the granted scopes (the ones recorded on the refresh token); otherwise
    /// the character's current ones are used.
    pub fn issue_access_token(
        &mut self,
        character_id: i64,
        scopes: Option<&[String]>,
    ) -> Option<String> {
        let options = TokenOptions {
            scopes: scopes.map(<[String]>::to_vec),
            ..TokenOptions::default()
        };
        self.issue_token(character_id, &options)
    }

    pub fn issue_token(&mut self, character_id: i64, options: &TokenOptions) -> Option<String> {
        let character = self.character(character_id)?;
        let scopes = options.scopes.as_ref().unwrap_or(&character.scopes);
        // EVE sends a single scope as a string and several as an array.
        let scp = match scopes.as_slice() {
            _ if options.omit_scopes => None,
            [] => None,
            [one] => Some(serde_json::Value::String(one.clone())),
            many => Some(serde_json::json!(many)),
        };
        let now = now_secs() as i64;
        let expires_at = now
            + options
                .expires_in_secs
                .unwrap_or(ACCESS_TOKEN_SECONDS as i64);
        let granted = scopes.clone();
        let claims = Claims {
            iss: options.issuer.as_deref().unwrap_or(&self.issuer),
            sub: format!("CHARACTER:EVE:{character_id}"),
            aud: options
                .audience
                .clone()
                .unwrap_or_else(|| vec![self.config.client_id.clone(), "EVE Online".to_owned()]),
            azp: &self.config.client_id,
            name: &character.name,
            owner: &character.owner_hash,
            scp,
            exp: expires_at,
            iat: now,
            jti: random_token(),
            tenant: "tranquility",
            region: "world",
        };
        let header = serde_json::json!({"alg": "RS256", "typ": "JWT", "kid": self.kid});
        let b64 = |v: &[u8]| URL_SAFE_NO_PAD.encode(v);
        let signing_input = format!(
            "{}.{}",
            b64(header.to_string().as_bytes()),
            b64(&serde_json::to_vec(&claims).ok()?)
        );
        let mut signature = vec![0u8; self.signing_key.public_modulus_len()];
        self.signing_key
            .sign(
                &RSA_PKCS1_SHA256,
                &SystemRandom::new(),
                signing_input.as_bytes(),
                &mut signature,
            )
            .ok()?;
        let token = format!("{signing_input}.{}", b64(&signature));
        self.issued_tokens.insert(
            token.clone(),
            IssuedToken {
                character_id,
                scopes: granted,
                expires_at,
            },
        );
        Some(token)
    }
}

impl Inner {
    /// Back to the fixture: original characters, no codes or refresh tokens, no pending failures.
    /// The signing key stays, so tokens already in flight keep verifying.
    pub fn reset(&mut self) {
        self.characters = self.config.characters.clone();
        self.codes.clear();
        self.refresh_tokens.clear();
        self.fail_next_token = 0;
        self.token_options = TokenOptions::default();
        self.orders = default_orders();
        self.market_orders.clear();
        self.wallets = default_wallets();
        self.assets = default_assets();
        self.skills = default_skills();
        self.standings = default_standings();
        self.station_owners = default_station_owners();
        self.prices = default_prices();
        self.esi_expires_secs = ESI_EXPIRES_SECONDS;
        self.esi_calls.clear();
        self.names_requests.clear();
        self.station_requests.clear();
        self.esi_requests.clear();
        self.structures_denied.clear();
        self.esi_failure = None;
        self.esi_delay_ms = 0;
    }

    pub fn count_esi_call(&mut self, route: &'static str, headers: &axum::http::HeaderMap) {
        *self.esi_calls.entry(route).or_default() += 1;
        let text = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        self.esi_requests.push(EsiRequestRecord {
            route,
            user_agent: text("user-agent"),
            compatibility_date: text("x-compatibility-date"),
        });
    }
}

fn rfc3339(epoch_secs: i64) -> String {
    time::OffsetDateTime::from_unix_timestamp(epoch_secs)
        .expect("timestamp in range")
        .format(&time::format_description::well_known::Rfc3339)
        .expect("format")
}

#[allow(clippy::too_many_arguments)]
fn order(
    id: i64,
    type_id: i64,
    location_id: i64,
    buy: bool,
    price: f64,
    remain: i64,
    total: i64,
    escrow: Option<f64>,
) -> Value {
    let mut order = json!({
        "order_id": id, "type_id": type_id, "region_id": 10000002, "location_id": location_id,
        "is_corporation": false, "price": price,
        "volume_remain": remain, "volume_total": total, "range": "station",
        "issued": rfc3339(now_secs() as i64 - 5 * 86_400), "duration": 90,
    });
    // Like ESI: the flag, escrow and min_volume only come with buy orders.
    if buy {
        order["is_buy_order"] = json!(true);
    }
    if let Some(escrow) = escrow {
        order["escrow"] = json!(escrow);
    }
    if buy {
        order["min_volume"] = json!(1);
    }
    order
}

/// Alice: two sell orders and a buy order in a structure. Bob: a sell order for the same item in
/// the same station as one of Alice's. Carol has none.
fn default_orders() -> HashMap<i64, Vec<Value>> {
    HashMap::from([
        (
            90000001,
            vec![
                order(5001, 34, 60003760, false, 1500000.0, 10, 20, None),
                order(5002, 35, 60003760, false, 950.5, 100, 100, None),
                order(
                    5003,
                    36,
                    1000000000001,
                    true,
                    250000.0,
                    15,
                    20,
                    Some(2500000.0),
                ),
            ],
        ),
        (
            90000002,
            vec![order(6001, 34, 60003760, false, 1600000.0, 5, 5, None)],
        ),
        (90000003, vec![]),
    ])
}

fn default_wallets() -> HashMap<i64, f64> {
    HashMap::from([(90000001, 2_500_000.0), (90000002, 100.5), (90000003, 0.0)])
}

fn asset(item_id: i64, type_id: i64, location_id: i64, quantity: i64) -> Value {
    json!({
        "item_id": item_id, "type_id": type_id, "location_id": location_id,
        "location_type": "station", "location_flag": "Hangar", "quantity": quantity,
        "is_singleton": false,
    })
}

/// Alice: two stacks in Jita. Bob: one stack in Amarr. Carol has none.
fn default_assets() -> HashMap<i64, MarketFixture> {
    let fixture = |assets: Vec<Value>| MarketFixture {
        orders: assets,
        page_size: ASSETS_PAGE_SIZE,
    };
    HashMap::from([
        (
            90000001,
            fixture(vec![
                asset(2001, 34, 60003760, 1000),
                asset(2002, 35, 60003760, 50),
            ]),
        ),
        (90000002, fixture(vec![asset(3001, 34, 60008494, 20)])),
        (90000003, fixture(vec![])),
    ])
}

/// Tritanium and Pyerite with an average; Mexallon with only an adjusted price.
fn default_prices() -> Vec<Value> {
    vec![
        json!({"type_id": 34, "average_price": 5.5, "adjusted_price": 5.1}),
        json!({"type_id": 35, "average_price": 10.25, "adjusted_price": 9.9}),
        json!({"type_id": 36, "adjusted_price": 70.0}),
    ]
}

fn skill(skill_id: i64, active: i64, trained: i64) -> Value {
    json!({
        "skill_id": skill_id, "active_skill_level": active, "trained_skill_level": trained,
        "skillpoints_in_skill": 256000,
    })
}

/// Accounting (16622) and Broker Relations (3446), with Connections (3359) as one that is not
/// read. Alice: Accounting trained to 5 but active 4, Broker Relations 5. Bob: Accounting 3,
/// Broker Relations 2. Carol: nothing.
fn default_skills() -> HashMap<i64, Vec<Value>> {
    HashMap::from([
        (
            90000001,
            vec![skill(16622, 4, 5), skill(3446, 5, 5), skill(3359, 3, 3)],
        ),
        (90000002, vec![skill(16622, 3, 3), skill(3446, 2, 2)]),
        (90000003, vec![]),
    ])
}

fn standing(from_id: i64, from_type: &str, standing: f64) -> Value {
    json!({"from_id": from_id, "from_type": from_type, "standing": standing})
}

/// Alice: Caldari Navy (1000035) 1.5, Caldari State (500001) 2.0 and an agent. The others none.
fn default_standings() -> HashMap<i64, Vec<Value>> {
    HashMap::from([
        (
            90000001,
            vec![
                standing(3009841, "agent", 3.2),
                standing(1000035, "npc_corp", 1.5),
                standing(500001, "faction", 2.0),
            ],
        ),
        (90000002, vec![]),
        (90000003, vec![]),
    ])
}

/// The owners of the stations in `NAMES`, as the static data export lists them.
fn default_station_owners() -> HashMap<i64, i64> {
    HashMap::from([
        (60003760, 1000035),
        (60008494, 1000086),
        (60011866, 1000120),
    ])
}
