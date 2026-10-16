-- One entry per successful poll and character: the totals of its orders at that moment, including
-- polls that returned no orders. The dashboard's history reads this instead of order_snapshots,
-- which cannot show that all orders closed.
CREATE TABLE order_poll_totals (
    character_id bigint        NOT NULL REFERENCES characters (character_id) ON DELETE CASCADE,
    polled_at    timestamptz   NOT NULL,
    sell_value   numeric(20,2) NOT NULL,
    escrow       numeric(20,2) NOT NULL,
    sell_corp    numeric(20,2) NOT NULL,
    escrow_corp  numeric(20,2) NOT NULL,
    sell_count   integer       NOT NULL,
    buy_count    integer       NOT NULL,
    -- Allowed on a hypertable because it contains the time column.
    UNIQUE (character_id, polled_at)
);

SELECT create_hypertable('order_poll_totals', 'polled_at', chunk_time_interval => interval '30 days');

CREATE INDEX order_poll_totals_character_polled_idx ON order_poll_totals (character_id, polled_at DESC);

-- Existing snapshots become history. Polls that returned no orders left no snapshot rows and
-- cannot be recovered.
-- backfill
INSERT INTO order_poll_totals
    (character_id, polled_at, sell_value, escrow, sell_corp, escrow_corp, sell_count, buy_count)
SELECT character_id,
       polled_at,
       COALESCE(SUM(price * volume_remain) FILTER (WHERE NOT is_buy_order), 0),
       COALESCE(SUM(COALESCE(escrow, 0)) FILTER (WHERE is_buy_order), 0),
       COALESCE(SUM(price * volume_remain) FILTER (WHERE NOT is_buy_order AND is_corporation), 0),
       COALESCE(SUM(COALESCE(escrow, 0)) FILTER (WHERE is_buy_order AND is_corporation), 0),
       COUNT(*) FILTER (WHERE NOT is_buy_order),
       COUNT(*) FILTER (WHERE is_buy_order)
FROM order_snapshots
GROUP BY character_id, polled_at;
