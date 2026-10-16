CREATE TABLE accounts (
    id         uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at timestamptz NOT NULL DEFAULT now()
);

-- Convention for later changes: every per-character table references characters(character_id)
-- ON DELETE CASCADE, so unlinking a character deletes all of its data.
CREATE TABLE characters (
    character_id  bigint      PRIMARY KEY,
    account_id    uuid        NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    name          text        NOT NULL,
    owner_hash    text        NOT NULL,
    scopes        text[]      NOT NULL DEFAULT '{}',
    needs_relogin boolean     NOT NULL DEFAULT false,
    created_at    timestamptz NOT NULL DEFAULT now(),
    updated_at    timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX characters_account_id_idx ON characters (account_id);

-- Token columns hold AES-256-GCM ciphertext (nonce || ciphertext), never plain tokens.
CREATE TABLE character_tokens (
    character_id      bigint      PRIMARY KEY REFERENCES characters (character_id) ON DELETE CASCADE,
    refresh_token     bytea       NOT NULL,
    access_token      bytea,
    access_expires_at timestamptz,
    updated_at        timestamptz NOT NULL DEFAULT now()
);
