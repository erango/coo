# Coo — Architecture & Plan

Coo is a fast, native-feeling **mail + calendar desktop client** (an Outlook
alternative) for Microsoft 365 and Google Workspace / Gmail. Rust + Dioxus 0.7
desktop + Tokio, same shape as Kompass. macOS-first.

## 1. Goals

- **Blazing fast.** Every interaction served from a local store; the network is
  only ever a background concern. Cold start to usable inbox < 300 ms on a warm
  cache. Folder switch / message open < 16 ms (one frame) when cached.
- **Two ecosystems, one UX.** M365 and Google accounts side by side, unified
  inbox and unified calendar.
- **Keyboard-first.** Everything reachable via shortcuts and a ⌘K palette.

## 2. Protocol decisions

| Need | Provider | API | Why |
|---|---|---|---|
| M365 mail | Microsoft | **Microsoft Graph** (`/me/messages`, `/mailFolders/{id}/messages/delta`) | EWS is being retired for Exchange Online (blocking began Oct 2026). Graph is the only forward path. On-prem Exchange is out of scope. |
| M365 calendar | Microsoft | **Graph** (`/me/calendarView/delta`, `/me/events`) | Same auth, delta sync, Teams meetings for free via `isOnlineMeeting`. |
| Gmail / Workspace mail | Google | **Gmail API** (`history.list`, `messages.get?format=raw`) | Native labels/threads, incremental sync via `historyId`, batch endpoints. Better than IMAP for labels and speed. |
| Google calendar | Google | **Calendar API v3** (`events.list` + `syncToken`) | Incremental sync, Meet links via `conferenceData`. |
| Zoom (phase 2) | Zoom | Zoom REST `POST /users/me/meetings` (user OAuth app) | Insert join URL into event body/location. |
| Google Meet (phase 2) | Google | `conferenceData.createRequest` on Google events; **Meet API** `spaces.create` for Meet links on M365 events | |
| Teams (phase 2, nearly free) | Microsoft | Graph `isOnlineMeeting: true` | |

| iCloud mail | Apple | **IMAP** `imap.mail.me.com:993` + **SMTP** `smtp.mail.me.com:587` | Apple has no mail REST API or third-party OAuth. Auth = Apple ID + **app-specific password** (stored in Keychain). |
| iCloud calendar | Apple | **CalDAV** `caldav.icloud.com` (sync-collection / ctag+etag) | Same app-specific password. |

iCloud lands as `pigeon-imap` (generic IMAP: CONDSTORE `MODSEQ` + `UIDVALIDITY`
for incremental sync, IDLE for push) and `pigeon-caldav`, both behind the same
`MailProvider` / `CalendarProvider` traits. Message ids are
`{uidvalidity}:{uid}` per folder; threading is computed locally from
`References` / `In-Reply-To`. Bonus: this also unlocks Fastmail, Yahoo and any
other generic IMAP/CalDAV account.

### Auth

- OAuth 2.0 **authorization code + PKCE**, public desktop client, **loopback
  redirect** (`http://127.0.0.1:<random-port>/callback`) opened in the system
  browser. No embedded webview login (Google blocks it).
- Microsoft: multi-tenant Entra app registration, `common` authority.
  Scopes: `offline_access User.Read Mail.ReadWrite Mail.Send Calendars.ReadWrite
  MailboxSettings.Read People.Read`. Some tenants require admin consent.
- Google: "Desktop app" OAuth client. Scopes: `https://mail.google.com/` (full access: emptying Trash needs it), `gmail.send`,
  `calendar`, `contacts.readonly`, `userinfo.email`.
  **Risk:** Gmail scopes are *restricted* — a public app needs Google
  verification + a CASA security assessment. **Decision:** ship as an
  unverified **test app** (≤100 allow-listed test users, "unverified app"
  warning screen). Revisit verification only if Coo goes public.
- Refresh tokens stored in the **macOS Keychain** (`keyring` crate). Never on
  disk in plaintext. Access tokens kept in memory only.

## 3. Local-first data model

The UI never waits on the network. Flow:

```
Provider API ──▶ sync engine ──▶ SQLite (WAL) ──▶ in-memory view models ──▶ UI
                     ▲                                          │
                     └──────────── Cmd (user actions) ◀─────────┘
```

- **Store: SQLite** via `rusqlite` (bundled), WAL mode, one writer task, read
  pool. Tables: `accounts`, `folders` (Graph folders + Gmail labels normalized),
  `messages` (headers + snippet + flags), `message_folders` (m:n, for labels),
  `threads`, `bodies` (lazy, compressed with `zstd`), `attachments` (metadata;
  blobs on disk under the cache dir, fetched lazily), `events`, `calendars`,
  `contacts`, `sync_state` (delta links / historyId / syncToken per scope),
  `outbox` (pending mutations).
- **Search: SQLite FTS5** over subject/from/to/snippet/body text. Local first,
  falls back to server search (`$search` / Gmail `q`) for older mail.
- **Optimistic mutations.** Archive/delete/flag/read/move apply to the local DB
  immediately, enqueue in `outbox`, and are replayed to the server by the
  engine. Failures roll back and surface a toast.
- **Retention window.** Initial sync pulls headers for the last N days (default
  90) per folder, newest first, so the inbox is usable within seconds; older
  mail backfills in the background or on demand.

### Sync

- **Graph mail:** per-folder `messages/delta` (store `@odata.deltaLink`).
  Folder tree via `mailFolders/delta`. Poll every 30 s + on window focus + after
  any local mutation. (Graph change-notification webhooks need a public
  endpoint; not for a desktop app. Revisit with a tiny relay later if needed.)
- **Gmail:** full bootstrap with `messages.list` + batched `messages.get
  (format=metadata)`, then `history.list?startHistoryId=` every 30 s. On
  `404 historyId expired`, re-bootstrap. Bodies fetched lazily as `format=raw`
  and parsed with `mail-parser`.
- **Calendars:** Graph `calendarView/delta` over a rolling window
  (-30d…+365d); Google `events.list` with `syncToken`, `singleEvents=true` for
  the same window. Store expanded instances; keep series masters for editing.
- **Rate limits:** central per-account token bucket; honor `Retry-After` (Graph
  429/503) and Gmail quota-unit costs; exponential backoff with jitter.
- **Batching:** Graph `$batch` (20 req), Gmail batch endpoint (≤50 req).

### Rendering mail

- Dioxus desktop runs in a system webview (WKWebView on macOS), which is a real
  HTML engine — good for email.
- Sanitize HTML with `ammonia` (strip scripts, event handlers, forms), render
  inside a sandboxed `iframe srcdoc` with a strict CSP.
- **Remote images blocked by default** (tracking pixels); per-sender allowlist.
- `cid:` inline images rewritten to local cached blobs.
- Plain-text mail rendered natively with link detection.

### Many accounts

Design target is **5–10 accounts** at once. Each account syncs independently
(own task, own cursors, own token source and rate-limit budget), concurrently,
so one slow or re-auth-needed account never stalls the others. Unified views
are plain cross-account SQL queries.

### Sending (and replying from a different account)

The From address is chosen at send time, not tied to the account that
received the message: reply to a mail that arrived at account A from account B.

- We **build the MIME ourselves** (`mail-builder`) for every send, setting
  `In-Reply-To` / `References` from the original message so the recipient's
  client threads the reply, whichever account sends it.
- Gmail: `users.messages.send` with the raw MIME (+ `threadId` when the
  sending account already has the thread).
- Graph: `POST /me/sendMail` with the base64 MIME body (`Content-Type:
  text/plain`). The JSON message API can't set `In-Reply-To` (only `x-` custom
  headers), and `/reply` only works on messages in the same mailbox.
- From picker = every account + its aliases (Gmail `settings.sendAs`, Graph
  send-as / shared mailboxes later).
- Drafts are local-first; when From changes, the server-side draft moves
  (delete on old account, create on new).

### RTL / bidi

- Composer paragraphs use `dir="auto"` (first strong character decides);
  manual override per paragraph via Ctrl+Right Shift (RTL) / Ctrl+Left Shift
  (LTR), detected with `KeyboardEvent.location`. Overrides write an explicit
  `dir="rtl|ltr"` on the block.
- Outgoing HTML keeps `dir` attributes on each block (Outlook and Gmail honor
  them); the text/plain alternative gets no marks.
- All list/detail text nodes (subjects, snippets, names) render with
  `dir="auto"` / `unicode-bidi: plaintext`.

## 4. Crate layout

```
crates/
  pigeon-core/      UI-agnostic: no provider code
    model.rs        Account, Folder, MessageHeader, MailChange, Event, ...
    store/          SQLite schema (numbered .sql migrations via user_version), queries, FTS5
    provider.rs     MailProvider / CalendarProvider traits
    sync.rs         sync drivers written once against the traits
    auth.rs         OAuth PKCE + loopback listener, TokenSource (refresh + rotation)
    tokens.rs       refresh-token store: Keychain, or 0600 file for dev
    http.rs         shared reqwest client, retry/backoff honoring Retry-After
    text.rs         address-list parsing, entity decoding, FTS query building
  pigeon-graph/     Microsoft Graph impl of the provider traits
  pigeon-google/    Gmail + Google Calendar impl
  pigeon-engine/    wires providers to core: login, account registry, concurrent
                    multi-account sync; grows the Cmd/Delta loop in phase 1
  pigeon-cli/       `coo` dev harness (login/sync/ls/search/events)
  pigeon-bin/       (phase 1) Dioxus 0.7 desktop app, renders from signals
  pigeon-imap/      (phase 1.5) IMAP + SMTP (iCloud, generic)
  pigeon-caldav/    (phase 1.5) CalDAV (iCloud, generic)
  pigeon-meetings/  (phase 2) Zoom / Meet / Teams link creation
```

**Engine↔UI contract (same as Kompass):** UI sends `Cmd` over a channel; engine
streams `Delta` back; UI holds view models in signals. The UI thread never does
I/O or SQL.

### Key dependencies

`tokio`, `reqwest` (rustls, http2), `serde`/`serde_json`, `rusqlite`
(bundled, FTS5), hand-rolled migrations, hand-rolled OAuth (`sha2`, `getrandom`), `security-framework` (Keychain),
`mail-parser`, `mail-builder`, `ammonia`, `zstd`, `chrono` + `chrono-tz`,
`rrule` (recurrence display), `thiserror`, `tracing`, `dioxus 0.7` (desktop).

Provider clients are hand-written thin wrappers over `reqwest` with typed serde
models for only the endpoints we use — the generated Graph/Google SDKs are
huge and slow to compile.

## 5. Phases

**Phase 0 — skeleton (now, while design happens)**
- Workspace, crates, CI (`cargo test`, clippy, fmt).
- OAuth PKCE + loopback + keychain for both providers. CLI harness
  (`pigeon-cli login|sync|ls`) to exercise the engine without the UI.
- SQLite schema + migrations + FTS.

**Status (2026-10-23):** phase 0 done. Phase 1 engine: bodies + attachments,
optimistic actions + outbox + undo, compose/reply/forward/send with From
switching, calendar incremental sync + writes + RSVP + free/busy, engine
Cmd/Delta service, and a first Dioxus UI (mail three-pane, composer with
cross-account From, RTL editor, week calendar, onboarding) — done, unit-tested
and visually checked on seeded data; not yet run against live accounts.

**Phase 1 — mail + calendar MVP**
- Mail: initial + delta sync, folder/label tree, thread list, message view,
  read/unread, flag/star, archive, delete, move, compose/reply/forward (HTML +
  plain), attachments, send, drafts, local search, unified inbox.
- Calendar: sync, day/week/month views, event detail, create/edit/delete,
  RSVP, multiple calendars with colors, free/busy view of attendees.
- Notifications (new mail, event reminders), menu bar badge.
- UI built from the Claude Design bundle.

**Phase 1.5 — iCloud** (built; untested against a live account)
- `pigeon-imap` (IMAP sync + hand-rolled SMTP submission) and `pigeon-caldav`
  (discovery, calendar-query with server-side `expand`, sync-collection,
  PUT/DELETE, PARTSTAT RSVP); app-specific-password onboarding.
- Known gaps: no IDLE push (polled like the rest), no CONDSTORE (flags pass
  over the window instead), no free/busy, recurring-series edits/deletes and
  single-occurrence changes not supported, send-as aliases not listed.

**Phase 2**
- Zoom + Google Meet + Teams link creation in the event composer.
- Snooze, send later, undo send (outbox delay), templates.
- Contacts autocomplete from People APIs + local history.
- Rules/filters display, shared mailboxes / delegated calendars (Graph).

**Later:** IMAP/JMAP provider, Windows/Linux builds, offline compose for
long trips, AI triage/summaries.

## 6. Release & signing

- **Developer ID signed + notarized** `.app` / `.dmg` (Apple Developer account
  available). Hardened runtime. Reuse Kompass's `bundle.sh` / `notarize.sh`
  shape; GitHub Releases (+ Homebrew cask later).
- Signing matters beyond Gatekeeper: Keychain item ACLs are tied to the code
  signature, so a stable signing identity means no repeated "allow access"
  prompts for stored refresh tokens across updates. Dev builds sign with an
  Apple Development cert for the same reason.
- Notifications (`UNUserNotificationCenter`) and a stable bundle id
  (**`il.co.frost-media.pigeon`**) also want a signed bundle. Keychain service
  name uses the same id.

## 7. Decisions

1. **Distribution:** personal side project; private/test-app distribution for
   now (Google test app, Entra multi-tenant + personal accounts).
2. **Landing view:** unified inbox.
3. **Threading:** user toggle, both supported. Store messages individually;
   thread grouping is a view over `thread_id` (Gmail `threadId`, Graph
   `conversationId`), so switching is a query change, not a re-sync.
