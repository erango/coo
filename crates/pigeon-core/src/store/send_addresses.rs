//! Send-from addresses no API lists (iCloud): detected from the account's
//! own mail, plus the ones the user added or removed by hand.

use std::collections::HashMap;

use rusqlite::params;

use super::Store;
use crate::Result;
use crate::compose::is_icloud_mail;

/// A From of mail in Sent is certainly ours; a lone recipient of incoming
/// mail very likely is.
const SENT_WEIGHT: u32 = 3;
const RECEIVED_WEIGHT: u32 = 1;

impl Store {
    /// The account's iCloud Mail addresses (icloud.com / me.com / mac.com)
    /// seen in its own mail, most used first: From of messages in its Sent
    /// folder, and the recipient of messages elsewhere that went to exactly
    /// one address (To + Cc), so other people's iCloud addresses on group
    /// mail aren't picked up. Drafts, Spam and Trash don't count.
    pub fn detected_icloud_addresses(&self, account_id: &str) -> Result<Vec<String>> {
        Ok(self.send_address_candidates(account_id)?.into_iter().filter(|e| is_icloud_mail(e)).collect())
    }

    /// The same evidence for addresses at any domain, most used first: an
    /// iCloud+ custom domain's addresses look like anyone else's until the
    /// domain's mail servers are checked (the engine does that).
    pub fn send_address_candidates(&self, account_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT lower(m.from_addr), 1 FROM messages m
             WHERE m.account_id = ?1 AND m.from_addr IS NOT NULL
               AND EXISTS (SELECT 1 FROM message_folders mf
                           JOIN folders f ON f.account_id = mf.account_id AND f.id = mf.folder_id
                           WHERE mf.message_pk = m.pk AND f.role = 'sent')
             UNION ALL
             SELECT lower(coalesce(json_extract(m.to_json, '$[0].email'), json_extract(m.cc_json, '$[0].email'))), 0
             FROM messages m
             WHERE m.account_id = ?1
               AND json_array_length(m.to_json) + json_array_length(m.cc_json) = 1
               AND NOT EXISTS (SELECT 1 FROM message_folders mf
                               JOIN folders f ON f.account_id = mf.account_id AND f.id = mf.folder_id
                               WHERE mf.message_pk = m.pk AND f.role IN ('sent', 'drafts', 'spam', 'trash'))",
        )?;
        let rows = stmt.query_map([account_id], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, bool>(1)?)))?;
        let mut score: HashMap<String, u32> = HashMap::new();
        for row in rows {
            let (email, sent) = row?;
            let Some(email) = email.map(|e| e.trim().to_string()).filter(|e| crate::compose::is_plain_address(e))
            else {
                continue;
            };
            *score.entry(email).or_default() += if sent { SENT_WEIGHT } else { RECEIVED_WEIGHT };
        }
        let mut out: Vec<(String, u32)> = score.into_iter().collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Ok(out.into_iter().map(|(e, _)| e).collect())
    }

    /// Hand-made send-from entries, oldest first: (address, removed).
    pub fn send_address_overrides(&self, account_id: &str) -> Result<Vec<(String, bool)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT email, removed FROM send_addresses WHERE account_id = ?1 ORDER BY added_at, email",
        )?;
        let rows = stmt.query_map([account_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Record a detected address (custom-domain alias) unless the user
    /// already decided about it: a removed one stays removed.
    pub fn note_send_address(&self, account_id: &str, email: &str, now: i64) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO send_addresses (account_id, email, removed, added_at) VALUES (?1, ?2, 0, ?3)",
            params![account_id, email.trim().to_lowercase(), now],
        )?;
        Ok(())
    }

    /// Add (or bring back) a send-from address.
    pub fn add_send_address(&self, account_id: &str, email: &str, now: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO send_addresses (account_id, email, removed, added_at) VALUES (?1, ?2, 0, ?3)
             ON CONFLICT (account_id, email) DO UPDATE SET removed = 0, added_at = excluded.added_at",
            params![account_id, email.trim().to_lowercase(), now],
        )?;
        Ok(())
    }

    /// Stop offering an address, whether added by hand or detected.
    pub fn remove_send_address(&self, account_id: &str, email: &str, now: i64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO send_addresses (account_id, email, removed, added_at) VALUES (?1, ?2, 1, ?3)
             ON CONFLICT (account_id, email) DO UPDATE SET removed = 1",
            params![account_id, email.trim().to_lowercase(), now],
        )?;
        Ok(())
    }
}
