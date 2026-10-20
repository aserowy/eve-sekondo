use async_trait::async_trait;
use sqlx::{PgPool, Row, postgres::PgRow};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    dashboard::{CharacterFigures, HistoryPoint, TopItem},
    esi::EsiOrder,
    freshness::FreshnessRecord,
    orders::{Dir, OrderRow, Side, Sort, SortKey},
    range::{BUCKET_CTES, Range},
    store::{
        OrderStore, PollTotalsRow, RememberedSellOrder, StoreResult, SyncState,
        pg::fee::FEE_RATES_SQL,
    },
};

impl sqlx::FromRow<'_, PgRow> for CharacterFigures {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            freshness: FreshnessRecord::from_row(row)?,
            sell_cents: row.try_get("sell_cents")?,
            escrow_cents: row.try_get("escrow_cents")?,
            sell_corp_cents: row.try_get("sell_corp_cents")?,
            escrow_corp_cents: row.try_get("escrow_corp_cents")?,
            sell_count: row.try_get("sell_count")?,
            buy_count: row.try_get("buy_count")?,
        })
    }
}

pub struct PgOrderStore {
    pool: PgPool,
}

impl PgOrderStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Joins `st` to the current orders `o`: how many other public orders undercut each of them and
/// the best of those prices and their remaining volume, or NULL when there is no standing (design §5). The listing and the
/// change detection both use it, so a signal is sent exactly when what is shown changes.
///
/// A standing exists only when the order's pair was refreshed once, the order is not in a player
/// structure and, for a buy order, its own row in the market data tells its solar system.
/// A sell order is compared at its station. A buy order is compared at its station with the buy
/// orders that reach it: any order there, a `region` range, and a `solarsystem` or jump range
/// from the same solar system. The own range is not read.
pub(crate) const STANDING_JOIN: &str = "LEFT JOIN LATERAL (
        SELECT CASE WHEN a.available THEN c.undercut END AS undercut,
               CASE WHEN a.available THEN c.best_price END AS best_price,
               CASE WHEN a.available THEN c.undercut_volume END AS undercut_volume
        FROM (SELECT o.location_id < 100000000
                 AND EXISTS (SELECT 1 FROM market_sync s
                             WHERE s.region_id = o.region_id AND s.type_id = o.type_id
                               AND s.last_success_at IS NOT NULL)
                 AND (NOT o.is_buy_order
                      OR EXISTS (SELECT 1 FROM market_orders own WHERE own.order_id = o.order_id))
                 AS available) a,
        LATERAL (
            SELECT count(*) AS undercut,
                   CASE WHEN o.is_buy_order THEN max(m.price) ELSE min(m.price) END AS best_price,
                   sum(m.volume_remain)::bigint AS undercut_volume
            FROM market_orders m
            WHERE m.region_id = o.region_id AND m.type_id = o.type_id
              AND m.order_id <> o.order_id
              AND CASE WHEN o.is_buy_order THEN
                    m.is_buy_order AND m.price > o.price
                    AND (m.location_id = o.location_id
                         OR m.range = 'region'
                         OR (m.range <> 'station'
                             AND m.system_id = (SELECT own.system_id FROM market_orders own
                                                WHERE own.order_id = o.order_id)))
                  ELSE
                    NOT m.is_buy_order AND m.price < o.price AND m.location_id = o.location_id
                  END
        ) c
    ) st ON true";

/// The per-unit difference to the best price in cents, for an order that is undercut. Needs
/// `STANDING_JOIN` (`st`) and the current order (`o`).
pub(crate) const GAP_CENTS: &str =
    "CASE WHEN st.undercut > 0 THEN (abs(st.best_price - o.price) * 100)::bigint END";

/// The margin of a sell order in cents (design §6). Needs the cost row `sc`, the fee rates `f` and
/// the current order `o`. The cost per unit is the whole known cost over the total volume, so a
/// part that was sold already carries its share.
const MARGIN_CENTS: &str = "CASE WHEN NOT o.is_buy_order AND NOT o.is_corporation
          AND sc.covered_units = o.volume_total
          AND f.sales_tax IS NOT NULL AND f.broker_fee IS NOT NULL
     THEN (round(o.volume_remain * o.price * (1 - f.sales_tax / 100 - f.broker_fee / 100)
                 - o.volume_remain * sc.cost / o.volume_total, 2) * 100)::bigint
END";

/// The purchase cost of the remaining volume in cents, for an order whose cost covers its whole
/// volume.
const COST_CENTS: &str = "CASE WHEN sc.covered_units = o.volume_total AND o.volume_total > 0
     THEN (round(o.volume_remain * sc.cost / o.volume_total, 2) * 100)::bigint
END";

/// The average cost per unit in cents, for an order whose cost covers its whole volume.
const COST_PER_UNIT_CENTS: &str =
    "CASE WHEN sc.covered_units = o.volume_total AND o.volume_total > 0
     THEN round(sc.cost * 100 / o.volume_total)::bigint
END";

/// Columns of one order, as the parallel arrays `unnest` takes.
#[derive(Default)]
struct OrderColumns {
    order_id: Vec<i64>,
    type_id: Vec<i64>,
    region_id: Vec<i64>,
    location_id: Vec<i64>,
    is_buy_order: Vec<bool>,
    is_corporation: Vec<bool>,
    price_cents: Vec<i64>,
    volume_remain: Vec<i64>,
    volume_total: Vec<i64>,
    min_volume: Vec<i64>,
    range: Vec<String>,
    escrow_cents: Vec<Option<i64>>,
    issued: Vec<OffsetDateTime>,
    duration: Vec<i32>,
}

impl OrderColumns {
    fn new(orders: &[EsiOrder]) -> Self {
        let mut c = Self::default();
        for o in orders {
            c.order_id.push(o.order_id);
            c.type_id.push(o.type_id);
            c.region_id.push(o.region_id);
            c.location_id.push(o.location_id);
            c.is_buy_order.push(o.is_buy_order);
            c.is_corporation.push(o.is_corporation);
            c.price_cents.push(o.price_cents);
            c.volume_remain.push(o.volume_remain);
            c.volume_total.push(o.volume_total);
            c.min_volume.push(o.min_volume);
            c.range.push(o.range.clone());
            c.escrow_cents.push(o.escrow_cents);
            c.issued.push(o.issued);
            c.duration.push(o.duration);
        }
        c
    }
}

/// `SELECT` over the order arrays bound from `$3` on. Prices and escrow arrive as cents and are
/// turned into exact `numeric` here.
const ORDER_ROWS: &str = "SELECT o.order_id, o.type_id, o.region_id, o.location_id, o.is_buy_order,
        o.is_corporation, o.price_cents::numeric / 100 AS price, o.volume_remain, o.volume_total,
        o.min_volume, o.range, o.escrow_cents::numeric / 100 AS escrow, o.issued, o.duration
    FROM unnest($3::bigint[], $4::bigint[], $5::bigint[], $6::bigint[], $7::bool[], $8::bool[],
        $9::bigint[], $10::bigint[], $11::bigint[], $12::bigint[], $13::text[], $14::bigint[],
        $15::timestamptz[], $16::int[])
        AS o(order_id, type_id, region_id, location_id, is_buy_order, is_corporation, price_cents,
        volume_remain, volume_total, min_volume, range, escrow_cents, issued, duration)";

const ORDER_COLUMNS: &str =
    "order_id, type_id, region_id, location_id, is_buy_order, is_corporation,
    price, volume_remain, volume_total, min_volume, range, escrow, issued, duration";

macro_rules! bind_orders {
    ($query:expr, $c:expr) => {
        $query
            .bind(&$c.order_id)
            .bind(&$c.type_id)
            .bind(&$c.region_id)
            .bind(&$c.location_id)
            .bind(&$c.is_buy_order)
            .bind(&$c.is_corporation)
            .bind(&$c.price_cents)
            .bind(&$c.volume_remain)
            .bind(&$c.volume_total)
            .bind(&$c.min_volume)
            .bind(&$c.range)
            .bind(&$c.escrow_cents)
            .bind(&$c.issued)
            .bind(&$c.duration)
    };
}

/// The SQL `ORDER BY` terms for a direction. Only these literals ever reach the query text.
/// The standing and the gap put orders without a standing last in both directions, so the
/// first term does not follow the direction.
fn order_by(key: SortKey, direction: &str) -> String {
    match key {
        // `undercut > 0` is false for "Top", so it comes first in ascending order.
        SortKey::Standing => {
            format!("(st.undercut IS NULL), (st.undercut > 0) {direction}, st.undercut {direction}")
        }
        // A top order has no best price to differ from: it counts as no difference.
        SortKey::Gap => {
            format!("(st.undercut IS NULL), COALESCE(abs(st.best_price - o.price), 0) {direction}")
        }
        SortKey::UndercutVolume => {
            format!("(st.undercut IS NULL), COALESCE(st.undercut_volume, 0) {direction}")
        }
        SortKey::Margin => {
            format!("({MARGIN_CENTS}) IS NULL, ({MARGIN_CENTS}) {direction}")
        }
        other => format!("{} {direction}", expression(other)),
    }
}

fn expression(key: SortKey) -> &'static str {
    match key {
        SortKey::Item => "lower(COALESCE(tn.name, '#' || o.type_id))",
        SortKey::Price => "o.price",
        SortKey::Volume => "o.volume_remain",
        SortKey::Amount => {
            "CASE WHEN o.is_buy_order THEN COALESCE(o.escrow, 0) ELSE o.price * o.volume_remain END"
        }
        SortKey::TimeLeft => "o.issued + o.duration * interval '1 day'",
        SortKey::Standing | SortKey::Gap | SortKey::UndercutVolume | SortKey::Margin => {
            unreachable!("ordered by `order_by`")
        }
    }
}

#[async_trait]
impl OrderStore for PgOrderStore {
    /// The current orders of the selected characters, restricted to the account in the query itself
    /// so a selection can never reach another account's data.
    async fn list_orders(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        side: Side,
        sort: Sort,
    ) -> StoreResult<Vec<OrderRow>> {
        let direction = match sort.dir {
            Dir::Asc => "ASC",
            Dir::Desc => "DESC",
        };
        let sql = format!(
            "SELECT o.order_id, o.character_id, c.name AS character_name,
                o.type_id, tn.name AS type_name,
                o.region_id, rn.name AS region_name,
                o.location_id, ln.name AS location_name, (ln.id IS NOT NULL) AS location_known,
                o.is_corporation,
                (o.price * 100)::bigint AS price_cents,
                o.volume_remain, o.volume_total,
                (o.price * o.volume_remain * 100)::bigint AS value_cents,
                (o.escrow * 100)::bigint AS escrow_cents,
                o.issued, o.duration,
                st.undercut,
                (st.best_price * 100)::bigint AS best_price_cents,
                {GAP_CENTS} AS gap_cents,
                st.undercut_volume,
                CASE WHEN NOT o.is_buy_order AND NOT o.is_corporation THEN sc.covered_units END
                    AS covered_units,
                {MARGIN_CENTS} AS margin_cents,
                {COST_PER_UNIT_CENTS} AS cost_per_unit_cents,
                CASE WHEN NOT o.is_buy_order AND NOT o.is_corporation THEN {COST_CENTS} END
                    AS cost_cents
         FROM current_orders o
         JOIN characters c ON c.character_id = o.character_id
         LEFT JOIN names tn ON tn.id = o.type_id
         LEFT JOIN names rn ON rn.id = o.region_id
         LEFT JOIN names ln ON ln.id = o.location_id
         {STANDING_JOIN}
         LEFT JOIN sell_order_costs sc ON sc.order_id = o.order_id
         LEFT JOIN ({FEE_RATES_SQL}) f
                ON f.character_id = o.character_id AND f.location_id = o.location_id
         WHERE c.account_id = $2 AND o.character_id = ANY($1) AND o.is_buy_order = $3
         ORDER BY {}, o.order_id",
            order_by(sort.key, direction)
        );
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(character_ids)
            .bind(account_id)
            .bind(side == Side::Buy)
            .fetch_all(&self.pool)
            .await?)
    }

    async fn list_freshness(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
    ) -> StoreResult<Vec<FreshnessRecord>> {
        Ok(sqlx::query_as(
            "SELECT c.character_id, c.name, c.needs_relogin, s.last_success_at, s.next_poll_at
             FROM characters c
             LEFT JOIN order_sync s ON s.character_id = c.character_id
             WHERE c.account_id = $1 AND c.character_id = ANY($2)
             ORDER BY c.created_at, c.character_id",
        )
        .bind(account_id)
        .bind(character_ids)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn character_figures(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
    ) -> StoreResult<Vec<CharacterFigures>> {
        Ok(sqlx::query_as(
            "SELECT c.character_id, c.name, c.needs_relogin, s.last_success_at, s.next_poll_at,
                    (COALESCE(SUM(o.price * o.volume_remain) FILTER (WHERE NOT o.is_buy_order), 0) * 100)::bigint
                        AS sell_cents,
                    (COALESCE(SUM(COALESCE(o.escrow, 0)) FILTER (WHERE o.is_buy_order), 0) * 100)::bigint
                        AS escrow_cents,
                    (COALESCE(SUM(o.price * o.volume_remain) FILTER (WHERE NOT o.is_buy_order AND o.is_corporation), 0) * 100)::bigint
                        AS sell_corp_cents,
                    (COALESCE(SUM(COALESCE(o.escrow, 0)) FILTER (WHERE o.is_buy_order AND o.is_corporation), 0) * 100)::bigint
                        AS escrow_corp_cents,
                    COUNT(o.order_id) FILTER (WHERE NOT o.is_buy_order) AS sell_count,
                    COUNT(o.order_id) FILTER (WHERE o.is_buy_order) AS buy_count
             FROM characters c
             LEFT JOIN order_sync s ON s.character_id = c.character_id
             LEFT JOIN current_orders o ON o.character_id = c.character_id
             WHERE c.account_id = $1 AND c.character_id = ANY($2)
             GROUP BY c.character_id, c.name, c.needs_relogin, s.last_success_at, s.next_poll_at, c.created_at
             ORDER BY c.created_at, c.character_id",
        )
        .bind(account_id)
        .bind(character_ids)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn top_items(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        side: Side,
        limit: i64,
    ) -> StoreResult<Vec<TopItem>> {
        let amount = match side {
            Side::Sell => "o.price * o.volume_remain",
            Side::Buy => "COALESCE(o.escrow, 0)",
        };
        let sql = format!(
            "SELECT o.type_id,
                    COALESCE(n.name, '#' || o.type_id) AS label,
                    (SUM({amount}) * 100)::bigint AS cents
             FROM current_orders o
             JOIN characters c ON c.character_id = o.character_id
             LEFT JOIN names n ON n.id = o.type_id
             WHERE c.account_id = $1 AND o.character_id = ANY($2) AND o.is_buy_order = $3
             GROUP BY o.type_id, n.name
             ORDER BY SUM({amount}) DESC, lower(COALESCE(n.name, '#' || o.type_id)), o.type_id
             LIMIT {limit}"
        );
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(account_id)
            .bind(character_ids)
            .bind(side == Side::Buy)
            .fetch_all(&self.pool)
            .await?)
    }

    async fn history(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        range: Range,
        now: OffsetDateTime,
    ) -> StoreResult<Vec<HistoryPoint>> {
        let seconds = |d: Duration| d.as_seconds_f64();
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "{BUCKET_CTES}
             SELECT e.at,
                    (COALESCE(SUM(t.sell_value), 0) * 100)::bigint AS sell_cents,
                    (COALESCE(SUM(t.escrow), 0) * 100)::bigint AS escrow_cents,
                    COALESCE(SUM(t.sell_count), 0)::bigint AS sell_count,
                    COALESCE(SUM(t.buy_count), 0)::bigint AS buy_count,
                    COUNT(t.character_id) AS contributors
             FROM ends e
             CROSS JOIN chars c
             LEFT JOIN LATERAL (
                 SELECT p.character_id, p.sell_value, p.escrow, p.sell_count, p.buy_count
                 FROM order_poll_totals p
                 WHERE p.character_id = c.character_id AND p.polled_at <= e.at
                 ORDER BY p.polled_at DESC
                 LIMIT 1
             ) t ON true
             GROUP BY e.at
             ORDER BY e.at"
        )))
        .bind(account_id)
        .bind(character_ids)
        .bind(now)
        .bind(seconds(range.bucket()))
        .bind(seconds(range.span()))
        .fetch_all(&self.pool)
        .await?)
    }

    async fn replace_orders(
        &self,
        character_id: i64,
        polled_at: OffsetDateTime,
        orders: &[EsiOrder],
        next_poll_at: OffsetDateTime,
        esi_expires_at: Option<OffsetDateTime>,
    ) -> StoreResult<()> {
        let columns = OrderColumns::new(orders);
        let mut tx = self.pool.begin().await?;

        sqlx::query("DELETE FROM current_orders WHERE character_id = $1")
            .bind(character_id)
            .execute(&mut *tx)
            .await?;
        let insert_current = format!(
            "INSERT INTO current_orders (character_id, updated_at, {ORDER_COLUMNS})
             SELECT $1, $2, rows.* FROM ({ORDER_ROWS}) AS rows"
        );
        bind_orders!(
            sqlx::query(sqlx::AssertSqlSafe(insert_current))
                .bind(character_id)
                .bind(polled_at),
            columns
        )
        .execute(&mut *tx)
        .await?;

        let insert_snapshot = format!(
            "INSERT INTO order_snapshots (character_id, polled_at, {ORDER_COLUMNS})
             SELECT $1, $2, rows.* FROM ({ORDER_ROWS}) AS rows"
        );
        bind_orders!(
            sqlx::query(sqlx::AssertSqlSafe(insert_snapshot))
                .bind(character_id)
                .bind(polled_at),
            columns
        )
        .execute(&mut *tx)
        .await?;

        // Every personal sell order is remembered as first seen: a reprice moves `issued` in ESI
        // but not here, and a closed order stays.
        let new_types: Vec<i64> = sqlx::query_scalar(
            "INSERT INTO sell_order_history (order_id, character_id, type_id, first_issued, volume_total)
             SELECT order_id, character_id, type_id, issued, volume_total
             FROM current_orders
             WHERE character_id = $1 AND NOT is_buy_order AND NOT is_corporation
             ON CONFLICT (order_id) DO NOTHING
             RETURNING type_id",
        )
        .bind(character_id)
        .fetch_all(&mut *tx)
        .await?;
        // What a new order took is settled right away, in the same transaction.
        let mut new_types = new_types;
        new_types.sort_unstable();
        new_types.dedup();
        crate::store::pg::trade::recompute(&mut tx, character_id, &new_types).await?;

        // Computed from the rows just written, so the history and the current figures agree, and
        // a poll without orders records zeros. Two polls in the same instant keep the later one
        // rather than failing the whole poll.
        sqlx::query(
            "INSERT INTO order_poll_totals
                 (character_id, polled_at, sell_value, escrow, sell_corp, escrow_corp, sell_count, buy_count)
             SELECT $1, $2,
                    COALESCE(SUM(price * volume_remain) FILTER (WHERE NOT is_buy_order), 0),
                    COALESCE(SUM(COALESCE(escrow, 0)) FILTER (WHERE is_buy_order), 0),
                    COALESCE(SUM(price * volume_remain) FILTER (WHERE NOT is_buy_order AND is_corporation), 0),
                    COALESCE(SUM(COALESCE(escrow, 0)) FILTER (WHERE is_buy_order AND is_corporation), 0),
                    COUNT(*) FILTER (WHERE NOT is_buy_order),
                    COUNT(*) FILTER (WHERE is_buy_order)
             FROM current_orders WHERE character_id = $1
             ON CONFLICT (character_id, polled_at) DO UPDATE SET
                 sell_value = excluded.sell_value,
                 escrow = excluded.escrow,
                 sell_corp = excluded.sell_corp,
                 escrow_corp = excluded.escrow_corp,
                 sell_count = excluded.sell_count,
                 buy_count = excluded.buy_count",
        )
        .bind(character_id)
        .bind(polled_at)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO order_sync
                 (character_id, next_poll_at, last_success_at, last_attempt_at, last_error, esi_expires_at, failures)
             VALUES ($1, $2, $3, $3, NULL, $4, 0)
             ON CONFLICT (character_id) DO UPDATE SET
                 next_poll_at = excluded.next_poll_at,
                 last_success_at = excluded.last_success_at,
                 last_attempt_at = excluded.last_attempt_at,
                 last_error = NULL,
                 esi_expires_at = excluded.esi_expires_at,
                 failures = 0",
        )
        .bind(character_id)
        .bind(next_poll_at)
        .bind(polled_at)
        .bind(esi_expires_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn make_due(&self, character_id: i64, now: OffsetDateTime) -> StoreResult<()> {
        sqlx::query(
            "UPDATE order_sync SET next_poll_at = LEAST(next_poll_at, $2) WHERE character_id = $1",
        )
        .bind(character_id)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn record_failure(
        &self,
        character_id: i64,
        now: OffsetDateTime,
        reason: &str,
        backoff_start: Duration,
        interval: Duration,
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO order_sync (character_id, next_poll_at, last_attempt_at, last_error, failures)
             VALUES ($1, $2, $3, $4, 1)
             ON CONFLICT (character_id) DO UPDATE SET
                 next_poll_at = $3 + LEAST(
                     $5::float8 * power(2, LEAST(order_sync.failures, 20)) * interval '1 second',
                     $6::float8 * interval '1 second'),
                 last_attempt_at = $3,
                 last_error = $4,
                 failures = order_sync.failures + 1",
        )
        .bind(character_id)
        .bind(now + backoff_start.min(interval))
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
        now: OffsetDateTime,
        next_poll_at: OffsetDateTime,
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO order_sync (character_id, next_poll_at, last_attempt_at)
             VALUES ($1, $2, $3)
             ON CONFLICT (character_id) DO UPDATE SET
                 next_poll_at = excluded.next_poll_at,
                 last_attempt_at = excluded.last_attempt_at",
        )
        .bind(character_id)
        .bind(next_poll_at)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_unscheduled(&self) -> StoreResult<Vec<i64>> {
        Ok(sqlx::query_scalar(
            "SELECT c.character_id FROM characters c
             WHERE NOT c.needs_relogin
               AND NOT EXISTS (SELECT 1 FROM order_sync s WHERE s.character_id = c.character_id)",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    async fn insert_schedule(
        &self,
        character_ids: &[i64],
        next_poll_at: &[OffsetDateTime],
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO order_sync (character_id, next_poll_at)
             SELECT * FROM unnest($1::bigint[], $2::timestamptz[])
             ON CONFLICT (character_id) DO NOTHING",
        )
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
    ) -> StoreResult<Vec<i64>> {
        Ok(sqlx::query_scalar(
            "UPDATE order_sync SET next_poll_at = $1 + $2::float8 * interval '1 second'
             WHERE character_id IN (
                 SELECT s.character_id FROM order_sync s
                 JOIN characters c USING (character_id)
                 WHERE NOT c.needs_relogin AND s.next_poll_at <= $1
                 ORDER BY s.next_poll_at
                 LIMIT $3
                 FOR UPDATE OF s SKIP LOCKED)
             RETURNING character_id",
        )
        .bind(now)
        .bind(lease.as_seconds_f64())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?)
    }
    async fn order_sync(&self, character_id: i64) -> StoreResult<Option<SyncState>> {
        Ok(sqlx::query_as(
            "SELECT next_poll_at, last_success_at, last_attempt_at, last_error, esi_expires_at, failures FROM order_sync WHERE character_id = $1",
        )
        .bind(character_id)
        .fetch_optional(&self.pool)
        .await?)
    }
    async fn snapshot_counts(&self, character_id: i64) -> StoreResult<Vec<(OffsetDateTime, i64)>> {
        Ok(sqlx::query_as(
            "SELECT polled_at, count(*) FROM order_snapshots WHERE character_id = $1
             GROUP BY polled_at ORDER BY polled_at",
        )
        .bind(character_id)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn poll_totals(&self, character_id: i64) -> StoreResult<Vec<PollTotalsRow>> {
        let rows: Vec<(OffsetDateTime, i64, i64, i64, i64, i32, i32)> = sqlx::query_as(
            "SELECT polled_at, (sell_value * 100)::bigint, (escrow * 100)::bigint,
                    (sell_corp * 100)::bigint, (escrow_corp * 100)::bigint, sell_count, buy_count
             FROM order_poll_totals WHERE character_id = $1 ORDER BY polled_at",
        )
        .bind(character_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(polled_at, sell, escrow, sell_corp, escrow_corp, sells, buys)| PollTotalsRow {
                    polled_at,
                    sell_cents: sell,
                    escrow_cents: escrow,
                    sell_corp_cents: sell_corp,
                    escrow_corp_cents: escrow_corp,
                    sell_count: i64::from(sells),
                    buy_count: i64::from(buys),
                },
            )
            .collect())
    }

    async fn remembered_orders(&self, character_id: i64) -> StoreResult<Vec<RememberedSellOrder>> {
        let rows: Vec<(i64, i64, OffsetDateTime, i64)> = sqlx::query_as(
            "SELECT order_id, type_id, first_issued, volume_total FROM sell_order_history
             WHERE character_id = $1 ORDER BY order_id",
        )
        .bind(character_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(order_id, type_id, first_issued, volume_total)| RememberedSellOrder {
                    order_id,
                    type_id,
                    first_issued,
                    volume_total,
                },
            )
            .collect())
    }

    async fn seed_order(&self, character_id: i64, o: &EsiOrder) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO current_orders (character_id, order_id, type_id, region_id, location_id,
                 is_buy_order, is_corporation, price, volume_remain, volume_total, min_volume,
                 range, escrow, issued, duration)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8::numeric / 100, $9, $10, $11, $12,
                     $13::numeric / 100, $14, $15)
             ON CONFLICT (order_id) DO UPDATE SET
                 character_id = excluded.character_id, type_id = excluded.type_id,
                 region_id = excluded.region_id, location_id = excluded.location_id,
                 is_buy_order = excluded.is_buy_order, is_corporation = excluded.is_corporation,
                 price = excluded.price, volume_remain = excluded.volume_remain,
                 volume_total = excluded.volume_total, min_volume = excluded.min_volume,
                 range = excluded.range, escrow = excluded.escrow, issued = excluded.issued,
                 duration = excluded.duration, updated_at = now()",
        )
        .bind(character_id)
        .bind(o.order_id)
        .bind(o.type_id)
        .bind(o.region_id)
        .bind(o.location_id)
        .bind(o.is_buy_order)
        .bind(o.is_corporation)
        .bind(o.price_cents)
        .bind(o.volume_remain)
        .bind(o.volume_total)
        .bind(o.min_volume)
        .bind(&o.range)
        .bind(o.escrow_cents)
        .bind(o.issued)
        .bind(o.duration)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
