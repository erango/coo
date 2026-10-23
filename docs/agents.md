# Using Pigeon from an AI agent

Pigeon keeps a local, synced copy of your mail and calendars. Agents can use
it two ways: the `pigeon` CLI with `--json`, or `pigeon mcp` (Model Context
Protocol over stdio). Both read **your own accounts** from your machine.

## One-time setup

```sh
cargo build --release -p pigeon-cli        # → target/release/pigeon
source .env.local                          # OAuth client ids (see README)
export PIGEON_TOKEN_STORE=file             # optional: avoid Keychain prompts on rebuilds
./target/release/pigeon login google       # repeat per account; opens the browser
./target/release/pigeon login microsoft
./target/release/pigeon sync               # first sync can take a few minutes
```

## MCP (recommended for Claude)

Read-only by default. Write tools (mail actions, reply/send, RSVP, create
event) appear only with `--allow-write`, and reply/send still only save a
draft unless the call passes `send: true`.

**Claude Code**

```sh
claude mcp add pigeon \
  -e PIGEON_MS_CLIENT_ID=... -e PIGEON_GOOGLE_CLIENT_ID=... -e PIGEON_GOOGLE_CLIENT_SECRET=... \
  -e PIGEON_TOKEN_STORE=file \
  -- /absolute/path/to/pigeon mcp
# add `--allow-write` after `mcp` to enable write tools
```

**Claude Desktop** (`~/Library/Application Support/Claude/claude_desktop_config.json`)

```json
{
  "mcpServers": {
    "pigeon": {
      "command": "/absolute/path/to/pigeon",
      "args": ["mcp"],
      "env": {
        "PIGEON_MS_CLIENT_ID": "...",
        "PIGEON_GOOGLE_CLIENT_ID": "...",
        "PIGEON_GOOGLE_CLIENT_SECRET": "...",
        "PIGEON_TOKEN_STORE": "file"
      }
    }
  }
}
```

Tools: `list_accounts`, `list_folders`, `list_messages`, `search_mail`,
`get_message`, `get_thread`, `list_events`, `free_busy`, `sync`; with
`--allow-write` also `mail_action`, `undo`, `reply`, `send_email`, `rsvp`,
`create_event`.

## CLI

Add `--json` to any command for structured output on stdout (errors too).

| Command | What |
|---|---|
| `pigeon --json accounts` | accounts |
| `pigeon --json ls [--threads] [--unread] [-a ACCOUNT] [-f FOLDER]` | mailbox listing (`inbox`, `sent`, … across accounts, or a folder) |
| `pigeon --json search "invoice from:dana has:attachment" [--server]` | search; `--server` also asks the servers |
| `pigeon --json get ACCOUNT MESSAGE_ID` | one message with plain-text body |
| `pigeon --json thread ACCOUNT THREAD_ID` | whole conversation |
| `pigeon --json agenda --from today --to +7d` | events |
| `pigeon --json freebusy ACCOUNT a@x.com,b@y.com --from +1d --to +2d` | busy times |
| `pigeon --json sync` | pull new mail/events |

Search operators: `from:` `to:` `has:attachment` `is:unread` `is:read`
`is:flagged` `in:<inbox|sent|drafts|trash|archive|starred>` `account:<text>`
`before:YYYY-MM-DD` `after:YYYY-MM-DD`, plus free text.

Times: `now`, `today`, `tomorrow`, `+3d`, `-6h`, `+2w`, `YYYY-MM-DD`,
`YYYY-MM-DDTHH:MM` (local), or RFC 3339. Output times are RFC 3339 in the
local zone; all-day events use `YYYY-MM-DD`.

**Writes** (`act`, `undo`, `send`, `reply`, `rsvp`, `event-create`,
`event-delete`, `logout`) run directly at a terminal, but from a script or
agent they need `--yes`. `send` and `reply` accept `--dry-run` (save a draft,
don't send). `PIGEON_AGENT_READONLY=1` blocks all writes regardless.

**Exit codes:** 0 ok · 1 error · 2 bad usage · 3 sign-in required (run
`pigeon login`) · 4 write blocked.
