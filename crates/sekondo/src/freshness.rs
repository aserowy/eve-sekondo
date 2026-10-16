//! How fresh a character's orders are, shared by the orders page and the dashboard.

use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{format, selection::CharacterSelection};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FreshnessRecord {
    pub character_id: i64,
    pub name: String,
    pub needs_relogin: bool,
    pub last_success_at: Option<OffsetDateTime>,
    pub next_poll_at: Option<OffsetDateTime>,
}

pub async fn list_freshness(
    pool: &PgPool,
    account_id: Uuid,
    selection: &CharacterSelection,
) -> Result<Vec<FreshnessRecord>, sqlx::Error> {
    sqlx::query_as(
        "SELECT c.character_id, c.name, c.needs_relogin, s.last_success_at, s.next_poll_at
         FROM characters c
         LEFT JOIN order_sync s ON s.character_id = c.character_id
         WHERE c.account_id = $1 AND c.character_id = ANY($2)
         ORDER BY c.created_at, c.character_id",
    )
    .bind(account_id)
    .bind(selection.ids())
    .fetch_all(pool)
    .await
}

/// Stale: needs a re-login, never polled successfully, or the last success is older than twice
/// the refresh interval.
pub fn is_stale(record: &FreshnessRecord, now: OffsetDateTime, interval: Duration) -> bool {
    record.needs_relogin
        || record
            .last_success_at
            .is_none_or(|last| now - last > interval * 2)
}

pub struct FreshnessView {
    pub name: String,
    pub stale: bool,
    pub needs_relogin: bool,
    pub waiting_for_first_update: bool,
    pub last_update: String,
    pub next_update: String,
}

pub fn freshness_view(
    record: &FreshnessRecord,
    now: OffsetDateTime,
    interval: Duration,
) -> FreshnessView {
    FreshnessView {
        name: record.name.clone(),
        stale: is_stale(record, now, interval),
        needs_relogin: record.needs_relogin,
        waiting_for_first_update: record.last_success_at.is_none() && !record.needs_relogin,
        last_update: record
            .last_success_at
            .map_or_else(|| "never".to_owned(), format::eve_time),
        next_update: match (record.needs_relogin, record.next_poll_at) {
            (false, Some(next)) => format::eve_time(next),
            _ => "\u{2014}".to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    const NOW: OffsetDateTime = datetime!(2026-10-16 12:00 UTC);
    const INTERVAL: Duration = Duration::minutes(20);

    fn record(needs_relogin: bool, last_success_at: Option<OffsetDateTime>) -> FreshnessRecord {
        FreshnessRecord {
            character_id: 1,
            name: "Alice".to_owned(),
            needs_relogin,
            last_success_at,
            next_poll_at: None,
        }
    }

    #[test]
    fn a_character_needing_re_login_is_stale_even_when_just_polled() {
        assert!(is_stale(&record(true, Some(NOW)), NOW, INTERVAL));
    }

    #[test]
    fn a_character_never_polled_is_stale() {
        assert!(is_stale(&record(false, None), NOW, INTERVAL));
    }

    #[test]
    fn exactly_twice_the_interval_is_still_fresh() {
        let last = NOW - INTERVAL * 2;
        assert!(!is_stale(&record(false, Some(last)), NOW, INTERVAL));
    }

    #[test]
    fn just_over_twice_the_interval_is_stale() {
        let last = NOW - INTERVAL * 2 - Duration::seconds(1);
        assert!(is_stale(&record(false, Some(last)), NOW, INTERVAL));
    }
}
