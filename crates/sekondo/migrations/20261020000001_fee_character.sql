-- The character whose fees the opportunity analyses prefer. No foreign key: unlinking a character
-- must not touch the filters, and a stale id is never among the selected characters.
ALTER TABLE opportunity_settings ADD COLUMN fee_character_id bigint;
