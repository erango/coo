//! Serde shapes for the subset of Graph we use.

use chrono::{DateTime, NaiveDateTime};
use pigeon_core::model::Address;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Page<T> {
    pub value: Vec<T>,
    #[serde(rename = "@odata.nextLink")]
    pub next_link: Option<String>,
    #[serde(rename = "@odata.deltaLink")]
    pub delta_link: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    pub display_name: Option<String>,
    pub mail: Option<String>,
    pub user_principal_name: Option<String>,
}

#[derive(Deserialize)]
pub struct Id {
    pub id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MailFolder {
    pub id: String,
    pub display_name: String,
    pub parent_folder_id: Option<String>,
    #[serde(default)]
    pub child_folder_count: u32,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Recipient {
    pub email_address: EmailAddress,
}

#[derive(Deserialize, Default)]
pub struct EmailAddress {
    pub name: Option<String>,
    pub address: Option<String>,
}

impl EmailAddress {
    pub fn to_address(&self) -> Option<Address> {
        let email = self.address.as_deref().filter(|a| !a.is_empty())?;
        Some(Address::new(self.name.as_deref(), email))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Flag {
    pub flag_status: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    #[serde(rename = "@removed")]
    pub removed: Option<serde_json::Value>,
    pub conversation_id: Option<String>,
    pub internet_message_id: Option<String>,
    pub subject: Option<String>,
    pub from: Option<Recipient>,
    #[serde(default)]
    pub to_recipients: Vec<Recipient>,
    #[serde(default)]
    pub cc_recipients: Vec<Recipient>,
    pub body_preview: Option<String>,
    pub received_date_time: Option<String>,
    pub is_read: Option<bool>,
    pub flag: Option<Flag>,
    pub has_attachments: Option<bool>,
    pub is_draft: Option<bool>,
    pub parent_folder_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GCalendar {
    pub id: String,
    pub name: String,
    pub hex_color: Option<String>,
    #[serde(default)]
    pub is_default_calendar: bool,
    #[serde(default)]
    pub can_edit: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DateTimeTz {
    pub date_time: String,
}

#[derive(Deserialize)]
pub struct Location {
    #[serde(rename = "displayName")]
    pub display_name: Option<String>,
}

#[derive(Deserialize)]
pub struct ResponseStatus {
    pub response: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GAttendee {
    #[serde(default)]
    pub email_address: EmailAddress,
    pub status: Option<ResponseStatus>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineMeeting {
    pub join_url: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GEvent {
    pub id: String,
    pub series_master_id: Option<String>,
    pub subject: Option<String>,
    pub start: DateTimeTz,
    pub end: DateTimeTz,
    #[serde(default)]
    pub is_all_day: bool,
    #[serde(default)]
    pub is_cancelled: bool,
    pub original_start_time_zone: Option<String>,
    pub location: Option<Location>,
    pub organizer: Option<Recipient>,
    #[serde(default)]
    pub attendees: Vec<GAttendee>,
    pub response_status: Option<ResponseStatus>,
    pub online_meeting: Option<OnlineMeeting>,
    pub last_modified_date_time: Option<String>,
}

/// RFC 3339 (`2026-10-23T09:00:00Z`) to epoch ms.
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp_millis())
}

/// Graph `dateTimeTimeZone.dateTime` (`2026-10-23T09:00:00.0000000`), already
/// in UTC thanks to the `outlook.timezone` preference.
pub fn parse_graph_utc(s: &str) -> Option<i64> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").ok().map(|d| d.and_utc().timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_times() {
        assert_eq!(parse_graph_utc("1970-01-01T00:00:01.0000000"), Some(1000));
        assert_eq!(parse_graph_utc("1970-01-01T00:01:00"), Some(60_000));
        assert_eq!(parse_rfc3339("1970-01-01T01:00:00+01:00"), Some(0));
        assert_eq!(parse_graph_utc("garbage"), None);
    }
}
