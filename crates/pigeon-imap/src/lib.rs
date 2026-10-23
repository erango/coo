//! IMAP + SMTP backend. iCloud is the first preset; the code is generic so
//! Fastmail/Yahoo/self-hosted servers can follow.
//!
//! Sync model per folder: newest-first bootstrap over the window, then new
//! UIDs above the last seen one, plus a cheap flags pass over the window
//! that also detects deletions (`MailChange::Retain`). UIDVALIDITY changes
//! resync the folder.

mod conn;
mod mail;
pub mod parse;
mod smtp;

pub use conn::login;

use conn::Pool;

/// Where an account's servers are.
#[derive(Debug, Clone, Copy)]
pub struct ServerConfig {
    pub label: &'static str,
    pub imap_host: &'static str,
    pub imap_port: u16,
    pub smtp_host: &'static str,
    pub smtp_port: u16,
    /// Try the name part of the address first (iCloud's documented form).
    pub local_part_first: bool,
}

impl ServerConfig {
    /// iCloud Mail (support.apple.com/102525): IMAP 993 TLS, SMTP 587 STARTTLS.
    pub const ICLOUD: ServerConfig = ServerConfig {
        label: "iCloud",
        imap_host: "imap.mail.me.com",
        imap_port: 993,
        smtp_host: "smtp.mail.me.com",
        smtp_port: 587,
        local_part_first: true,
    };

    /// IMAP usernames to try, in order.
    pub fn usernames(&self, email: &str) -> Vec<String> {
        let local = email.split('@').next().unwrap_or(email).to_string();
        if self.local_part_first && local != email { vec![local, email.to_string()] } else { vec![email.to_string()] }
    }
}

pub struct Imap {
    pool: Pool,
    cfg: ServerConfig,
    email: String,
    password: String,
    /// Initial sync reaches back this many days.
    pub mail_window_days: i64,
}

impl Imap {
    pub fn new(cfg: ServerConfig, email: &str, password: &str) -> Self {
        Imap {
            // iCloud allows a handful of connections per account; stay well under.
            pool: Pool::new(cfg, email.to_string(), password.to_string(), 3),
            cfg,
            email: email.to_string(),
            password: password.to_string(),
            mail_window_days: 90,
        }
    }
}

/// Check credentials by logging in (and out).
pub async fn verify(cfg: &ServerConfig, email: &str, password: &str) -> pigeon_core::Result<()> {
    let mut s = login(cfg, email, password).await?;
    let _ = s.logout().await;
    Ok(())
}
