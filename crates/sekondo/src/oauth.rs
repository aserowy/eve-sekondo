//! The pieces of the OAuth2 authorization code flow with PKCE that the app owns.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

/// Exactly the scopes the app asks for, and requires on every token.
pub const REQUIRED_SCOPES: [&str; 4] = [
    "esi-markets.read_character_orders.v1",
    "esi-universe.read_structures.v1",
    "esi-wallet.read_character_wallet.v1",
    "esi-assets.read_assets.v1",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Intent {
    Login,
    AddCharacter,
}

/// What the session remembers between the redirect to SSO and the callback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingFlow {
    pub state: String,
    pub code_verifier: String,
    pub intent: Intent,
}

fn random_url_safe() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}

pub fn code_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

impl PendingFlow {
    pub fn new(intent: Intent) -> Self {
        Self {
            state: random_url_safe(),
            code_verifier: random_url_safe(),
            intent,
        }
    }

    pub fn authorize_url(&self, sso_base_url: &Url, client_id: &str, redirect_uri: &str) -> Url {
        let mut url = sso_base_url
            .join("v2/oauth/authorize")
            .expect("valid base url");
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("client_id", client_id)
            .append_pair("scope", &REQUIRED_SCOPES.join(" "))
            .append_pair("code_challenge", &code_challenge(&self.code_verifier))
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &self.state);
        url
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn challenge_matches_the_rfc_7636_example() {
        assert_eq!(
            code_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn flows_are_unique_and_verifier_is_long_enough() {
        let (a, b) = (
            PendingFlow::new(Intent::Login),
            PendingFlow::new(Intent::Login),
        );
        assert_ne!(a.state, b.state);
        assert_ne!(a.code_verifier, b.code_verifier);
        assert!((43..=128).contains(&a.code_verifier.len()));
    }
}
