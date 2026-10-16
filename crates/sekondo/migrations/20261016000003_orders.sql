-- Market orders: the current copy per character, the poll history (a TimescaleDB hypertable),
-- per-character sync state and a global name cache. Every per-character table references
-- characters(character_id) ON DELETE CASCADE.
CREATE EXTENSION IF NOT EXISTS timescaledb;

CREATE TABLE current_orders (
    order_id       bigint        PRIMARY KEY,
    character_id   bigint        NOT NULL REFERENCES characters (character_id) ON DELETE CASCADE,
    type_id        bigint        NOT NULL,
    region_id      bigint        NOT NULL,
    location_id    bigint        NOT NULL,
    is_buy_order   boolean       NOT NULL,
    is_corporation boolean       NOT NULL,
    price          numeric(20,2) NOT NULL,
    volume_remain  bigint        NOT NULL,
    volume_total   bigint        NOT NULL,
    min_volume     bigint        NOT NULL,
    range          text          NOT NULL,
    escrow         numeric(20,2),
    issued         timestamptz   NOT NULL,
    duration       integer       NOT NULL,
    updated_at     timestamptz   NOT NULL DEFAULT now()
);

CREATE INDEX current_orders_character_id_idx ON current_orders (character_id);

-- No primary key: TimescaleDB would require the time column in it, and nothing reads by key.
CREATE TABLE order_snapshots (
    polled_at      timestamptz   NOT NULL,
    character_id   bigint        NOT NULL REFERENCES characters (character_id) ON DELETE CASCADE,
    order_id       bigint        NOT NULL,
    type_id        bigint        NOT NULL,
    region_id      bigint        NOT NULL,
    location_id    bigint        NOT NULL,
    is_buy_order   boolean       NOT NULL,
    is_corporation boolean       NOT NULL,
    price          numeric(20,2) NOT NULL,
    volume_remain  bigint        NOT NULL,
    volume_total   bigint        NOT NULL,
    min_volume     bigint        NOT NULL,
    range          text          NOT NULL,
    escrow         numeric(20,2),
    issued         timestamptz   NOT NULL,
    duration       integer       NOT NULL
);

SELECT create_hypertable('order_snapshots', 'polled_at', chunk_time_interval => interval '7 days');

CREATE INDEX order_snapshots_character_polled_idx ON order_snapshots (character_id, polled_at DESC);

CREATE TABLE order_sync (
    character_id    bigint      PRIMARY KEY REFERENCES characters (character_id) ON DELETE CASCADE,
    next_poll_at    timestamptz NOT NULL,
    last_success_at timestamptz,
    last_attempt_at timestamptz,
    last_error      text,
    esi_expires_at  timestamptz,
    failures        integer     NOT NULL DEFAULT 0
);

-- Item, region, station and structure names are public information, so this is not per account.
-- A NULL name is a structure ESI would not describe; it is retried after retry_after.
CREATE TABLE names (
    id          bigint      PRIMARY KEY,
    category    text        NOT NULL,
    name        text,
    resolved_at timestamptz NOT NULL,
    retry_after timestamptz
);
