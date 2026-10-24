//! Connected services (Zoom) and the Zoom meetings Coo created for events.

use rusqlite::{OptionalExtension, params};

use super::Store;
use crate::Result;
use crate::model::{ConnectedService, ZoomMeetingLink};

impl Store {
    pub fn upsert_service(&self, s: &ConnectedService) -> Result<()> {
        self.conn.execute(
            "INSERT INTO services (id, email, display_name, connected_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (id) DO UPDATE SET email = excluded.email, display_name = excluded.display_name,
               connected_at = excluded.connected_at",
            params![s.id, s.email, s.display_name, s.connected_at],
        )?;
        Ok(())
    }

    pub fn service(&self, id: &str) -> Result<Option<ConnectedService>> {
        Ok(self
            .conn
            .prepare_cached("SELECT id, email, display_name, connected_at FROM services WHERE id = ?1")?
            .query_row([id], |r| {
                Ok(ConnectedService {
                    id: r.get(0)?,
                    email: r.get(1)?,
                    display_name: r.get(2)?,
                    connected_at: r.get(3)?,
                })
            })
            .optional()?)
    }

    pub fn delete_service(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM services WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn set_zoom_meeting(&self, account_id: &str, event_id: &str, link: &ZoomMeetingLink) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO zoom_meetings (account_id, event_id, meeting_id, join_url) VALUES (?1, ?2, ?3, ?4)",
            params![account_id, event_id, link.meeting_id, link.join_url],
        )?;
        Ok(())
    }

    pub fn zoom_meeting(&self, account_id: &str, event_id: &str) -> Result<Option<ZoomMeetingLink>> {
        Ok(self
            .conn
            .prepare_cached("SELECT meeting_id, join_url FROM zoom_meetings WHERE account_id = ?1 AND event_id = ?2")?
            .query_row([account_id, event_id], |r| Ok(ZoomMeetingLink { meeting_id: r.get(0)?, join_url: r.get(1)? }))
            .optional()?)
    }

    pub fn delete_zoom_meeting(&self, account_id: &str, event_id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM zoom_meetings WHERE account_id = ?1 AND event_id = ?2", [account_id, event_id])?;
        Ok(())
    }
}
