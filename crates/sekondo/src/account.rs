//! The signed-in account, as carried by the session.

use axum::{
    extract::{FromRequestParts, OptionalFromRequestParts},
    http::request::Parts,
    response::{IntoResponse, Redirect, Response},
};
use time::OffsetDateTime;
use tower_sessions::Session;
use uuid::Uuid;

use crate::session as lifetime;

const ACCOUNT_ID: &str = "account_id";

/// Signs the session in as `account_id`. Callers rotate the session ID first (`cycle_id`) when a
/// user signs in.
pub async fn sign_in(
    session: &Session,
    account_id: Uuid,
    now: OffsetDateTime,
) -> Result<(), tower_sessions::session::Error> {
    session.insert(ACCOUNT_ID, account_id).await?;
    lifetime::start(session, now).await
}

pub async fn current_account_id(session: &Session) -> Option<Uuid> {
    session.get::<Uuid>(ACCOUNT_ID).await.ok().flatten()
}

/// Extractor for pages that need an account. Unauthenticated requests are redirected to `/`.
#[derive(Debug, Clone, Copy)]
pub struct CurrentAccount {
    pub id: Uuid,
}

impl<S: Send + Sync> FromRequestParts<S> for CurrentAccount {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let session = Session::from_request_parts(parts, state)
            .await
            .map_err(|e| e.into_response())?;
        match current_account_id(&session).await {
            Some(id) => Ok(Self { id }),
            None => Err(Redirect::to("/").into_response()),
        }
    }
}

impl<S: Send + Sync> OptionalFromRequestParts<S> for CurrentAccount {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &S,
    ) -> Result<Option<Self>, Self::Rejection> {
        let session = Session::from_request_parts(parts, state)
            .await
            .map_err(|e| e.into_response())?;
        Ok(current_account_id(&session).await.map(|id| Self { id }))
    }
}
