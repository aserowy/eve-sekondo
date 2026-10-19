-- Trades: the personal sales from wallet transactions, kept like the purchases, and the cost each
-- sale took from the purchase lots (first in, first out, separate from the sell orders). Everything
-- per character cascades on unlink.

CREATE TABLE sales (
    transaction_id bigint        PRIMARY KEY,
    character_id   bigint        NOT NULL REFERENCES characters (character_id) ON DELETE CASCADE,
    type_id        bigint        NOT NULL,
    location_id    bigint        NOT NULL,
    quantity       bigint        NOT NULL,
    unit_price     numeric(20,2) NOT NULL,
    sold_at        timestamptz   NOT NULL
);

CREATE INDEX sales_allocation_idx ON sales (character_id, type_id, sold_at, transaction_id);
CREATE INDEX sales_page_idx ON sales (character_id, sold_at);

-- What the allocation gave each sale: the units it took and their total price.
CREATE TABLE sale_costs (
    transaction_id bigint        PRIMARY KEY REFERENCES sales (transaction_id) ON DELETE CASCADE,
    covered_units  bigint        NOT NULL,
    cost           numeric(24,2) NOT NULL
);

-- The next transactions poll of a character reads its whole ESI window once, so that the sales
-- from before this migration are filled in. The tests run everything after the marker line.
ALTER TABLE wealth_sync ADD COLUMN full_read boolean NOT NULL DEFAULT false;

-- full read
UPDATE wealth_sync SET full_read = true WHERE kind = 'transactions';
