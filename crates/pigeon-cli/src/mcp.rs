//! `coo mcp`: a Model Context Protocol server over stdio (newline-
//! delimited JSON-RPC 2.0), so Claude and other agents can use the user's
//! mail and calendar through Coo's local store.
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

/// The server's `instructions`: the shared guide, phrased for tool use.
fn instructions(writes_allowed: bool) -> String {
    use pigeon_engine::agent_setup::guide::*;
    let writes = if writes_allowed {
        WRITE_ETIQUETTE.to_string()
    } else {
        "This server is read-only: there are no tools that send or change anything. If the user wants that, they \
         turn on sending in Coo › Help › Connect an AI Agent, click Update, and restart this agent."
            .to_string()
    };
    format!(
        "{PURPOSE}\n\n{WORKFLOW} Tools: list_accounts → list_messages or search_mail → get_thread (or get_message); \
         list_events / free_busy for calendars; sync when data looks stale.\n\nMailboxes are roles across all \
         accounts (inbox, sent, drafts, trash, archive, starred) or, with an account, a folder name from \
         list_folders.\n\n{OPERATORS}\n\n{TIMES}\n\n{writes}\n\n{ERRORS}"
    )
}

struct Tool {
    name: &'static str,
    description: &'static str,
    schema: Value,
    write: bool,
}

/// Account parameter (all tools take ids from list_accounts).
const ACCOUNT: &str = "Account id from list_accounts (e.g. google:me@gmail.com), or its email.";

fn tools() -> Vec<Tool> {
    let s = |props: Value, required: &[&str]| json!({"type": "object", "properties": props, "required": required});
    let str_ = |d: &str| json!({"type": "string", "description": d});
    let ex = |d: &str, examples: Value| json!({"type": "string", "description": d, "examples": examples});
    let int = |d: &str, default: u64| json!({"type": "integer", "description": d, "default": default, "minimum": 1});
    let bool_ = |d: &str, default: bool| json!({"type": "boolean", "description": d, "default": default});
    let strs = |d: &str| json!({"type": "array", "items": {"type": "string"}, "description": d});
    vec![
        Tool {
            name: "list_accounts",
            description: "List the user's signed-in mail/calendar accounts (id, provider, email, name). Call this \
                first: every other tool takes an account id from here.",
            schema: s(json!({}), &[]),
            write: false,
        },
        Tool {
            name: "list_folders",
            description: "Folders/labels of one account with unread and total counts. Use it to find a folder \
                name for list_messages, or to answer \"how many unread in X\".",
            schema: s(json!({"account": str_(ACCOUNT)}), &["account"]),
            write: false,
        },
        Tool {
            name: "list_messages",
            description: "Newest mail in a mailbox, as conversations (default) or single messages. Use for \
                \"what's in my inbox\", triage, or browsing a folder. To find something specific use search_mail; \
                to read a conversation use get_thread with the thread_id from here.",
            schema: s(
                json!({
                    "mailbox": ex("A role across all accounts (inbox, sent, drafts, trash, archive, starred), or a \
                        folder name from list_folders together with `account`.", json!(["inbox", "sent", "Receipts"])),
                    "account": str_("Only this account (required for a folder name)."),
                    "threads": bool_("Group into conversations.", true),
                    "unread_only": bool_("Only unread.", false),
                    "limit": int("Max results (max 300).", 30),
                }),
                &[],
            ),
            write: false,
        },
        Tool {
            name: "search_mail",
            description: "Search messages across all accounts with the app's operators (from:, to:, \
                has:attachment, is:unread, is:flagged, in:, account:, before:, after:) plus free text. Searches the \
                local copy (recent months) instantly; set include_server for older mail (slower, asks every server). \
                Results are single messages; read context with get_thread.",
            schema: s(
                json!({
                    "query": ex("Operators and free text.", json!(["invoice from:dana has:attachment",
                        "is:unread in:inbox after:2026-10-01", "\"board meeting\" account:work"])),
                    "include_server": bool_("Also search the mail servers for older matches.", false),
                    "limit": int("Max results (max 200).", 30),
                }),
                &["query"],
            ),
            write: false,
        },
        Tool {
            name: "get_message",
            description: "One message with its plain-text body, recipients, attachments and threading headers. \
                Prefer get_thread when the message is part of a conversation (replies, context).",
            schema: s(
                json!({"account": str_(ACCOUNT), "message_id": str_("`id` from list_messages/search_mail."),
                       "max_chars": int("Body length cap.", DEFAULT_MAX_CHARS as u64)}),
                &["account", "message_id"],
            ),
            write: false,
        },
        Tool {
            name: "get_thread",
            description: "A whole conversation, oldest first, with each message's plain-text body. Use before \
                summarizing a discussion or drafting a reply. The newest message gets most of the length budget.",
            schema: s(
                json!({"account": str_(ACCOUNT), "thread_id": str_("`thread_id` from list_messages/search_mail."),
                       "max_chars": int("Total body budget across messages.", DEFAULT_MAX_CHARS as u64)}),
                &["account", "thread_id"],
            ),
            write: false,
        },
        Tool {
            name: "list_events",
            description: "Calendar events across all the user's calendars in a time range, with attendees, the \
                user's own RSVP and meeting links. Use for agenda questions (\"what's tomorrow\", \"when am I free\" \
                for the user). For other people's availability use free_busy.",
            schema: s(
                json!({"from": ex("Range start.", json!(["now", "today", "2026-11-02"])),
                       "to": ex("Range end.", json!(["+7d", "tomorrow", "2026-11-03T18:00"])),
                       "account": str_("Only this account.")}),
                &[],
            ),
            write: false,
        },
        Tool {
            name: "free_busy",
            description: "Busy intervals for other people (colleagues, guests), asked through one of the user's \
                accounts: use the account in the same organization. For the user's own calendar use list_events.",
            schema: s(
                json!({"account": str_(ACCOUNT), "emails": strs("Addresses to check."),
                       "from": ex("Range start (default now).", json!(["now", "tomorrow"])),
                       "to": ex("Range end (default +1d).", json!(["+1d", "+3d"]))}),
                &["account", "emails"],
            ),
            write: false,
        },
        Tool {
            name: "sync",
            description: "Pull new mail and events from the servers (all accounts, or one). Call when something \
                the user expects is missing or the data looks stale. The first sync of an account can take minutes.",
            schema: s(json!({"account": str_("Only this account.")}), &[]),
            write: false,
        },
        Tool {
            name: "mail_action",
            description: "Change messages on the server: archive, trash, flag/unflag, mark read/unread, or move to \
                a folder. Ask the user first for anything but read state. Returns an undo_handle for `undo`.",
            schema: s(
                json!({"account": str_(ACCOUNT), "message_ids": strs("Message ids (same account)."),
                       "action": ex("What to do.", json!(["archive", "trash", "flag", "unflag", "read", "unread", "move"])),
                       "to_folder": str_("Target folder name for move.")}),
                &["account", "message_ids", "action"],
            ),
            write: true,
        },
        Tool {
            name: "undo",
            description: "Revert a mail_action using the undo_handle it returned (works for about 10 minutes).",
            schema: s(
                json!({"account": str_(ACCOUNT), "undo_handle": json!({"type": "integer", "description": "From mail_action."})}),
                &["account", "undo_handle"],
            ),
            write: true,
        },
        Tool {
            name: "reply",
            description: "Reply, reply-all or forward a message. By default saves a draft and returns it (from, \
                to, cc, subject) so you can show the user; set send=true only after they agree. The quoted original \
                and threading headers are added for you. `from` can be any of the user's addresses, even another \
                account's.",
            schema: s(
                json!({"account": str_("Account the original is in."), "message_id": str_("Message to answer."),
                       "body": str_("Plain text written above the quoted original."),
                       "mode": ex("Default reply.", json!(["reply", "reply_all", "forward"])),
                       "from": str_("Send from this address instead of the default."),
                       "to": strs("Recipients (required for forward; overrides for replies)."),
                       "send": bool_("Actually send. Leave false to save a draft for the user to review.", false)}),
                &["account", "message_id", "body"],
            ),
            write: true,
        },
        Tool {
            name: "send_email",
            description: "Compose a new message (not a reply; use reply for that). Saves a draft by default; set \
                send=true only after the user agrees.",
            schema: s(
                json!({"from": str_("One of the user's addresses (see list_accounts)."), "to": strs("Recipients."),
                       "cc": strs("Cc recipients."), "subject": str_("Subject line."),
                       "body": str_("Plain text."),
                       "send": bool_("Actually send. Leave false to save a draft.", false)}),
                &["from", "to", "subject", "body"],
            ),
            write: true,
        },
        Tool {
            name: "rsvp",
            description: "Answer a meeting invitation; the organizer is notified. Only after the user decides. Ids \
                come from list_events.",
            schema: s(
                json!({"account": str_(ACCOUNT), "calendar_id": str_("From list_events."),
                       "event_id": str_("From list_events."),
                       "response": ex("The answer.", json!(["accept", "tentative", "decline"])),
                       "comment": str_("Optional note to the organizer.")}),
                &["account", "calendar_id", "event_id", "response"],
            ),
            write: true,
        },
        Tool {
            name: "create_event",
            description: "Create an event on the user's calendar; attendees get invitations immediately, so confirm \
                the details with the user first. Check people's availability with free_busy before proposing a time.",
            schema: s(
                json!({"account": str_(ACCOUNT), "title": str_("Event title."),
                       "start": ex("Event start.", json!(["2026-11-02T10:00", "tomorrow"])),
                       "end": ex("Event end.", json!(["2026-11-02T10:30"])),
                       "all_day": bool_("All-day event (start/end are dates; end is inclusive).", false),
                       "attendees": strs("Invitee addresses."), "location": str_("Location."),
                       "description": str_("Plain text."), "calendar_id": str_("Default: the account's primary calendar.")}),
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
                "serverInfo": { "name": "coo", "version": env!("CARGO_PKG_VERSION") },
                "instructions": instructions(w.allowed),
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
                Err(e) => {
                    let (kind, message) = ops::describe(&e);
                    json!({
                        "content": [{ "type": "text", "text": message }],
                        "structuredContent": { "error": { "kind": kind, "message": message } },
                        "isError": true,
                    })
                }
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
    // Running as an agent's subprocess: nobody can answer a Keychain dialog,
    // so reads that need one fail with a clear error instead of hanging.
    let _headless = pigeon_core::tokens::headless();
    tracing::info!(writes = w.allowed, "coo MCP server on stdio");
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
    fn descriptions_say_when_to_use_each_tool() {
        for t in tools() {
            assert!(t.description.len() >= 80, "{} description is too thin: {:?}", t.name, t.description);
            for (k, p) in t.schema["properties"].as_object().unwrap() {
                assert!(p["description"].as_str().is_some_and(|d| d.len() >= 5), "{}.{k} needs a description", t.name);
            }
        }
        let i = instructions(false);
        assert!(i.contains("list_accounts") && i.contains("from:") && i.contains("+3d") && i.contains("read-only"));
        assert!(instructions(true).contains("draft first"));
    }

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
