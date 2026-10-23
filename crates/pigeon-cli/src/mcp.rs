//! `pigeon mcp`: a Model Context Protocol server over stdio (newline-
//! delimited JSON-RPC 2.0), so Claude and other agents can use the user's
//! mail and calendar through Pigeon's local store.
//!
//! Read-only unless started with `--allow-write`. stdout carries protocol
//! messages only; logs go to stderr.

use std::sync::Arc;

use pigeon_core::compose::ReplyKind;
use pigeon_engine::Pigeon;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::ops::{self, Writes};

const SUPPORTED_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
const DEFAULT_MAX_CHARS: usize = 20_000;

const INSTRUCTIONS: &str = "Pigeon gives access to the user's own mail and calendars (Microsoft 365, Gmail) \
from a local, synced copy. Start with list_accounts. Mailboxes are roles across all accounts \
(inbox, sent, drafts, trash, archive, starred) or a folder name with an account. search_mail \
supports operators: from: to: has:attachment is:unread is:flagged in:<role> account:<text> \
before:YYYY-MM-DD after:YYYY-MM-DD. Times accept now, today, tomorrow, +3d, -6h, YYYY-MM-DD, \
YYYY-MM-DDTHH:MM (local) or RFC 3339. Call sync for fresh data. Write tools exist only when the \
server was started with --allow-write; confirm with the user before sending mail or replying to invites.";

struct Tool {
    name: &'static str,
    description: &'static str,
    schema: Value,
    write: bool,
}

fn tools() -> Vec<Tool> {
    let s = |props: Value, required: &[&str]| json!({"type": "object", "properties": props, "required": required});
    let str_ = |d: &str| json!({"type": "string", "description": d});
    let int = |d: &str| json!({"type": "integer", "description": d});
    let bool_ = |d: &str| json!({"type": "boolean", "description": d});
    let strs = |d: &str| json!({"type": "array", "items": {"type": "string"}, "description": d});
    vec![
        Tool {
            name: "list_accounts",
            description: "Signed-in mail/calendar accounts.",
            schema: s(json!({}), &[]),
            write: false,
        },
        Tool {
            name: "list_folders",
            description: "Folders/labels of an account with unread and total counts.",
            schema: s(json!({"account": str_("Account id or email (or a unique prefix).")}), &["account"]),
            write: false,
        },
        Tool {
            name: "list_messages",
            description: "Newest messages (or conversations) in a mailbox.",
            schema: s(
                json!({
                    "mailbox": str_("Role across all accounts (inbox, sent, drafts, trash, archive, starred) or, with account, a folder name. Default inbox."),
                    "account": str_("Limit to one account (required for named folders)."),
                    "threads": bool_("Group into conversations (default true)."),
                    "unread_only": bool_("Only unread (default false)."),
                    "limit": int("Max results (default 30, max 300)."),
                }),
                &[],
            ),
            write: false,
        },
        Tool {
            name: "search_mail",
            description: "Search messages with the same operators as the app. Set include_server to also query the mail servers for older matches (slower).",
            schema: s(
                json!({
                    "query": str_("e.g. invoice from:dana has:attachment after:2026-01-01"),
                    "include_server": bool_("Also search the servers (default false)."),
                    "limit": int("Max results (default 30, max 200)."),
                }),
                &["query"],
            ),
            write: false,
        },
        Tool {
            name: "get_message",
            description: "One message with its plain-text body, recipients, attachments and threading headers.",
            schema: s(
                json!({"account": str_("Account id or email."), "message_id": str_("Message id from list/search."),
                       "max_chars": int("Body length cap (default 20000).")}),
                &["account", "message_id"],
            ),
            write: false,
        },
        Tool {
            name: "get_thread",
            description: "A whole conversation, oldest first, with bodies.",
            schema: s(
                json!({"account": str_("Account id or email."), "thread_id": str_("thread_id from list/search."),
                       "max_chars": int("Total body budget (default 20000).")}),
                &["account", "thread_id"],
            ),
            write: false,
        },
        Tool {
            name: "list_events",
            description: "Calendar events across all calendars in a time range (default: now to +7d).",
            schema: s(
                json!({"from": str_("Start (default now)."), "to": str_("End (default +7d)."),
                       "account": str_("Limit to one account.")}),
                &[],
            ),
            write: false,
        },
        Tool {
            name: "free_busy",
            description: "Busy intervals for people, as seen by one of the user's accounts.",
            schema: s(
                json!({"account": str_("Account to ask through."), "emails": strs("Addresses to check."),
                       "from": str_("Start (default now)."), "to": str_("End (default +1d).")}),
                &["account", "emails"],
            ),
            write: false,
        },
        Tool {
            name: "sync",
            description: "Pull new mail and events from the servers (all accounts, or one). Can take a while on first run.",
            schema: s(json!({"account": str_("Only this account.")}), &[]),
            write: false,
        },
        Tool {
            name: "mail_action",
            description: "Archive, trash, flag, mark read/unread, or move messages. Returns an undo_handle.",
            schema: s(
                json!({"account": str_("Account id or email."), "message_ids": strs("Message ids."),
                       "action": str_("read | unread | flag | unflag | archive | trash | move"),
                       "to_folder": str_("Target folder for move.")}),
                &["account", "message_ids", "action"],
            ),
            write: true,
        },
        Tool {
            name: "undo",
            description: "Undo a mail_action by its undo_handle.",
            schema: s(
                json!({"account": str_("Account id or email."), "undo_handle": int("From mail_action.")}),
                &["account", "undo_handle"],
            ),
            write: true,
        },
        Tool {
            name: "reply",
            description: "Reply, reply-all or forward. Saves a draft; sends only if send=true.",
            schema: s(
                json!({"account": str_("Account the original is in."), "message_id": str_("Message to answer."),
                       "body": str_("Plain text; quoted original is appended."),
                       "mode": str_("reply | reply_all | forward (default reply)"),
                       "from": str_("Send from this address (any of the user's accounts/aliases)."),
                       "to": strs("Recipients (required for forward)."),
                       "send": bool_("Actually send (default false: draft only).")}),
                &["account", "message_id", "body"],
            ),
            write: true,
        },
        Tool {
            name: "send_email",
            description: "Compose a new message. Saves a draft; sends only if send=true.",
            schema: s(
                json!({"from": str_("One of the user's addresses."), "to": strs("Recipients."), "cc": strs("Cc."),
                       "subject": str_("Subject."), "body": str_("Plain text."),
                       "send": bool_("Actually send (default false: draft only).")}),
                &["from", "to", "subject", "body"],
            ),
            write: true,
        },
        Tool {
            name: "rsvp",
            description: "Answer a meeting invitation (notifies the organizer).",
            schema: s(
                json!({"account": str_("Account id or email."), "calendar_id": str_("From list_events."),
                       "event_id": str_("From list_events."), "response": str_("accept | tentative | decline"),
                       "comment": str_("Optional note to the organizer.")}),
                &["account", "calendar_id", "event_id", "response"],
            ),
            write: true,
        },
        Tool {
            name: "create_event",
            description: "Create a calendar event; invitations go to attendees.",
            schema: s(
                json!({"account": str_("Account id or email."), "title": str_("Title."),
                       "start": str_("Start time."), "end": str_("End time."),
                       "all_day": bool_("All-day (start/end are dates; end inclusive)."),
                       "attendees": strs("Invitees."), "location": str_("Location."),
                       "description": str_("Plain text."), "calendar_id": str_("Default: primary.")}),
                &["account", "title", "start", "end"],
            ),
            write: true,
        },
    ]
}

fn arg<'a>(args: &'a Value, k: &str) -> Option<&'a str> {
    args.get(k).and_then(Value::as_str)
}

fn need<'a>(args: &'a Value, k: &str) -> pigeon_core::Result<&'a str> {
    arg(args, k).ok_or_else(|| pigeon_core::Error::Other(format!("missing argument `{k}`")))
}

fn strings(args: &Value, k: &str) -> Vec<String> {
    match args.get(k) {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(String::from).collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn num(args: &Value, k: &str, default: u64, max: u64) -> u64 {
    args.get(k).and_then(Value::as_u64).unwrap_or(default).clamp(1, max)
}

fn flag(args: &Value, k: &str, default: bool) -> bool {
    args.get(k).and_then(Value::as_bool).unwrap_or(default)
}

fn when(args: &Value, k: &str, default: &str) -> pigeon_core::Result<i64> {
    ops::parse_when(arg(args, k).unwrap_or(default), chrono::Local::now())
}

async fn call(app: &Pigeon, w: Writes, name: &str, a: &Value) -> pigeon_core::Result<Value> {
    let j = |v: &dyn erased::Ser| v.to_value();
    Ok(match name {
        "list_accounts" => j(&ops::accounts(app)?),
        "list_folders" => j(&ops::folders(app, need(a, "account")?)?),
        "list_messages" => {
            let limit = num(a, "limit", 30, 300) as u32;
            match ops::list(
                app,
                arg(a, "account"),
                arg(a, "mailbox").unwrap_or("inbox"),
                flag(a, "threads", true),
                flag(a, "unread_only", false),
                limit,
            )? {
                ops::Listing::Threads(t) => j(&t),
                ops::Listing::Messages(m) => j(&m),
            }
        }
        "search_mail" => {
            j(&ops::search(app, need(a, "query")?, flag(a, "include_server", false), num(a, "limit", 30, 200) as u32)
                .await?)
        }
        "get_message" => j(&ops::get_message(
            app,
            need(a, "account")?,
            need(a, "message_id")?,
            num(a, "max_chars", DEFAULT_MAX_CHARS as u64, 200_000) as usize,
        )
        .await?),
        "get_thread" => j(&ops::get_thread(
            app,
            need(a, "account")?,
            need(a, "thread_id")?,
            num(a, "max_chars", DEFAULT_MAX_CHARS as u64, 400_000) as usize,
        )
        .await?),
        "list_events" => j(&ops::agenda(app, when(a, "from", "now")?, when(a, "to", "+7d")?, arg(a, "account"))?),
        "free_busy" => j(&ops::free_busy(
            app,
            need(a, "account")?,
            &strings(a, "emails"),
            when(a, "from", "now")?,
            when(a, "to", "+1d")?,
        )
        .await?),
        "sync" => j(&ops::sync(app, arg(a, "account")).await?),
        "mail_action" => j(&ops::act(
            app,
            w,
            need(a, "account")?,
            &strings(a, "message_ids"),
            need(a, "action")?,
            arg(a, "to_folder"),
            None,
        )
        .await?),
        "undo" => {
            let handle = a
                .get("undo_handle")
                .and_then(Value::as_i64)
                .ok_or_else(|| pigeon_core::Error::Other("missing argument `undo_handle`".into()))?;
            j(&ops::undo(app, w, need(a, "account")?, handle).await?)
        }
        "reply" => {
            let kind = match arg(a, "mode").unwrap_or("reply") {
                "reply_all" | "all" => ReplyKind::ReplyAll,
                "forward" => ReplyKind::Forward,
                _ => ReplyKind::Reply,
            };
            let to = strings(a, "to");
            let r = ops::Reply {
                account: need(a, "account")?,
                message_id: need(a, "message_id")?,
                kind,
                from: arg(a, "from"),
                to: &to,
                body: need(a, "body")?,
            };
            j(&ops::reply(app, w, r, !flag(a, "send", false)).await?)
        }
        "send_email" => {
            let (to, cc) = (strings(a, "to"), strings(a, "cc"));
            let m = ops::NewMail {
                from: need(a, "from")?,
                to: &to,
                cc: &cc,
                bcc: &[],
                subject: need(a, "subject")?,
                body: need(a, "body")?,
                attach: &[],
            };
            j(&ops::send(app, w, m, !flag(a, "send", false)).await?)
        }
        "rsvp" => {
            ops::rsvp(
                app,
                w,
                need(a, "account")?,
                need(a, "calendar_id")?,
                need(a, "event_id")?,
                need(a, "response")?,
                arg(a, "comment"),
            )
            .await?
        }
        "create_event" => {
            let all_day = flag(a, "all_day", false);
            let start = when(a, "start", "")?;
            let mut end = when(a, "end", "")?;
            if all_day {
                end += 86_400_000; // inclusive end date -> exclusive midnight
            }
            let attendees = strings(a, "attendees");
            j(&ops::create_event(
                app,
                w,
                ops::NewEvent {
                    account: need(a, "account")?,
                    title: need(a, "title")?,
                    start,
                    end,
                    all_day,
                    attendees: &attendees,
                    location: arg(a, "location"),
                    description: arg(a, "description"),
                    calendar: arg(a, "calendar_id"),
                },
            )
            .await?)
        }
        other => return Err(pigeon_core::Error::Other(format!("unknown tool {other}"))),
    })
}

/// Tiny shim so `call` can serialize any result type uniformly.
mod erased {
    pub trait Ser {
        fn to_value(&self) -> serde_json::Value;
    }
    impl<T: serde::Serialize> Ser for T {
        fn to_value(&self) -> serde_json::Value {
            serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
        }
    }
}

fn tool_list(w: Writes) -> Value {
    let list: Vec<Value> = tools()
        .into_iter()
        .filter(|t| !t.write || w.allowed)
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.schema,
                "annotations": {
                    "readOnlyHint": !t.write,
                    "destructiveHint": t.write,
                    "openWorldHint": t.write,
                },
            })
        })
        .collect();
    json!({ "tools": list })
}

/// Handle one JSON-RPC message; `None` for notifications.
async fn handle(app: &Pigeon, w: Writes, msg: Value) -> Option<Value> {
    let id = msg.get("id").cloned()?;
    let method = msg.get("method").and_then(Value::as_str).unwrap_or_default();
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
            let version = SUPPORTED_VERSIONS.iter().find(|v| **v == asked).copied().unwrap_or(SUPPORTED_VERSIONS[0]);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "pigeon", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tool_list(w)),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let known = tools().into_iter().find(|t| t.name == name);
            let outcome = match known {
                Some(t) if t.write && !w.allowed => Err(pigeon_core::Error::Other(format!(
                    "{}: {} is a write tool and {}",
                    ops::WRITE_BLOCKED,
                    name,
                    w.reason
                ))),
                Some(_) => call(app, w, &name, &args).await,
                None => Err(pigeon_core::Error::Other(format!("unknown tool {name}"))),
            };
            // Tool failures are results (isError), not protocol errors.
            Ok(match outcome {
                Ok(v) => json!({
                    "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }],
                    "structuredContent": if v.is_object() { v.clone() } else { json!({ "result": v }) },
                    "isError": false,
                }),
                Err(e) => json!({ "content": [{ "type": "text", "text": e.to_string() }], "isError": true }),
            })
        }
        _ => Err(json!({ "code": -32601, "message": format!("method not found: {method}") })),
    };
    Some(match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err(e) => json!({ "jsonrpc": "2.0", "id": id, "error": e }),
    })
}

pub async fn serve(app: Pigeon, allow_write: bool) -> pigeon_core::Result<()> {
    let w = Writes::for_mcp(allow_write);
    tracing::info!(writes = w.allowed, "pigeon MCP server on stdio");
    let app = Arc::new(app);
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
    // Writer task: replies may finish out of order (tools run concurrently).
    let writer = tokio::spawn(async move {
        let mut out = tokio::io::stdout();
        while let Some(v) = rx.recv().await {
            let mut line = serde_json::to_vec(&v).unwrap_or_default();
            line.push(b'\n');
            if out.write_all(&line).await.is_err() || out.flush().await.is_err() {
                break;
            }
        }
    });
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let _ =
                    tx.send(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": e.to_string()}}));
                continue;
            }
        };
        let (app, tx) = (app.clone(), tx.clone());
        tokio::spawn(async move {
            if let Some(reply) = handle(&app, w, msg).await {
                let _ = tx.send(reply);
            }
        });
    }
    drop(tx);
    let _ = writer.await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_server_hides_write_tools() {
        let ro = tool_list(Writes { allowed: false, reason: "x" });
        let names: Vec<&str> = ro["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"search_mail") && names.contains(&"get_thread"));
        assert!(!names.contains(&"reply") && !names.contains(&"mail_action"));
        let rw = tool_list(Writes { allowed: true, reason: "" });
        assert!(rw["tools"].as_array().unwrap().iter().any(|t| t["name"] == "reply"));
    }

    #[test]
    fn every_tool_schema_is_an_object_with_known_required_fields() {
        for t in tools() {
            assert_eq!(t.schema["type"], "object", "{}", t.name);
            for r in t.schema["required"].as_array().unwrap() {
                assert!(t.schema["properties"].get(r.as_str().unwrap()).is_some(), "{} requires unknown {r}", t.name);
            }
        }
    }

    #[tokio::test]
    async fn protocol_basics_without_touching_mail() {
        let dir = std::env::temp_dir().join(format!("pigeon-mcp-{}", pigeon_core::auth::random_token(6)));
        // SAFETY: test-only env for an isolated data dir; set before use.
        unsafe { std::env::set_var("PIGEON_DATA_DIR", &dir) };
        let app = Pigeon::open().unwrap();
        let w = Writes { allowed: false, reason: "read-only" };

        let init = handle(
            &app,
            w,
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}),
        )
        .await
        .unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert!(handle(&app, w, json!({"jsonrpc":"2.0","method":"notifications/initialized"})).await.is_none());

        let r = handle(
            &app,
            w,
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_accounts","arguments":{}}}),
        )
        .await
        .unwrap();
        assert_eq!(r["result"]["isError"], false);
        assert_eq!(r["result"]["structuredContent"]["result"], json!([]));

        let blocked = handle(&app, w, json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"reply","arguments":{"account":"a","message_id":"m","body":"hi"}}})).await.unwrap();
        assert_eq!(blocked["result"]["isError"], true);
        assert!(blocked["result"]["content"][0]["text"].as_str().unwrap().starts_with(ops::WRITE_BLOCKED));

        let unknown = handle(&app, w, json!({"jsonrpc":"2.0","id":4,"method":"nope"})).await.unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
        std::fs::remove_dir_all(dir).ok();
    }
}
