-- The address an account goes by (Settings › Accounts › Main address): the
-- sidebar shows it and new mail comes from it. One of the account's
-- send-from addresses (an alias); missing row = the account's own address.
CREATE TABLE main_addresses (
    account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    email      TEXT NOT NULL   -- lowercased
) WITHOUT ROWID;
