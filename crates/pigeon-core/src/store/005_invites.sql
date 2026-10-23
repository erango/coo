-- RFC 5545 UID, to link invite emails to calendar events.
ALTER TABLE events ADD COLUMN ical_uid TEXT;
CREATE INDEX events_ical_uid ON events (ical_uid);
-- Parsed meeting invite carried by a message (JSON `model::Invite`).
ALTER TABLE bodies ADD COLUMN invite_json TEXT;
