-- The public market orders of every (region, item) pair a linked character has an order for, and
-- the sync state per pair. Public data: no account or character column.
CREATE TABLE market_orders (
    order_id     bigint        PRIMARY KEY,
    region_id    bigint        NOT NULL,
    type_id      bigint        NOT NULL,
    location_id  bigint        NOT NULL,
    system_id    bigint        NOT NULL,
    is_buy_order boolean       NOT NULL,
    price        numeric(20,2) NOT NULL,
    range        text          NOT NULL
);

CREATE INDEX market_orders_region_type_idx ON market_orders (region_id, type_id);

CREATE TABLE market_sync (
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
