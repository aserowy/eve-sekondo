-- Hourly and daily rollups of the station snapshots, as hierarchical continuous aggregates, and
-- the daily figures the analyses read. Only completed periods are read (`materialized_only`).

CREATE MATERIALIZED VIEW station_hourly
WITH (timescaledb.continuous, timescaledb.materialized_only = true) AS
SELECT
    region_id, station_id, type_id,
    time_bucket(interval '1 hour', recorded_at) AS bucket,
    min(best_buy)  AS best_buy_min,  max(best_buy)  AS best_buy_max,  avg(best_buy)  AS best_buy_avg,
    min(best_sell) AS best_sell_min, max(best_sell) AS best_sell_max, avg(best_sell) AS best_sell_avg,
    min(spread)    AS spread_min,    max(spread)    AS spread_max,    avg(spread)    AS spread_avg,
    avg(buy_near_units)   AS buy_near_units_avg,   avg(buy_near_orders)  AS buy_near_orders_avg,
    avg(sell_near_units)  AS sell_near_units_avg,  avg(sell_near_orders) AS sell_near_orders_avg,
    avg(buy_units)        AS buy_units_avg,        avg(sell_units)       AS sell_units_avg,
    sum(buy_taken)::bigint AS buy_taken,           sum(sell_taken)::bigint AS sell_taken,
    count(*)              AS snapshots
FROM station_snapshots
GROUP BY region_id, station_id, type_id, bucket
WITH NO DATA;

-- Averages are weighted by the snapshots of each hour, so a day averages over its snapshots.
CREATE MATERIALIZED VIEW station_daily
WITH (timescaledb.continuous, timescaledb.materialized_only = true) AS
SELECT
    region_id, station_id, type_id,
    time_bucket(interval '1 day', bucket) AS bucket,
    min(best_buy_min)  AS best_buy_min,  max(best_buy_max)  AS best_buy_max,
    sum(best_buy_avg * snapshots) / sum(snapshots)  AS best_buy_avg,
    min(best_sell_min) AS best_sell_min, max(best_sell_max) AS best_sell_max,
    sum(best_sell_avg * snapshots) / sum(snapshots) AS best_sell_avg,
    min(spread_min)    AS spread_min,    max(spread_max)    AS spread_max,
    sum(spread_avg * snapshots) / sum(snapshots)    AS spread_avg,
    sum(buy_near_units_avg * snapshots) / sum(snapshots)   AS buy_near_units_avg,
    sum(buy_near_orders_avg * snapshots) / sum(snapshots)  AS buy_near_orders_avg,
    sum(sell_near_units_avg * snapshots) / sum(snapshots)  AS sell_near_units_avg,
    sum(sell_near_orders_avg * snapshots) / sum(snapshots) AS sell_near_orders_avg,
    sum(buy_units_avg * snapshots) / sum(snapshots)        AS buy_units_avg,
    sum(sell_units_avg * snapshots) / sum(snapshots)       AS sell_units_avg,
    sum(buy_taken)::bigint AS buy_taken, sum(sell_taken)::bigint AS sell_taken,
    sum(snapshots)::bigint AS snapshots,
    count(*)        AS hours_covered
FROM station_hourly
GROUP BY region_id, station_id, type_id, time_bucket(interval '1 day', bucket)
WITH NO DATA;

SELECT add_continuous_aggregate_policy('station_hourly',
    start_offset => interval '2 days', end_offset => interval '1 hour',
    schedule_interval => interval '15 minutes');
SELECT add_continuous_aggregate_policy('station_daily',
    start_offset => interval '4 days', end_offset => interval '1 day',
    schedule_interval => interval '1 hour');
SELECT add_retention_policy('station_hourly', interval '30 days');
SELECT add_retention_policy('station_daily', interval '30 days');

-- One row per item and history day. Low, high and spread come from the day's own rollup when it
-- covers at least 20 of 24 hours, otherwise from the history; average and volume always come from
-- the history, so indicators never mix transaction prices with book prices.
CREATE VIEW station_daily_figures AS
SELECT
    h.region_id,
    d.station_id,
    h.type_id,
    h.day,
    h.average,
    h.volume,
    CASE WHEN d.hours_covered >= 20 THEN d.best_buy_min  ELSE h.lowest  END AS low,
    CASE WHEN d.hours_covered >= 20 THEN d.best_sell_max ELSE h.highest END AS high,
    CASE WHEN d.hours_covered >= 20 THEN d.spread_max
         ELSE (h.highest - h.lowest) / h.lowest END AS spread,
    CASE WHEN d.hours_covered >= 20 THEN 'own' ELSE 'history' END AS source,
    d.hours_covered
FROM market_history h
LEFT JOIN station_daily d
    ON d.region_id = h.region_id AND d.type_id = h.type_id AND d.bucket = h.day::timestamptz;
