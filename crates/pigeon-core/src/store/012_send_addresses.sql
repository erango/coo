-- Send-from addresses the user added or removed by hand (iCloud: its SMTP
-- only accepts the account's iCloud Mail addresses and aliases, which no
-- API lists). removed = 1 hides an address Coo detected from local mail.
CREATE TABLE send_addresses (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    email      TEXT NOT NULL,   -- lowercased
    removed    INTEGER NOT NULL DEFAULT 0,
    added_at   INTEGER NOT NULL,
    PRIMARY KEY (account_id, email)
) WITHOUT ROWID;
