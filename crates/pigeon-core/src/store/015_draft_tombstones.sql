-- Drafts that were sent or discarded (by the app or an agent's `coo`
-- helper): an editor still holding one must not save it back. Pruned after
-- a month.
CREATE TABLE draft_tombstones (
    id TEXT PRIMARY KEY,
    at INTEGER NOT NULL  -- epoch ms
) WITHOUT ROWID;
