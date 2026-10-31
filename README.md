# Coo

A fast, keyboard-first mail + calendar desktop app for macOS: Microsoft 365 /
Outlook, Gmail / Google Workspace and iCloud in one inbox and one calendar.
Rust + Dioxus. Download: **[erango.github.io/coo](https://erango.github.io/coo/)**.

## Agent friendly

Coo lets AI agents (Claude Code, Claude Desktop, or anything that speaks MCP
or runs a CLI) work with your mail and calendars, locally, on your Mac:

- **`coo mcp`**: a Model Context Protocol server. Read-only by default (search,
  read threads, list events, free/busy); write tools (archive, reply, send,
  RSVP, create events) only with `--allow-write`, and replies/sends are drafts
  unless the agent explicitly asks to send.
- **`coo --json …`**: every CLI command has stable JSON output and exit codes;
  writes from scripts need `--yes`, and `PIGEON_AGENT_READONLY=1` blocks them.
- **Self-describing**: `coo agent-help` prints the full guide for agents, and
  `coo install-skill` adds a Claude Code skill so Claude reaches for Coo on its
  own.
- **One click in the app**: Help › Connect an AI Agent… shows (and can add) the
  exact setup for Claude Code, Claude Desktop and other agents.

Nothing goes through a third-party server: the agent talks to Coo on your Mac,
and Coo talks to your providers. Details: [docs/agents.md](docs/agents.md).

## Setup (dev)

You need your own OAuth client registrations.

**Microsoft (Entra ID)**
1. Azure portal → App registrations → New registration.
2. Supported accounts: *Any Entra ID tenant + personal Microsoft accounts*.
3. Authentication → Add platform → *Mobile and desktop applications* →
   redirect URI `http://localhost`. Enable *Allow public client flows*.
4. API permissions (delegated, Microsoft Graph): `User.Read`, `Mail.ReadWrite`,
   `Mail.Send`, `Calendars.ReadWrite`, `offline_access`. For sender photos
   (asked for later, in Settings › Appearance, never at sign-in) also
   `Contacts.Read` and `User.ReadBasic.All`; some organizations need an
   admin to approve the latter, and Coo then asks for contacts only.
5. `export PIGEON_MS_CLIENT_ID=<application (client) id>`

**Google**
1. Google Cloud console → new project → enable *Gmail API* and *Google
   Calendar API*.
2. OAuth consent screen: External, *Testing*; add your accounts as test users.
3. Credentials → OAuth client ID → *Desktop app*.
4. `export PIGEON_GOOGLE_CLIENT_ID=... PIGEON_GOOGLE_CLIENT_SECRET=...`
   (a desktop client's "secret" is not confidential; Google still requires it).
5. Optional, attaching from Google Drive: enable the *Google Drive API* and
   the *Google Picker API*, add the `.../auth/drive.file` scope on the OAuth
   consent screen's Data Access page, create an API key restricted to the
   Google Picker API, and `export PIGEON_GOOGLE_API_KEY=...`. Without the key
   the composer's "From Google Drive…" is unavailable. The picker runs in
   your browser; Coo only ever sees the files you pick. Accounts that signed
   in before this are asked once to allow Drive access.

Then:

```sh
export PIGEON_TOKEN_STORE=file   # avoid Keychain prompts on every rebuild
cargo cli login google
cargo cli login microsoft
cargo cli sync
cargo cli ls --threads
```

Using Coo from Claude or another agent (CLI `--json`, `coo mcp`): see
[docs/agents.md](docs/agents.md).

Optional crash reporting: `export PIGEON_SENTRY_DSN=<dsn>` (Sentry project,
platform Rust). Off when unset; `PIGEON_TELEMETRY=off` disables it regardless.

Release builds can bake the client ids in at compile time by setting the same
env vars during `cargo build --release`.

## Releasing

Bump `[workspace.package] version` in `Cargo.toml`, then
`git tag vX.Y.Z && git push origin vX.Y.Z`. The release workflow:

1. builds a universal `Coo.app` with Sparkle 2 embedded (`scripts/bundle.sh`,
   build number = commit count), signs it inside-out with the Developer ID and
   embeds the provisioning profile (Time Sensitive reminders);
2. notarizes and staples the app and the `.dmg`;
3. signs the dmg for Sparkle (EdDSA) and writes `appcast.xml`;
4. publishes a GitHub Release with the dmg, its `.sha256` and `appcast.xml`,
   here and in the public [erango/coo-releases](https://github.com/erango/coo-releases)
   (needs the `RELEASES_TOKEN` secret: a fine-grained token with Contents
   read/write on that repo). Downloads and the update feed point there.

Installed apps read coo-releases' `releases/latest/download/appcast.xml` (older builds read this repo's) and check every
6 h, when they come back to the foreground (at most hourly) and when About
opens. Updates download in the background; then the sidebar shows "Update
ready · Restart" (also Coo → Restart to Update, and the menu-bar menu). While
Coo sits in the menu bar it installs by itself after 10 minutes, unless
something is being written or sent (Settings → About & updates, or Coo →
Check for Updates…).

Secrets (repo settings): `MACOS_CERT_P12` + `MACOS_CERT_PASSWORD` (Developer
ID .p12), `MACOS_SIGN_IDENTITY`, `APPLE_API_KEY_ID` / `APPLE_API_ISSUER_ID` /
`APPLE_API_KEY_P8` (notarization), `PIGEON_PROVISION_PROFILE_B64`,
`SPARKLE_ED_PRIVATE_KEY`, plus the OAuth ids, `PIGEON_GOOGLE_API_KEY` (Drive
picker) and `PIGEON_SENTRY_DSN` that get
compiled in. Without the signing secrets the build is ad-hoc signed; without
`SPARKLE_ED_PRIVATE_KEY` no appcast is published and installs won't see the
release. The Sparkle key's original copy is in the release manager's login
Keychain (`generate_keys --account coo`); its public half is in `bundle.sh`.

Local signed build: `PIGEON_SIGN_IDENTITY="Developer ID Application: …"
PIGEON_PROVISION_PROFILE=… ./scripts/bundle.sh`, then
`PIGEON_NOTARY_PROFILE=<notarytool profile> ./scripts/notarize.sh dist/Coo.app`.

## Download site

The download site (erango.github.io/coo) lives in the public
[erango/erango.github.io](https://github.com/erango/erango.github.io) repo,
under `coo/`. Its Download button reads the latest release from
[erango/coo-releases](https://github.com/erango/coo-releases).
