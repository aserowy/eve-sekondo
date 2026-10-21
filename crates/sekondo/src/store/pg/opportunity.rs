use std::collections::HashMap;

use async_trait::async_trait;
use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

use crate::{
    opportunities::{
        analysis::{Direction, ExpandedHour},
        settings::Settings,
        tables::Stats,
    },
    store::{OpportunityStore, StoreResult, SyncRow, WindowRow},
};

pub struct PgOpportunityStore {
    pool: PgPool,
}

impl PgOpportunityStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl OpportunityStore for PgOpportunityStore {
    async fn load_settings(&self, account_id: Uuid) -> StoreResult<Option<Settings>> {
        let row: Option<(f64, f64, String, f64)> = sqlx::query_as(
            "SELECT min_margin::float8, min_isk_per_day::float8, search, capital::float8
             FROM opportunity_settings WHERE account_id = $1",
        )
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(
            row.map(|(min_margin, min_isk_per_day, search, capital)| Settings {
                min_margin,
                min_isk_per_day,
                search,
                capital,
            }),
        )
    }

    async fn save_settings(&self, account_id: Uuid, settings: &Settings) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO opportunity_settings (account_id, min_margin, min_isk_per_day, search, capital)
             VALUES ($1, $2::float8::numeric, $3::float8::numeric, $4, $5::float8::numeric)
             ON CONFLICT (account_id) DO UPDATE SET
                 min_margin = excluded.min_margin, min_isk_per_day = excluded.min_isk_per_day,
                 search = excluded.search, capital = excluded.capital",
        )
        .bind(account_id)
        .bind(settings.min_margin)
        .bind(settings.min_isk_per_day)
        .bind(&settings.search)
        .bind(settings.capital)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn load_fee_character(&self, account_id: Uuid) -> StoreResult<Option<i64>> {
        let row: Option<(Option<i64>,)> = sqlx::query_as(
            "SELECT fee_character_id FROM opportunity_settings WHERE account_id = $1",
        )
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.and_then(|(id,)| id))
    }

    async fn save_fee_character(
        &self,
        account_id: Uuid,
        character_id: Option<i64>,
    ) -> StoreResult<()> {
        sqlx::query(
            "INSERT INTO opportunity_settings (account_id, fee_character_id) VALUES ($1, $2)
             ON CONFLICT (account_id) DO UPDATE SET fee_character_id = excluded.fee_character_id",
        )
        .bind(account_id)
        .bind(character_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn station_sync(&self, region_id: i64, station_id: i64) -> StoreResult<Vec<SyncRow>> {
        Ok(sqlx::query_as(
            "SELECT kind, last_success_at, last_error FROM station_sync
         WHERE region_id = $1 AND station_id = $2",
        )
        .bind(region_id)
        .bind(station_id)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn own_snapshot_days(&self, region_id: i64, station_id: i64) -> StoreResult<i64> {
        Ok(sqlx::query_scalar(
            "SELECT count(DISTINCT date_trunc('day', recorded_at)) FROM station_snapshots
         WHERE region_id = $1 AND station_id = $2
           AND type_id = (SELECT type_id FROM station_snapshots
                          WHERE region_id = $1 AND station_id = $2
                          ORDER BY recorded_at LIMIT 1)",
        )
        .bind(region_id)
        .bind(station_id)
        .fetch_one(&self.pool)
        .await?)
    }

    async fn load_stats(&self, region_id: i64, station_id: i64) -> StoreResult<Vec<Stats>> {
        let mut stats: Vec<Stats> = sqlx::query_as(
            "SELECT s.type_id, n.name,
                (s.best_buy * 100)::bigint AS best_buy_cents,
                (s.best_sell * 100)::bigint AS best_sell_cents,
                s.buy_near_orders, s.sell_near_orders, s.rho, s.volume_7d, s.volume_30d, s.sigma,
                s.sma5, s.sma20, s.cross_day, s.volume_rising, s.percent_b, s.rsi14
         FROM station_item_stats s
         LEFT JOIN names n ON n.id = s.type_id
         WHERE s.region_id = $1 AND s.station_id = $2
           AND s.best_buy IS NOT NULL AND s.best_sell IS NOT NULL
           AND s.buy_near_orders IS NOT NULL AND s.sell_near_orders IS NOT NULL",
        )
        .bind(region_id)
        .bind(station_id)
        .fetch_all(&self.pool)
        .await?;
        let windows: Vec<(i64, String, Option<i16>, i16, String, i64, i32, i32, f64)> =
            sqlx::query_as(
                "SELECT type_id, kind, weekday, hour, direction, price_cents, occurrences, covered,
                        v_win
                 FROM station_item_windows WHERE region_id = $1 AND station_id = $2
                 ORDER BY type_id, kind, weekday, hour, direction",
            )
            .bind(region_id)
            .bind(station_id)
            .fetch_all(&self.pool)
            .await?;
        let mut by_item: HashMap<i64, Vec<WindowRow>> = HashMap::new();
        for (type_id, kind, weekday, hour, direction, price, occurrences, covered, v_win) in windows
        {
            if let Some(window) = WindowRow::from_columns(
                type_id,
                &kind,
                weekday,
                hour,
                &direction,
                price,
                occurrences,
                covered,
                v_win,
            ) {
                by_item.entry(type_id).or_default().push(window);
            }
        }
        for s in &mut stats {
            s.windows = by_item.remove(&s.type_id).unwrap_or_default();
        }
        Ok(stats)
    }

    async fn own_orders_at(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        location_id: i64,
    ) -> StoreResult<Vec<(i64, bool)>> {
        Ok(sqlx::query_as(
            "SELECT DISTINCT o.type_id, o.is_buy_order FROM current_orders o
         JOIN characters c ON c.character_id = o.character_id
         WHERE c.account_id = $1 AND o.character_id = ANY($2) AND o.location_id = $3",
        )
        .bind(account_id)
        .bind(character_ids)
        .bind(location_id)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn history_days(
        &self,
        region_id: i64,
        type_id: i64,
        from: Date,
    ) -> StoreResult<Vec<(Date, f64, f64)>> {
        Ok(sqlx::query_as(
            "SELECT day, average::float8, volume::float8 FROM market_history
         WHERE region_id = $1 AND type_id = $2 AND day >= $3 ORDER BY day",
        )
        .bind(region_id)
        .bind(type_id)
        .bind(from)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn price_points(
        &self,
        region_id: i64,
        station_id: i64,
        type_id: i64,
        from: OffsetDateTime,
    ) -> StoreResult<Vec<(OffsetDateTime, f64, f64, f64)>> {
        Ok(sqlx::query_as(
            "SELECT bucket, best_buy_avg::float8, best_sell_avg::float8, spread_avg::float8
         FROM station_hourly
         WHERE region_id = $1 AND station_id = $2 AND type_id = $3 AND bucket >= $4
         ORDER BY bucket",
        )
        .bind(region_id)
        .bind(station_id)
        .bind(type_id)
        .bind(from)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn latest_depth(
        &self,
        region_id: i64,
        station_id: i64,
        type_id: i64,
    ) -> StoreResult<Option<(i64, i32, i64, i32)>> {
        Ok(sqlx::query_as(
            "SELECT buy_near_units, buy_near_orders, sell_near_units, sell_near_orders
         FROM station_snapshots
         WHERE region_id = $1 AND station_id = $2 AND type_id = $3
         ORDER BY recorded_at DESC LIMIT 1",
        )
        .bind(region_id)
        .bind(station_id)
        .bind(type_id)
        .fetch_optional(&self.pool)
        .await?)
    }

    async fn item_hour_points(
        &self,
        region_id: i64,
        station_id: i64,
        type_id: i64,
        from: OffsetDateTime,
    ) -> StoreResult<Vec<(OffsetDateTime, f64, f64, i64)>> {
        Ok(sqlx::query_as(
            "SELECT bucket, spread_avg::float8, spread_max::float8, round(best_sell_max * 100)::bigint
         FROM station_hourly
         WHERE region_id = $1 AND station_id = $2 AND type_id = $3 AND bucket >= $4
         ORDER BY bucket",
        )
        .bind(region_id)
        .bind(station_id)
        .bind(type_id)
        .bind(from)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn item_expanded_hours(
        &self,
        region_id: i64,
        station_id: i64,
        type_id: i64,
        from: OffsetDateTime,
    ) -> StoreResult<Vec<ExpandedHour>> {
        let rows: Vec<(OffsetDateTime, String, f64, i64, i64)> = sqlx::query_as(
            "SELECT start, direction, ratio, extreme_cents, taken FROM station_expanded_hours
             WHERE region_id = $1 AND station_id = $2 AND type_id = $3 AND start >= $4
             ORDER BY start",
        )
        .bind(region_id)
        .bind(station_id)
        .bind(type_id)
        .bind(from)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|(start, direction, ratio, extreme_cents, taken)| {
                Some(ExpandedHour {
                    start,
                    direction: Direction::parse(&direction)?,
                    ratio,
                    extreme_cents,
                    taken,
                })
            })
            .collect())
    }

    async fn item_day_points(
        &self,
        region_id: i64,
        type_id: i64,
        from: Date,
    ) -> StoreResult<Vec<(Date, f64, i64)>> {
        Ok(sqlx::query_as(
            "SELECT day, spread::float8, round(high * 100)::bigint FROM station_daily_figures
         WHERE region_id = $1 AND type_id = $2 AND day >= $3 ORDER BY day",
        )
        .bind(region_id)
        .bind(type_id)
        .bind(from)
        .fetch_all(&self.pool)
        .await?)
    }
}
