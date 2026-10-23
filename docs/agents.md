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
./target/release/pigeon login icloud --email name@icloud.com   # app-specific password; or PIGEON_ICLOUD_PASSWORD
./target/release/pigeon sync               # first sync can take a few minutes
```

## Easiest: the app does it

**Pigeon › Help › Connect an AI Agent…** (also in ⌘K and Settings) shows the
exact setup for Claude Code (terminal or desktop app), Claude Desktop and
other agents, with Copy buttons. In the Claude Code desktop app tab, **Add to
Claude Code…** writes the entry for you (after you confirm). Everything below
is what that screen generates.

Agents learn the rest from Pigeon itself: the MCP server describes its tools
and rules, `pigeon --help` points agents at `pigeon agent-help` (the full,
always-current guide), and the optional skill tells Claude when to reach for
Pigeon.

## MCP (recommended for Claude)

Read-only by default. Write tools (mail actions, reply/send, RSVP, create
event) appear only with `--allow-write`, and reply/send still only save a
draft unless the call passes `send: true`.

Print a ready-made config, with this binary's absolute path and every env var
it needs (agents launched from GUI apps don't get your shell's PATH or env):

```sh
pigeon mcp-config --client claude-code            # `claude mcp add --scope user …` for a terminal
pigeon mcp-config --client claude-code-desktop    # JSON for ~/.claude.json
pigeon mcp-config --client claude-desktop         # JSON for claude_desktop_config.json
# add --allow-write to any of them for write tools
```

- **Claude Code (terminal and desktop app)** share user-scope MCP servers in
  `~/.claude.json` (top-level `"mcpServers"`), so one setup covers both.
  `claude mcp add --scope user …` writes it from a terminal; the app's
  **Add to Claude Code…** button merges it in (other servers and settings
  kept, timestamped backup next to the file). Restart Claude Code, or reload
  its window, afterwards.
- **Claude Desktop** reads `~/Library/Application Support/Claude/claude_desktop_config.json`
  (Settings › Developer › Edit Config); quit and reopen Claude after editing.
  Claude Code's desktop app also loads servers from this file.

Tools: `list_accounts`, `list_folders`, `list_messages`, `search_mail`,
`get_message`, `get_thread`, `list_events`, `free_busy`, `sync`; with
`--allow-write` also `mail_action`, `undo`, `reply`, `send_email`, `rsvp`,
`create_event`.

### Sign-in storage (Keychain vs. file)

The agent's `pigeon mcp` must read the same sign-ins as the app, so the
generated config uses the app's token store:

- **File store** (`PIGEON_TOKEN_STORE=file`, typical for dev builds): the
  config carries `PIGEON_TOKEN_STORE=file`; nothing else to do.
- **Keychain** (the default): macOS asks before a different program reads
  Pigeon's Keychain items, and an MCP subprocess has no one to answer. Run
  `pigeon doctor` once in Terminal and choose **Always Allow** (again after
  updating Pigeon). Until then the server answers with a clear "needs your
  approval" error instead of hanging: it runs with Keychain UI disabled.

## Skill (Claude Code)

```sh
pigeon install-skill        # → ~/.claude/skills/pigeon/SKILL.md (won't overwrite a changed file without --force)
```

The skill's description makes Claude use Pigeon for the user's mail and
calendar tasks; its body points at `pigeon agent-help`. Claude Code picks up
skill changes in the running session, but restart it if `~/.claude/skills`
didn't exist before. Whether the Claude Code desktop app reads personal skills
isn't documented; the MCP server works without the skill.

## Discovering usage

- `pigeon agent-help`: the agent guide (workflow, every command with an
  example, JSON shapes, operators, time formats, write rules, exit codes).
- `pigeon --help`: its first line sends agents to `agent-help`.
- `pigeon --json doctor`: setup checks (data dir, accounts, sign-in access,
  client ids).

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
