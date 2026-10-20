//! Cookie sessions, with an idle timeout and an absolute lifetime.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use time::{Duration, OffsetDateTime};
use tower_sessions::{
    ExpiredDeletion, Expiry, Session, SessionManagerLayer, SessionStore,
    cookie::{SameSite, time::Duration as CookieDuration},
    session::{Id, Record},
    session_store,
};

use crate::{app::AppState, store::SessionData};

pub const COOKIE_NAME: &str = "__Host-sekondo";
pub const IDLE_TIMEOUT: Duration = Duration::days(7);
pub const ABSOLUTE_LIFETIME: Duration = Duration::days(30);

const STARTED_AT: &str = "started_at";
const LAST_SEEN: &str = "last_seen";

/// The tower-sessions store over whichever session data the app was built with.
#[derive(Clone)]
pub struct AppSessionStore {
    data: Arc<dyn SessionData>,
}

impl AppSessionStore {
    pub fn new(data: Arc<dyn SessionData>) -> Self {
        Self { data }
    }
}

impl std::fmt::Debug for AppSessionStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppSessionStore").finish_non_exhaustive()
    }
}

#[async_trait]
impl SessionStore for AppSessionStore {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        self.data.create(record).await
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        self.data.save(record).await
    }

    async fn load(&self, id: &Id) -> session_store::Result<Option<Record>> {
        self.data.load(id).await
    }

    async fn delete(&self, id: &Id) -> session_store::Result<()> {
        self.data.delete(id).await
    }
}

#[async_trait]
impl ExpiredDeletion for AppSessionStore {
    async fn delete_expired(&self) -> session_store::Result<()> {
        self.data.delete_expired().await
    }
}

/// `__Host-` makes browsers enforce Secure, Path=/ and no Domain. SameSite=Lax (not Strict) so the
/// cookie is sent on the SSO callback, a top-level cross-site GET.
pub fn layer(store: AppSessionStore) -> SessionManagerLayer<AppSessionStore> {
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
pub async fn is_active(store: &AppSessionStore, id: &Id, now: OffsetDateTime) -> bool {
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
