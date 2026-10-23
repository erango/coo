-- Local-first drafts (JSON-encoded `compose::Draft`).
CREATE TABLE drafts (
    id         TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    json       TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
