//! Minimal SMTP submission client: STARTTLS, AUTH PLAIN, one message.
//! Never retried (a retry after a timeout could deliver twice).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use pigeon_core::{Error, Result};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use crate::ServerConfig;
use crate::conn::upgrade;
use crate::parse::{dot_stuff, envelope, smtp_line};

struct Smtp<S> {
    io: BufReader<S>,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Smtp<S> {
    /// Read one (possibly multi-line) reply: (code, text).
    async fn reply(&mut self) -> Result<(u16, String)> {
        let mut text = String::new();
        loop {
            let mut line = String::new();
            if self.io.read_line(&mut line).await? == 0 {
                return Err(Error::Other("SMTP server closed the connection".into()));
            }
            let (code, last) = smtp_line(&line).ok_or_else(|| Error::Other(format!("bad SMTP reply: {line}")))?;
            text.push_str(line.get(4..).unwrap_or_default().trim_end());
            text.push(' ');
            if last {
                return Ok((code, text.trim_end().to_string()));
            }
        }
    }

    async fn cmd(&mut self, line: &str, expect: &[u16]) -> Result<String> {
        self.io.get_mut().write_all(line.as_bytes()).await?;
        self.io.get_mut().write_all(b"\r\n").await?;
        self.io.get_mut().flush().await?;
        let (code, text) = self.reply().await?;
        if expect.contains(&code) {
            return Ok(text);
        }
        // Don't echo credentials into errors.
        let shown = if line.starts_with("AUTH") { "AUTH" } else { line };
        Err(match code {
            535 | 534 => Error::Auth(format!("SMTP login rejected ({code} {text}); use an app-specific password")),
            _ => Error::Other(format!("SMTP `{shown}` failed: {code} {text}")),
        })
    }
}

/// Send `mime` through the account's submission server.
pub async fn send(cfg: &ServerConfig, email: &str, password: &str, mime: &[u8]) -> Result<()> {
    let (from, rcpt, clean) = envelope(mime)?;
    if rcpt.is_empty() {
        return Err(Error::Other("no recipients".into()));
    }
    let from = from.unwrap_or_else(|| email.to_string());

    let tcp = TcpStream::connect((cfg.smtp_host, cfg.smtp_port)).await?;
    let mut plain = Smtp { io: BufReader::new(tcp) };
    let (code, text) = plain.reply().await?;
    if code != 220 {
        return Err(Error::Other(format!("SMTP greeting: {code} {text}")));
    }
    plain.cmd("EHLO pigeon.local", &[250]).await?;
    plain.cmd("STARTTLS", &[220]).await?;
    let tls = upgrade(plain.io.into_inner(), cfg.smtp_host).await?;
    let mut s = Smtp { io: BufReader::new(tls) };
    s.cmd("EHLO pigeon.local", &[250]).await?;
    let token = STANDARD.encode(format!("\0{email}\0{password}"));
    s.cmd(&format!("AUTH PLAIN {token}"), &[235]).await?;
    s.cmd(&format!("MAIL FROM:<{from}>"), &[250]).await?;
    for r in &rcpt {
        s.cmd(&format!("RCPT TO:<{r}>"), &[250, 251]).await?;
    }
    s.cmd("DATA", &[354]).await?;
    let mut data = dot_stuff(&clean);
    data.extend_from_slice(b".");
    let data = String::from_utf8_lossy(&data).into_owned();
    s.cmd(&data, &[250]).await?;
    let _ = s.cmd("QUIT", &[221]).await;
    Ok(())
}
