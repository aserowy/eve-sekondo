//! Accounts, characters and their encrypted tokens.

use secrecy::SecretString;
use sqlx::{PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::crypto::{CryptoError, TokenCipher};

#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Crypto(#[from] CryptoError),
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CharacterRecord {
    pub character_id: i64,
    pub account_id: Uuid,
    pub name: String,
    pub owner_hash: String,
    pub scopes: Vec<String>,
    pub needs_relogin: bool,
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

pub struct StoredTokens {
    pub refresh_token: SecretString,
    pub access_token: Option<SecretString>,
    pub access_expires_at: Option<OffsetDateTime>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DeleteOutcome {
    pub deleted: bool,
    pub account_deleted: bool,
}

#[derive(Clone, Debug)]
pub struct Repo {
    pool: PgPool,
    cipher: TokenCipher,
}

const CHARACTER_COLUMNS: &str = "character_id, account_id, name, owner_hash, scopes, needs_relogin";

impl Repo {
    pub fn new(pool: PgPool, cipher: TokenCipher) -> Self {
        Self { pool, cipher }
    }

    pub async fn create_account(&self) -> Result<Uuid, RepoError> {
        Ok(
            sqlx::query_scalar("INSERT INTO accounts DEFAULT VALUES RETURNING id")
                .fetch_one(&self.pool)
                .await?,
        )
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
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO characters (character_id, account_id, name, owner_hash, scopes)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (character_id) DO UPDATE SET
                 name = excluded.name,
                 owner_hash = excluded.owner_hash,
                 scopes = excluded.scopes,
                 needs_relogin = false,
                 updated_at = now()",
        )
        .bind(c.character_id)
        .bind(account_id)
        .bind(&c.name)
        .bind(&c.owner_hash)
        .bind(&c.scopes)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO character_tokens (character_id, refresh_token, access_token, access_expires_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (character_id) DO UPDATE SET
                 refresh_token = excluded.refresh_token,
                 access_token = excluded.access_token,
                 access_expires_at = excluded.access_expires_at,
                 updated_at = now()",
        )
        .bind(c.character_id)
        .bind(refresh)
        .bind(access)
        .bind(c.access_expires_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn find_character(
        &self,
        character_id: i64,
    ) -> Result<Option<CharacterRecord>, RepoError> {
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {CHARACTER_COLUMNS} FROM characters WHERE character_id = $1"
        )))
        .bind(character_id)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn list_characters(
        &self,
        account_id: Uuid,
    ) -> Result<Vec<CharacterRecord>, RepoError> {
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {CHARACTER_COLUMNS} FROM characters WHERE account_id = $1 ORDER BY created_at, character_id"
        )))
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn load_tokens(&self, character_id: i64) -> Result<Option<StoredTokens>, RepoError> {
        type TokenRow = (Vec<u8>, Option<Vec<u8>>, Option<OffsetDateTime>);
        let row: Option<TokenRow> = sqlx::query_as(
            "SELECT refresh_token, access_token, access_expires_at FROM character_tokens WHERE character_id = $1",
        )
        .bind(character_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|(refresh, access, access_expires_at)| {
            Ok(StoredTokens {
                refresh_token: self.cipher.decrypt(character_id, &refresh)?,
                access_token: access
                    .map(|a| self.cipher.decrypt(character_id, &a))
                    .transpose()?,
                access_expires_at,
            })
        })
        .transpose()
    }

    /// Deletes a character (tokens and per-character data cascade). Removes the account too when
    /// it was the account's last character.
    pub async fn delete_character(&self, character_id: i64) -> Result<DeleteOutcome, RepoError> {
        let mut tx = self.pool.begin().await?;
        let outcome = delete_in_tx(&mut tx, None, character_id).await?;
        tx.commit().await?;
        Ok(outcome)
    }

    /// Like [`Repo::delete_character`], but only when the character belongs to `account_id`.
    pub async fn unlink_character(
        &self,
        account_id: Uuid,
        character_id: i64,
    ) -> Result<DeleteOutcome, RepoError> {
        let mut tx = self.pool.begin().await?;
        let outcome = delete_in_tx(&mut tx, Some(account_id), character_id).await?;
        tx.commit().await?;
        Ok(outcome)
    }
}

async fn delete_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    only_account: Option<Uuid>,
    character_id: i64,
) -> Result<DeleteOutcome, RepoError> {
    let account: Option<Uuid> =
        sqlx::query_scalar("SELECT account_id FROM characters WHERE character_id = $1 FOR UPDATE")
            .bind(character_id)
            .fetch_optional(&mut **tx)
            .await?;
    let none = DeleteOutcome {
        deleted: false,
        account_deleted: false,
    };
    let Some(account) = account else {
        return Ok(none);
    };
    if only_account.is_some_and(|a| a != account) {
        return Ok(none);
    }

    sqlx::query("DELETE FROM characters WHERE character_id = $1")
        .bind(character_id)
        .execute(&mut **tx)
        .await?;
    let remaining: i64 =
        sqlx::query_scalar("SELECT count(*) FROM characters WHERE account_id = $1")
            .bind(account)
            .fetch_one(&mut **tx)
            .await?;
    let account_deleted = remaining == 0;
    if account_deleted {
        sqlx::query("DELETE FROM accounts WHERE id = $1")
            .bind(account)
            .execute(&mut **tx)
            .await?;
    }
    Ok(DeleteOutcome {
        deleted: true,
        account_deleted,
    })
}

impl Repo {
    /// Creates an account holding just this character, atomically.
    pub async fn create_account_with_character(
        &self,
        c: &CharacterUpsert,
    ) -> Result<Uuid, RepoError> {
        let refresh = self.cipher.encrypt(c.character_id, &c.refresh_token);
        let access = self.cipher.encrypt(c.character_id, &c.access_token);
        let mut tx = self.pool.begin().await?;
        let account_id: Uuid =
            sqlx::query_scalar("INSERT INTO accounts DEFAULT VALUES RETURNING id")
                .fetch_one(&mut *tx)
                .await?;
        sqlx::query("INSERT INTO characters (character_id, account_id, name, owner_hash, scopes) VALUES ($1, $2, $3, $4, $5)")
            .bind(c.character_id)
            .bind(account_id)
            .bind(&c.name)
            .bind(&c.owner_hash)
            .bind(&c.scopes)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO character_tokens (character_id, refresh_token, access_token, access_expires_at) VALUES ($1, $2, $3, $4)")
            .bind(c.character_id)
            .bind(refresh)
            .bind(access)
            .bind(c.access_expires_at)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(account_id)
    }
}

impl Repo {
    pub async fn mark_needs_relogin(&self, character_id: i64) -> Result<(), RepoError> {
        sqlx::query("UPDATE characters SET needs_relogin = true, updated_at = now() WHERE character_id = $1")
            .bind(character_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_scopes(&self, character_id: i64, scopes: &[String]) -> Result<(), RepoError> {
        sqlx::query(
            "UPDATE characters SET scopes = $2, updated_at = now() WHERE character_id = $1",
        )
        .bind(character_id)
        .bind(scopes)
        .execute(&self.pool)
        .await?;
        Ok(())
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
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE character_tokens SET refresh_token = $2, access_token = $3, access_expires_at = $4, updated_at = now()
             WHERE character_id = $1",
        )
        .bind(character_id)
        .bind(refresh)
        .bind(access)
        .bind(access_expires_at)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE characters SET scopes = $2, updated_at = now() WHERE character_id = $1",
        )
        .bind(character_id)
        .bind(scopes)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(feature = "test-hooks")]
impl Repo {
    /// Makes the stored access token look expired, so the next `access_token` call refreshes.
    pub async fn expire_access_token(&self, character_id: i64) -> Result<(), RepoError> {
        sqlx::query("UPDATE character_tokens SET access_expires_at = now() - interval '1 hour' WHERE character_id = $1")
            .bind(character_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
