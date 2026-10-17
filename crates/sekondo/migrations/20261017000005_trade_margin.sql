-- Trade margin: the personal purchases from wallet transactions, every personal sell order ever
-- seen, and the stored result of matching the first to the second (first in, first out). Everything
-- per character cascades on unlink.

CREATE TABLE purchases (
    transaction_id bigint        PRIMARY KEY,
    character_id   bigint        NOT NULL REFERENCES characters (character_id) ON DELETE CASCADE,
    type_id        bigint        NOT NULL,
    location_id    bigint        NOT NULL,
    quantity       bigint        NOT NULL,
    unit_price     numeric(20,2) NOT NULL,
    bought_at      timestamptz   NOT NULL
);

CREATE INDEX purchases_allocation_idx
    ON purchases (character_id, type_id, bought_at, transaction_id);

-- A sell order as first seen: ESI moves `issued` when the order is repriced, this does not.
CREATE TABLE sell_order_history (
    order_id     bigint      PRIMARY KEY,
    character_id bigint      NOT NULL REFERENCES characters (character_id) ON DELETE CASCADE,
    type_id      bigint      NOT NULL,
    first_issued timestamptz NOT NULL,
    volume_total bigint      NOT NULL
);

CREATE INDEX sell_order_history_character_type_idx ON sell_order_history (character_id, type_id);

-- What the allocation gave each remembered order: the units it took and their total price.
CREATE TABLE sell_order_costs (
    order_id      bigint        PRIMARY KEY REFERENCES sell_order_history (order_id) ON DELETE CASCADE,
    covered_units bigint        NOT NULL,
    cost          numeric(24,2) NOT NULL
);

ALTER TABLE wealth_sync DROP CONSTRAINT wealth_sync_kind_check;
ALTER TABLE wealth_sync
    ADD CONSTRAINT wealth_sync_kind_check CHECK (kind IN ('wallet', 'assets', 'transactions'));

-- Sell orders the history already shows, with the issue time they had when first seen. Corporation
-- and buy orders are not remembered. The tests run everything after the marker line.
-- backfill
INSERT INTO sell_order_history (order_id, character_id, type_id, first_issued, volume_total)
SELECT DISTINCT ON (order_id) order_id, character_id, type_id, issued, volume_total
FROM order_snapshots
WHERE NOT is_buy_order AND NOT is_corporation
ORDER BY order_id, polled_at;
