//! The shared name cache: type, region, station, solar system and structure names, resolved
//! through ESI for whoever needs them (the order poller and the wealth poller).

use std::collections::BTreeSet;

use secrecy::SecretString;
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};

use crate::esi::{EsiClient, EsiError};

/// Location IDs from here up are player structures; everything below is public data.
pub const STRUCTURE_ID_MIN: i64 = 100_000_000;
/// How long a name ESI would not give is left alone before asking again.
const UNRESOLVABLE_RETRY: Duration = Duration::hours(24);
/// ESI's limit for `/universe/names`.
const NAMES_BATCH: usize = 1000;

/// Caches the names of `ids` that are not known yet (design §4). Structures are resolved with
/// `structure_token`. Anything ESI cannot give right now is left for the next call.
pub async fn resolve(
    pool: &PgPool,
    esi: &EsiClient,
    structure_token: &SecretString,
    ids: impl IntoIterator<Item = i64>,
    now: OffsetDateTime,
) {
    let wanted: BTreeSet<i64> = ids.into_iter().collect();
    let wanted: Vec<i64> = wanted.into_iter().collect();
    let known: Vec<i64> = match sqlx::query_scalar(
        "SELECT id FROM names WHERE id = ANY($1) AND (name IS NOT NULL OR retry_after > $2)",
    )
    .bind(&wanted)
    .bind(now)
    .fetch_all(pool)
    .await
    {
        Ok(known) => known,
        Err(e) => {
            tracing::error!("cannot read the name cache: {e}");
            return;
        }
    };
    let (structures, public): (Vec<i64>, Vec<i64>) = wanted
        .into_iter()
        .filter(|id| !known.contains(id))
        .partition(|id| *id >= STRUCTURE_ID_MIN);

    for batch in public.chunks(NAMES_BATCH) {
        if !resolve_public(pool, esi, batch, now).await {
            return;
        }
    }
    for id in structures {
        match esi.structure(id, structure_token).await {
            Ok(structure) => {
                store(
                    pool,
                    &[(id, "structure".to_owned(), Some(structure.name))],
                    now,
                    None,
                )
                .await;
            }
            Err(EsiError::Forbidden | EsiError::NotFound) => {
                store(
                    pool,
                    &[(id, "structure".to_owned(), None)],
                    now,
                    Some(now + UNRESOLVABLE_RETRY),
                )
                .await;
            }
            // Nothing is cached, so the next call asks again.
            Err(e) => {
                tracing::warn!(structure_id = id, "cannot resolve a structure name: {e}");
                return;
            }
        }
    }
}

/// Resolves public IDs. ESI fails a whole request for one unknown ID, so a failing batch is
/// split in halves until the bad IDs stand alone. Returns false when ESI is not available.
async fn resolve_public(pool: &PgPool, esi: &EsiClient, ids: &[i64], now: OffsetDateTime) -> bool {
    let mut pending = vec![ids.to_vec()];
    while let Some(batch) = pending.pop() {
        match esi.names(&batch).await {
            Ok(names) => {
                let rows: Vec<_> = names
                    .into_iter()
                    .map(|n| (n.id, n.category, Some(n.name)))
                    .collect();
                store(pool, &rows, now, None).await;
            }
            Err(EsiError::NotFound) if batch.len() > 1 => {
                let (left, right) = batch.split_at(batch.len() / 2);
                pending.push(left.to_vec());
                pending.push(right.to_vec());
            }
            Err(EsiError::NotFound) => {
                store(
                    pool,
                    &[(batch[0], "unknown".to_owned(), None)],
                    now,
                    Some(now + UNRESOLVABLE_RETRY),
                )
                .await;
            }
            Err(e) => {
                tracing::warn!("cannot resolve names: {e}");
                return false;
            }
        }
    }
    true
}

async fn store(
    pool: &PgPool,
    rows: &[(i64, String, Option<String>)],
    resolved_at: OffsetDateTime,
    retry_after: Option<OffsetDateTime>,
) {
    let ids: Vec<i64> = rows.iter().map(|r| r.0).collect();
    let categories: Vec<&str> = rows.iter().map(|r| r.1.as_str()).collect();
    let names: Vec<Option<&str>> = rows.iter().map(|r| r.2.as_deref()).collect();
    let result = sqlx::query(
        "INSERT INTO names (id, category, name, resolved_at, retry_after)
         SELECT n.id, n.category, n.name, $4, $5
         FROM unnest($1::bigint[], $2::text[], $3::text[]) AS n(id, category, name)
         ON CONFLICT (id) DO UPDATE SET
             category = excluded.category,
             name = excluded.name,
             resolved_at = excluded.resolved_at,
             retry_after = excluded.retry_after",
    )
    .bind(&ids)
    .bind(&categories)
    .bind(&names)
    .bind(resolved_at)
    .bind(retry_after)
    .execute(pool)
    .await;
    if let Err(e) = result {
        tracing::error!("cannot cache names: {e}");
    }
}
