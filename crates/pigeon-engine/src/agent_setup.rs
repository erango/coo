//! Connecting AI agents to Coo: the guide text agents read, the MCP config
//! snippets for each client, the Claude Code skill, and the two installers
//! (skill file, Claude Code user-scope MCP entry). Shared by the CLI
//! (`agent-help`, `mcp-config`, `install-skill`) and the app's
//! "Connect an AI agent" popup, so they can't drift apart.
//!
//! Agents are often launched from GUI apps (Claude Code desktop, Claude
//! Desktop), which don't inherit the shell's PATH or env: every snippet uses
//! the absolute binary path and carries every env var the server needs.

use std::path::{Path, PathBuf};

use pigeon_core::auth::OAuthApp;
use pigeon_core::model::ProviderKind;
use pigeon_core::{Error, Result};
use serde_json::{Value, json};

/// Building blocks of the agent guide, reused by `coo agent-help`, the MCP
/// server's `instructions`, and the skill.
pub mod guide {
    pub const PURPOSE: &str = "Coo is the user's own mail and calendar client (Microsoft 365, Gmail/Workspace, \
        iCloud). It keeps a local, synced copy, so reads are instant and work offline.";

    pub const WORKFLOW: &str = "Workflow: list accounts first; then list a mailbox or search; then read a whole \
        thread (not just one message) before summarizing or replying. Message and thread ids are per account: \
        always pass the account id they came with. If results look stale or a message is missing, sync, then retry.";

    pub const OPERATORS: &str = "Search operators: from:<text> to:<text> has:attachment is:unread is:read \
        is:flagged in:<inbox|sent|drafts|trash|spam|archive|starred> account:<text> before:YYYY-MM-DD \
        after:YYYY-MM-DD, plus free text (prefix matched; quote phrases).";

    pub const TIMES: &str = "Times: now, today, tomorrow, yesterday, +3d, -6h, +2w, YYYY-MM-DD (local midnight), \
        YYYY-MM-DDTHH:MM (local), or RFC 3339. Output times are RFC 3339 in the local zone; all-day events use \
        YYYY-MM-DD.";

    pub const WRITE_ETIQUETTE: &str = "Writes: draft first and show the user what will be sent; only send, \
        archive/delete, or answer invitations after the user agrees. Mail actions return an undo handle. Writes \
        are off unless the user enabled them (MCP --allow-write; CLI --yes when not at a terminal; \
        PIGEON_AGENT_READONLY=1 blocks everything).";

    pub const EXIT_CODES: &str = "Exit codes: 0 ok, 1 error, 2 bad usage, 3 sign-in required (the user signs \
        in again in Coo), 4 write blocked.";

    pub const ERRORS: &str = "Errors carry a kind and one plain sentence on what to do: write_blocked (writes are \
        off: the user turns on sending in Coo › Help › Connect an AI Agent and clicks Update), sign_in_required \
        (the user signs in again in Coo), sign_in_unavailable (Coo isn't running: ask the user to open it), \
        provider_error (Google/Microsoft refused; their message says why, e.g. a missing permission), network. \
        Tell the user the message; don't retry writes blindly.";
}

/// Where a snippet is going.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    /// `claude` in a terminal.
    ClaudeCodeCli,
    /// The Claude Code desktop app: shares `~/.claude.json` with the CLI.
    ClaudeCodeDesktop,
    /// The Claude chat app (`claude_desktop_config.json`).
    ClaudeDesktop,
}

impl Client {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "claude-code" | "claude-code-cli" | "cli" | "terminal" => Some(Client::ClaudeCodeCli),
            "claude-code-desktop" | "code-desktop" => Some(Client::ClaudeCodeDesktop),
            "claude-desktop" | "desktop" => Some(Client::ClaudeDesktop),
            _ => None,
        }
    }
}

/// Everything needed to start `coo mcp` from a client that has no shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setup {
    /// Absolute path of the `coo` CLI.
    pub binary: PathBuf,
    /// Env vars the server needs, in a stable order.
    pub env: Vec<(String, String)>,
    pub allow_write: bool,
    /// `keychain` or `file`: the store the app is using (the agent must use the same).
    pub token_store: &'static str,
    /// Client-id variables that are neither in the env nor built in.
    pub missing: Vec<&'static str>,
}

const CLIENT_VARS: [(&str, ProviderKind); 3] = [
    ("PIGEON_MS_CLIENT_ID", ProviderKind::Microsoft),
    ("PIGEON_GOOGLE_CLIENT_ID", ProviderKind::Google),
    ("PIGEON_GOOGLE_CLIENT_SECRET", ProviderKind::Google),
];

impl Setup {
    /// Snapshot this process's configuration (data dir, token store, client
    /// ids) for an agent's `coo mcp`.
    pub fn detect(binary: PathBuf, allow_write: bool, token_store: &'static str) -> Self {
        let mut env = Vec::new();
        if let Some(dir) = std::env::var_os("PIGEON_DATA_DIR") {
            env.push(("PIGEON_DATA_DIR".to_string(), absolute(Path::new(&dir)).to_string_lossy().into_owned()));
        }
        if token_store == "file" {
            env.push(("PIGEON_TOKEN_STORE".to_string(), "file".to_string()));
        }
        let mut missing = Vec::new();
        for (var, provider) in CLIENT_VARS {
            match std::env::var(var) {
                Ok(v) if !v.is_empty() => env.push((var.to_string(), v)),
                // Not in the env: fine if it's built into this build.
                _ if OAuthApp::from_env(provider).is_ok() => {}
                _ => missing.push(var),
            }
        }
        Setup { binary, env, allow_write, token_store, missing }
    }

    pub fn args(&self) -> Vec<String> {
        let mut a = vec!["mcp".to_string()];
        if self.allow_write {
            a.push("--allow-write".to_string());
        }
        a
    }

    fn binary_str(&self) -> String {
        self.binary.to_string_lossy().into_owned()
    }

    /// One line for a terminal: user scope, so it works in every project and
    /// in the Claude Code desktop app.
    pub fn claude_code_command(&self) -> String {
        let mut parts = vec!["claude mcp add --scope user coo".to_string()];
        for (k, v) in &self.env {
            parts.push(format!("-e {}", shell_quote(&format!("{k}={v}"))));
        }
        parts.push("--".to_string());
        parts.push(shell_quote(&self.binary_str()));
        parts.extend(self.args().iter().map(|a| shell_quote(a)));
        parts.join(" ")
    }

    /// The server entry. Claude Code wants `"type": "stdio"`; Claude Desktop
    /// doesn't use it.
    pub fn server_entry(&self, with_type: bool) -> Value {
        let env: serde_json::Map<String, Value> =
            self.env.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect();
        let mut e = json!({ "command": self.binary_str(), "args": self.args(), "env": env });
        if with_type {
            e.as_object_mut().expect("object").shift_insert(0, "type".into(), json!("stdio"));
        }
        e
    }

    /// The `mcpServers` block for a client's JSON config file.
    pub fn config_json(&self, client: Client) -> String {
        let entry = self.server_entry(client != Client::ClaudeDesktop);
        serde_json::to_string_pretty(&json!({ "mcpServers": { "coo": entry } })).unwrap_or_default()
    }

    /// Something to paste into the agent so it knows what it has.
    pub fn agent_prompt(&self) -> String {
        let writes = if self.allow_write {
            "You may draft replies and mail actions, but ask me before sending anything, archiving or deleting, \
             or answering invitations."
        } else {
            "It's read-only: you can't send or change anything."
        };
        format!(
            "You have Coo, my mail and calendar, as the `coo` MCP server (all my accounts: search mail, read \
             threads, list events, check free/busy). Start with list_accounts. Read whole threads before you \
             summarize or reply. {writes} If the tools aren't there, run `{} agent-help` instead.",
            self.binary_str()
        )
    }

    /// Plain-language notes for the setup screen and `mcp-config`.
    pub fn notes(&self) -> Vec<String> {
        let mut n = Vec::new();
        if self.token_store == "keychain" {
            n.push(
                "Coo shares your sign-ins with the agent while it's running. Closing the window keeps it running in \
                 the menu bar; if Coo has quit, the agent opens it in the background."
                    .to_string(),
            );
        }
        if !self.missing.is_empty() {
            n.push(format!(
                "Not configured here: {}. The agent can read what's already synced, but can't refresh sign-ins until \
                 these are set (add them to the config's env).",
                self.missing.join(", ")
            ));
        }
        n
    }
}

/// POSIX shell single-quoting (only when needed).
pub fn shell_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,".contains(c)) {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn absolute(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| Error::Other("HOME is not set".into()))
}

/// The `coo` CLI to point agents at: the app bundle's
/// `Contents/Helpers/coo`, then next to `exe` (same cargo target dir),
/// then a release build beside it, then PATH and the usual install
/// locations (GUI apps get a minimal PATH). Never `exe` itself: on a
/// case-insensitive disk `MacOS/pigeon` *is* the app's `MacOS/Coo`.
pub fn find_cli_binary(exe: &Path) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = exe.parent() {
        if let Some(contents) = dir.parent() {
            candidates.push(contents.join("Helpers").join("coo"));
        }
        candidates.push(dir.join("coo"));
        if let Some(target) = dir.parent() {
            candidates.push(target.join("release").join("coo"));
            candidates.push(target.join("debug").join("coo"));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|d| d.join("coo")));
    }
    for d in ["/opt/homebrew/bin", "/usr/local/bin"] {
        candidates.push(PathBuf::from(d).join("coo"));
    }
    if let Ok(h) = home() {
        candidates.push(h.join(".cargo/bin/coo"));
    }
    candidates.into_iter().find(|p| is_executable(p) && !same_file(p, exe)).map(|p| absolute(&p))
}

fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (std::fs::metadata(a), std::fs::metadata(b)) {
            (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        absolute(a) == absolute(b)
    }
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

// ---- skill ----

pub fn default_skill_dir() -> Result<PathBuf> {
    Ok(home()?.join(".claude").join("skills").join("coo"))
}

/// SKILL.md: the description makes Claude reach for Coo on the user's own
/// mail/calendar tasks; the body points at the always-current guide.
pub fn skill_md(binary: &Path) -> String {
    let bin = binary.to_string_lossy();
    format!(
        "---\n\
         name: pigeon\n\
         description: Use for the user's own email and calendar through Coo — search or list mail, read and \
         summarize threads, find attachments, check the agenda or someone's free/busy, draft replies, triage the \
         inbox. Covers all their Microsoft 365, Gmail/Workspace and iCloud accounts from a local synced copy.\n\
         ---\n\n\
         # Coo (mail + calendar)\n\n\
         If the `pigeon` MCP tools are available, use them. Otherwise use the CLI:\n\n\
         1. Run `{bin} agent-help` first. It is the current guide: commands, JSON shapes, search operators, \
         time formats, write rules.\n\
         2. Always pass `--json` and parse stdout.\n\
         3. Start with `{bin} --json accounts`, then `ls` / `search`, then `thread` to read a conversation.\n\
         4. Writes (send, reply, archive, RSVP) need the user's OK first; from here they also need `--yes`. \
         Prefer `--dry-run` to show a draft.\n"
    )
}

/// Write the skill. Refuses to overwrite a different existing file unless
/// `force`. Returns the path and whether anything changed.
pub fn install_skill(dir: Option<PathBuf>, binary: &Path, force: bool) -> Result<(PathBuf, bool)> {
    let dir = match dir {
        Some(d) => d,
        None => default_skill_dir()?,
    };
    let path = dir.join("SKILL.md");
    let content = skill_md(binary);
    match std::fs::read_to_string(&path) {
        Ok(existing) if existing == content => return Ok((path, false)),
        Ok(_) if !force => {
            return Err(Error::Other(format!("{} exists and differs; pass --force to replace it", path.display())));
        }
        _ => {}
    }
    std::fs::create_dir_all(&dir)?;
    std::fs::write(&path, content)?;
    Ok((path, true))
}

// ---- Claude Code user-scope config ----

pub fn claude_code_config_path() -> Result<PathBuf> {
    Ok(home()?.join(".claude.json"))
}

pub fn claude_desktop_config_path() -> Result<PathBuf> {
    Ok(home()?.join("Library/Application Support/Claude/claude_desktop_config.json"))
}

/// A `coo` server already configured in an agent's settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Existing {
    pub client: Client,
    pub path: PathBuf,
    /// Started with `--allow-write`.
    pub allow_write: bool,
}

impl Existing {
    pub fn client_name(&self) -> &'static str {
        match self.client {
            Client::ClaudeDesktop => "Claude Desktop",
            _ => "Claude Code",
        }
    }
}

/// The `coo` entry in one config file, if any.
pub fn read_entry(path: &Path, client: Client) -> Option<Existing> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let entry = v.get("mcpServers")?.get("coo")?;
    let allow_write = entry["args"].as_array().is_some_and(|a| a.iter().any(|x| x.as_str() == Some("--allow-write")));
    Some(Existing { client, path: path.to_path_buf(), allow_write })
}

/// Coo entries in Claude Code (`~/.claude.json`) and Claude Desktop.
pub fn existing_entries() -> Vec<Existing> {
    let mut out = Vec::new();
    if let Some(e) = claude_code_config_path().ok().and_then(|p| read_entry(&p, Client::ClaudeCodeDesktop)) {
        out.push(e);
    }
    if let Some(e) = claude_desktop_config_path().ok().and_then(|p| read_entry(&p, Client::ClaudeDesktop)) {
        out.push(e);
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigInstall {
    pub path: PathBuf,
    /// Copy of the previous file (None if there was no file).
    pub backup: Option<PathBuf>,
    /// An existing `coo` entry was replaced.
    pub replaced: bool,
}

/// Add (or update) `mcpServers.pigeon` in a Claude Code config file
/// (`~/.claude.json`: user scope, read by both the CLI and the desktop app).
/// Merges: every other key and server is kept. Backs the file up first and
/// writes atomically, keeping its permissions.
pub fn install_claude_code(setup: &Setup, path: &Path) -> Result<ConfigInstall> {
    install_config(setup, path, Client::ClaudeCodeDesktop)
}

/// Add (or update) `mcpServers.coo` in a client's config file, as
/// [`install_claude_code`] does (merge, backup, atomic write).
pub fn install_config(setup: &Setup, path: &Path, client: Client) -> Result<ConfigInstall> {
    let (mut root, original) = match std::fs::read_to_string(path) {
        Ok(text) => {
            let v: Value = serde_json::from_str(&text)
                .map_err(|e| Error::Other(format!("{} isn't valid JSON ({e}); not touching it", path.display())))?;
            (v, Some(text))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (json!({}), None),
        Err(e) => return Err(e.into()),
    };
    let obj = root
        .as_object_mut()
        .ok_or_else(|| Error::Other(format!("{} isn't a JSON object; not touching it", path.display())))?;
    let servers = obj.entry("mcpServers").or_insert_with(|| json!({}));
    let servers = servers
        .as_object_mut()
        .ok_or_else(|| Error::Other("`mcpServers` isn't an object; not touching the file".into()))?;
    let replaced = servers.insert("coo".into(), setup.server_entry(client != Client::ClaudeDesktop)).is_some();

    let backup = match &original {
        Some(text) => {
            let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
            let name = format!("{}.pigeon-backup-{stamp}", path.file_name().unwrap_or_default().to_string_lossy());
            let b = path.with_file_name(name);
            std::fs::write(&b, text)?;
            Some(b)
        }
        None => None,
    };
    let tmp = path.with_extension("json.pigeon-tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&root)? + "\n")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path).map(|m| m.permissions().mode()).unwrap_or(0o600);
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode & 0o777))?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(ConfigInstall { path: path.to_path_buf(), backup, replaced })
}

#[cfg(test)]
mod tests {
    #[test]
    fn existing_entry_reports_write_mode_and_updates() {
        let dir = std::env::temp_dir().join(format!("coo-existing-{}", pigeon_core::auth::random_token(6)));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("claude_desktop_config.json");
        std::fs::write(&path, r#"{"mcpServers":{"other":{"command":"x"},"coo":{"command":"/c","args":["mcp"]}}}"#)
            .unwrap();
        let e = read_entry(&path, Client::ClaudeDesktop).unwrap();
        assert!(!e.allow_write);
        assert_eq!(e.client_name(), "Claude Desktop");

        let mut s = setup(true);
        s.binary = PathBuf::from("/Applications/Coo.app/Contents/Helpers/coo");
        let r = install_config(&s, &path, Client::ClaudeDesktop).unwrap();
        assert!(r.replaced && r.backup.is_some());
        assert!(read_entry(&path, Client::ClaudeDesktop).unwrap().allow_write);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(v["mcpServers"]["other"].is_object(), "other servers kept");
        assert!(v["mcpServers"]["coo"].get("type").is_none(), "Claude Desktop entries have no type");
        assert!(read_entry(&dir.join("missing.json"), Client::ClaudeDesktop).is_none());
        std::fs::remove_dir_all(dir).ok();
    }

    use super::*;

    fn setup(allow_write: bool) -> Setup {
        Setup {
            binary: PathBuf::from("/Applications/Coo.app/Contents/Helpers/coo"),
            env: vec![("PIGEON_TOKEN_STORE".into(), "file".into()), ("PIGEON_MS_CLIENT_ID".into(), "abc".into())],
            allow_write,
            token_store: "file",
            missing: vec![],
        }
    }

    #[cfg(unix)]
    #[test]
    fn finds_bundled_cli_and_never_the_app_itself() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("pigeon-bundle-{}", pigeon_core::auth::random_token(6)));
        let macos = root.join("Coo.app/Contents/MacOS");
        let helpers = root.join("Coo.app/Contents/Helpers");
        std::fs::create_dir_all(&macos).unwrap();
        let exe = macos.join("Coo");
        std::fs::write(&exe, b"app").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        // On a case-insensitive disk MacOS/pigeon resolves to the app; it must not be chosen.
        let found = find_cli_binary(&exe);
        assert!(found.as_deref().is_none_or(|p| !same_file(p, &exe)), "picked the app itself: {found:?}");

        std::fs::create_dir_all(&helpers).unwrap();
        let cli = helpers.join("coo");
        std::fs::write(&cli, b"cli").unwrap();
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(find_cli_binary(&exe).unwrap(), absolute(&cli));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn claude_code_command_is_absolute_and_carries_env() {
        assert_eq!(
            setup(false).claude_code_command(),
            "claude mcp add --scope user coo -e PIGEON_TOKEN_STORE=file -e PIGEON_MS_CLIENT_ID=abc -- \
             /Applications/Coo.app/Contents/Helpers/coo mcp"
        );
        assert!(setup(true).claude_code_command().ends_with("coo mcp --allow-write"));
        assert_eq!(shell_quote("/My Apps/pigeon"), "'/My Apps/pigeon'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn config_json_per_client() {
        let code: Value = serde_json::from_str(&setup(false).config_json(Client::ClaudeCodeDesktop)).unwrap();
        let e = &code["mcpServers"]["coo"];
        assert_eq!(e["type"], "stdio");
        assert_eq!(e["args"], json!(["mcp"]));
        assert_eq!(e["env"]["PIGEON_TOKEN_STORE"], "file");
        let desktop: Value = serde_json::from_str(&setup(true).config_json(Client::ClaudeDesktop)).unwrap();
        assert!(desktop["mcpServers"]["coo"].get("type").is_none());
        assert_eq!(desktop["mcpServers"]["coo"]["args"], json!(["mcp", "--allow-write"]));
    }

    #[test]
    fn install_merges_backs_up_and_keeps_others() {
        let dir = std::env::temp_dir().join(format!("pigeon-cc-{}", pigeon_core::auth::random_token(6)));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".claude.json");
        let original = r#"{"numStartups": 3, "mcpServers": {"other": {"command": "x"}}, "projects": {"/a": {}}}"#;
        std::fs::write(&path, original).unwrap();

        let r = install_claude_code(&setup(false), &path).unwrap();
        assert!(!r.replaced);
        assert_eq!(std::fs::read_to_string(r.backup.as_ref().unwrap()).unwrap(), original);
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["other"]["command"], "x", "other servers kept");
        assert_eq!(v["numStartups"], 3, "other keys kept");
        assert_eq!(v["mcpServers"]["coo"]["type"], "stdio");
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["numStartups", "mcpServers", "projects"], "order preserved");

        assert!(install_claude_code(&setup(true), &path).unwrap().replaced);

        std::fs::write(&path, "{ not json").unwrap();
        assert!(install_claude_code(&setup(false), &path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json", "invalid files are left alone");

        let fresh = dir.join("new.json");
        let r = install_claude_code(&setup(false), &fresh).unwrap();
        assert_eq!(r.backup, None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn skill_install_refuses_to_clobber() {
        let dir = std::env::temp_dir().join(format!("pigeon-skill-{}", pigeon_core::auth::random_token(6)));
        let bin = Path::new("/usr/local/bin/coo");
        let (path, changed) = install_skill(Some(dir.clone()), bin, false).unwrap();
        assert!(changed);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("---\nname: pigeon\ndescription: "));
        assert!(text.contains("/usr/local/bin/coo agent-help"));
        assert!(!install_skill(Some(dir.clone()), bin, false).unwrap().1, "identical: no-op");
        std::fs::write(&path, "custom").unwrap();
        assert!(install_skill(Some(dir.clone()), bin, false).is_err());
        assert!(install_skill(Some(dir.clone()), bin, true).unwrap().1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn finds_a_binary_next_to_the_app() {
        let dir = std::env::temp_dir().join(format!("pigeon-bin-{}", pigeon_core::auth::random_token(6)));
        std::fs::create_dir_all(&dir).unwrap();
        let cli = dir.join("coo");
        std::fs::write(&cli, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(find_cli_binary(&dir.join("pigeon-app")), Some(std::fs::canonicalize(&cli).unwrap()));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
