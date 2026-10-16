use std::{
    collections::HashMap,
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

pub struct Inner {
    pub config: StubConfig,
    pub issuer: String,
    pub characters: Vec<Character>,
    pub codes: HashMap<String, PendingCode>,
    pub refresh_tokens: HashMap<String, RefreshRecord>,
    pub fail_next_token: u32,
    pub token_calls: u64,
    pub jwks_calls: u64,
    /// Applied to every access token the token endpoint issues (tests make them odd on purpose).
    pub token_options: TokenOptions,
    key_generation: u32,
    pub kid: String,
    signing_key: KeyPair,
    pub jwk_n: String,
    pub jwk_e: String,
}

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
        &self,
        character_id: i64,
        scopes: Option<&[String]>,
    ) -> Option<String> {
        let options = TokenOptions {
            scopes: scopes.map(<[String]>::to_vec),
            ..TokenOptions::default()
        };
        self.issue_token(character_id, &options)
    }

    pub fn issue_token(&self, character_id: i64, options: &TokenOptions) -> Option<String> {
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
            exp: now
                + options
                    .expires_in_secs
                    .unwrap_or(ACCESS_TOKEN_SECONDS as i64),
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
        Some(format!("{signing_input}.{}", b64(&signature)))
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
    }
}
