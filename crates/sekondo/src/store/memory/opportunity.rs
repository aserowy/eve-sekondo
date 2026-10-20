use async_trait::async_trait;
use time::{Date, OffsetDateTime, Time};
use uuid::Uuid;

use super::{
    Shared,
    numeric::Numeric,
    station::daily_figures,
    tables::{SettingsRow, Tables},
};
use crate::{
    opportunities::{settings::Settings, tables::Stats},
    store::{OpportunityStore, StoreResult, SyncRow},
};

pub struct MemOpportunityStore {
    pub(super) shared: std::sync::Arc<Shared>,
}

fn name_of(tables: &Tables, id: i64) -> Option<String> {
    tables.names.get(&id).and_then(|n| n.name.clone())
}

#[async_trait]
impl OpportunityStore for MemOpportunityStore {
    async fn load_settings(&self, account_id: Uuid) -> StoreResult<Option<Settings>> {
        let tables = self.shared.lock();
        Ok(tables
            .opportunity_settings
            .get(&account_id)
            .map(|row| Settings {
                min_margin: row.min_margin.to_f64(),
                min_isk_per_day: row.min_isk_per_day.to_f64(),
                search: row.search.clone(),
                capital: row.capital.to_f64(),
            }))
    }

    async fn save_settings(&self, account_id: Uuid, settings: &Settings) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        if !tables.accounts.contains_key(&account_id) {
            return Err(crate::store::StoreError::MissingParent);
        }
        let fee_character_id = tables
            .opportunity_settings
            .get(&account_id)
            .and_then(|row| row.fee_character_id);
        tables
            .opportunity_settings
            .insert(account_id, SettingsRow::of(settings, fee_character_id));
        Ok(())
    }

    async fn load_fee_character(&self, account_id: Uuid) -> StoreResult<Option<i64>> {
        let tables = self.shared.lock();
        Ok(tables
            .opportunity_settings
            .get(&account_id)
            .and_then(|row| row.fee_character_id))
    }

    async fn save_fee_character(
        &self,
        account_id: Uuid,
        character_id: Option<i64>,
    ) -> StoreResult<()> {
        let mut tables = self.shared.lock();
        if !tables.accounts.contains_key(&account_id) {
            return Err(crate::store::StoreError::MissingParent);
        }
        tables
            .opportunity_settings
            .entry(account_id)
            .or_insert_with(|| SettingsRow::of(&Settings::default(), None))
            .fee_character_id = character_id;
        Ok(())
    }

    async fn station_sync(&self, region_id: i64, station_id: i64) -> StoreResult<Vec<SyncRow>> {
        let tables = self.shared.lock();
        Ok(tables
            .station_sync
            .iter()
            .filter(|((region, station, _), _)| *region == region_id && *station == station_id)
            .map(|((_, _, kind), row)| SyncRow {
                kind: kind.clone(),
                last_success_at: row.last_success_at,
                last_error: row.last_error.clone(),
            })
            .collect())
    }

    async fn own_snapshot_days(&self, region_id: i64, station_id: i64) -> StoreResult<i64> {
        let tables = self.shared.lock();
        let snapshots = tables
            .station_snapshots
            .iter()
            .filter(|s| s.region_id == region_id && s.station_id == station_id);
        let Some(oldest) = snapshots.clone().min_by_key(|s| s.recorded_at) else {
            return Ok(0);
        };
        let days: std::collections::BTreeSet<OffsetDateTime> = snapshots
            .filter(|s| s.type_id == oldest.type_id)
            .map(|s| s.recorded_at.replace_time(Time::MIDNIGHT))
            .collect();
        Ok(days.len() as i64)
    }

    async fn load_stats(&self, region_id: i64, station_id: i64) -> StoreResult<Vec<Stats>> {
        let tables = self.shared.lock();
        Ok(tables
            .item_stats
            .iter()
            .filter(|((region, station, _), _)| *region == region_id && *station == station_id)
            .filter_map(|((_, _, type_id), s)| {
                let (best_buy, best_sell) = (s.best_buy.as_ref()?, s.best_sell.as_ref()?);
                let (buy_near, sell_near) = (s.buy_near_orders?, s.sell_near_orders?);
                let cents = |n: &Numeric| n.mul(&Numeric::int(100)).to_i64();
                Some(Stats {
                    type_id: *type_id,
                    name: name_of(&tables, *type_id),
                    best_buy_cents: cents(best_buy)?,
                    best_sell_cents: cents(best_sell)?,
                    buy_near_orders: buy_near,
                    sell_near_orders: sell_near,
                    rho: s.rho,
                    volume_7d: s.volume_7d,
                    volume_30d: s.volume_30d,
                    sigma: s.sigma,
                    sma5: s.sma5,
                    sma20: s.sma20,
                    cross_day: s.cross_day,
                    volume_rising: s.volume_rising,
                    percent_b: s.percent_b,
                    rsi14: s.rsi14,
                    expansion_kind: s.expansion_kind.clone(),
                    expansion_weekday: s.expansion_weekday,
                    expansion_hour: s.expansion_hour,
                    expansion_sell_cents: s.expansion_sell.as_ref().and_then(cents),
                })
            })
            .collect())
    }

    async fn own_orders_at(
        &self,
        account_id: Uuid,
        character_ids: &[i64],
        location_id: i64,
    ) -> StoreResult<Vec<(i64, bool)>> {
        let tables = self.shared.lock();
        let found: std::collections::BTreeSet<(i64, bool)> = tables
            .current_orders
            .values()
            .filter(|o| o.location_id == location_id && character_ids.contains(&o.character_id))
            .filter(|o| {
                tables
                    .characters
                    .get(&o.character_id)
                    .is_some_and(|c| c.account_id == account_id)
            })
            .map(|o| (o.type_id, o.is_buy_order))
            .collect();
        Ok(found.into_iter().collect())
    }

    async fn history_days(
        &self,
        region_id: i64,
        type_id: i64,
        from: Date,
    ) -> StoreResult<Vec<(Date, f64, f64)>> {
        let tables = self.shared.lock();
        Ok(tables
            .market_history
            .iter()
            .filter(|((region, t, day), _)| *region == region_id && *t == type_id && *day >= from)
            .map(|((_, _, day), h)| (*day, h.average.to_f64(), h.volume as f64))
            .collect())
    }

    async fn price_points(
        &self,
        region_id: i64,
        station_id: i64,
        type_id: i64,
        from: OffsetDateTime,
    ) -> StoreResult<Vec<(OffsetDateTime, f64, f64, f64)>> {
        let tables = self.shared.lock();
        Ok(tables
            .hourly
            .iter()
            .filter(|((region, station, t, bucket), _)| {
                *region == region_id && *station == station_id && *t == type_id && *bucket >= from
            })
            .map(|((_, _, _, bucket), b)| {
                (
                    *bucket,
                    b.best_buy_avg.to_f64(),
                    b.best_sell_avg.to_f64(),
                    b.spread_avg.to_f64(),
                )
            })
            .collect())
    }

    async fn latest_depth(
        &self,
        region_id: i64,
        station_id: i64,
        type_id: i64,
    ) -> StoreResult<Option<(i64, i32, i64, i32)>> {
        let tables = self.shared.lock();
        Ok(tables
            .station_snapshots
            .iter()
            .filter(|s| {
                s.region_id == region_id && s.station_id == station_id && s.type_id == type_id
            })
            .max_by_key(|s| s.recorded_at)
            .map(|s| {
                (
                    s.buy_near_units,
                    s.buy_near_orders,
                    s.sell_near_units,
                    s.sell_near_orders,
                )
            }))
    }

    async fn item_hour_points(
        &self,
        region_id: i64,
        station_id: i64,
        type_id: i64,
        from: OffsetDateTime,
    ) -> StoreResult<Vec<(OffsetDateTime, f64, f64, i64)>> {
        let tables = self.shared.lock();
        Ok(tables
            .hourly
            .iter()
            .filter(|((region, station, t, bucket), _)| {
                *region == region_id && *station == station_id && *t == type_id && *bucket >= from
            })
            .map(|((_, _, _, bucket), b)| {
                (
                    *bucket,
                    b.spread_avg.to_f64(),
                    b.spread_max.to_f64(),
                    b.best_sell_max
                        .mul(&Numeric::int(100))
                        .to_i64()
                        .unwrap_or_default(),
                )
            })
            .collect())
    }

    async fn item_day_points(
        &self,
        region_id: i64,
        type_id: i64,
        from: Date,
    ) -> StoreResult<Vec<(Date, f64, i64)>> {
        let tables = self.shared.lock();
        let mut points: Vec<(Date, f64, i64)> =
            daily_figures(&tables, region_id, from, Some(type_id))
                .into_iter()
                .filter_map(|f| {
                    Some((
                        f.day,
                        f.spread?.to_f64(),
                        f.high.mul(&Numeric::int(100)).to_i64()?,
                    ))
                })
                .collect();
        points.sort_by_key(|p| p.0);
        Ok(points)
    }
}
