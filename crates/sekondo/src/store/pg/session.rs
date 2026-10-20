use std::collections::HashMap;

use async_trait::async_trait;
use sqlx::PgPool;
use time::OffsetDateTime;
use tower_sessions::{
    session::{Id, Record},
    session_store,
};

use crate::store::SessionData;

#[derive(Clone, Debug)]
pub struct PgSessionStore {
    pool: PgPool,
}

impl PgSessionStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn backend(e: sqlx::Error) -> session_store::Error {
    session_store::Error::Backend(e.to_string())
}

#[async_trait]
impl SessionData for PgSessionStore {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        let mut tx = self.pool.begin().await.map_err(backend)?;
        loop {
            let exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions WHERE id = $1)")
                    .bind(record.id.to_string())
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(backend)?;
            if !exists {
                break;
            }
            record.id = Id::default();
        }
        upsert(&mut *tx, record).await?;
        tx.commit().await.map_err(backend)
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        upsert(&self.pool, record).await
    }

    async fn load(&self, id: &Id) -> session_store::Result<Option<Record>> {
        let row: Option<(serde_json::Value, OffsetDateTime)> = sqlx::query_as(
            "SELECT data, expiry_date FROM sessions WHERE id = $1 AND expiry_date > now()",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(backend)?;
        row.map(|(data, expiry_date)| {
            let data: HashMap<String, serde_json::Value> = serde_json::from_value(data)
                .map_err(|e| session_store::Error::Decode(e.to_string()))?;
            Ok(Record {
                id: *id,
                data,
                expiry_date,
            })
        })
        .transpose()
    }

    async fn delete(&self, id: &Id) -> session_store::Result<()> {
        sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(())
    }

    async fn delete_expired(&self) -> session_store::Result<()> {
        sqlx::query("DELETE FROM sessions WHERE expiry_date < now()")
            .execute(&self.pool)
            .await
            .map_err(backend)?;
        Ok(())
    }
}

async fn upsert<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    record: &Record,
) -> session_store::Result<()> {
    let data = serde_json::to_value(&record.data)
        .map_err(|e| session_store::Error::Encode(e.to_string()))?;
    sqlx::query(
        "INSERT INTO sessions (id, data, expiry_date) VALUES ($1, $2, $3)
         ON CONFLICT (id) DO UPDATE SET data = excluded.data, expiry_date = excluded.expiry_date",
    )
    .bind(record.id.to_string())
    .bind(data)
    .bind(record.expiry_date)
    .execute(executor)
    .await
    .map_err(backend)?;
    Ok(())
}
