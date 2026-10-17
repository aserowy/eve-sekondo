-- The remaining volume of every public market order, for the volume sitting in front of an order.
-- Rows from before the first refresh with this column read 0 until that refresh replaces them.
ALTER TABLE market_orders ADD COLUMN volume_remain bigint NOT NULL DEFAULT 0;
