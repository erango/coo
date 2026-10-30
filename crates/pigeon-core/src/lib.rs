//! UI-agnostic core: data model, local store, OAuth, HTTP plumbing, and the
//! provider traits + sync drivers that backends plug into.

pub mod actions;
pub mod auth;
pub mod bodies;
pub mod broker;
pub mod compose;
pub mod drive;
pub mod error;
pub mod html;
pub mod http;
pub mod ics;
pub mod identity;
pub mod images;
pub mod meeting;
pub mod model;
pub mod mojibake;
pub mod paths;
pub mod photos;
pub mod provider;
pub mod provider_detect;
pub mod quotes;
pub mod search;
pub mod search_complete;
pub mod store;
pub mod sync;
pub mod text;
pub mod tokens;

pub use error::{Error, Result};

/// Current time as Unix epoch milliseconds.
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
