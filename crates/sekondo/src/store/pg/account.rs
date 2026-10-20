use async_trait::async_trait;
use sqlx::{PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::store::{
    AccountStore, CharacterRecord, DeleteOutcome, NewCharacter, SealedTokens, StoreResult,
};

pub struct PgAccountStore {
    pool: PgPool,
}

impl PgAccountStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[derive(sqlx::FromRow)]
struct CharacterRow {
    character_id: i64,
    account_id: Uuid,
    name: String,
    owner_hash: String,
    scopes: Vec<String>,
    needs_relogin: bool,
}

impl From<CharacterRow> for CharacterRecord {
    fn from(r: CharacterRow) -> Self {
        Self {
            character_id: r.character_id,
            account_id: r.account_id,
            name: r.name,
            owner_hash: r.owner_hash,
            scopes: r.scopes,
            needs_relogin: r.needs_relogin,
        }
    }
}

const CHARACTER_COLUMNS: &str = "character_id, account_id, name, owner_hash, scopes, needs_relogin";

#[async_trait]
impl AccountStore for PgAccountStore {
    async fn create_account(&self) -> StoreResult<Uuid> {
        Ok(
            sqlx::query_scalar("INSERT INTO accounts DEFAULT VALUES RETURNING id")
                .fetch_one(&self.pool)
                .await?,
        )
    }

    async fn upsert_character(
        &self,
        account_id: Uuid,
        c: &NewCharacter,
        refresh: Vec<u8>,
        access: Vec<u8>,
        access_expires_at: OffsetDateTime,
    ) -> StoreResult<()> {
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
        .bind(access_expires_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn create_account_with_character(
        &self,
        c: &NewCharacter,
        refresh: Vec<u8>,
        access: Vec<u8>,
        access_expires_at: OffsetDateTime,
    ) -> StoreResult<Uuid> {
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
            .bind(access_expires_at)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(account_id)
    }

    async fn find_character(&self, character_id: i64) -> StoreResult<Option<CharacterRecord>> {
        let row: Option<CharacterRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {CHARACTER_COLUMNS} FROM characters WHERE character_id = $1"
        )))
        .bind(character_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(Into::into))
    }

    async fn list_characters(&self, account_id: Uuid) -> StoreResult<Vec<CharacterRecord>> {
        let rows: Vec<CharacterRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {CHARACTER_COLUMNS} FROM characters WHERE account_id = $1 ORDER BY created_at, character_id"
        )))
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn load_tokens(&self, character_id: i64) -> StoreResult<Option<SealedTokens>> {
        type TokenRow = (Vec<u8>, Option<Vec<u8>>, Option<OffsetDateTime>);
        let row: Option<TokenRow> = sqlx::query_as(
            "SELECT refresh_token, access_token, access_expires_at FROM character_tokens WHERE character_id = $1",
        )
        .bind(character_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(
            |(refresh_token, access_token, access_expires_at)| SealedTokens {
                refresh_token,
                access_token,
                access_expires_at,
            },
        ))
    }

    async fn delete_character(&self, character_id: i64) -> StoreResult<DeleteOutcome> {
        let mut tx = self.pool.begin().await?;
        let outcome = delete_in_tx(&mut tx, None, character_id).await?;
        tx.commit().await?;
        Ok(outcome)
    }

    async fn unlink_character(
        &self,
        account_id: Uuid,
        character_id: i64,
    ) -> StoreResult<DeleteOutcome> {
        let mut tx = self.pool.begin().await?;
        let outcome = delete_in_tx(&mut tx, Some(account_id), character_id).await?;
        tx.commit().await?;
        Ok(outcome)
    }

    async fn account_ids(&self) -> StoreResult<Vec<Uuid>> {
        Ok(
            sqlx::query_scalar("SELECT DISTINCT account_id FROM characters")
                .fetch_all(&self.pool)
                .await?,
        )
    }

    async fn account_count(&self) -> StoreResult<i64> {
        Ok(sqlx::query_scalar("SELECT count(*) FROM accounts")
            .fetch_one(&self.pool)
            .await?)
    }

    async fn mark_needs_relogin(&self, character_id: i64) -> StoreResult<()> {
        sqlx::query("UPDATE characters SET needs_relogin = true, updated_at = now() WHERE character_id = $1")
            .bind(character_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn set_scopes(&self, character_id: i64, scopes: &[String]) -> StoreResult<()> {
        sqlx::query(
            "UPDATE characters SET scopes = $2, updated_at = now() WHERE character_id = $1",
        )
        .bind(character_id)
        .bind(scopes)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn save_tokens(
        &self,
        character_id: i64,
        refresh: Vec<u8>,
        access: Vec<u8>,
        access_expires_at: OffsetDateTime,
        scopes: &[String],
    ) -> StoreResult<()> {
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

    async fn seed_access_expiry(
        &self,
        character_id: i64,
        access_expires_at: OffsetDateTime,
    ) -> StoreResult<()> {
        sqlx::query("UPDATE character_tokens SET access_expires_at = $2 WHERE character_id = $1")
            .bind(character_id)
            .bind(access_expires_at)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

async fn delete_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    only_account: Option<Uuid>,
    character_id: i64,
) -> StoreResult<DeleteOutcome> {
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
