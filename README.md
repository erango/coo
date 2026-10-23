# Coo

A fast, keyboard-first mail + calendar desktop app for Microsoft 365, Google
Workspace / Gmail and iCloud. Rust + Dioxus. Early development: the
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
4. publishes a GitHub Release with the dmg, its `.sha256` and `appcast.xml`.

Installed apps read `releases/latest/download/appcast.xml`, check every 6 h,
download in the background and install on quit (Settings → About & updates,
or Coo → Check for Updates…).

Secrets (repo settings): `MACOS_CERT_P12` + `MACOS_CERT_PASSWORD` (Developer
ID .p12), `MACOS_SIGN_IDENTITY`, `APPLE_API_KEY_ID` / `APPLE_API_ISSUER_ID` /
`APPLE_API_KEY_P8` (notarization), `PIGEON_PROVISION_PROFILE_B64`,
`SPARKLE_ED_PRIVATE_KEY`, plus the OAuth ids and `PIGEON_SENTRY_DSN` that get
compiled in. Without the signing secrets the build is ad-hoc signed; without
`SPARKLE_ED_PRIVATE_KEY` no appcast is published and installs won't see the
release. The Sparkle key's original copy is in the release manager's login
Keychain (`generate_keys --account coo`); its public half is in `bundle.sh`.

Local signed build: `PIGEON_SIGN_IDENTITY="Developer ID Application: …"
PIGEON_PROVISION_PROFILE=… ./scripts/bundle.sh`, then
`PIGEON_NOTARY_PROFILE=<notarytool profile> ./scripts/notarize.sh dist/Coo.app`.
