-- Watches (saved rules for mail you care about) and Needs you (what they
-- caught). Written by the app and by the `coo` helper an agent runs.
CREATE TABLE watches (
    id          TEXT PRIMARY KEY,
    json        TEXT NOT NULL,   -- crate::watches::Watch
    created_at  INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE needs_you (
    id          INTEGER PRIMARY KEY,
    watch_id    TEXT NOT NULL REFERENCES watches(id) ON DELETE CASCADE,
    account_id  TEXT NOT NULL,
    message_id  TEXT NOT NULL,
    thread_id   TEXT NOT NULL,
    caught_at   INTEGER NOT NULL,
    done_at     INTEGER,
    done_by     TEXT NOT NULL DEFAULT '',
    done_note   TEXT NOT NULL DEFAULT '',
    UNIQUE (watch_id, account_id, message_id)
);
CREATE INDEX needs_you_open ON needs_you (done_at, caught_at DESC);
