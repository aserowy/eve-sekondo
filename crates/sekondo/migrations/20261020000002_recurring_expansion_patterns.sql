-- no-transaction
-- Recurring spread expansions are found from a year of rollups instead of 30 days.
--
-- The hourly and daily rollups are kept for 365 days and compressed after 7 days, which is well
-- beyond the 2 and 4 days their refresh policies reach back. Snapshots and history keep 30 days.
-- Each completed UTC day is evaluated once into `station_flagged_days` and
-- `station_expanded_hours`, and the recurring windows of every item are stored in
-- `station_item_windows`, so the analysis never reads the rollups of a whole year again.
--
-- Every step is its own transaction, like `20261018000003`: removing a policy locks TimescaleDB's
-- job catalog, and a running job holds its own lock while it waits for that catalog. Every step
-- is idempotent, so a migration that stopped halfway runs again from the top on the next start.

BEGIN;
SELECT remove_retention_policy('station_hourly', if_exists => true);
COMMIT;

BEGIN;
SELECT add_retention_policy('station_hourly', interval '365 days', if_not_exists => true);
COMMIT;

BEGIN;
SELECT remove_retention_policy('station_daily', if_exists => true);
COMMIT;

BEGIN;
SELECT add_retention_policy('station_daily', interval '365 days', if_not_exists => true);
COMMIT;

BEGIN;
ALTER MATERIALIZED VIEW station_hourly SET (
    timescaledb.compress = true,
    timescaledb.compress_segmentby = 'region_id, station_id, type_id',
    timescaledb.compress_orderby = 'bucket'
);
COMMIT;

BEGIN;
SELECT add_compression_policy('station_hourly', interval '7 days', if_not_exists => true);
COMMIT;

BEGIN;
ALTER MATERIALIZED VIEW station_daily SET (
    timescaledb.compress = true,
    timescaledb.compress_segmentby = 'region_id, station_id, type_id',
    timescaledb.compress_orderby = 'bucket'
);
COMMIT;

BEGIN;
SELECT add_compression_policy('station_daily', interval '7 days', if_not_exists => true);
COMMIT;

-- One row per item and completed UTC day: its hours with a rollup and how many of them expanded.
-- The day is covered when `hours` is at least 20.
BEGIN;
CREATE TABLE IF NOT EXISTS station_flagged_days (
    region_id      bigint   NOT NULL,
    station_id     bigint   NOT NULL,
    type_id        bigint   NOT NULL,
    day            date     NOT NULL,
    hours          smallint NOT NULL,
    expanded_hours smallint NOT NULL,
    PRIMARY KEY (region_id, station_id, type_id, day)
);
CREATE INDEX IF NOT EXISTS station_flagged_days_day
    ON station_flagged_days (region_id, station_id, day);
COMMIT;

-- The expanded hours of the covered days. `ratio` is the trough (minimum best buy over the day's
-- median average best buy) of a dip or the peak (maximum best sell over the day's median average
-- best sell) of a spike; `extreme_cents` is that minimum or maximum; `taken` the units taken from
-- the side that moved.
BEGIN;
CREATE TABLE IF NOT EXISTS station_expanded_hours (
    region_id     bigint           NOT NULL,
    station_id    bigint           NOT NULL,
    type_id       bigint           NOT NULL,
    start         timestamptz      NOT NULL,
    direction     text             NOT NULL CHECK (direction IN ('dip', 'spike')),
    ratio         double precision NOT NULL,
    extreme_cents bigint           NOT NULL,
    taken         bigint           NOT NULL,
    PRIMARY KEY (region_id, station_id, type_id, start)
);
CREATE INDEX IF NOT EXISTS station_expanded_hours_start
    ON station_expanded_hours (region_id, station_id, start);
COMMIT;

-- Every recurring window of an item. `price_cents` is S for a spike and B for a dip.
BEGIN;
CREATE TABLE IF NOT EXISTS station_item_windows (
    region_id   bigint           NOT NULL,
    station_id  bigint           NOT NULL,
    type_id     bigint           NOT NULL,
    kind        text             NOT NULL CHECK (kind IN ('hour', 'weekday_hour')),
    weekday     smallint,
    hour        smallint         NOT NULL,
    direction   text             NOT NULL CHECK (direction IN ('dip', 'spike')),
    price_cents bigint           NOT NULL,
    occurrences integer          NOT NULL,
    covered     integer          NOT NULL,
    v_win       double precision NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS station_item_windows_key
    ON station_item_windows (region_id, station_id, type_id, kind, (coalesce(weekday, -1)), hour,
                             direction);
COMMIT;

-- The windows live in their own table now. The next refresh recomputes every item.
BEGIN;
ALTER TABLE station_item_stats
    DROP COLUMN IF EXISTS expansion_kind,
    DROP COLUMN IF EXISTS expansion_weekday,
    DROP COLUMN IF EXISTS expansion_hour,
    DROP COLUMN IF EXISTS expansion_sell;
UPDATE station_item_stats SET expansion_at = NULL;
COMMIT;
