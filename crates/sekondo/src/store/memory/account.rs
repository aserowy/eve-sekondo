use async_trait::async_trait;
use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    Shared,
    tables::{CharacterRow, Tables, TokenRow},
};
use crate::store::{
    AccountStore, CharacterRecord, DeleteOutcome, NewCharacter, SealedTokens, StoreError,
    StoreResult,
};

pub struct MemAccountStore {
    pub(super) shared: std::sync::Arc<Shared>,
}

fn record(row: &CharacterRow) -> CharacterRecord {
    CharacterRecord {
        character_id: row.character_id,
        account_id: row.account_id,
        name: row.name.clone(),
        owner_hash: row.owner_hash.clone(),
        scopes: row.scopes.clone(),
        needs_relogin: row.needs_relogin,
    }
}

/// `timestamptz` keeps microseconds.
fn micros(at: OffsetDateTime) -> OffsetDateTime {
    super::micros(at)
}

#[async_trait]
impl AccountStore for MemAccountStore {
    async fn create_account(&self) -> StoreResult<Uuid> {
        let now = self.shared.now();
        let id = Uuid::new_v4();
        self.shared.lock().accounts.insert(id, now);
        Ok(id)
    }

    async fn upsert_character(
        &self,
        account_id: Uuid,
        c: &NewCharacter,
        refresh_token: Vec<u8>,
        access_token: Vec<u8>,
        access_expires_at: OffsetDateTime,
    ) -> StoreResult<()> {
        let now = self.shared.now();
        let mut tables = self.shared.lock();
        match tables.characters.get_mut(&c.character_id) {
            Some(existing) => {
                existing.name = c.name.clone();
                existing.owner_hash = c.owner_hash.clone();
                existing.scopes = c.scopes.clone();
                existing.needs_relogin = false;
                existing.updated_at = now;
            }
            None => {
                if !tables.accounts.contains_key(&account_id) {
                    return Err(StoreError::MissingParent);
                }
                tables.characters.insert(
                    c.character_id,
                    CharacterRow {
                        character_id: c.character_id,
                        account_id,
                        name: c.name.clone(),
                        owner_hash: c.owner_hash.clone(),
                        scopes: c.scopes.clone(),
                        needs_relogin: false,
                        created_at: now,
                        updated_at: now,
                    },
                );
            }
        }
        tables.tokens.insert(
            c.character_id,
            TokenRow {
                refresh_token,
                access_token: Some(access_token),
                access_expires_at: Some(micros(access_expires_at)),
            },
        );
        Ok(())
    }

    async fn create_account_with_character(
        &self,
        c: &NewCharacter,
        refresh_token: Vec<u8>,
        access_token: Vec<u8>,
        access_expires_at: OffsetDateTime,
    ) -> StoreResult<Uuid> {
        let now = self.shared.now();
        let mut tables = self.shared.lock();
        if tables.characters.contains_key(&c.character_id) {
            return Err(StoreError::Backend(
                "duplicate key value violates unique constraint \"characters_pkey\"".to_owned(),
            ));
        }
        let account_id = Uuid::new_v4();
        tables.accounts.insert(account_id, now);
        tables.characters.insert(
            c.character_id,
            CharacterRow {
                character_id: c.character_id,
                account_id,
                name: c.name.clone(),
                owner_hash: c.owner_hash.clone(),
                scopes: c.scopes.clone(),
                needs_relogin: false,
                created_at: now,
                updated_at: now,
            },
        );
        tables.tokens.insert(
            c.character_id,
            TokenRow {
                refresh_token,
                access_token: Some(access_token),
                access_expires_at: Some(micros(access_expires_at)),
            },
        );
        Ok(account_id)
    }

    async fn find_character(&self, character_id: i64) -> StoreResult<Option<CharacterRecord>> {
        Ok(self.shared.lock().characters.get(&character_id).map(record))
    }

    async fn list_characters(&self, account_id: Uuid) -> StoreResult<Vec<CharacterRecord>> {
        let tables = self.shared.lock();
        let mut rows: Vec<&CharacterRow> = tables
            .characters
            .values()
            .filter(|c| c.account_id == account_id)
            .collect();
        rows.sort_by_key(|c| (c.created_at, c.character_id));
        Ok(rows.into_iter().map(record).collect())
    }

    async fn load_tokens(&self, character_id: i64) -> StoreResult<Option<SealedTokens>> {
        Ok(self
            .shared
            .lock()
            .tokens
            .get(&character_id)
            .map(|t| SealedTokens {
                refresh_token: t.refresh_token.clone(),
                access_token: t.access_token.clone(),
                access_expires_at: t.access_expires_at,
            }))
    }

    async fn delete_character(&self, character_id: i64) -> StoreResult<DeleteOutcome> {
        Ok(delete(&mut self.shared.lock(), None, character_id))
    }

    async fn unlink_character(
        &self,
        account_id: Uuid,
        character_id: i64,
    ) -> StoreResult<DeleteOutcome> {
        Ok(delete(
            &mut self.shared.lock(),
            Some(account_id),
            character_id,
        ))
    }

    async fn account_ids(&self) -> StoreResult<Vec<Uuid>> {
        let tables = self.shared.lock();
        let mut ids: Vec<Uuid> = tables.characters.values().map(|c| c.account_id).collect();
        ids.sort_unstable();
        ids.dedup();
        Ok(ids)
    }

    async fn account_count(&self) -> StoreResult<i64> {
        Ok(self.shared.lock().accounts.len() as i64)
    }

    async fn mark_needs_relogin(&self, character_id: i64) -> StoreResult<()> {
        let now = self.shared.now();
        if let Some(c) = self.shared.lock().characters.get_mut(&character_id) {
            c.needs_relogin = true;
            c.updated_at = now;
        }
        Ok(())
    }

    async fn set_scopes(&self, character_id: i64, scopes: &[String]) -> StoreResult<()> {
        let now = self.shared.now();
        if let Some(c) = self.shared.lock().characters.get_mut(&character_id) {
            c.scopes = scopes.to_vec();
            c.updated_at = now;
        }
        Ok(())
    }

    async fn save_tokens(
        &self,
        character_id: i64,
        refresh_token: Vec<u8>,
        access_token: Vec<u8>,
        access_expires_at: OffsetDateTime,
        scopes: &[String],
    ) -> StoreResult<()> {
        let now = self.shared.now();
        let mut tables = self.shared.lock();
        if let Some(tokens) = tables.tokens.get_mut(&character_id) {
            tokens.refresh_token = refresh_token;
            tokens.access_token = Some(access_token);
            tokens.access_expires_at = Some(micros(access_expires_at));
        }
        if let Some(c) = tables.characters.get_mut(&character_id) {
            c.scopes = scopes.to_vec();
            c.updated_at = now;
        }
        Ok(())
    }

    async fn seed_access_expiry(
        &self,
        character_id: i64,
        access_expires_at: OffsetDateTime,
    ) -> StoreResult<()> {
        if let Some(tokens) = self.shared.lock().tokens.get_mut(&character_id) {
            tokens.access_expires_at = Some(micros(access_expires_at));
        }
        Ok(())
    }
}

/// Deletes a character (and what references it), and its account when it was the last one. With
/// `only_account`, only a character of that account.
fn delete(tables: &mut Tables, only_account: Option<Uuid>, character_id: i64) -> DeleteOutcome {
    let none = DeleteOutcome {
        deleted: false,
        account_deleted: false,
    };
    let Some(account) = tables.characters.get(&character_id).map(|c| c.account_id) else {
        return none;
    };
    if only_account.is_some_and(|a| a != account) {
        return none;
    }
    tables.delete_character(character_id);
    let account_deleted = !tables.characters.values().any(|c| c.account_id == account);
    if account_deleted {
        tables.delete_account(account);
    }
    DeleteOutcome {
        deleted: true,
        account_deleted,
    }
}
