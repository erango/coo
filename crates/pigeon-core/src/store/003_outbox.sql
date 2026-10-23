ALTER TABLE outbox ADD COLUMN state TEXT NOT NULL DEFAULT 'pending';
ALTER TABLE outbox ADD COLUMN next_attempt_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE outbox ADD COLUMN snapshot_json TEXT NOT NULL DEFAULT '[]';
CREATE INDEX outbox_pending ON outbox (account_id, state, id);
