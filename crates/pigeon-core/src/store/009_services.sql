-- Connected services that aren't mail/calendar accounts (Zoom today). The
-- OAuth refresh token lives in the token store; this is who's connected.
CREATE TABLE services (
    id           TEXT PRIMARY KEY,   -- 'zoom'
    email        TEXT NOT NULL,
    display_name TEXT NOT NULL,
    connected_at INTEGER NOT NULL
);

-- Zoom meetings Coo created for calendar events, so moving or deleting the
-- event updates or deletes the meeting. Only meetings Coo made are listed.
CREATE TABLE zoom_meetings (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    event_id   TEXT NOT NULL,
    meeting_id TEXT NOT NULL,
    join_url   TEXT NOT NULL,
    PRIMARY KEY (account_id, event_id)
);
