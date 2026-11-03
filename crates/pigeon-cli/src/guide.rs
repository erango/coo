//! `coo agent-help`: the one guide agents read. Shared phrasing comes from
//! `pigeon_engine::agent_setup::guide` (also used by the MCP server's
//! instructions and the skill), so the three can't disagree.

use pigeon_engine::agent_setup::guide::*;

/// (command, one-line example, what it's for). Every subcommand must be here
/// (checked by a test).
const COMMANDS: &[(&str, &str, &str)] = &[
    ("accounts", "coo --json accounts", "signed-in accounts (ids for every other command)"),
    ("folders", "coo --json folders me@x.com", "an account's folders/labels with counts"),
    ("ls", "coo --json ls --threads --unread -n 20", "a mailbox; -f <role|folder> -a <account>"),
    ("search", "coo --json search \"invoice from:dana has:attachment\"", "search; --server also asks the servers"),
    ("thread", "coo --json thread <account> <thread_id>", "a whole conversation with bodies"),
    ("get", "coo --json get <account> <message_id>", "one message with its plain-text body"),
    ("attachment", "coo --json attachment <account> <message_id> <attachment_id>", "download; prints the local path"),
    ("agenda", "coo --json agenda --from today --to +7d", "events in a range (-a <account>)"),
    ("freebusy", "coo --json freebusy <account> a@x.com,b@y.com --from +1d --to +2d", "busy intervals"),
    ("sync", "coo --json sync", "pull new mail and events (all accounts, or one)"),
    (
        "watch",
        "coo --json watch --from @acme.com --subject invoice",
        "wait for new mail; one JSON line per match (--thread --account --once --include-mine)",
    ),
    ("watches", "coo --json watches", "the running watches"),
    ("unwatch", "coo --json unwatch <watch_id>", "stop a watch (--all: every one)"),
    ("senders", "coo --json senders", "every address the user can send from"),
    (
        "reply",
        "coo --json reply <account> <message_id> --body \"…\" --dry-run",
        "reply/--all/--forward; draft with --dry-run",
    ),
    ("send", "coo --json send --from me@x.com --to a@y.com --subject Hi --body \"…\" --dry-run", "new message"),
    ("draft-send", "coo --json draft-send <draft_id>", "send a saved draft"),
    ("draft-delete", "coo --json draft-delete <draft_id>", "delete a saved draft"),
    (
        "act",
        "coo --json --yes act archive <account> <message_id>",
        "read|unread|flag|unflag|archive|trash|delete_forever|spam|not_spam|phishing|move --to <folder>",
    ),
    ("undo", "coo --json --yes undo <account> <undo_handle>", "revert an act"),
    ("rsvp", "coo --json --yes rsvp <account> <calendar_id> <event_id> accept", "accept|tentative|decline"),
    (
        "event-create",
        "coo --json --yes event-create <account> --title T --start 2026-11-02T10:00 --duration 30",
        "new event (--video meet|zoom adds a call)",
    ),
    ("event-delete", "coo --json --yes event-delete <account> <calendar_id> <event_id>", "delete/cancel an event"),
    ("login", "coo login google", "a person signs in (browser); agents can't"),
    ("logout", "coo logout <account>", "remove an account (person only)"),
    ("mcp", "coo mcp [--allow-write]", "serve all of this over MCP on stdio"),
    ("mcp-config", "coo mcp-config --client claude-code", "print the MCP setup for a client"),
    ("install-skill", "coo install-skill", "install the Claude Code skill"),
    ("doctor", "coo --json doctor", "check setup and sign-in access"),
    ("agent-help", "coo agent-help", "this guide"),
    ("telemetry-test", "coo telemetry-test", "developer: send a test error to Sentry"),
];

const SHAPES: &str = "\
JSON shapes (fields; times RFC 3339 local):
  account:  id provider email name
  message:  account id thread_id subject from{name,email} to[] cc[] snippet received_at unread flagged has_attachments draft
  thread:   account thread_id subject participants[] message_count unread_count has_draft latest{message}
  full message (get / thread items): message fields + text truncated attachments[{id,name,content_type,size,inline}] in_reply_to references[]
  event:    account calendar_id id title start end all_day location organizer attendees[{email,name,response,optional}] my_response online_url cancelled
  act/undo: undo_handle sent deferred rejected[] needs_sign_in
  draft:    draft_id from from_account to[] cc[] subject crosses_accounts message_id(null until sent) in_server_drafts note
  watch lines: {event:\"watching\", id, filters, stop} first, then {event:\"mail\", watch, ...message}; \
{event:\"coo_not_running\"|\"coo_running\"} when the app (which syncs) stops or comes back
  error (stdout, non-zero exit): {\"error\": {\"kind\", \"message\"}}";

pub fn agent_help() -> String {
    let width = COMMANDS.iter().map(|(_, ex, _)| ex.len()).max().unwrap_or(0).min(72);
    let mut cmds = String::new();
    for (_, example, what) in COMMANDS {
        cmds.push_str(&format!("  {example:<width$}  {what}\n"));
    }
    format!(
        "PIGEON FOR AGENTS\n\n{PURPOSE}\n\n{WORKFLOW}\n\n\
         Always pass --json and parse stdout. Ids come from earlier results; never invent them.\n\n\
         COMMANDS\n{cmds}\n{SHAPES}\n\n{OPERATORS}\n\n{TIMES}\n\n\
         WRITES\n{WRITE_ETIQUETTE}\nCLI: from a non-interactive caller, writes need --yes; send/reply take --dry-run \
         (save a draft to the account's Drafts folder, don't send; needs no --yes).\n\n{EXIT_CODES}\n{ERRORS}\n\n\
         MONITORING\nTo be told about new mail (a reply on a thread, mail from someone, a subject), run \
         `coo --json watch ...` as a background task (Claude Code: Bash with run_in_background, or Monitor) and act \
         on each line it prints. Repeat a flag to match any of its values; different flags must all match. It reports \
         only mail that arrives after it starts, never your own unless --include-mine. Stop it with `coo unwatch <id>` \
         (the id is on its first line; `coo watches` lists them), or stop the background task.\n\n\
         MCP\n`coo mcp` serves the same operations as tools on stdio (read-only; add --allow-write for writes). \
         `coo mcp-config --client claude-code|claude-code-desktop|claude-desktop` prints a ready config with the \
         absolute path and env.\n"
    )
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn mentions_every_subcommand() {
        let help = agent_help();
        for sub in crate::Cli::command().get_subcommands().filter(|s| !s.is_hide_set()) {
            let name = sub.get_name();
            let entry = COMMANDS.iter().find(|(c, ex, _)| *c == name && ex.contains(&format!(" {name}")));
            let (_, example, _) = entry.unwrap_or_else(|| panic!("agent-help is missing `{name}`"));
            assert!(help.contains(example), "{name}");
        }
    }

    #[test]
    fn covers_the_rules() {
        let h = agent_help();
        for needle in ["--json", "--yes", "--dry-run", "PIGEON_AGENT_READONLY", "Exit codes", "from:", "+7d", "mcp"] {
            assert!(h.contains(needle), "missing {needle}");
        }
    }
}
