//! The shared name cache: type, region, station, solar system and structure names, resolved
//! through ESI for whoever needs them (the order poller and the wealth poller).

use std::collections::BTreeSet;

use secrecy::SecretString;
use time::{Duration, OffsetDateTime};

use crate::{
    esi::{EsiClient, EsiError},
    store::{MarketStore, NameRow},
};

/// Location IDs from here up are player structures; everything below is public data.
pub const STRUCTURE_ID_MIN: i64 = 100_000_000;
/// How long a name ESI would not give is left alone before asking again.
const UNRESOLVABLE_RETRY: Duration = Duration::hours(24);
/// ESI's limit for `/universe/names`.
const NAMES_BATCH: usize = 1000;

/// Caches the names of `ids` that are not known yet (design §4). Structures are resolved with
/// `structure_token`. Anything ESI cannot give right now is left for the next call.
pub async fn resolve(
    store: &dyn MarketStore,
    esi: &EsiClient,
    structure_token: &SecretString,
    ids: impl IntoIterator<Item = i64>,
    now: OffsetDateTime,
) {
    let wanted: BTreeSet<i64> = ids.into_iter().collect();
    let wanted: Vec<i64> = wanted.into_iter().collect();
    let known: Vec<i64> = match store.known_names(&wanted, now).await {
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
        if !resolve_public(store, esi, batch, now).await {
            return;
        }
    }
    for id in structures {
        match esi.structure(id, structure_token).await {
            Ok(structure) => {
                store_names(
                    store,
                    &[NameRow {
                        id,
                        category: "structure".to_owned(),
                        name: Some(structure.name),
                    }],
                    now,
                    None,
                )
                .await;
            }
            Err(EsiError::Forbidden | EsiError::NotFound) => {
                store_names(
                    store,
                    &[NameRow {
                        id,
                        category: "structure".to_owned(),
                        name: None,
                    }],
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

/// Caches the names of public `ids` (item types, say) that are not known yet. Needs no token:
/// nothing here is a structure. Anything ESI cannot give right now is left for the next call.
pub async fn resolve_public_names(
    store: &dyn MarketStore,
    esi: &EsiClient,
    ids: impl IntoIterator<Item = i64>,
    now: OffsetDateTime,
) {
    let wanted: BTreeSet<i64> = ids.into_iter().collect();
    let wanted: Vec<i64> = wanted.into_iter().collect();
    let known: Vec<i64> = match store.known_names(&wanted, now).await {
        Ok(known) => known,
        Err(e) => {
            tracing::error!("cannot read the name cache: {e}");
            return;
        }
    };
    let unknown: Vec<i64> = wanted
        .into_iter()
        .filter(|id| !known.contains(id))
        .collect();
    for batch in unknown.chunks(NAMES_BATCH) {
        if !resolve_public(store, esi, batch, now).await {
            return;
        }
    }
}

/// Resolves public IDs. ESI fails a whole request for one unknown ID, so a failing batch is
/// split in halves until the bad IDs stand alone. Returns false when ESI is not available.
async fn resolve_public(
    store: &dyn MarketStore,
    esi: &EsiClient,
    ids: &[i64],
    now: OffsetDateTime,
) -> bool {
    let mut pending = vec![ids.to_vec()];
    while let Some(batch) = pending.pop() {
        match esi.names(&batch).await {
            Ok(names) => {
                let rows: Vec<_> = names
                    .into_iter()
                    .map(|n| NameRow {
                        id: n.id,
                        category: n.category,
                        name: Some(n.name),
                    })
                    .collect();
                store_names(store, &rows, now, None).await;
            }
            Err(EsiError::NotFound) if batch.len() > 1 => {
                let (left, right) = batch.split_at(batch.len() / 2);
                pending.push(left.to_vec());
                pending.push(right.to_vec());
            }
            Err(EsiError::NotFound) => {
                store_names(
                    store,
                    &[NameRow {
                        id: batch[0],
                        category: "unknown".to_owned(),
                        name: None,
                    }],
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

async fn store_names(
    store: &dyn MarketStore,
    rows: &[NameRow],
    resolved_at: OffsetDateTime,
    retry_after: Option<OffsetDateTime>,
) {
    if let Err(e) = store.store_names(rows, resolved_at, retry_after).await {
        tracing::error!("cannot cache names: {e}");
    }
}
