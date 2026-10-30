//! Google contact photos: which photo URL each address has in an account's
//! contacts. Addresses are stored as `photos::address_hash`.

use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::Result;
use crate::photos::address_hash;

impl Store {
    /// Replace an account's contact photos with `photos` (address, URL),
    /// recording the time.
    pub fn replace_contact_photos(&mut self, account_id: &str, photos: &[(String, String)], now: i64) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM contact_photos WHERE account_id = ?1", [account_id])?;
        {
            let mut ins = tx.prepare_cached(
                "INSERT OR REPLACE INTO contact_photos (account_id, address_hash, url) VALUES (?1, ?2, ?3)",
            )?;
            for (email, url) in photos {
                ins.execute(params![account_id, address_hash(email), url])?;
            }
        }
        tx.execute(
            "INSERT INTO contact_photos_synced (account_id, synced_at) VALUES (?1, ?2)
             ON CONFLICT (account_id) DO UPDATE SET synced_at = excluded.synced_at",
            params![account_id, now],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// When an account's contact photos were last read; None if never.
    pub fn contact_photos_synced_at(&self, account_id: &str) -> Result<Option<i64>> {
        Ok(self
            .conn
            .prepare_cached("SELECT synced_at FROM contact_photos_synced WHERE account_id = ?1")?
            .query_row([account_id], |r| r.get(0))
            .optional()?)
    }

    /// The photo for `email` in any of these accounts' contacts, in the
    /// accounts' order: (account id, URL).
    pub fn contact_photo(&self, email: &str, account_ids: &[String]) -> Result<Option<(String, String)>> {
        let hash = address_hash(email);
        let mut stmt =
            self.conn.prepare_cached("SELECT url FROM contact_photos WHERE account_id = ?1 AND address_hash = ?2")?;
        for id in account_ids {
            if let Some(url) = stmt.query_row(params![id, hash], |r| r.get::<_, String>(0)).optional()? {
                return Ok(Some((id.clone(), url)));
            }
        }
        Ok(None)
    }
}
