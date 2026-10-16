//! Valid ESI access tokens on demand, and the needs-re-login state.

use std::{collections::HashMap, sync::Arc};

use secrecy::{ExposeSecret, SecretString};
use time::Duration;
use tokio::sync::Mutex;

use crate::{
    clock::Clock,
    repo::{Repo, RepoError},
    sso::{SsoClient, TokenError as ValidationError},
};

/// Refresh when less than this is left on the access token.
const REFRESH_MARGIN: Duration = Duration::seconds(60);

#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("no such character")]
    UnknownCharacter,
    /// The user has to log in with this character again. SSO is not contacted while this holds.
    #[error("the character needs to log in again")]
    NeedsRelogin,
    /// Network or SSO server trouble: nothing was changed, retry later.
    #[error("temporary failure: {0}")]
    Temporary(String),
}

impl From<RepoError> for TokenError {
    fn from(e: RepoError) -> Self {
        Self::Temporary(e.to_string())
    }
}

pub struct TokenService {
    repo: Repo,
    sso: Arc<SsoClient>,
    clock: Arc<dyn Clock>,
    /// One lock per character, so two refreshes never race on a rotating refresh token.
    locks: std::sync::Mutex<HashMap<i64, Arc<Mutex<()>>>>,
}

impl TokenService {
    pub fn new(repo: Repo, sso: Arc<SsoClient>, clock: Arc<dyn Clock>) -> Self {
        Self {
            repo,
            sso,
            clock,
            locks: std::sync::Mutex::new(HashMap::new()),
        }
    }

    fn lock_for(&self, character_id: i64) -> Arc<Mutex<()>> {
        self.locks
            .lock()
            .unwrap()
            .entry(character_id)
            .or_default()
            .clone()
    }

    pub async fn access_token(&self, character_id: i64) -> Result<SecretString, TokenError> {
        let lock = self.lock_for(character_id);
        let _guard = lock.lock().await;

        // Read inside the lock: a caller that waited sees the token the previous one stored.
        let character = self
            .repo
            .find_character(character_id)
            .await?
            .ok_or(TokenError::UnknownCharacter)?;
        if character.needs_relogin {
            return Err(TokenError::NeedsRelogin);
        }

        let stored = match self.repo.load_tokens(character_id).await {
            Ok(Some(tokens)) => tokens,
            Ok(None) => return self.mark(character_id).await,
            // A lost or changed encryption key: same outcome as a rejected refresh token.
            Err(RepoError::Crypto(e)) => {
                tracing::warn!(character_id, "stored tokens cannot be decrypted: {e}");
                return self.mark(character_id).await;
            }
            Err(e) => return Err(e.into()),
        };

        if let (Some(access), Some(expires_at)) = (&stored.access_token, stored.access_expires_at)
            && expires_at - self.clock.now() > REFRESH_MARGIN
        {
            return Ok(access.clone());
        }

        let refreshed = match self.sso.refresh(&stored.refresh_token).await {
            Ok(tokens) => tokens,
            Err(e) if e.is_rejection() => {
                tracing::info!(character_id, "refresh token rejected: {e}");
                return self.mark(character_id).await;
            }
            Err(e) => return Err(TokenError::Temporary(e.to_string())),
        };

        let verified = self
            .sso
            .validate(refreshed.access_token.expose_secret())
            .await
            .map_err(|e: ValidationError| TokenError::Temporary(e.to_string()))?;

        // The scope check runs on every validation, so a refresh that came back with fewer
        // scopes marks the character too.
        if !verified.missing_scopes().is_empty() {
            tracing::info!(character_id, missing = ?verified.missing_scopes(), "granted scopes are incomplete");
            self.repo.set_scopes(character_id, &verified.scopes).await?;
            return self.mark(character_id).await;
        }

        let refresh_token = refreshed.refresh_token.unwrap_or(stored.refresh_token);
        self.repo
            .save_tokens(
                character_id,
                &refresh_token,
                &refreshed.access_token,
                verified.expires_at,
                &verified.scopes,
            )
            .await?;
        Ok(refreshed.access_token)
    }

    async fn mark(&self, character_id: i64) -> Result<SecretString, TokenError> {
        self.repo.mark_needs_relogin(character_id).await?;
        Err(TokenError::NeedsRelogin)
    }
}
