//! "Connect an AI agent": what to paste (or click) so Claude Code (terminal
//! or desktop app), Claude Desktop, or any MCP agent can use Coo. All the
//! snippets come from `pigeon_engine::agent_setup`, the same code behind
//! `coo mcp-config`, so the app and the CLI never disagree.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use dioxus::prelude::*;
use pigeon_engine::agent_setup::{self, Client, Setup};
use pigeon_engine::service::Toast;

use super::Icon;
use crate::state::AppState;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    CodeTerminal,
    CodeDesktop,
    ClaudeDesktop,
    Other,
}

const TABS: [(Tab, &str); 4] = [
    (Tab::CodeTerminal, "Claude Code · Terminal"),
    (Tab::CodeDesktop, "Claude Code · Desktop app"),
    (Tab::ClaudeDesktop, "Claude Desktop"),
    (Tab::Other, "Other agents"),
];

fn toast(s: AppState, message: impl Into<String>, error: bool) {
    static SEQ: AtomicU64 = AtomicU64::new((1 << 62) + (1 << 40));
    s.push_toast(Toast {
        id: SEQ.fetch_add(1, Ordering::Relaxed),
        message: message.into(),
        error,
        undo: None,
        cancel_send: None,
    });
}

/// Copy to the clipboard from the web view (falls back to execCommand when
/// the async clipboard API isn't available to this origin).
fn copy(s: AppState, text: String, what: &'static str) {
    spawn(async move {
        let js = format!(
            "const t = {text:?};\
             try {{ await navigator.clipboard.writeText(t); return true; }} catch (_) {{\
               const a = document.createElement('textarea'); a.value = t; a.style.position = 'fixed'; a.style.opacity = '0';\
               document.body.appendChild(a); a.select(); const ok = document.execCommand('copy'); a.remove(); return ok; }}"
        );
        match document::eval(&js).join::<bool>().await {
            Ok(true) => toast(s, format!("Copied {what}"), false),
            _ => toast(s, "Couldn't copy: select the text and press ⌘C", true),
        }
    });
}

/// The token store this app is using (agents must use the same one).
fn token_store_kind() -> &'static str {
    pigeon_core::paths::data_dir().map(|d| pigeon_core::tokens::TokenStore::from_env(&d).kind()).unwrap_or("keychain")
}

fn cli_binary() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|exe| agent_setup::find_cli_binary(&exe))
}

#[component]
fn Snippet(text: String, what: &'static str, #[props(default)] prose: bool) -> Element {
    let s = use_context::<AppState>();
    let t = text.clone();
    let class = if prose { "agent-snippet__code agent-snippet__code--prose" } else { "agent-snippet__code t-mono" };
    rsx! {
        div { class: "agent-snippet",
            pre { class: "{class}", "{text}" }
            button { class: "btn btn--secondary btn--sm agent-snippet__copy", r#type: "button",
                onclick: move |_| copy(s, t.clone(), what),
                Icon { name: "copy", class: "icon--sm" } "Copy"
            }
        }
    }
}

#[component]
pub fn AgentSetup() -> Element {
    let s = use_context::<AppState>();
    let mut open = s.agent_setup_open;
    let mut tab = use_signal(|| {
        // Debug builds: PIGEON_DEBUG_OPEN=agent-setup:<tab> opens on that tab (screenshots).
        match std::env::var("PIGEON_DEBUG_OPEN").ok().filter(|_| cfg!(debug_assertions)).as_deref() {
            Some("agent-setup:desktop") => Tab::CodeDesktop,
            Some("agent-setup:claude") => Tab::ClaudeDesktop,
            Some("agent-setup:other") => Tab::Other,
            _ => Tab::CodeTerminal,
        }
    });
    let mut allow_write = use_signal(|| false);
    let mut confirm_add = use_signal(|| false);
    let mut result = use_signal(|| None::<(bool, String)>);
    let binary = use_hook(cli_binary);
    let store = use_hook(token_store_kind);
    let current = *tab.read();
    // (Computed up front: an `if` directly in a component prop trips rsx.)
    let outcome: Option<(&'static str, &'static str, String)> = result
        .read()
        .clone()
        .map(|(ok, msg)| if ok { ("banner banner--accent", "check", msg) } else { ("banner", "alert", msg) });

    let Some(bin) = binary else {
        return rsx! {
            div { class: "agent-setup-scrim", onclick: move |_| open.set(false),
                div { class: "dialog agent-setup", role: "dialog", onclick: move |e| e.stop_propagation(),
                    div { class: "dialog__title", "Connect an AI agent" }
                    div { class: "dialog__body",
                        "Agents talk to Coo through its command-line tool, which isn't installed next to the app. Build it with "
                        code { class: "t-mono", "cargo build --release -p pigeon-cli" }
                        " and put "
                        code { class: "t-mono", "target/release/pigeon" }
                        " in /usr/local/bin (or next to Coo), then open this again."
                    }
                    div { class: "dialog__actions", button { class: "btn btn--primary", onclick: move |_| open.set(false), "Close" } }
                }
            }
        };
    };

    let setup = Setup::detect(bin.clone(), *allow_write.read(), store);
    let bin_str = bin.to_string_lossy().into_owned();
    let config_path =
        agent_setup::claude_code_config_path().map(|p| p.display().to_string()).unwrap_or("~/.claude.json".into());
    let setup_for_add = setup.clone();
    let bin_for_skill = bin.clone();
    let add_to_claude_code = move |_| {
        let r =
            agent_setup::claude_code_config_path().and_then(|p| agent_setup::install_claude_code(&setup_for_add, &p));
        confirm_add.set(false);
        result.set(Some(match r {
            Ok(r) => (
                true,
                format!(
                    "{} Coo in {}.{} Restart Claude Code (or reload its window) to pick it up.",
                    if r.replaced { "Updated" } else { "Added" },
                    r.path.display(),
                    r.backup.map(|b| format!(" Backup: {}.", b.display())).unwrap_or_default()
                ),
            ),
            Err(e) => (false, format!("Didn't change anything: {e}")),
        }));
    };
    let install_skill = move |_| {
        let r = agent_setup::install_skill(None, &bin_for_skill, true);
        result.set(Some(match r {
            Ok((path, _)) => (
                true,
                format!(
                    "Installed the skill at {}. Claude Code loads it in new sessions (restart it if it was open).",
                    path.display()
                ),
            ),
            Err(e) => (false, format!("Couldn't install the skill: {e}")),
        }));
    };

    rsx! {
        div { class: "agent-setup-scrim", onclick: move |_| open.set(false),
            div { class: "dialog agent-setup", role: "dialog", "aria-label": "Connect an AI agent",
                onclick: move |e| e.stop_propagation(),
                div { class: "agent-setup__head",
                    div {
                        div { class: "dialog__title", "Connect an AI agent" }
                        div { class: "dialog__body",
                            "Let Claude (or any MCP agent) search and read your mail and calendar through Coo, on this Mac. Read-only unless you allow more below."
                        }
                    }
                    button { class: "icon-btn icon-btn--sm", "aria-label": "Close (Esc)", onclick: move |_| open.set(false), Icon { name: "x" } }
                }

                label { class: "agent-setup__toggle",
                    input { class: "switch", r#type: "checkbox", checked: *allow_write.read(),
                        onchange: move |e| allow_write.set(e.checked()) }
                    div {
                        div { "Allow the agent to send and change things " code { class: "t-mono", "--allow-write" } }
                        div { class: "t-3", style: "font-size:12px",
                            "Archive, flag, reply, RSVP, create events. Replies are saved as drafts unless the agent asks to send, and it's told to confirm with you first."
                        }
                    }
                }

                div { class: "segmented agent-setup__tabs", role: "tablist",
                    for (t, label) in TABS {
                        button { role: "tab", "aria-selected": "{current == t}", onclick: move |_| { tab.set(t); result.set(None); }, "{label}" }
                    }
                }

                div { class: "agent-setup__body",
                    match current {
                        Tab::CodeTerminal => rsx! {
                            p { "Run this once in Terminal. It adds Coo for all your projects; the Claude Code desktop app uses the same setting." }
                            Snippet { text: setup.claude_code_command(), what: "command" }
                        },
                        Tab::CodeDesktop => rsx! {
                            p { "No terminal needed: Coo can add itself to Claude Code's settings ("
                                code { class: "t-mono", "{config_path}" }
                                ", shared by the desktop app and the terminal)." }
                            if *confirm_add.read() {
                                div { class: "banner banner--info agent-setup__confirm",
                                    span { class: "spacer",
                                        "This adds a \"pigeon\" entry to "
                                        code { class: "t-mono", "{config_path}" }
                                        ". Your other servers and settings stay as they are, and a backup is saved next to it."
                                    }
                                    button { class: "btn btn--primary btn--sm", onclick: add_to_claude_code, "Add" }
                                    button { class: "btn btn--ghost btn--sm", onclick: move |_| confirm_add.set(false), "Cancel" }
                                }
                            } else {
                                div { button { class: "btn btn--primary", r#type: "button", onclick: move |_| confirm_add.set(true),
                                    Icon { name: "plus", class: "icon--sm" } "Add to Claude Code…" } }
                            }
                            p { class: "t-3", "Or merge this into that file yourself (keep your other \"mcpServers\" entries):" }
                            Snippet { text: setup.config_json(Client::ClaudeCodeDesktop), what: "config" }
                            p { class: "t-3", "Then restart Claude Code (or reload its window)." }
                        },
                        Tab::ClaudeDesktop => rsx! {
                            p { "In Claude, open Settings › Developer › Edit Config, and merge this into "
                                code { class: "t-mono", "claude_desktop_config.json" }
                                ". Then quit and reopen Claude. (Claude Code's desktop app also reads this file.)" }
                            Snippet { text: setup.config_json(Client::ClaudeDesktop), what: "config" }
                        },
                        Tab::Other => rsx! {
                            p { "Any MCP client: start "
                                code { class: "t-mono", "{bin_str} mcp" }
                                " over stdio with this config (absolute path and env included):" }
                            Snippet { text: setup.config_json(Client::ClaudeCodeDesktop), what: "config" }
                            p { "Agents without MCP can use the CLI. Tell them to run:" }
                            Snippet { text: format!("{bin_str} agent-help"), what: "command" }
                        },
                    }
                }

                div { class: "agent-setup__row",
                    div { class: "spacer",
                        div { "Claude Code skill " span { class: "t-3", "(optional)" } }
                        div { class: "t-3", style: "font-size:12px",
                            "Lets Claude reach for Coo by itself for mail and calendar tasks. Installs to ~/.claude/skills/coo. The desktop app may need a restart to see it."
                        }
                    }
                    button { class: "btn btn--secondary btn--sm", r#type: "button", onclick: install_skill, "Install skill" }
                }

                if let Some((class, icon, msg)) = outcome {
                    div { class: "{class}", role: "status",
                        Icon { name: icon, class: "icon--sm" }
                        span { dir: "auto", "{msg}" }
                    }
                }

                for note in setup.notes() {
                    div { class: "banner banner--info", Icon { name: "alert", class: "icon--sm" } span { "{note}" } }
                }

                div { class: "agent-setup__label", "Then tell your agent" }
                Snippet { text: setup.agent_prompt(), what: "prompt", prose: true }

                div { class: "dialog__actions",
                    button { class: "btn btn--primary", r#type: "button", onclick: move |_| open.set(false), "Done" }
                }
            }
        }
    }
}
