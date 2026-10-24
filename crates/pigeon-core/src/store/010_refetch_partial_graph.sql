-- Graph delta "updated" items carry only the changed properties (say,
-- isRead), and sync stored them as whole headers: blank subject and sender,
-- date 0, so they sank to the bottom of the list as "(unknown)". Drop the
-- delta cursor of every Microsoft folder holding such a row; the next sync
-- re-reads those folders in full and rewrites the headers.
DELETE FROM sync_state
WHERE (account_id, scope) IN (
    SELECT DISTINCT m.account_id, 'folder:' || mf.folder_id
    FROM messages m JOIN message_folders mf ON mf.message_pk = m.pk
    WHERE m.received_at = 0 AND m.account_id LIKE 'microsoft:%'
);
