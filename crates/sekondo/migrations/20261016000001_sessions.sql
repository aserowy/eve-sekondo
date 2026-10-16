-- Server-side sessions for tower-sessions (see src/session.rs).
CREATE TABLE sessions (
    id          text        PRIMARY KEY,
    data        jsonb       NOT NULL,
    expiry_date timestamptz NOT NULL
);

CREATE INDEX sessions_expiry_date_idx ON sessions (expiry_date);
