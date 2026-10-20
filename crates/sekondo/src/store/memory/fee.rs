use std::{collections::BTreeMap, sync::OnceLock};

use async_trait::async_trait;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::{
    Shared, backoff,
    numeric::Numeric,
    tables::{Sync, Tables},
};
use crate::{
    fee_poller::Kind,
    fees::{ACCOUNTING, BROKER_RELATIONS, CharacterFee, FeeRow, FeeSnapshot},
    oauth::LOCATION_SCOPE,
    opportunities::STATION_ID,
    store::{FeeStore, StandingInput, StoreError, StoreResult, SyncState},
};

pub struct MemFeeStore {
    pub(super) shared: std::sync::Arc<Shared>,
}

/// Which NPC corporation belongs to which faction: the migration's table.
fn factions() -> &'static BTreeMap<i64, i64> {
    static FACTIONS: OnceLock<BTreeMap<i64, i64>> = OnceLock::new();
    FACTIONS.get_or_init(|| {
        include_str!("../../../data/npc_corporation_factions.csv")
            .lines()
            .filter(|l| !l.starts_with('#') && !l.starts_with("corporation_id") && !l.is_empty())
            .filter_map(|l| {
                let (corporation, faction) = l.split_once(',')?;
                Some((corporation.parse().ok()?, faction.parse().ok()?))
            })
            .collect()
    })
}

/// A structure ID starts here: the app's own `STRUCTURE_ID_MIN`, as the SQL spells it.
const STRUCTURE: i64 = 100_000_000;

/// The rates of one character at one location, from what is stored of its skills, standings and
/// the location's owner.
pub(super) fn fee_row(tables: &Tables, character_id: i64, location_id: i64) -> FeeRow {
    let polled = |kind: Kind| {
        tables
            .fee_sync
            .get(&(character_id, kind.as_str().to_owned()))
            .is_some_and(|s| s.last_success_at.is_some())
    };
    let is_structure = location_id >= STRUCTURE;
    let level = |skill: i64| {
        polled(Kind::Skills).then(|| {
            tables
                .character_skills
                .get(&(character_id, skill))
                .copied()
                .map_or(0, i32::from)
        })
    };
    let accounting = level(ACCOUNTING);
    let broker_relations = level(BROKER_RELATIONS);
    let owner = tables
        .station_owners
        .get(&location_id)
        .map(|(owner, _)| *owner);
    let standings_known = !is_structure && polled(Kind::Standings) && owner.is_some();
    let standing = |from_type: &str, from_id: i64| {
        tables
            .character_standings
            .get(&(character_id, from_id, from_type.to_owned()))
            .cloned()
    };
    let (faction_standing, corp_standing) = match owner {
        Some(owner) if standings_known => {
            let faction = match factions().get(&owner) {
                None => Numeric::int(0),
                Some(faction) => standing("faction", *faction).unwrap_or_else(|| Numeric::int(0)),
            };
            let corp = standing("npc_corp", owner).unwrap_or_else(|| Numeric::int(0));
            (Some(faction), Some(corp))
        }
        _ => (None, None),
    };
    let sales_tax = accounting.map(|level| {
        Numeric::parse("7.5").expect("a literal").mul(
            &Numeric::int(1).sub(
                &Numeric::parse("0.11")
                    .expect("a literal")
                    .mul(&Numeric::int(i64::from(level))),
            ),
        )
    });
    let broker_fee = match (
        is_structure,
        broker_relations,
        &faction_standing,
        &corp_standing,
    ) {
        (false, Some(br), Some(fs), Some(cs)) => Some(
            Numeric::int(3)
                .sub(
                    &Numeric::parse("0.3")
                        .expect("a literal")
                        .mul(&Numeric::int(i64::from(br))),
                )
                .sub(&Numeric::parse("0.03").expect("a literal").mul(fs))
                .sub(&Numeric::parse("0.02").expect("a literal").mul(cs)),
        ),
        _ => None,
    };
    let min_margin = match (&broker_fee, &sales_tax) {
        (Some(b), Some(t)) => minimum_margin(b, t),
        _ => None,
    };
    FeeRow {
        character_id,
        location_id,
        is_structure,
        accounting,
        broker_relations,
        faction_standing: faction_standing.map(|n| n.to_string()),
        corp_standing: corp_standing.map(|n| n.to_string()),
        sales_tax: sales_tax.map(|n| n.to_string()),
        broker_fee: broker_fee.map(|n| n.to_string()),
        min_margin: min_margin.map(|n| n.to_string()),
    }
}

/// `((1 + b / 100) / (1 - b / 100 - t / 100) - 1) * 100`, in percent.
fn minimum_margin(broker_fee: &Numeric, sales_tax: &Numeric) -> Option<Numeric> {
    let hundred = Numeric::int(100);
    let b = broker_fee.div(&hundred)?;
    let t = sales_tax.div(&hundred)?;
    let quotient = Numeric::int(1)
        .add(&b)
        .div(&Numeric::int(1).sub(&b).sub(&t))?;
    Some(quotient.sub(&Numeric::int(1)).mul(&hundred))
}

/// The rates as numbers, for the margins of orders and sales: `(sales tax, broker fee)` in percent.
pub(super) fn rates(
    tables: &Tables,
    character_id: i64,
    location_id: i64,
) -> (Option<Numeric>, Option<Numeric>) {
    let row = fee_row(tables, character_id, location_id);
    (
        row.sales_tax.and_then(|t| Numeric::parse(&t)),
        row.broker_fee.and_then(|t| Numeric::parse(&t)),
    )
}

/// One row per (character, location) of the current orders of these characters.
fn order_rows(tables: &Tables, account: Option<Uuid>, character_ids: &[i64]) -> Vec<FeeRow> {
    let mut pairs: Vec<(i64, i64)> = tables
        .current_orders
        .values()
        .filter(|o| character_ids.contains(&o.character_id))
        .filter(|o| in_account(tables, account, o.character_id))
        .map(|o| (o.character_id, o.location_id))
        .collect();
    pairs.sort_unstable();
    pairs.dedup();
    pairs
        .into_iter()
        .map(|(character, location)| fee_row(tables, character, location))
        .collect()
}

fn in_account(tables: &Tables, account: Option<Uuid>, character_id: i64) -> bool {
    account.is_none_or(|a| {
        tables
            .characters
            .get(&character_id)
            .is_some_and(|c| c.account_id == a)
    })
}

/// The fees at the fee location of each character: where it is docked when it granted the
/// location scope, otherwise Jita 4-4.
fn fee_locations(
    tables: &Tables,
    account: Option<Uuid>,
    character_ids: &[i64],
) -> Vec<CharacterFee> {
    let mut ids: Vec<i64> = character_ids
        .iter()
        .copied()
        .filter(|id| tables.characters.contains_key(id) && in_account(tables, account, *id))
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids.into_iter()
        .map(|id| {
            let character = &tables.characters[&id];
            let has_scope = character.scopes.iter().any(|s| s == LOCATION_SCOPE);
            let location = if has_scope {
                tables
                    .character_locations
                    .get(&id)
                    .map_or(STATION_ID, |(location, _, _)| *location)
            } else {
                STATION_ID
            };
            CharacterFee {
                row: fee_row(tables, id, location),
                needs_location_scope: !has_scope,
            }
        })
        .collect()
}

/// The rates at the locations of the stored sales, as decimal text.
fn sale_rates(
    tables: &Tables,
    character_ids: &[i64],
) -> Vec<(i64, i64, Option<String>, Option<String>)> {
    let mut pairs: Vec<(i64, i64)> = tables
        .sales
        .values()
        .filter(|s| character_ids.contains(&s.character_id))
        .map(|s| (s.character_id, s.location_id))
        .collect();
    pairs.sort_unstable();
    pairs.dedup();
    pairs
        .into_iter()
        .map(|(character, location)| {
            let row = fee_row(tables, character, location);
            (character, location, row.sales_tax, row.broker_fee)
        })
        .collect()
}

/// What the pages show of these characters' fees right now.
fn snapshot(tables: &Tables, character_ids: &[i64]) -> FeeSnapshot {
    FeeSnapshot {
        orders: order_rows(tables, None, character_ids),
        fee_location: fee_locations(tables, None, character_ids),
        sales: sale_rates(tables, character_ids),
    }
}

fn require_character(tables: &Tables, character_id: i64) -> StoreResult<()> {
    if tables.characters.contains_key(&character_id) {
        Ok(())
    } else {
        Err(StoreError::MissingParent)
    }
}

fn record_success(
    tables: &mut Tables,
    character_id: i64,
    kind: Kind,
    polled_at: OffsetDateTime,
    next_poll_at: OffsetDateTime,
    expires_at: Option<OffsetDateTime>,
) {
    let row = tables
        .fee_sync
        .entry((character_id, kind.as_str().to_owned()))
        .or_insert_with(|| Sync::scheduled(next_poll_at));
    row.next_poll_at = next_poll_at;
    row.last_success_at = Some(polled_at);
    row.last_attempt_at = Some(polled_at);
    row.last_error = None;
    row.esi_expires_at = expires_at;
    row.failures = 0;
}

#[async_trait]
impl FeeStore for MemFeeStore {
    async fn for_characters(&self, character_ids: &[i64]) -> StoreResult<Vec<FeeRow>> {
        Ok(order_rows(&self.shared.lock(), None, character_ids))
    }

    async fn for_account(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
    ) -> StoreResult<Vec<FeeRow>> {
        Ok(order_rows(
            &self.shared.lock(),
            Some(account_id),
            character_ids,
        ))
    }

    async fn at_location(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        location_id: i64,
    ) -> StoreResult<Vec<FeeRow>> {
        let tables = self.shared.lock();
        let mut ids: Vec<i64> = character_ids
            .iter()
            .copied()
            .filter(|id| {
                tables.characters.contains_key(id) && in_account(&tables, Some(account_id), *id)
            })
            .collect();
        ids.sort_unstable();
        ids.dedup();
        Ok(ids
            .into_iter()
            .map(|id| fee_row(&tables, id, location_id))
            .collect())
    }

    async fn at_fee_location(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
    ) -> StoreResult<Vec<CharacterFee>> {
        Ok(fee_locations(
            &self.shared.lock(),
            Some(account_id),
            character_ids,
        ))
    }

    async fn has_scope(&self, character_id: i64, scope: &str) -> StoreResult<Option<bool>> {
        Ok(self
            .shared
            .lock()
            .characters
            .get(&character_id)
            .map(|c| c.scopes.iter().any(|s| s == scope)))
    }

    async fn make_due(
        &self,
        character_id: i64,
        kind: Kind,
        now: OffsetDateTime,
    ) -> StoreResult<()> {
        if let Some(row) = self
            .shared
            .lock()
            .fee_sync
            .get_mut(&(character_id, kind.as_str().to_owned()))
        {
            row.next_poll_at = row.next_poll_at.min(now);
        }
        Ok(())
    }

    async fn store_skills(
        &self,
        character_id: i64,
        skill_ids: &[i64],
        levels: &[i16],
        polled_at: OffsetDateTime,
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<bool> {
        let mut tables = self.shared.lock();
        require_character(&tables, character_id)?;
        let mut distinct = skill_ids.to_vec();
        distinct.sort_unstable();
        distinct.dedup();
        if distinct.len() != skill_ids.len() {
            return Err(StoreError::Backend(
                "duplicate key value violates unique constraint \"character_skills_pkey\""
                    .to_owned(),
            ));
        }
        let before = snapshot(&tables, &[character_id]);
        tables
            .character_skills
            .retain(|(c, _), _| *c != character_id);
        for (skill, level) in skill_ids.iter().zip(levels) {
            tables
                .character_skills
                .insert((character_id, *skill), *level);
        }
        record_success(
            &mut tables,
            character_id,
            Kind::Skills,
            polled_at,
            next_poll_at,
            esi_expires_at,
        );
        Ok(before != snapshot(&tables, &[character_id]))
    }

    async fn store_standings(
        &self,
        character_id: i64,
        standings: &[StandingInput],
        polled_at: OffsetDateTime,
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<bool> {
        let mut tables = self.shared.lock();
        require_character(&tables, character_id)?;
        let parsed: Vec<Numeric> = standings
            .iter()
            .map(|s| {
                Numeric::parse(&s.standing).ok_or_else(|| {
                    StoreError::Backend("invalid input syntax for type numeric".to_owned())
                })
            })
            .collect::<Result<_, _>>()?;
        let before = snapshot(&tables, &[character_id]);
        tables
            .character_standings
            .retain(|(c, _, _), _| *c != character_id);
        for (input, standing) in standings.iter().zip(parsed) {
            // ON CONFLICT DO NOTHING: the first of a repeated pair stays.
            tables
                .character_standings
                .entry((character_id, input.from_id, input.from_type.clone()))
                .or_insert(standing);
        }
        record_success(
            &mut tables,
            character_id,
            Kind::Standings,
            polled_at,
            next_poll_at,
            esi_expires_at,
        );
        Ok(before != snapshot(&tables, &[character_id]))
    }

    async fn store_location(
        &self,
        character_id: i64,
        docked: Option<(i64, &str)>,
        polled_at: OffsetDateTime,
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<bool> {
        let mut tables = self.shared.lock();
        require_character(&tables, character_id)?;
        let before = snapshot(&tables, &[character_id]);
        if let Some((location_id, kind)) = docked {
            tables.character_locations.insert(
                character_id,
                (location_id, kind.to_owned(), super::micros(polled_at)),
            );
        }
        record_success(
            &mut tables,
            character_id,
            Kind::Location,
            polled_at,
            next_poll_at,
            esi_expires_at,
        );
        Ok(before != snapshot(&tables, &[character_id]))
    }

    async fn record_failure(
        &self,
        character_id: i64,
        kind: Kind,
        now: OffsetDateTime,
        reason: &str,
        backoff_start: Duration,
        interval: Duration,
    ) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        require_character(&tables, character_id)?;
        match tables
            .fee_sync
            .get_mut(&(character_id, kind.as_str().to_owned()))
        {
            Some(row) => {
                row.next_poll_at = now + backoff(backoff_start, row.failures, interval);
                row.last_attempt_at = Some(now);
                row.last_error = Some(reason.to_owned());
                row.failures += 1;
            }
            None => {
                let mut row = Sync::scheduled(now + backoff_start);
                row.last_attempt_at = Some(now);
                row.last_error = Some(reason.to_owned());
                row.failures = 1;
                tables
                    .fee_sync
                    .insert((character_id, kind.as_str().to_owned()), row);
            }
        }
        Ok(())
    }

    async fn record_deferral(
        &self,
        character_id: i64,
        kind: Kind,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
    ) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        require_character(&tables, character_id)?;
        let row = tables
            .fee_sync
            .entry((character_id, kind.as_str().to_owned()))
            .or_insert_with(|| Sync::scheduled(next_poll_at));
        row.next_poll_at = next_poll_at;
        row.last_attempt_at = Some(now);
        Ok(())
    }

    async fn stations_without_owner(&self, structure_id_min: i64) -> StoreResult<Vec<i64>> {
        let tables = self.shared.lock();
        let mut ids: Vec<i64> = tables
            .current_orders
            .values()
            .map(|o| o.location_id)
            .chain(tables.sales.values().map(|s| s.location_id))
            .chain(
                tables
                    .character_locations
                    .values()
                    .filter(|(_, kind, _)| kind == "station")
                    .map(|(location, _, _)| *location),
            )
            .filter(|id| *id < structure_id_min && !tables.station_owners.contains_key(id))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        Ok(ids)
    }

    async fn station_owner_known(&self, station_id: i64) -> StoreResult<bool> {
        Ok(self.shared.lock().station_owners.contains_key(&station_id))
    }

    async fn insert_station_owner(
        &self,
        station_id: i64,
        owner_id: i64,
        now: OffsetDateTime,
    ) -> StoreResult<()> {
        self.shared
            .lock()
            .station_owners
            .entry(station_id)
            .or_insert((owner_id, super::micros(now)));
        Ok(())
    }

    async fn store_station_owner(
        &self,
        station_id: i64,
        owner_id: i64,
        now: OffsetDateTime,
    ) -> StoreResult<Vec<Uuid>> {
        let mut tables = self.shared.lock();
        let mut characters: Vec<i64> = tables
            .current_orders
            .values()
            .filter(|o| o.location_id == station_id)
            .map(|o| o.character_id)
            .chain(
                tables
                    .sales
                    .values()
                    .filter(|s| s.location_id == station_id)
                    .map(|s| s.character_id),
            )
            .chain(
                tables
                    .character_locations
                    .iter()
                    .filter(|(_, (location, _, _))| *location == station_id)
                    .map(|(character, _)| *character),
            )
            .filter(|id| tables.characters.contains_key(id))
            .collect();
        characters.sort_unstable();
        characters.dedup();
        let before = snapshot(&tables, &characters);
        tables
            .station_owners
            .entry(station_id)
            .or_insert((owner_id, super::micros(now)));
        let after = snapshot(&tables, &characters);
        let mut accounts: Vec<Uuid> = characters
            .iter()
            .filter(|id| before.of_character(**id) != after.of_character(**id))
            .map(|id| tables.characters[id].account_id)
            .collect();
        accounts.sort_unstable();
        accounts.dedup();
        Ok(accounts)
    }

    async fn list_unscheduled(&self, kind: Kind, scope: Option<&str>) -> StoreResult<Vec<i64>> {
        let tables = self.shared.lock();
        Ok(tables
            .characters
            .values()
            .filter(|c| !c.needs_relogin)
            .filter(|c| scope.is_none_or(|s| c.scopes.iter().any(|x| x == s)))
            .filter(|c| {
                !tables
                    .fee_sync
                    .contains_key(&(c.character_id, kind.as_str().to_owned()))
            })
            .map(|c| c.character_id)
            .collect())
    }

    async fn insert_schedule(
        &self,
        kind: Kind,
        character_ids: &[i64],
        next_poll_at: &[OffsetDateTime],
    ) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        if character_ids
            .iter()
            .any(|id| !tables.characters.contains_key(id))
        {
            return Err(StoreError::MissingParent);
        }
        for (id, at) in character_ids.iter().zip(next_poll_at) {
            tables
                .fee_sync
                .entry((*id, kind.as_str().to_owned()))
                .or_insert_with(|| Sync::scheduled(super::micros(*at)));
        }
        Ok(())
    }

    async fn claim_due(
        &self,
        now: OffsetDateTime,
        lease: Duration,
        limit: i64,
    ) -> StoreResult<Vec<(i64, Kind)>> {
        let mut tables = self.shared.lock();
        let mut due: Vec<(OffsetDateTime, i64, String)> = tables
            .fee_sync
            .iter()
            .filter(|(_, row)| row.next_poll_at <= now)
            .filter(|((id, kind), _)| {
                tables.characters.get(id).is_some_and(|c| {
                    !c.needs_relogin
                        && (kind != "location" || c.scopes.iter().any(|s| s == LOCATION_SCOPE))
                })
            })
            .map(|((id, kind), row)| (row.next_poll_at, *id, kind.clone()))
            .collect();
        due.sort();
        due.truncate(limit.max(0) as usize);
        let mut claimed = Vec::new();
        for (_, id, kind) in due {
            if let Some(row) = tables.fee_sync.get_mut(&(id, kind.clone())) {
                row.next_poll_at = now + lease;
            }
            if let Some(kind) = Kind::parse(&kind) {
                claimed.push((id, kind));
            }
        }
        Ok(claimed)
    }
    async fn fee_sync(&self, character_id: i64, kind: Kind) -> StoreResult<Option<SyncState>> {
        Ok(self
            .shared
            .lock()
            .fee_sync
            .get(&(character_id, kind.as_str().to_owned()))
            .map(Sync::state))
    }

    async fn seed_skill(&self, character_id: i64, skill_id: i64, level: i16) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        require_character(&tables, character_id)?;
        tables
            .character_skills
            .insert((character_id, skill_id), level);
        Ok(())
    }

    async fn seed_standing(&self, character_id: i64, standing: &StandingInput) -> StoreResult<()> {
        let value = Numeric::parse(&standing.standing).ok_or_else(|| {
            StoreError::Backend("invalid input syntax for type numeric".to_owned())
        })?;
        let mut tables = self.shared.lock();
        require_character(&tables, character_id)?;
        tables.character_standings.insert(
            (character_id, standing.from_id, standing.from_type.clone()),
            value,
        );
        Ok(())
    }

    async fn seed_fee_success(
        &self,
        character_id: i64,
        kind: Kind,
        at: OffsetDateTime,
    ) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        require_character(&tables, character_id)?;
        record_success(&mut tables, character_id, kind, at, at, None);
        Ok(())
    }

    async fn skills_of(&self, character_id: i64) -> StoreResult<Vec<(i64, i16)>> {
        let tables = self.shared.lock();
        Ok(tables
            .character_skills
            .iter()
            .filter(|((c, _), _)| *c == character_id)
            .map(|((_, skill), level)| (*skill, *level))
            .collect())
    }

    async fn standings_of(&self, character_id: i64) -> StoreResult<Vec<(i64, String, String)>> {
        let tables = self.shared.lock();
        let mut rows: Vec<_> = tables
            .character_standings
            .iter()
            .filter(|((c, _, _), _)| *c == character_id)
            .map(|((_, from_id, from_type), standing)| {
                (*from_id, from_type.clone(), standing.to_string())
            })
            .collect();
        rows.sort();
        Ok(rows)
    }

    async fn location_of(&self, character_id: i64) -> StoreResult<Option<(i64, String)>> {
        let tables = self.shared.lock();
        Ok(tables
            .character_locations
            .get(&character_id)
            .map(|(id, kind, _)| (*id, kind.clone())))
    }

    async fn station_owner(&self, station_id: i64) -> StoreResult<Option<i64>> {
        let tables = self.shared.lock();
        Ok(tables
            .station_owners
            .get(&station_id)
            .map(|(owner, _)| *owner))
    }

    async fn station_owner_count(&self) -> StoreResult<i64> {
        Ok(self.shared.lock().station_owners.len() as i64)
    }
}
