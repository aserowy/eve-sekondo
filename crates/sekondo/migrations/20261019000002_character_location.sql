-- Where a character is docked, for the fee cards: the last station or structure ESI reported. A
-- character in open space keeps its last row. Per character, so it cascades on unlink and on a
-- change of owner (which deletes the character).
CREATE TABLE character_locations (
    character_id bigint      PRIMARY KEY REFERENCES characters (character_id) ON DELETE CASCADE,
    location_id  bigint      NOT NULL,
    kind         text        NOT NULL CHECK (kind IN ('station', 'structure')),
    updated_at   timestamptz NOT NULL
);

-- The location is polled like the skills and the standings.
ALTER TABLE fee_sync DROP CONSTRAINT fee_sync_kind_check;
ALTER TABLE fee_sync ADD CONSTRAINT fee_sync_kind_check CHECK (kind IN ('skills', 'standings', 'location'));
