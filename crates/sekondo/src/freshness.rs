//! How fresh a character's orders are, shared by the orders page and the dashboard.

use sqlx::PgPool;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{fees::FeeLocationView, format, selection::CharacterSelection};

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

/// How fresh the wallet transactions of each selected character are, for the trades page.
pub async fn list_transactions_freshness(
    pool: &PgPool,
    account_id: Uuid,
    selection: &CharacterSelection,
) -> Result<Vec<FreshnessRecord>, sqlx::Error> {
    sqlx::query_as(
        "SELECT c.character_id, c.name, c.needs_relogin, s.last_success_at, s.next_poll_at
         FROM characters c
         LEFT JOIN wealth_sync s ON s.character_id = c.character_id AND s.kind = 'transactions'
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

/// How fresh the wallet and the assets of one character are.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct WealthFreshnessRecord {
    pub character_id: i64,
    pub name: String,
    pub needs_relogin: bool,
    pub wallet_last_success_at: Option<OffsetDateTime>,
    pub wallet_next_poll_at: Option<OffsetDateTime>,
    pub assets_last_success_at: Option<OffsetDateTime>,
    pub assets_next_poll_at: Option<OffsetDateTime>,
}

pub async fn list_wealth_freshness(
    pool: &PgPool,
    account_id: Uuid,
    selection: &CharacterSelection,
) -> Result<Vec<WealthFreshnessRecord>, sqlx::Error> {
    sqlx::query_as(
        "SELECT c.character_id, c.name, c.needs_relogin,
                w.last_success_at AS wallet_last_success_at, w.next_poll_at AS wallet_next_poll_at,
                a.last_success_at AS assets_last_success_at, a.next_poll_at AS assets_next_poll_at
         FROM characters c
         LEFT JOIN wealth_sync w ON w.character_id = c.character_id AND w.kind = 'wallet'
         LEFT JOIN wealth_sync a ON a.character_id = c.character_id AND a.kind = 'assets'
         WHERE c.account_id = $1 AND c.character_id = ANY($2)
         ORDER BY c.created_at, c.character_id",
    )
    .bind(account_id)
    .bind(selection.ids())
    .fetch_all(pool)
    .await
}

pub struct WealthFreshnessView {
    pub name: String,
    pub needs_relogin: bool,
    pub wallet: FreshnessView,
    pub assets: FreshnessView,
}

/// The wallet is stale by the order refresh interval, the assets by the assets' hour.
pub fn wealth_freshness_view(
    record: &WealthFreshnessRecord,
    now: OffsetDateTime,
    wallet_interval: Duration,
    assets_interval: Duration,
) -> WealthFreshnessView {
    let kind = |last_success_at, next_poll_at, interval| {
        freshness_view(
            &FreshnessRecord {
                character_id: record.character_id,
                name: record.name.clone(),
                needs_relogin: record.needs_relogin,
                last_success_at,
                next_poll_at,
            },
            now,
            interval,
        )
    };
    WealthFreshnessView {
        name: record.name.clone(),
        needs_relogin: record.needs_relogin,
        wallet: kind(
            record.wallet_last_success_at,
            record.wallet_next_poll_at,
            wallet_interval,
        ),
        assets: kind(
            record.assets_last_success_at,
            record.assets_next_poll_at,
            assets_interval,
        ),
    }
}

pub struct FreshnessView {
    pub name: String,
    pub stale: bool,
    pub needs_relogin: bool,
    pub waiting_for_first_update: bool,
    pub last_update: String,
    pub next_update: String,
    /// What the market charges at each location with orders; only the orders page fills it.
    pub locations: Vec<FeeLocationView>,
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
        locations: Vec::new(),
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
    fn assets_follow_the_same_rule_with_twice_one_hour() {
        let hour = Duration::hours(1);
        assert!(is_stale(
            &record(false, Some(NOW - Duration::hours(3))),
            NOW,
            hour
        ));
        assert!(!is_stale(
            &record(false, Some(NOW - Duration::minutes(110))),
            NOW,
            hour
        ));
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

    #[test]
    fn transactions_are_stale_after_two_hours() {
        let hour = crate::wealth_poller::TRANSACTIONS_INTERVAL;
        assert!(is_stale(
            &record(false, Some(NOW - Duration::hours(3))),
            NOW,
            hour
        ));
        assert!(!is_stale(
            &record(false, Some(NOW - Duration::minutes(110))),
            NOW,
            hour
        ));
    }

    #[test]
    fn transactions_of_a_character_needing_re_login_or_never_polled_are_stale() {
        let hour = crate::wealth_poller::TRANSACTIONS_INTERVAL;
        assert!(is_stale(&record(true, Some(NOW)), NOW, hour));
        assert!(is_stale(&record(false, None), NOW, hour));
    }
}
