CREATE TABLE accounts (
    id           TEXT PRIMARY KEY,
    provider     TEXT NOT NULL,
    email        TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT '',
    created_at   INTEGER NOT NULL
);

-- Exchange folders and Gmail labels.
CREATE TABLE folders (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    id         TEXT NOT NULL,
    name       TEXT NOT NULL,
    parent_id  TEXT,
    role       TEXT,
    kind       TEXT NOT NULL,
    PRIMARY KEY (account_id, id)
) WITHOUT ROWID;

CREATE TABLE messages (
    pk                  INTEGER PRIMARY KEY,
    account_id          TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    id                  TEXT NOT NULL,
    thread_id           TEXT NOT NULL,
    internet_message_id TEXT,
    subject             TEXT NOT NULL DEFAULT '',
    from_name           TEXT,
    from_addr           TEXT,
    to_json             TEXT NOT NULL DEFAULT '[]',
    cc_json             TEXT NOT NULL DEFAULT '[]',
    -- Flattened to/cc names + addresses, indexed by FTS.
    recipients          TEXT NOT NULL DEFAULT '',
    snippet             TEXT NOT NULL DEFAULT '',
    received_at         INTEGER NOT NULL,
    is_read             INTEGER NOT NULL DEFAULT 0,
    is_flagged          INTEGER NOT NULL DEFAULT 0,
    has_attachments     INTEGER NOT NULL DEFAULT 0,
    is_draft            INTEGER NOT NULL DEFAULT 0,
    UNIQUE (account_id, id)
);
CREATE INDEX messages_thread ON messages (account_id, thread_id);
CREATE INDEX messages_received ON messages (received_at DESC);

-- Membership, with received_at denormalized so a folder lists straight off the index.
CREATE TABLE message_folders (
    message_pk  INTEGER NOT NULL REFERENCES messages(pk) ON DELETE CASCADE,
    account_id  TEXT NOT NULL,
    folder_id   TEXT NOT NULL,
    received_at INTEGER NOT NULL,
    PRIMARY KEY (message_pk, folder_id)
) WITHOUT ROWID;
CREATE INDEX message_folders_list ON message_folders (account_id, folder_id, received_at DESC);

CREATE VIRTUAL TABLE messages_fts USING fts5(
    subject, from_name, from_addr, recipients, snippet,
    content = 'messages', content_rowid = 'pk',
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER messages_fts_ai AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts (rowid, subject, from_name, from_addr, recipients, snippet)
    VALUES (new.pk, new.subject, new.from_name, new.from_addr, new.recipients, new.snippet);
END;
CREATE TRIGGER messages_fts_ad AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts (messages_fts, rowid, subject, from_name, from_addr, recipients, snippet)
    VALUES ('delete', old.pk, old.subject, old.from_name, old.from_addr, old.recipients, old.snippet);
END;
CREATE TRIGGER messages_fts_au AFTER UPDATE ON messages BEGIN
    INSERT INTO messages_fts (messages_fts, rowid, subject, from_name, from_addr, recipients, snippet)
    VALUES ('delete', old.pk, old.subject, old.from_name, old.from_addr, old.recipients, old.snippet);
    INSERT INTO messages_fts (rowid, subject, from_name, from_addr, recipients, snippet)
    VALUES (new.pk, new.subject, new.from_name, new.from_addr, new.recipients, new.snippet);
END;

-- Lazily fetched bodies (phase 1).
CREATE TABLE bodies (
    message_pk INTEGER PRIMARY KEY REFERENCES messages(pk) ON DELETE CASCADE,
    html       TEXT,
    text       TEXT,
    fetched_at INTEGER NOT NULL
);

CREATE TABLE attachments (
    message_pk   INTEGER NOT NULL REFERENCES messages(pk) ON DELETE CASCADE,
    id           TEXT NOT NULL,
    name         TEXT NOT NULL,
    content_type TEXT,
    size         INTEGER,
    content_id   TEXT,
    is_inline    INTEGER NOT NULL DEFAULT 0,
    -- Path under the cache dir once downloaded.
    local_path   TEXT,
    PRIMARY KEY (message_pk, id)
) WITHOUT ROWID;

CREATE TABLE calendars (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    id         TEXT NOT NULL,
    name       TEXT NOT NULL,
    color      TEXT,
    is_primary INTEGER NOT NULL DEFAULT 0,
    can_edit   INTEGER NOT NULL DEFAULT 0,
    visible    INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (account_id, id)
) WITHOUT ROWID;

CREATE TABLE events (
    account_id     TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    calendar_id    TEXT NOT NULL,
    id             TEXT NOT NULL,
    series_id      TEXT,
    title          TEXT NOT NULL DEFAULT '',
    start_at       INTEGER NOT NULL,
    end_at         INTEGER NOT NULL,
    all_day        INTEGER NOT NULL DEFAULT 0,
    timezone       TEXT,
    location       TEXT,
    organizer_name TEXT,
    organizer_addr TEXT,
    attendees_json TEXT NOT NULL DEFAULT '[]',
    my_response    TEXT NOT NULL DEFAULT 'none',
    online_url     TEXT,
    cancelled      INTEGER NOT NULL DEFAULT 0,
    updated_at     INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (account_id, calendar_id, id)
);
CREATE INDEX events_start ON events (start_at);

-- Provider sync cursors: Graph delta links, Gmail historyId, Calendar syncTokens.
CREATE TABLE sync_state (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    scope      TEXT NOT NULL,
    cursor     TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (account_id, scope)
) WITHOUT ROWID;

-- Optimistic local mutations awaiting replay to the server (phase 1).
CREATE TABLE outbox (
    id         INTEGER PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    op         TEXT NOT NULL,
    payload    TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    attempts   INTEGER NOT NULL DEFAULT 0,
    last_error TEXT
);
