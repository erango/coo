-- OAuth scopes granted to an account's sign-in (space-separated, as the
-- token endpoint reports them). Missing row = unknown (signed in before
-- Coo recorded scopes).
CREATE TABLE account_scopes (
    account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    scopes     TEXT NOT NULL
);
