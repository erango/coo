-- Google contact photos for sender avatars: per account, the photo URL of
-- each address in its contacts (keyed by `photos::address_hash`, so no
-- address is stored), and when the list was last read.
CREATE TABLE contact_photos (
    account_id   TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    address_hash TEXT NOT NULL,
    url          TEXT NOT NULL,
    PRIMARY KEY (account_id, address_hash)
) WITHOUT ROWID;
CREATE INDEX contact_photos_by_address ON contact_photos (address_hash);
CREATE TABLE contact_photos_synced (
    account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    synced_at  INTEGER NOT NULL
);
