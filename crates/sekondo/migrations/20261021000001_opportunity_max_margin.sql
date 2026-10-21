-- Items whose net margin is above this are outliers or traps: the tables do not list them. A
-- fraction, like min_margin: 0.80 is 80%, which existing accounts get without a backfill.
ALTER TABLE opportunity_settings ADD COLUMN max_margin numeric NOT NULL DEFAULT 0.80;
