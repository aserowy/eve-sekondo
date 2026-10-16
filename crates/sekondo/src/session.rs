//! Cookie sessions stored in Postgres, with an idle timeout and an absolute lifetime.

use std::collections::HashMap;

use async_trait::async_trait;
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use tower_sessions::{
    ExpiredDeletion, Expiry, Session, SessionManagerLayer, SessionStore,
    cookie::{SameSite, time::Duration as CookieDuration},
    session::{Id, Record},
    session_store,
};

use crate::app::AppState;

pub const COOKIE_NAME: &str = "__Host-sekondo";
pub const IDLE_TIMEOUT: Duration = Duration::days(7);
pub const ABSOLUTE_LIFETIME: Duration = Duration::days(30);

const STARTED_AT: &str = "started_at";
const LAST_SEEN: &str = "last_seen";

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
impl SessionStore for PgSessionStore {
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
}

#[async_trait]
impl ExpiredDeletion for PgSessionStore {
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

/// `__Host-` makes browsers enforce Secure, Path=/ and no Domain. SameSite=Lax (not Strict) so the
/// cookie is sent on the SSO callback, a top-level cross-site GET.
pub fn layer(store: PgSessionStore) -> SessionManagerLayer<PgSessionStore> {
    SessionManagerLayer::new(store)
        .with_name(COOKIE_NAME)
        .with_secure(true)
        .with_http_only(true)
        .with_same_site(SameSite::Lax)
        .with_path("/")
        .with_expiry(Expiry::OnInactivity(CookieDuration::seconds(
            IDLE_TIMEOUT.whole_seconds(),
        )))
}

/// Marks the session as signed in. Call after rotating the session ID (`Session::cycle_id`).
pub async fn start(
    session: &Session,
    now: OffsetDateTime,
) -> Result<(), tower_sessions::session::Error> {
    session.insert(STARTED_AT, now.unix_timestamp()).await?;
    session.insert(LAST_SEEN, now.unix_timestamp()).await?;
    Ok(())
}

/// Idle for too long, or older than the absolute lifetime (all Unix seconds).
fn lifetime_exceeded(started_at: i64, last_seen: i64, now: i64) -> bool {
    now - last_seen > IDLE_TIMEOUT.whole_seconds()
        || now - started_at > ABSOLUTE_LIFETIME.whole_seconds()
}

/// Whether the stored session still exists, is signed in and is within its lifetime. A read-only
/// check for long-lived connections: unlike [`enforce_lifetime`] it records no activity.
pub async fn is_active(store: &PgSessionStore, id: &Id, now: OffsetDateTime) -> bool {
    let Ok(Some(record)) = store.load(id).await else {
        return false;
    };
    let seconds = |key: &str| record.data.get(key).and_then(serde_json::Value::as_i64);
    if !record.data.contains_key(crate::account::ACCOUNT_ID) {
        return false;
    }
    match (seconds(STARTED_AT), seconds(LAST_SEEN)) {
        (Some(started_at), Some(last_seen)) => {
            !lifetime_exceeded(started_at, last_seen, now.unix_timestamp())
        }
        _ => false,
    }
}

/// Checked on every request: ends sessions idle for 7 days or older than 30 days, otherwise
/// records the activity.
pub async fn enforce_lifetime(
    State(state): State<AppState>,
    session: Session,
    request: Request,
    next: Next,
) -> Response {
    let now = state.clock.now().unix_timestamp();
    let started_at = session.get::<i64>(STARTED_AT).await.ok().flatten();
    let last_seen = session.get::<i64>(LAST_SEEN).await.ok().flatten();
    if let (Some(started_at), Some(last_seen)) = (started_at, last_seen) {
        if lifetime_exceeded(started_at, last_seen, now) {
            let _ = session.flush().await;
        } else {
            let _ = session.insert(LAST_SEEN, now).await;
        }
    }
    next.run(request).await
}
