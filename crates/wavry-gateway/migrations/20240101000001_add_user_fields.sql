-- Add username and public_key columns
ALTER TABLE users ADD COLUMN username TEXT NOT NULL DEFAULT '';
ALTER TABLE users ADD COLUMN public_key TEXT NOT NULL DEFAULT '';

-- Make username unique (after populating if needed, but here we assume empty/new DB is fine or manual fix)
-- SQLite doesn't support adding UNIQUE constraint via ALTER TABLE easily on existing column without re-creating table.
-- For now, we just add the column. Uniqueness enforcing in app or create unique index.
CREATE UNIQUE INDEX idx_users_username ON users(username);
