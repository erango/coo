-- Folder unread/total counters maintained by triggers, so the sidebar never
-- counts membership rows. Membership carries its own `unread` bit: on an
-- account or message delete the cascade removes the message row before its
-- memberships, so the delete trigger can't look the flag up.
ALTER TABLE message_folders ADD COLUMN unread INTEGER NOT NULL DEFAULT 0;
UPDATE message_folders SET unread = (SELECT NOT is_read FROM messages WHERE pk = message_pk);

CREATE TABLE folder_counts (
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    folder_id  TEXT NOT NULL,
    total      INTEGER NOT NULL DEFAULT 0,
    unread     INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (account_id, folder_id)
) WITHOUT ROWID;
INSERT INTO folder_counts (account_id, folder_id, total, unread)
    SELECT account_id, folder_id, COUNT(*), SUM(unread) FROM message_folders GROUP BY account_id, folder_id;

CREATE TRIGGER message_folders_count_ai AFTER INSERT ON message_folders BEGIN
    INSERT INTO folder_counts (account_id, folder_id, total, unread) VALUES (new.account_id, new.folder_id, 1, new.unread)
    ON CONFLICT (account_id, folder_id) DO UPDATE SET total = total + 1, unread = unread + excluded.unread;
END;
CREATE TRIGGER message_folders_count_ad AFTER DELETE ON message_folders BEGIN
    UPDATE folder_counts SET total = total - 1, unread = unread - old.unread
    WHERE account_id = old.account_id AND folder_id = old.folder_id;
END;
CREATE TRIGGER message_folders_count_au AFTER UPDATE OF unread ON message_folders WHEN old.unread IS NOT new.unread BEGIN
    UPDATE folder_counts SET unread = unread - old.unread + new.unread
    WHERE account_id = new.account_id AND folder_id = new.folder_id;
END;
CREATE TRIGGER messages_unread_au AFTER UPDATE OF is_read ON messages WHEN old.is_read IS NOT new.is_read BEGIN
    UPDATE message_folders SET unread = NOT new.is_read WHERE message_pk = new.pk;
END;
