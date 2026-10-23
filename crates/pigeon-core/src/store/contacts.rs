//! Recipient suggestions from local mail history (no contacts API needed).
//!
//! Every address seen in From/To/Cc is a candidate. Addresses the user has
//! written to (To/Cc of mail sent from one of their own addresses) count
//! triple; senders of incoming mail count once. Frequency is boosted by how
//! recently the address was seen.

use std::collections::HashMap;

use rusqlite::params;

use super::Store;
use crate::Result;
use crate::model::Address;

const SENT_WEIGHT: f64 = 3.0;
const RECEIVED_WEIGHT: f64 = 1.0;
const DAY_MS: f64 = 86_400_000.0;

struct Seen {
    score: f64,
    last_at: i64,
    name: Option<String>,
    name_at: i64,
}

impl Store {
    /// Up to `limit` people matching `query` (a prefix of the email or of
    /// any word of the display name), best first. `mine` are the user's own
    /// addresses: never suggested, and used to tell sent mail apart.
    pub fn suggest_contacts(&self, query: &str, mine: &[String], limit: usize, now: i64) -> Result<Vec<Address>> {
        let q = query.trim().to_lowercase();
        if q.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let like = q.replace(['%', '_', '\\'], "");
        let mine_json = serde_json::to_string(&mine.iter().map(|m| m.to_lowercase()).collect::<Vec<_>>())?;
        let mut stmt = self.conn.prepare_cached(
            "WITH mine(email) AS (SELECT value FROM json_each(?1)),
             cand(email, name, at, sent) AS (
                 SELECT lower(m.from_addr), m.from_name, m.received_at, 0
                 FROM messages m WHERE m.from_addr IS NOT NULL
               UNION ALL
                 SELECT lower(json_extract(j.value, '$.email')), json_extract(j.value, '$.name'), m.received_at,
                        lower(m.from_addr) IN (SELECT email FROM mine)
                 FROM messages m, json_each(m.to_json) j
               UNION ALL
                 SELECT lower(json_extract(j.value, '$.email')), json_extract(j.value, '$.name'), m.received_at,
                        lower(m.from_addr) IN (SELECT email FROM mine)
                 FROM messages m, json_each(m.cc_json) j
             )
             SELECT email, name, at, sent FROM cand
             WHERE email IS NOT NULL AND email NOT IN (SELECT email FROM mine)
               AND (email LIKE ?2 || '%' OR lower(name) LIKE ?2 || '%' OR lower(name) LIKE '% ' || ?2 || '%')",
        )?;
        let rows = stmt.query_map(params![mine_json, like], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, i64>(2)?, r.get::<_, bool>(3)?))
        })?;

        let mut seen: HashMap<String, Seen> = HashMap::new();
        for row in rows {
            let (email, name, at, sent) = row?;
            let e = seen.entry(email).or_insert(Seen { score: 0.0, last_at: at, name: None, name_at: i64::MIN });
            e.score += if sent { SENT_WEIGHT } else { RECEIVED_WEIGHT };
            e.last_at = e.last_at.max(at);
            if let Some(n) = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty())
                && at >= e.name_at
            {
                e.name = Some(n);
                e.name_at = at;
            }
        }

        let rank = |s: &Seen| {
            let age_days = ((now - s.last_at).max(0) as f64) / DAY_MS;
            s.score * (1.0 + 1.0 / (1.0 + age_days / 30.0))
        };
        let mut out: Vec<(f64, Address)> =
            seen.into_iter().map(|(email, s)| (rank(&s), Address::new(s.name.as_deref(), &email))).collect();
        out.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.email.cmp(&b.1.email)));
        Ok(out.into_iter().take(limit).map(|(_, a)| a).collect())
    }
}
