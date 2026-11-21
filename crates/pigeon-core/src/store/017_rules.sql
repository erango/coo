-- Mail rules (crate::rules) and what they did, for Settings › Rules ›
-- Activity and its Undo.
CREATE TABLE rules (
    id          TEXT PRIMARY KEY,
    json        TEXT NOT NULL,   -- crate::rules::Rule
    created_at  INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE rule_log (
    id          INTEGER PRIMARY KEY,
    rule_id     TEXT NOT NULL,
    account_id  TEXT NOT NULL,
    message_id  TEXT NOT NULL,
    at          INTEGER NOT NULL,
    undone_at   INTEGER,
    json        TEXT NOT NULL,   -- crate::rules::Logged, minus id/at/undone_at
    UNIQUE (rule_id, account_id, message_id)
);
CREATE INDEX rule_log_at ON rule_log (at DESC);
