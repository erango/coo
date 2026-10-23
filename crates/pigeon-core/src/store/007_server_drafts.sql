-- Server-side copies of local drafts (Gmail draft id, Graph message id,
-- IMAP id) and which account holds them. Mirrors Draft.server in the JSON.
ALTER TABLE drafts ADD COLUMN server_account TEXT;
ALTER TABLE drafts ADD COLUMN server_id TEXT;
