//! Talking to EVE SSO: JWKS cache and access-token validation.

use std::{
    collections::HashMap,
    sync::RwLock,
    time::{Duration, Instant},
};

use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use secrecy::SecretString;
use serde::Deserialize;
use time::OffsetDateTime;
use url::Url;

use crate::oauth::REQUIRED_SCOPES;

const EVE_AUDIENCE: &str = "EVE Online";
const SUBJECT_PREFIX: &str = "CHARACTER:EVE:";

#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("token is malformed: {0}")]
    Malformed(String),
    #[error("token is signed with an unknown key")]
    UnknownKey,
    #[error("could not fetch the SSO signing keys: {0}")]
    Jwks(String),
    #[error("token failed validation: {0}")]
    Invalid(jsonwebtoken::errors::Error),
    #[error("token has no valid character subject")]
    BadSubject,
    #[error("token is not meant for the EVE API")]
    WrongAudience,
}

/// What a valid access token tells us about the character.
#[derive(Debug, Clone)]
pub struct VerifiedToken {
    pub character_id: i64,
    pub name: String,
    pub owner_hash: String,
    pub scopes: Vec<String>,
    pub expires_at: OffsetDateTime,
}

impl VerifiedToken {
    /// Required scopes the token does not include.
    pub fn missing_scopes(&self) -> Vec<&'static str> {
        REQUIRED_SCOPES
            .iter()
            .copied()
            .filter(|required| !self.scopes.iter().any(|s| s == required))
            .collect()
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    fn into_vec(self) -> Vec<String> {
        match self {
            Self::One(s) => s.split_whitespace().map(str::to_owned).collect(),
            Self::Many(v) => v,
        }
    }
}

#[derive(Deserialize)]
struct Claims {
    sub: String,
    name: String,
    owner: String,
    exp: i64,
    aud: OneOrMany,
    #[serde(default)]
    scp: Option<OneOrMany>,
}

#[derive(Deserialize)]
struct JwkSet {
    keys: Vec<RawJwk>,
}

#[derive(Deserialize)]
struct RawJwk {
    kty: String,
    kid: Option<String>,
    n: Option<String>,
    e: Option<String>,
}

#[derive(Default)]
struct JwksCache {
    keys: HashMap<String, DecodingKey>,
    fetched_at: Option<Instant>,
}

pub struct SsoClient {
    pub(crate) http: reqwest::Client,
    pub(crate) base_url: Url,
    pub(crate) client_id: String,
    pub(crate) client_secret: SecretString,
    jwks: RwLock<JwksCache>,
    min_refetch: Duration,
}

impl std::fmt::Debug for SsoClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SsoClient")
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

impl SsoClient {
    /// `min_refetch` limits how often an unknown `kid` may trigger a JWKS fetch, so forged tokens
    /// cannot make the app hammer SSO.
    pub fn new(
        http: reqwest::Client,
        base_url: Url,
        client_id: String,
        client_secret: SecretString,
        min_refetch: Duration,
    ) -> Self {
        Self {
            http,
            base_url,
            client_id,
            client_secret,
            jwks: RwLock::new(JwksCache::default()),
            min_refetch,
        }
    }

    /// The issuer EVE has used in both forms: with and without the `https://` prefix.
    fn accepted_issuers(&self) -> [String; 2] {
        [
            self.base_url.origin().ascii_serialization(),
            self.base_url.authority().to_owned(),
        ]
    }

    fn cached_key(&self, kid: &str) -> Option<DecodingKey> {
        self.jwks.read().unwrap().keys.get(kid).cloned()
    }

    /// Fetches the signing keys unless that happened less than `min_refetch` ago. Returns whether
    /// a fetch was made.
    async fn refetch_jwks(&self) -> Result<bool, TokenError> {
        {
            let cache = self.jwks.read().unwrap();
            if cache
                .fetched_at
                .is_some_and(|t| t.elapsed() < self.min_refetch)
            {
                return Ok(false);
            }
        }
        let url = self.base_url.join("oauth/jwks").expect("valid base url");
        let set: JwkSet = self
            .http
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| TokenError::Jwks(e.to_string()))?
            .json()
            .await
            .map_err(|e| TokenError::Jwks(e.to_string()))?;

        let mut keys = HashMap::new();
        for jwk in set.keys.into_iter().filter(|k| k.kty == "RSA") {
            let (Some(kid), Some(n), Some(e)) = (jwk.kid, jwk.n, jwk.e) else {
                continue;
            };
            if let Ok(key) = DecodingKey::from_rsa_components(&n, &e) {
                keys.insert(kid, key);
            }
        }
        *self.jwks.write().unwrap() = JwksCache {
            keys,
            fetched_at: Some(Instant::now()),
        };
        Ok(true)
    }

    /// The key for `kid`, and whether it was fetched during this call (so it cannot be stale).
    async fn key_for(&self, kid: &str) -> Result<(DecodingKey, bool), TokenError> {
        if let Some(key) = self.cached_key(kid) {
            return Ok((key, false));
        }
        let fetched = self.refetch_jwks().await?;
        let key = self.cached_key(kid).ok_or(TokenError::UnknownKey)?;
        Ok((key, fetched))
    }

    fn verify(&self, token: &str, key: &DecodingKey) -> Result<VerifiedToken, TokenError> {
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&self.accepted_issuers());
        validation.set_audience(&[self.client_id.as_str()]);
        validation.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        validation.leeway = 10;
        let claims = jsonwebtoken::decode::<Claims>(token, key, &validation)
            .map_err(TokenError::Invalid)?
            .claims;

        if !claims.aud.into_vec().iter().any(|a| a == EVE_AUDIENCE) {
            return Err(TokenError::WrongAudience);
        }
        let character_id = claims
            .sub
            .strip_prefix(SUBJECT_PREFIX)
            .and_then(|id| id.parse().ok())
            .ok_or(TokenError::BadSubject)?;
        let expires_at =
            OffsetDateTime::from_unix_timestamp(claims.exp).map_err(|_| TokenError::BadSubject)?;
        Ok(VerifiedToken {
            character_id,
            name: claims.name,
            owner_hash: claims.owner,
            scopes: claims.scp.map(OneOrMany::into_vec).unwrap_or_default(),
            expires_at,
        })
    }

    /// Verifies signature, issuer, audience and expiry, and extracts the character.
    ///
    /// A bad signature under a cached key may just mean the issuer rotated its key and kept the
    /// `kid`, so that case refetches the keys (subject to the throttle) and tries once more.
    pub async fn validate(&self, token: &str) -> Result<VerifiedToken, TokenError> {
        let header =
            jsonwebtoken::decode_header(token).map_err(|e| TokenError::Malformed(e.to_string()))?;
        let kid = header
            .kid
            .ok_or_else(|| TokenError::Malformed("no kid".to_owned()))?;
        let (key, fresh) = self.key_for(&kid).await?;

        match self.verify(token, &key) {
            Err(TokenError::Invalid(e))
                if !fresh
                    && matches!(e.kind(), jsonwebtoken::errors::ErrorKind::InvalidSignature) =>
            {
                if !self.refetch_jwks().await? {
                    return Err(TokenError::Invalid(e));
                }
                let key = self.cached_key(&kid).ok_or(TokenError::UnknownKey)?;
                self.verify(token, &key)
            }
            result => result,
        }
    }
}

/// Tokens returned by the SSO token endpoint.
pub struct TokenSet {
    pub access_token: SecretString,
    /// SSO may omit it on refresh, in which case the old one stays valid.
    pub refresh_token: Option<SecretString>,
}

impl std::fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TokenSet(<redacted>)")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExchangeError {
    #[error("SSO rejected the request ({0})")]
    Rejected(reqwest::StatusCode),
    #[error("SSO is unavailable: {0}")]
    Unavailable(String),
}

impl ExchangeError {
    /// HTTP 400/401 mean the grant or the client is not valid (for a refresh: the refresh token is
    /// dead). Everything else (network errors, 5xx, unreadable answers) may be temporary.
    pub fn is_rejection(&self) -> bool {
        matches!(self, Self::Rejected(_))
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
}

/// Reads the body of a successful token answer.
pub fn decode_token_body(body: &str) -> Result<TokenSet, ExchangeError> {
    let body: TokenResponse =
        serde_json::from_str(body).map_err(|e| ExchangeError::Unavailable(e.to_string()))?;
    Ok(TokenSet {
        access_token: SecretString::from(body.access_token),
        refresh_token: body.refresh_token.map(SecretString::from),
    })
}

impl SsoClient {
    /// Sends a token request and returns the answer as it came, before it is interpreted.
    async fn send_token_request(
        &self,
        form: &[(&str, &str)],
    ) -> Result<reqwest::Response, ExchangeError> {
        use secrecy::ExposeSecret;
        let url = self
            .base_url
            .join("v2/oauth/token")
            .expect("valid base url");
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form)
            .finish();
        self.http
            .post(url)
            .basic_auth(&self.client_id, Some(self.client_secret.expose_secret()))
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(body)
            .send()
            .await
            .map_err(|e| ExchangeError::Unavailable(e.to_string()))
    }

    async fn token_request(&self, form: &[(&str, &str)]) -> Result<TokenSet, ExchangeError> {
        let res = self.send_token_request(form).await?;
        let status = res.status();
        if matches!(status.as_u16(), 400 | 401) {
            return Err(ExchangeError::Rejected(status));
        }
        if !status.is_success() {
            return Err(ExchangeError::Unavailable(format!("HTTP {status}")));
        }
        let body = res
            .text()
            .await
            .map_err(|e| ExchangeError::Unavailable(e.to_string()))?;
        decode_token_body(&body)
    }

    /// The code exchange's answer as it came, for checking it against a contract.
    pub async fn exchange_code_response(
        &self,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
    ) -> Result<reqwest::Response, ExchangeError> {
        self.send_token_request(&code_exchange_form(code, code_verifier, redirect_uri))
            .await
    }

    /// The refresh's answer as it came, for checking it against a contract.
    pub async fn refresh_response(
        &self,
        refresh_token: &SecretString,
    ) -> Result<reqwest::Response, ExchangeError> {
        use secrecy::ExposeSecret;
        self.send_token_request(&refresh_form(refresh_token.expose_secret()))
            .await
    }

    pub async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
    ) -> Result<TokenSet, ExchangeError> {
        self.token_request(&code_exchange_form(code, code_verifier, redirect_uri))
            .await
    }

    pub async fn refresh(&self, refresh_token: &SecretString) -> Result<TokenSet, ExchangeError> {
        use secrecy::ExposeSecret;
        self.token_request(&refresh_form(refresh_token.expose_secret()))
            .await
    }
}

fn code_exchange_form<'a>(
    code: &'a str,
    code_verifier: &'a str,
    redirect_uri: &'a str,
) -> [(&'static str, &'a str); 4] {
    [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("code_verifier", code_verifier),
        ("redirect_uri", redirect_uri),
    ]
}

fn refresh_form(refresh_token: &str) -> [(&'static str, &str); 2] {
    [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
    ]
}
