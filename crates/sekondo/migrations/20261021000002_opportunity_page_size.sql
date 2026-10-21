-- The page size the account chose for the opportunity tables. NULL: it never chose one, so the
-- default applies. No check constraint: the sizes on offer are the application's to change.
ALTER TABLE opportunity_settings ADD COLUMN per_page integer;
