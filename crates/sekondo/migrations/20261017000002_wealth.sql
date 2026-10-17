-- Wealth beyond orders: the wallet balance and the inventory of each character, ESI's market
-- prices to value the inventory with, and the history of both. Everything per character cascades
-- on unlink; prices are public and belong to nobody.

-- One row per successful wallet poll. The current balance is the latest row.
CREATE TABLE wallet_balances (
    character_id bigint        NOT NULL REFERENCES characters (character_id) ON DELETE CASCADE,
    polled_at    timestamptz   NOT NULL,
    balance      numeric(20,2) NOT NULL,
    -- Allowed on a hypertable because it contains the time column.
    UNIQUE (character_id, polled_at)
);

SELECT create_hypertable('wallet_balances', 'polled_at', chunk_time_interval => interval '30 days');

CREATE INDEX wallet_balances_character_polled_idx ON wallet_balances (character_id, polled_at DESC);

-- What each character owns now. Replaced as a whole by every successful assets poll.
-- top_location_id is the outermost location (a station, a structure or a solar system), NULL when
-- it cannot be resolved.
CREATE TABLE current_assets (
    item_id           bigint  PRIMARY KEY,
    character_id      bigint  NOT NULL REFERENCES characters (character_id) ON DELETE CASCADE,
    type_id           bigint  NOT NULL,
    location_id       bigint  NOT NULL,
    location_type     text    NOT NULL,
    location_flag     text    NOT NULL,
    quantity          bigint  NOT NULL,
    is_singleton      boolean NOT NULL,
    is_blueprint_copy boolean NOT NULL,
    top_location_id   bigint
);

CREATE INDEX current_assets_character_idx ON current_assets (character_id);

-- ESI's market prices: public data, one row per item, and the sync state of the single refresh.
CREATE TABLE item_prices (
    type_id       bigint PRIMARY KEY,
    average_price numeric(20,2)
);

CREATE TABLE price_sync (
    -- At most one row: the refresh is one call for everybody.
    singleton       boolean     PRIMARY KEY DEFAULT true CHECK (singleton),
    next_poll_at    timestamptz NOT NULL,
    last_success_at timestamptz,
    last_attempt_at timestamptz,
    last_error      text,
    esi_expires_at  timestamptz,
    failures        integer     NOT NULL DEFAULT 0
);

-- The value of a character's inventory, written in every transaction that changes the assets or
-- the prices, so the latest row always equals the live valuation.
CREATE TABLE inventory_values (
    character_id bigint        NOT NULL REFERENCES characters (character_id) ON DELETE CASCADE,
    recorded_at  timestamptz   NOT NULL,
    value        numeric(20,2) NOT NULL,
    UNIQUE (character_id, recorded_at)
);

SELECT create_hypertable('inventory_values', 'recorded_at', chunk_time_interval => interval '30 days');

CREATE INDEX inventory_values_character_recorded_idx ON inventory_values (character_id, recorded_at DESC);

CREATE TABLE wealth_sync (
    character_id    bigint      NOT NULL REFERENCES characters (character_id) ON DELETE CASCADE,
    kind            text        NOT NULL CHECK (kind IN ('wallet', 'assets')),
    next_poll_at    timestamptz NOT NULL,
    last_success_at timestamptz,
    last_attempt_at timestamptz,
    last_error      text,
    esi_expires_at  timestamptz,
    failures        integer     NOT NULL DEFAULT 0,
    PRIMARY KEY (character_id, kind)
);
