# CLAUDE.md

Guidance for working in this repo.

## What this is

Coo — a fast mail + calendar **desktop app** (Outlook alternative), Rust +
Dioxus 0.7 + Tokio, macOS-first. Personal side project. Accounts: Microsoft
365 (Graph), Google (Gmail + Calendar APIs), iCloud later (IMAP/CalDAV).
Expect 5–10 accounts per user. See `ARCHITECTURE.md` (plan, protocols, phases)
and `DESIGN_SPEC.md` (brief handed to Claude Design).

## Layout

- `crates/pigeon-core` — UI-agnostic, no provider code: `model.rs` (normalized
  types), `store/` (SQLite + FTS5, `NNN_*.sql` migrations keyed on
  `user_version`; tests in `store/tests.rs`), `provider.rs` (traits),
  `sync.rs` (drivers), `auth.rs` (OAuth PKCE loopback + `TokenSource`),
  `tokens.rs` (Keychain / dev file), `http.rs` (retry/backoff), `text.rs`.
- `crates/pigeon-graph`, `crates/pigeon-google` — provider impls. Thin
  hand-written `reqwest` + serde clients; only the fields we use.
- `crates/pigeon-engine` — login, account registry, concurrent sync of all
  accounts. Will own the UI `Cmd`/`Delta` loop.
- `crates/pigeon-cli` — `coo` binary, dev harness for the engine.
- `crates/pigeon-bin` — Dioxus 0.7 desktop app (`pigeon-app`). `ui/` components
  render from `state::AppState` signals; the engine side is
  `pigeon-engine/src/service.rs` (`Cmd` in, `Delta` out, 30 s sync tick).
  CSS is `include_str!`'d straight from `design/` (the Claude Design bundle,
  single source of truth; don't fork it — app-only glue goes in
  `pigeon-bin/src/app.css`). `bridge.rs` is the one JS bridge: shortcuts, RTL
  paragraph override (Ctrl+Right/Left Shift), mail-frame sizing + link
  interception.

**Rules:** the UI never does I/O or SQL; `Store` is sync, never held across
`.await`. Sync cursors are written in the same transaction as the changes they
cover. Provider JSON → model mapping lives in pure functions with fixture
tests.

## Dev workflow

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt

cargo app                 # desktop app

# Harness (needs OAuth client ids, see README):
cargo cli login microsoft|google
cargo cli sync            # all accounts, concurrently
cargo cli ls --threads    # unified inbox
cargo cli search invoice
cargo cli events --days 3
```

- `PIGEON_TOKEN_STORE=file` keeps refresh tokens in a 0600 file instead of the
  Keychain (unsigned dev builds otherwise prompt after every rebuild).
- `PIGEON_DATA_DIR=...` isolates the DB/tokens (default
  `~/Library/Application Support/il.co.frost-media.pigeon`).
- `RUST_LOG=pigeon=debug` for request-level logs.
- Crash/error reporting: Sentry, on only when `PIGEON_SENTRY_DSN` is set
  (`PIGEON_TELEMETRY=off` forces it off). Verify with `cargo cli telemetry-test`. `tracing::error!` becomes a Sentry
  event, `warn!`/`info!` breadcrumbs. Everything is scrubbed of email
  addresses and URL query strings (`pigeon-engine/src/telemetry.rs`). Never
  log mail content, subjects, names or event titles; account ids contain the
  address, so they get scrubbed, but prefer not logging them at `error`.
- Releasing: push a `vX.Y.Z` tag; `.github/workflows/release.yml` builds,
  signs, notarizes and publishes the dmg plus the Sparkle `appcast.xml`
  (README → Releasing). Updates are Sparkle 2 (`pigeon-bin/src/updater.rs`,
  loaded at runtime so `cargo app` runs without it). Sparkle compares
  `CFBundleVersion`, which must only grow (the commit count). Coo rarely
  quits (menu-bar mode), so the updater delegate takes Sparkle's
  install-on-quit block and Coo restarts into the update itself.
- Login/sync hit **real mailboxes**. Sync is read-only today; anything that
  mutates server state (send, move, delete) must be explicit.

## Conventions

- Bundle id / Keychain service: `il.co.frost-media.pigeon`.
- Timestamps are epoch milliseconds UTC everywhere in the model and store.
- Content is bidi: render text with `dir="auto"`; composer RTL rules in
  `ARCHITECTURE.md`.
- Prefer pure, testable functions in core/providers and cover them.
