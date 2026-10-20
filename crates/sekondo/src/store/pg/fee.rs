use async_trait::async_trait;
use sqlx::{PgConnection, PgExecutor, PgPool};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    fee_poller::Kind,
    fees::{CharacterFee, FeeRow, FeeSnapshot},
    oauth::LOCATION_SCOPE,
    opportunities::STATION_ID,
    store::{FeeStore, StandingInput, StoreResult, SyncState},
};

pub struct PgFeeStore {
    pool: PgPool,
}

impl PgFeeStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// The locations of the current orders, one row per (character, location). `$1` is the character
/// IDs and `$2` the account the characters must belong to (`NULL`: any).
macro_rules! order_locations_sql {
    () => {
        "SELECT DISTINCT o.character_id, o.location_id
            FROM current_orders o
            JOIN characters c ON c.character_id = o.character_id
            WHERE o.character_id = ANY($1) AND ($2::uuid IS NULL OR c.account_id = $2)"
    };
}

/// The locations of the stored sales, one row per (character, location). `$1` is the character IDs
/// and `$2` the account the characters must belong to (`NULL`: any).
macro_rules! sale_locations_sql {
    () => {
        "SELECT DISTINCT s.character_id, s.location_id
            FROM sales s
            JOIN characters c ON c.character_id = s.character_id
            WHERE s.character_id = ANY($1) AND ($2::uuid IS NULL OR c.account_id = $2)"
    };
}

/// One fixed location (`$3`) for every character of `$1` that belongs to the account `$2`.
macro_rules! fixed_location_sql {
    () => {
        "SELECT c.character_id, $3::bigint AS location_id
            FROM characters c
            WHERE c.character_id = ANY($1) AND ($2::uuid IS NULL OR c.account_id = $2)"
    };
}

/// The fee location of every character of `$1` that belongs to the account `$2` (`NULL`: any): the
/// station or structure it is docked in when it has granted the location scope (`$3`) and has been
/// reported docked, otherwise Jita 4-4 (`$4`).
macro_rules! fee_location_sql {
    () => {
        "SELECT c.character_id,
                CASE WHEN $3::text = ANY(c.scopes) THEN COALESCE(cl.location_id, $4::bigint)
                     ELSE $4::bigint END AS location_id
            FROM characters c
            LEFT JOIN character_locations cl ON cl.character_id = c.character_id
            WHERE c.character_id = ANY($1) AND ($2::uuid IS NULL OR c.account_id = $2)"
    };
}

/// The rates query as one literal, so that [`FEES_SQL`] can be built from it with `concat!`. It
/// works out the rates for the (character, location) rows of `$locations`.
macro_rules! fee_rates_sql {
    ($locations:expr) => {
        concat!(
            "
        SELECT l.character_id, l.location_id, l.is_structure,
               l.accounting, l.broker_relations, l.faction_standing, l.corp_standing,
               CASE WHEN l.accounting IS NOT NULL THEN 7.5 * (1 - 0.11 * l.accounting) END AS sales_tax,
               CASE WHEN NOT l.is_structure
                         AND l.broker_relations IS NOT NULL
                         AND l.faction_standing IS NOT NULL
                         AND l.corp_standing IS NOT NULL
                    THEN 3 - 0.3 * l.broker_relations - 0.03 * l.faction_standing
                           - 0.02 * l.corp_standing
               END AS broker_fee
        FROM (
            SELECT loc.character_id, loc.location_id,
                   (loc.location_id >= 100000000) AS is_structure,
                   CASE WHEN skills_sync.last_success_at IS NOT NULL
                        THEN COALESCE(acc.active_level, 0)::integer END AS accounting,
                   CASE WHEN skills_sync.last_success_at IS NOT NULL
                        THEN COALESCE(br.active_level, 0)::integer END AS broker_relations,
                   CASE WHEN loc.location_id < 100000000
                             AND standings_sync.last_success_at IS NOT NULL
                             AND so.owner_id IS NOT NULL
                        THEN CASE WHEN nf.faction_id IS NULL THEN 0
                                  ELSE COALESCE(fs.standing, 0) END
                   END AS faction_standing,
                   CASE WHEN loc.location_id < 100000000
                             AND standings_sync.last_success_at IS NOT NULL
                             AND so.owner_id IS NOT NULL
                        THEN COALESCE(cs.standing, 0)
                   END AS corp_standing
            FROM (",
            $locations,
            ") loc
            LEFT JOIN fee_sync skills_sync
                   ON skills_sync.character_id = loc.character_id AND skills_sync.kind = 'skills'
            LEFT JOIN fee_sync standings_sync
                   ON standings_sync.character_id = loc.character_id AND standings_sync.kind = 'standings'
            LEFT JOIN character_skills acc
                   ON acc.character_id = loc.character_id AND acc.skill_id = 16622
            LEFT JOIN character_skills br
                   ON br.character_id = loc.character_id AND br.skill_id = 3446
            LEFT JOIN station_owners so ON so.station_id = loc.location_id
            LEFT JOIN npc_corporation_factions nf ON nf.corporation_id = so.owner_id
            LEFT JOIN character_standings fs
                   ON fs.character_id = loc.character_id AND fs.from_type = 'faction'
                  AND fs.from_id = nf.faction_id
            LEFT JOIN character_standings cs
                   ON cs.character_id = loc.character_id AND cs.from_type = 'npc_corp'
                  AND cs.from_id = so.owner_id
        ) l
"
        )
    };
}

/// One row per (character, location) of the current orders. `$1` is the character IDs and `$2`
/// the account the characters must belong to (`NULL`: any).
///
/// - `accounting` and `broker_relations` are `NULL` until the skills were polled once, and 0 when
///   polled but not trained.
/// - `faction_standing` and `corp_standing` are `NULL` for a structure, until the standings were
///   polled once and until the station's owner is known. They are 0 when polled and absent, and
///   the faction standing is 0 when the owner is in no faction.
/// - `sales_tax`: 7.5 × (1 − 0.11 × accounting). `broker_fee` (NPC station only):
///   3 − 0.3·broker relations − 0.03·faction standing − 0.02·corporation standing.
///   `min_margin`: ((1 + b) / (1 − b − t) − 1) × 100 for a known broker fee b and sales tax t.
///   All in percent, and not clamped: the formula cannot fall below 1.0%.
const FEES_SQL: &str = concat!(
    "
    SELECT f.character_id, f.location_id, f.is_structure,
           f.accounting, f.broker_relations,
           f.faction_standing::text AS faction_standing, f.corp_standing::text AS corp_standing,
           f.sales_tax::text AS sales_tax, f.broker_fee::text AS broker_fee,
           CASE WHEN f.broker_fee IS NOT NULL AND f.sales_tax IS NOT NULL THEN
               (((1 + f.broker_fee / 100) / (1 - f.broker_fee / 100 - f.sales_tax / 100) - 1) * 100)::text
           END AS min_margin
    FROM (",
    fee_rates_sql!(order_locations_sql!()),
    ") f
    ORDER BY f.character_id, f.location_id"
);

/// Like [`FEES_SQL`], for one fixed location (`$3`) instead of the locations of the current
/// orders: what the characters pay there whether or not they have an order there.
const LOCATION_FEES_SQL: &str = concat!(
    "
    SELECT f.character_id, f.location_id, f.is_structure,
           f.accounting, f.broker_relations,
           f.faction_standing::text AS faction_standing, f.corp_standing::text AS corp_standing,
           f.sales_tax::text AS sales_tax, f.broker_fee::text AS broker_fee,
           CASE WHEN f.broker_fee IS NOT NULL AND f.sales_tax IS NOT NULL THEN
               (((1 + f.broker_fee / 100) / (1 - f.broker_fee / 100 - f.sales_tax / 100) - 1) * 100)::text
           END AS min_margin
    FROM (",
    fee_rates_sql!(fixed_location_sql!()),
    ") f
    ORDER BY f.character_id"
);

/// Like [`LOCATION_FEES_SQL`], at the fee location of each character (see [`fee_location_sql`]).
const FEE_LOCATION_FEES_SQL: &str = concat!(
    "
    SELECT f.character_id, f.location_id, f.is_structure,
           f.accounting, f.broker_relations,
           f.faction_standing::text AS faction_standing, f.corp_standing::text AS corp_standing,
           f.sales_tax::text AS sales_tax, f.broker_fee::text AS broker_fee,
           CASE WHEN f.broker_fee IS NOT NULL AND f.sales_tax IS NOT NULL THEN
               (((1 + f.broker_fee / 100) / (1 - f.broker_fee / 100 - f.sales_tax / 100) - 1) * 100)::text
           END AS min_margin,
           NOT ($3::text = ANY(ch.scopes)) AS needs_location_scope
    FROM (",
    fee_rates_sql!(fee_location_sql!()),
    ") f
    JOIN characters ch ON ch.character_id = f.character_id
    ORDER BY f.character_id"
);

/// The rates part of [`FEES_SQL`], with its `$1` and `$2`: one row per (character, location) of the
/// current orders with `sales_tax` and `broker_fee` as `numeric` percent (and the inputs they are
/// worked out from). The orders listing joins it to work out the margin.
pub(crate) const FEE_RATES_SQL: &str = fee_rates_sql!(order_locations_sql!());

/// Like [`FEE_RATES_SQL`], for the locations of the stored sales. The trades listing joins it to
/// work out the margin of a sale.
pub(crate) const SALE_FEE_RATES_SQL: &str = fee_rates_sql!(sale_locations_sql!());

#[derive(sqlx::FromRow)]
struct CharacterFeeRow {
    #[sqlx(flatten)]
    row: FeeRow,
    needs_location_scope: bool,
}

async fn for_characters<'e>(
    executor: impl PgExecutor<'e>,
    character_ids: &[i64],
) -> Result<Vec<FeeRow>, sqlx::Error> {
    sqlx::query_as(FEES_SQL)
        .bind(character_ids)
        .bind(None::<Uuid>)
        .fetch_all(executor)
        .await
}

async fn sale_rates_for_characters<'e>(
    executor: impl PgExecutor<'e>,
    character_ids: &[i64],
) -> Result<Vec<(i64, i64, Option<String>, Option<String>)>, sqlx::Error> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT f.character_id, f.location_id, f.sales_tax::text, f.broker_fee::text
         FROM ({SALE_FEE_RATES_SQL}) f
         ORDER BY f.character_id, f.location_id"
    )))
    .bind(character_ids)
    .bind(None::<Uuid>)
    .fetch_all(executor)
    .await
}

async fn fee_locations<'e>(
    executor: impl PgExecutor<'e>,
    account_id: Option<Uuid>,
    character_ids: &[i64],
) -> Result<Vec<CharacterFee>, sqlx::Error> {
    let rows: Vec<CharacterFeeRow> = sqlx::query_as(FEE_LOCATION_FEES_SQL)
        .bind(character_ids)
        .bind(account_id)
        .bind(LOCATION_SCOPE)
        .bind(STATION_ID)
        .fetch_all(executor)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| CharacterFee {
            row: r.row,
            needs_location_scope: r.needs_location_scope,
        })
        .collect())
}

/// What the pages show of the fees of these characters, read inside the transaction.
async fn snapshot(
    conn: &mut PgConnection,
    character_ids: &[i64],
) -> Result<FeeSnapshot, sqlx::Error> {
    Ok(FeeSnapshot {
        orders: for_characters(&mut *conn, character_ids).await?,
        fee_location: fee_locations(&mut *conn, None, character_ids).await?,
        sales: sale_rates_for_characters(&mut *conn, character_ids).await?,
    })
}

async fn record_success(
    tx: &mut PgConnection,
    character_id: i64,
    kind: Kind,
    polled_at: OffsetDateTime,
    next_poll_at: OffsetDateTime,
    expires_at: Option<OffsetDateTime>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO fee_sync
                 (character_id, kind, next_poll_at, last_success_at, last_attempt_at, last_error,
                  esi_expires_at, failures)
             VALUES ($1, $2, $3, $4, $4, NULL, $5, 0)
             ON CONFLICT (character_id, kind) DO UPDATE SET
                 next_poll_at = excluded.next_poll_at,
                 last_success_at = excluded.last_success_at,
                 last_attempt_at = excluded.last_attempt_at,
                 last_error = NULL,
                 esi_expires_at = excluded.esi_expires_at,
                 failures = 0",
    )
    .bind(character_id)
    .bind(kind.as_str())
    .bind(next_poll_at)
    .bind(polled_at)
    .bind(expires_at)
    .execute(tx)
    .await?;
    Ok(())
}

#[async_trait]
impl FeeStore for PgFeeStore {
    async fn for_characters(&self, character_ids: &[i64]) -> StoreResult<Vec<FeeRow>> {
        Ok(for_characters(&self.pool, character_ids).await?)
    }

    async fn for_account(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
    ) -> StoreResult<Vec<FeeRow>> {
        Ok(sqlx::query_as(FEES_SQL)
            .bind(character_ids)
            .bind(account_id)
            .fetch_all(&self.pool)
            .await?)
    }

    async fn at_location(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        location_id: i64,
    ) -> StoreResult<Vec<FeeRow>> {
        Ok(sqlx::query_as(LOCATION_FEES_SQL)
            .bind(character_ids)
            .bind(account_id)
            .bind(location_id)
            .fetch_all(&self.pool)
            .await?)
    }

    async fn at_fee_location(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
    ) -> StoreResult<Vec<CharacterFee>> {
        Ok(fee_locations(&self.pool, Some(account_id), character_ids).await?)
    }

    async fn has_scope(&self, character_id: i64, scope: &str) -> StoreResult<Option<bool>> {
        Ok(
            sqlx::query_scalar("SELECT $2 = ANY(scopes) FROM characters WHERE character_id = $1")
                .bind(character_id)
                .bind(scope)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    async fn make_due(
        &self,
        character_id: i64,
        kind: Kind,
        now: OffsetDateTime,
    ) -> StoreResult<()> {
        sqlx::query(
            "UPDATE fee_sync SET next_poll_at = LEAST(next_poll_at, $3)
                     WHERE character_id = $1 AND kind = $2",
        )
        .bind(character_id)
        .bind(kind.as_str())
        .bind(now)
        .execute(&self.pool)
        .await?;
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
        let mut tx = self.pool.begin().await?;
        let before = snapshot(&mut tx, &[character_id]).await?;
        sqlx::query("DELETE FROM character_skills WHERE character_id = $1")
            .bind(character_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO character_skills (character_id, skill_id, active_level)
             SELECT $1, s, l FROM unnest($2::bigint[], $3::smallint[]) AS u(s, l)",
        )
        .bind(character_id)
        .bind(skill_ids)
        .bind(levels)
        .execute(&mut *tx)
        .await?;
        record_success(
            &mut tx,
            character_id,
            Kind::Skills,
            polled_at,
            next_poll_at,
            esi_expires_at,
        )
        .await?;
        let after = snapshot(&mut tx, &[character_id]).await?;
        tx.commit().await?;
        Ok(before != after)
    }

    async fn store_standings(
        &self,
        character_id: i64,
        standings: &[StandingInput],
        polled_at: OffsetDateTime,
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<bool> {
        let from_id: Vec<i64> = standings.iter().map(|s| s.from_id).collect();
        let from_type: Vec<&str> = standings.iter().map(|s| s.from_type.as_str()).collect();
        let standing: Vec<&str> = standings.iter().map(|s| s.standing.as_str()).collect();
        let mut tx = self.pool.begin().await?;
        let before = snapshot(&mut tx, &[character_id]).await?;
        sqlx::query("DELETE FROM character_standings WHERE character_id = $1")
            .bind(character_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO character_standings (character_id, from_id, from_type, standing)
             SELECT $1, i, t, s::numeric
             FROM unnest($2::bigint[], $3::text[], $4::text[]) AS u(i, t, s)
             ON CONFLICT DO NOTHING",
        )
        .bind(character_id)
        .bind(&from_id)
        .bind(&from_type)
        .bind(&standing)
        .execute(&mut *tx)
        .await?;
        record_success(
            &mut tx,
            character_id,
            Kind::Standings,
            polled_at,
            next_poll_at,
            esi_expires_at,
        )
        .await?;
        let after = snapshot(&mut tx, &[character_id]).await?;
        tx.commit().await?;
        Ok(before != after)
    }

    async fn store_location(
        &self,
        character_id: i64,
        docked: Option<(i64, &str)>,
        polled_at: OffsetDateTime,
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<bool> {
        let mut tx = self.pool.begin().await?;
        let before = snapshot(&mut tx, &[character_id]).await?;
        if let Some((location_id, kind)) = docked {
            sqlx::query(
                "INSERT INTO character_locations (character_id, location_id, kind, updated_at)
                 VALUES ($1, $2, $3, $4)
                 ON CONFLICT (character_id) DO UPDATE SET
                     location_id = excluded.location_id,
                     kind = excluded.kind,
                     updated_at = excluded.updated_at",
            )
            .bind(character_id)
            .bind(location_id)
            .bind(kind)
            .bind(polled_at)
            .execute(&mut *tx)
            .await?;
        }
        record_success(
            &mut tx,
            character_id,
            Kind::Location,
            polled_at,
            next_poll_at,
            esi_expires_at,
        )
        .await?;
        let after = snapshot(&mut tx, &[character_id]).await?;
        tx.commit().await?;
        Ok(before != after)
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
        sqlx::query(
            "INSERT INTO fee_sync (character_id, kind, next_poll_at, last_attempt_at, last_error, failures)
             VALUES ($1, $2, $3, $4, $5, 1)
             ON CONFLICT (character_id, kind) DO UPDATE SET
                 next_poll_at = $4 + LEAST(
                     $6::float8 * power(2, LEAST(fee_sync.failures, 20)) * interval '1 second',
                     $7::float8 * interval '1 second'),
                 last_attempt_at = $4,
                 last_error = $5,
                 failures = fee_sync.failures + 1",
        )
        .bind(character_id)
        .bind(kind.as_str())
        .bind(now + backoff_start)
        .bind(now)
        .bind(reason)
        .bind(backoff_start.as_seconds_f64())
        .bind(interval.as_seconds_f64())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn record_deferral(
        &self,
        character_id: i64,
        kind: Kind,
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO fee_sync (character_id, kind, next_poll_at, last_attempt_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (character_id, kind) DO UPDATE SET
                 next_poll_at = excluded.next_poll_at,
                 last_attempt_at = excluded.last_attempt_at",
        )
        .bind(character_id)
        .bind(kind.as_str())
        .bind(next_poll_at)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn stations_without_owner(&self, structure_id_min: i64) -> StoreResult<Vec<i64>> {
        Ok(sqlx::query_scalar(
            "SELECT l.location_id FROM (
                 SELECT location_id FROM current_orders
                 UNION
                 SELECT location_id FROM sales
                 UNION
                 SELECT location_id FROM character_locations WHERE kind = 'station'
             ) l
             WHERE l.location_id < $1
               AND NOT EXISTS (SELECT 1 FROM station_owners s WHERE s.station_id = l.location_id)
             ORDER BY l.location_id",
        )
        .bind(structure_id_min)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn station_owner_known(&self, station_id: i64) -> StoreResult<bool> {
        Ok(
            sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM station_owners WHERE station_id = $1)",
            )
            .bind(station_id)
            .fetch_one(&self.pool)
            .await?,
        )
    }

    async fn insert_station_owner(
        &self,
        station_id: i64,
        owner_id: i64,
        now: OffsetDateTime,
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO station_owners (station_id, owner_id, resolved_at) VALUES ($1, $2, $3)
         ON CONFLICT (station_id) DO NOTHING",
        )
        .bind(station_id)
        .bind(owner_id)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn store_station_owner(
        &self,
        station_id: i64,
        owner_id: i64,
        now: OffsetDateTime,
    ) -> StoreResult<Vec<Uuid>> {
        let mut tx = self.pool.begin().await?;
        let characters: Vec<(i64, Uuid)> = sqlx::query_as(
            "SELECT DISTINCT t.character_id, c.account_id FROM (
                 SELECT character_id FROM current_orders WHERE location_id = $1
                 UNION
                 SELECT character_id FROM sales WHERE location_id = $1
                 UNION
                 SELECT character_id FROM character_locations WHERE location_id = $1
             ) t
             JOIN characters c ON c.character_id = t.character_id",
        )
        .bind(station_id)
        .fetch_all(&mut *tx)
        .await?;
        let ids: Vec<i64> = characters.iter().map(|(id, _)| *id).collect();
        let before = snapshot(&mut tx, &ids).await?;
        sqlx::query(
            "INSERT INTO station_owners (station_id, owner_id, resolved_at) VALUES ($1, $2, $3)
             ON CONFLICT (station_id) DO NOTHING",
        )
        .bind(station_id)
        .bind(owner_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        let after = snapshot(&mut tx, &ids).await?;
        tx.commit().await?;

        let mut accounts: Vec<Uuid> = characters
            .iter()
            .filter(|(id, _)| before.of_character(*id) != after.of_character(*id))
            .map(|(_, account)| *account)
            .collect();
        accounts.sort_unstable();
        accounts.dedup();
        Ok(accounts)
    }

    async fn list_unscheduled(&self, kind: Kind, scope: Option<&str>) -> StoreResult<Vec<i64>> {
        Ok(sqlx::query_scalar(
            "SELECT c.character_id FROM characters c
                 WHERE NOT c.needs_relogin
                   AND ($2::text IS NULL OR $2 = ANY(c.scopes))
                   AND NOT EXISTS (SELECT 1 FROM fee_sync s
                                   WHERE s.character_id = c.character_id AND s.kind = $1)",
        )
        .bind(kind.as_str())
        .bind(scope)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn insert_schedule(
        &self,
        kind: Kind,
        character_ids: &[i64],
        next_poll_at: &[OffsetDateTime],
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO fee_sync (character_id, kind, next_poll_at)
                 SELECT c, $1, t FROM unnest($2::bigint[], $3::timestamptz[]) AS u(c, t)
                 ON CONFLICT (character_id, kind) DO NOTHING",
        )
        .bind(kind.as_str())
        .bind(character_ids)
        .bind(next_poll_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn claim_due(
        &self,
        now: OffsetDateTime,
        lease: Duration,
        limit: i64,
    ) -> StoreResult<Vec<(i64, Kind)>> {
        let claimed: Vec<(i64, String)> = sqlx::query_as(
            "UPDATE fee_sync w SET next_poll_at = $1 + $2::float8 * interval '1 second'
             WHERE (w.character_id, w.kind) IN (
                 SELECT s.character_id, s.kind FROM fee_sync s
                 JOIN characters c USING (character_id)
                 WHERE NOT c.needs_relogin AND s.next_poll_at <= $1
                   AND (s.kind <> 'location' OR $4 = ANY(c.scopes))
                 ORDER BY s.next_poll_at
                 LIMIT $3
                 FOR UPDATE OF s SKIP LOCKED)
             RETURNING w.character_id, w.kind",
        )
        .bind(now)
        .bind(lease.as_seconds_f64())
        .bind(limit)
        .bind(LOCATION_SCOPE)
        .fetch_all(&self.pool)
        .await?;
        Ok(claimed
            .into_iter()
            .filter_map(|(id, kind)| Kind::parse(&kind).map(|kind| (id, kind)))
            .collect())
    }
    async fn fee_sync(&self, character_id: i64, kind: Kind) -> StoreResult<Option<SyncState>> {
        Ok(sqlx::query_as(
            "SELECT next_poll_at, last_success_at, last_attempt_at, last_error, esi_expires_at, failures FROM fee_sync WHERE character_id = $1 AND kind = $2",
        )
        .bind(character_id)
        .bind(kind.as_str())
        .fetch_optional(&self.pool)
        .await?)
    }

    async fn seed_skill(&self, character_id: i64, skill_id: i64, level: i16) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO character_skills (character_id, skill_id, active_level)
             VALUES ($1, $2, $3)
             ON CONFLICT (character_id, skill_id) DO UPDATE SET active_level = excluded.active_level",
        )
        .bind(character_id)
        .bind(skill_id)
        .bind(level)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn seed_standing(&self, character_id: i64, standing: &StandingInput) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO character_standings (character_id, from_id, from_type, standing)
             VALUES ($1, $2, $3, $4::numeric)
             ON CONFLICT (character_id, from_id, from_type) DO UPDATE SET standing = excluded.standing",
        )
        .bind(character_id)
        .bind(standing.from_id)
        .bind(&standing.from_type)
        .bind(&standing.standing)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn seed_fee_success(
        &self,
        character_id: i64,
        kind: Kind,
        at: OffsetDateTime,
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO fee_sync (character_id, kind, next_poll_at, last_success_at, last_attempt_at)
             VALUES ($1, $2, $3, $3, $3)
             ON CONFLICT (character_id, kind) DO UPDATE SET
                 next_poll_at = excluded.next_poll_at, last_success_at = excluded.last_success_at,
                 last_attempt_at = excluded.last_attempt_at, last_error = NULL, failures = 0",
        )
        .bind(character_id)
        .bind(kind.as_str())
        .bind(at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn skills_of(&self, character_id: i64) -> StoreResult<Vec<(i64, i16)>> {
        Ok(sqlx::query_as(
            "SELECT skill_id, active_level FROM character_skills
             WHERE character_id = $1 ORDER BY skill_id",
        )
        .bind(character_id)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn standings_of(&self, character_id: i64) -> StoreResult<Vec<(i64, String, String)>> {
        Ok(sqlx::query_as(
            "SELECT from_id, from_type, standing::text FROM character_standings
             WHERE character_id = $1 ORDER BY from_id, from_type",
        )
        .bind(character_id)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn location_of(&self, character_id: i64) -> StoreResult<Option<(i64, String)>> {
        Ok(sqlx::query_as(
            "SELECT location_id, kind FROM character_locations WHERE character_id = $1",
        )
        .bind(character_id)
        .fetch_optional(&self.pool)
        .await?)
    }

    async fn station_owner(&self, station_id: i64) -> StoreResult<Option<i64>> {
        Ok(
            sqlx::query_scalar("SELECT owner_id FROM station_owners WHERE station_id = $1")
                .bind(station_id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    async fn station_owner_count(&self) -> StoreResult<i64> {
        Ok(sqlx::query_scalar("SELECT count(*) FROM station_owners")
            .fetch_one(&self.pool)
            .await?)
    }
}
