# Pigeon

A fast, keyboard-first mail + calendar desktop app for Microsoft 365, Google
Workspace / Gmail (and iCloud, soon). Rust + Dioxus. Early development: the
engine and a CLI harness exist; the desktop UI is being designed.

## Setup (dev)

You need your own OAuth client registrations.

**Microsoft (Entra ID)**
1. Azure portal → App registrations → New registration.
2. Supported accounts: *Any Entra ID tenant + personal Microsoft accounts*.
3. Authentication → Add platform → *Mobile and desktop applications* →
   redirect URI `http://localhost`. Enable *Allow public client flows*.
4. API permissions (delegated, Microsoft Graph): `User.Read`, `Mail.ReadWrite`,
   `Mail.Send`, `Calendars.ReadWrite`, `offline_access`.
5. `export PIGEON_MS_CLIENT_ID=<application (client) id>`

**Google**
1. Google Cloud console → new project → enable *Gmail API* and *Google
   Calendar API*.
2. OAuth consent screen: External, *Testing*; add your accounts as test users.
3. Credentials → OAuth client ID → *Desktop app*.
4. `export PIGEON_GOOGLE_CLIENT_ID=... PIGEON_GOOGLE_CLIENT_SECRET=...`
   (a desktop client's "secret" is not confidential; Google still requires it).

Then:

```sh
export PIGEON_TOKEN_STORE=file   # avoid Keychain prompts on every rebuild
cargo cli login google
cargo cli login microsoft
cargo cli sync
cargo cli ls --threads
```

Optional crash reporting: `export PIGEON_SENTRY_DSN=<dsn>` (Sentry project,
platform Rust). Off when unset; `PIGEON_TELEMETRY=off` disables it regardless.

Release builds can bake the client ids in at compile time by setting the same
env vars during `cargo build --release`.
