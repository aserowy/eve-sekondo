//! Accounts, characters and their encrypted tokens. The store keeps ciphertext only; this wrapper
//! encrypts on the way in and decrypts on the way out.

use std::sync::Arc;

use secrecy::SecretString;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    crypto::{CryptoError, TokenCipher},
    store::{AccountStore, NewCharacter, StoreError},
};

pub use crate::store::{CharacterRecord, DeleteOutcome};

#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error(transparent)]
    Db(#[from] StoreError),
    #[error(transparent)]
    Crypto(#[from] CryptoError),
}

pub struct CharacterUpsert {
    pub character_id: i64,
    pub name: String,
    pub owner_hash: String,
    pub scopes: Vec<String>,
    pub refresh_token: SecretString,
    pub access_token: SecretString,
    pub access_expires_at: OffsetDateTime,
}

impl CharacterUpsert {
    fn identity(&self) -> NewCharacter {
        NewCharacter {
            character_id: self.character_id,
            name: self.name.clone(),
            owner_hash: self.owner_hash.clone(),
            scopes: self.scopes.clone(),
        }
    }
}

pub struct StoredTokens {
    pub refresh_token: SecretString,
    pub access_token: Option<SecretString>,
    pub access_expires_at: Option<OffsetDateTime>,
}

#[derive(Clone)]
pub struct Repo {
    store: Arc<dyn AccountStore>,
    cipher: TokenCipher,
}

impl std::fmt::Debug for Repo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Repo").finish_non_exhaustive()
    }
}

impl Repo {
    pub fn new(store: Arc<dyn AccountStore>, cipher: TokenCipher) -> Self {
        Self { store, cipher }
    }

    pub async fn create_account(&self) -> Result<Uuid, RepoError> {
        Ok(self.store.create_account().await?)
    }

    /// Inserts the character under `account_id`, or refreshes an existing one in place. An existing
    /// character is never moved to another account, and its needs-re-login flag is cleared.
    pub async fn upsert_character(
        &self,
        account_id: Uuid,
        c: &CharacterUpsert,
    ) -> Result<(), RepoError> {
        let refresh = self.cipher.encrypt(c.character_id, &c.refresh_token);
        let access = self.cipher.encrypt(c.character_id, &c.access_token);
        Ok(self
            .store
            .upsert_character(
                account_id,
                &c.identity(),
                refresh,
                access,
                c.access_expires_at,
            )
            .await?)
    }

    pub async fn find_character(
        &self,
        character_id: i64,
    ) -> Result<Option<CharacterRecord>, RepoError> {
        Ok(self.store.find_character(character_id).await?)
    }

    pub async fn list_characters(
        &self,
        account_id: Uuid,
    ) -> Result<Vec<CharacterRecord>, RepoError> {
        Ok(self.store.list_characters(account_id).await?)
    }

    pub async fn load_tokens(&self, character_id: i64) -> Result<Option<StoredTokens>, RepoError> {
        let Some(sealed) = self.store.load_tokens(character_id).await? else {
            return Ok(None);
        };
        Ok(Some(StoredTokens {
            refresh_token: self.cipher.decrypt(character_id, &sealed.refresh_token)?,
            access_token: sealed
                .access_token
                .map(|a| self.cipher.decrypt(character_id, &a))
                .transpose()?,
            access_expires_at: sealed.access_expires_at,
        }))
    }

    /// Deletes a character (tokens and per-character data cascade). Removes the account too when
    /// it was the account's last character.
    pub async fn delete_character(&self, character_id: i64) -> Result<DeleteOutcome, RepoError> {
        Ok(self.store.delete_character(character_id).await?)
    }

    /// Like [`Repo::delete_character`], but only when the character belongs to `account_id`.
    pub async fn unlink_character(
        &self,
        account_id: Uuid,
        character_id: i64,
    ) -> Result<DeleteOutcome, RepoError> {
        Ok(self
            .store
            .unlink_character(account_id, character_id)
            .await?)
    }

    /// Creates an account holding just this character, atomically.
    pub async fn create_account_with_character(
        &self,
        c: &CharacterUpsert,
    ) -> Result<Uuid, RepoError> {
        let refresh = self.cipher.encrypt(c.character_id, &c.refresh_token);
        let access = self.cipher.encrypt(c.character_id, &c.access_token);
        Ok(self
            .store
            .create_account_with_character(&c.identity(), refresh, access, c.access_expires_at)
            .await?)
    }

    pub async fn mark_needs_relogin(&self, character_id: i64) -> Result<(), RepoError> {
        Ok(self.store.mark_needs_relogin(character_id).await?)
    }

    pub async fn set_scopes(&self, character_id: i64, scopes: &[String]) -> Result<(), RepoError> {
        Ok(self.store.set_scopes(character_id, scopes).await?)
    }

    /// Stores freshly refreshed tokens (encrypted) and the scopes they carry.
    pub async fn save_tokens(
        &self,
        character_id: i64,
        refresh_token: &SecretString,
        access_token: &SecretString,
        access_expires_at: OffsetDateTime,
        scopes: &[String],
    ) -> Result<(), RepoError> {
        let refresh = self.cipher.encrypt(character_id, refresh_token);
        let access = self.cipher.encrypt(character_id, access_token);
        Ok(self
            .store
            .save_tokens(character_id, refresh, access, access_expires_at, scopes)
            .await?)
    }
}

#[cfg(feature = "test-hooks")]
impl Repo {
    /// Makes the stored access token look expired at `at`, so the next `access_token` call
    /// refreshes.
    pub async fn expire_access_token(
        &self,
        character_id: i64,
        at: OffsetDateTime,
    ) -> Result<(), RepoError> {
        Ok(self.store.seed_access_expiry(character_id, at).await?)
    }
}
