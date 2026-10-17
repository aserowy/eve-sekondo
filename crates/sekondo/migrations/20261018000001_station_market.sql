-- The built-in trading station: its current book, a top-of-book snapshot per refresh, ESI's daily
-- history, the per-item statistics derived from them, and the per-account filter settings. All of
-- it is public and keyed by region and station, except the settings.

CREATE TABLE station_sync (
    region_id       bigint      NOT NULL,
    station_id      bigint      NOT NULL,
    kind            text        NOT NULL CHECK (kind IN ('book', 'history')),
    next_poll_at    timestamptz NOT NULL,
    last_success_at timestamptz,
    last_attempt_at timestamptz,
    last_error      text,
    esi_expires_at  timestamptz,
    failures        integer     NOT NULL DEFAULT 0,
    PRIMARY KEY (region_id, station_id, kind)
);

-- The current orders at the station, replaced by every complete refresh.
CREATE TABLE station_orders (
    region_id     bigint        NOT NULL,
    station_id    bigint        NOT NULL,
    order_id      bigint        PRIMARY KEY,
    type_id       bigint        NOT NULL,
    is_buy_order  boolean       NOT NULL,
    price         numeric(20,2) NOT NULL,
    volume_remain bigint        NOT NULL
);

CREATE INDEX station_orders_item_idx ON station_orders (region_id, station_id, type_id);

-- One row per tradable item and refresh. Every figure is its own column.
CREATE TABLE station_snapshots (
    recorded_at      timestamptz   NOT NULL,
    region_id        bigint        NOT NULL,
    station_id       bigint        NOT NULL,
    type_id          bigint        NOT NULL,
    best_buy         numeric(20,2) NOT NULL,
    best_sell        numeric(20,2) NOT NULL,
    spread           numeric(20,8) NOT NULL,
    -- Units and orders priced within 1% of the side's best price.
    buy_near_units   bigint        NOT NULL,
    buy_near_orders  integer       NOT NULL,
    sell_near_units  bigint        NOT NULL,
    sell_near_orders integer       NOT NULL,
    buy_units        bigint        NOT NULL,
    sell_units       bigint        NOT NULL,
    -- Units taken since the previous refresh, from orders present in both.
    buy_taken        bigint        NOT NULL,
    sell_taken       bigint        NOT NULL
);

SELECT create_hypertable('station_snapshots', 'recorded_at', chunk_time_interval => interval '1 day');

CREATE INDEX station_snapshots_item_idx
    ON station_snapshots (region_id, station_id, type_id, recorded_at DESC);

ALTER TABLE station_snapshots SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'type_id',
    timescaledb.compress_orderby = 'recorded_at'
);
SELECT add_compression_policy('station_snapshots', interval '1 day');
SELECT add_retention_policy('station_snapshots', interval '30 days');

-- ESI's daily history per item. A daily job deletes days older than 30.
CREATE TABLE market_history (
    region_id   bigint        NOT NULL,
    type_id     bigint        NOT NULL,
    day         date          NOT NULL,
    average     numeric(20,2) NOT NULL,
    highest     numeric(20,2) NOT NULL,
    lowest      numeric(20,2) NOT NULL,
    volume      bigint        NOT NULL,
    order_count bigint        NOT NULL,
    PRIMARY KEY (region_id, type_id, day)
);

CREATE TABLE history_sync (
    region_id       bigint      NOT NULL,
    type_id         bigint      NOT NULL,
    next_poll_at    timestamptz NOT NULL,
    last_success_at timestamptz,
    last_attempt_at timestamptz,
    last_error      text,
    esi_expires_at  timestamptz,
    failures        integer     NOT NULL DEFAULT 0,
    PRIMARY KEY (region_id, type_id)
);

-- Everything about an item that does not depend on an account, recomputed at three points: the
-- book fields after a book refresh, the indicator fields after a history pass, the expansion
-- fields when the hourly rollup advanced.
CREATE TABLE station_item_stats (
    region_id         bigint        NOT NULL,
    station_id        bigint        NOT NULL,
    type_id           bigint        NOT NULL,
    -- book fields
    book_at           timestamptz,
    best_buy          numeric(20,2),
    best_sell         numeric(20,2),
    buy_near_orders   integer,
    sell_near_orders  integer,
    rho               double precision,
    -- indicator fields
    indicators_at     timestamptz,
    volume_7d         double precision,
    volume_30d        double precision,
    sigma             double precision,
    returns           integer,
    sma5              double precision,
    sma20             double precision,
    cross_day         date,
    volume_rising     boolean,
    percent_b         double precision,
    rsi14             double precision,
    -- expansion fields: the window with the highest sell price, or none
    expansion_at      timestamptz,
    expansion_kind    text CHECK (expansion_kind IN ('hour', 'weekday_hour', 'weekday')),
    expansion_weekday smallint,
    expansion_hour    smallint,
    expansion_sell    numeric(20,2),
    PRIMARY KEY (region_id, station_id, type_id)
);

CREATE TABLE opportunity_settings (
    account_id      uuid          PRIMARY KEY REFERENCES accounts (id) ON DELETE CASCADE,
    min_margin      numeric       NOT NULL DEFAULT 0.05,
    min_isk_per_day numeric       NOT NULL DEFAULT 5000000,
    search          text          NOT NULL DEFAULT '',
    capital         numeric       NOT NULL DEFAULT 100000000
);
