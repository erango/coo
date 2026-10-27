//! "Connect an AI agent": agents on the left with their status, a short
//! checklist for the chosen one on the right (access, install, restart,
//! tell it). All the snippets, statuses and steps come from
//! `pigeon_engine::agent_setup`, the same code behind `coo mcp-config`, so
//! the app and the CLI never disagree.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use dioxus::prelude::*;
use pigeon_engine::agent_setup::{self, Agent, Checklist, Client, Setup, Status, Step};

use super::Icon;
use crate::state::AppState;

/// How often to re-read the agents' configs while open, so a step ticks
/// itself after the user runs the command elsewhere.
const RECHECK: Duration = Duration::from_millis(2500);

/// The token store this app is using (agents must use the same one).
fn token_store_kind() -> &'static str {
    pigeon_core::paths::data_dir().map(|d| pigeon_core::tokens::TokenStore::from_env(&d).kind()).unwrap_or("keychain")
}

fn cli_binary() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|exe| agent_setup::find_cli_binary(&exe))
}

/// `~/…` for paths under HOME.
fn tilde(p: &Path) -> String {
    match std::env::var_os("HOME").map(PathBuf::from).and_then(|h| p.strip_prefix(h).ok().map(Path::to_path_buf)) {
        Some(rest) => format!("~/{}", rest.display()),
        None => p.display().to_string(),
    }
}

fn agent_icon(a: Agent) -> &'static str {
    match a {
        Agent::CodeTerminal => "terminal",
        Agent::CodeDesktop => "code-window",
        Agent::ClaudeDesktop => "chat",
        Agent::Other => "plug",
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Copy {
    Idle,
    Copied,
    Failed,
}

/// Copy from the web view (execCommand when the async clipboard API isn't
/// available to this origin). On failure the snippet's text is selected so
/// ⌘C works.
async fn copy_text(text: &str, select_id: &str) -> bool {
    let js = format!(
        "const t = {}; const id = {};\
         try {{ await navigator.clipboard.writeText(t); return true; }} catch (_) {{}}\
         const a = document.createElement('textarea'); a.value = t; a.style.position = 'fixed'; a.style.opacity = '0';\
         document.body.appendChild(a); a.select(); let ok = false; try {{ ok = document.execCommand('copy'); }} catch (_) {{}}\
         a.remove(); if (ok) return true;\
         const el = document.getElementById(id);\
         if (el) {{ const r = document.createRange(); r.selectNodeContents(el); const s = getSelection(); s.removeAllRanges(); s.addRange(r); }}\
         return false;",
        serde_json::to_string(text).unwrap_or_default(),
        serde_json::to_string(select_id).unwrap_or_default(),
    );
    matches!(document::eval(&js).join::<bool>().await, Ok(true))
}

/// A snippet with its own Copy button: the button reads "✓ Copied" for
/// 1.5 s (same width), the outline turns green, and screen readers hear it.
/// `primary` makes it the step's one action (a labelled button below the
/// text); otherwise a small "Copy" sits in the corner.
#[component]
fn Snippet(
    text: String,
    what: &'static str,
    #[props(default = "Copy")] label: &'static str,
    #[props(default)] primary: bool,
    #[props(default)] prose: bool,
) -> Element {
    let s = use_context::<AppState>();
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let id = use_hook(|| format!("agent-snippet-{}", SEQ.fetch_add(1, Ordering::Relaxed)));
    let mut state = use_signal(|| Copy::Idle);
    let mut generation = use_signal(|| 0u64);
    let code_class =
        if prose { "agent-snippet__code agent-snippet__code--prose" } else { "agent-snippet__code t-mono" };
    let st = *state.read();
    let data_state = match st {
        Copy::Idle => "idle",
        Copy::Copied => "copied",
        Copy::Failed => "failed",
    };
    let btn_kind = if primary { "btn--primary" } else { "btn--secondary btn--sm" };
    // Prose wraps under a corner button; it gets its button below too.
    let place = if primary || prose { "below" } else { "corner" };
    let t = text.clone();
    let sel = id.clone();
    let onclick = move |_| {
        let (t, sel) = (t.clone(), sel.clone());
        let g = *generation.peek() + 1;
        generation.set(g);
        spawn(async move {
            let ok = copy_text(&t, &sel).await;
            state.set(if ok { Copy::Copied } else { Copy::Failed });
            if !ok {
                s.local_toast(format!("Couldn't copy the {what}. It's selected: press ⌘C."), true);
            }
            tokio::time::sleep(Duration::from_millis(if ok { 1500 } else { 4000 })).await;
            if *generation.peek() == g {
                state.set(Copy::Idle);
            }
        });
    };
    rsx! {
        div { class: "agent-snippet", "data-state": data_state, "data-place": place,
            pre { id: "{id}", class: "{code_class}", "{text}" }
            div { class: "agent-copy--{place}",
                button { class: "btn {btn_kind} agent-copy", r#type: "button", "data-state": data_state, onclick,
                    match st {
                        Copy::Copied => rsx! { Icon { name: "check", class: "icon--sm" } "Copied" },
                        Copy::Failed => rsx! { Icon { name: "alert", class: "icon--sm" } "Press ⌘C" },
                        Copy::Idle => rsx! { Icon { name: "copy", class: "icon--sm" } "{label}" },
                    }
                }
            }
            span { class: "sr-only", "aria-live": "polite",
                match st {
                    Copy::Copied => format!("Copied {what}"),
                    Copy::Failed => format!("Couldn't copy the {what}. The text is selected: press Command C."),
                    Copy::Idle => String::new(),
                }
            }
        }
    }
}

/// A checklist row: number (or a tick), title, body.
#[component]
fn Item(n: u8, step: Step, title: String, children: Element) -> Element {
    let class = match step {
        Step::Done => "agent-step__num agent-step__num--done",
        Step::Current => "agent-step__num agent-step__num--now",
        Step::Todo => "agent-step__num",
    };
    rsx! {
        li { class: "agent-step", "aria-current": if step == Step::Current { "step" } else { "false" },
            div { class: "{class}",
                if step == Step::Done { Icon { name: "check", class: "icon--sm" } } else { "{n}" }
            }
            div { class: "agent-step__body",
                div { class: "agent-step__title", "{title}" }
                {children}
            }
        }
    }
}

fn chip_class(status: Status, wants_write: bool) -> &'static str {
    if status.needs_update(wants_write) {
        "agent-chip agent-chip--warn"
    } else if status.is_connected() {
        "agent-chip agent-chip--ok"
    } else {
        "agent-chip"
    }
}

#[component]
pub fn AgentSetup() -> Element {
    let s = use_context::<AppState>();
    let mut open = s.agent_setup_open;
    // What was configured when the dialog opened: an entry that differs now
    // was added or rewritten since, so that agent still needs a restart.
    let initial = use_hook(agent_setup::existing_entries);
    let mut entries = use_signal(|| initial.clone());
    // Debug builds: PIGEON_DEBUG_OPEN=agent-setup:<agent>[:send] opens on
    // that agent, optionally with "can send" chosen (screenshots).
    let debug = use_hook(|| {
        std::env::var("PIGEON_DEBUG_OPEN")
            .ok()
            .filter(|_| cfg!(debug_assertions))
            .and_then(|v| v.strip_prefix("agent-setup:").map(str::to_string))
            .unwrap_or_default()
    });
    let mut selected = use_signal(|| match debug.split(':').next() {
        Some("terminal") => Agent::CodeTerminal,
        Some("desktop") => Agent::CodeDesktop,
        Some("claude") => Agent::ClaudeDesktop,
        Some("other") => Agent::Other,
        _ => agent_setup::default_agent(&initial),
    });
    // Access per config entry (Claude Code's terminal and desktop app share
    // one), starting from what's configured.
    let mut access = use_signal(|| {
        let mut a = [false; 3];
        for agent in Agent::ALL {
            if let Status::Connected { allow_write } = Status::of(agent, &initial) {
                a[agent.slot()] = allow_write;
            }
        }
        if debug.ends_with(":send") {
            a[selected.peek().slot()] = true;
        }
        a
    });
    let mut show_json = use_signal(|| false);
    // The last install/update, shown under that agent's step.
    let mut installed = use_signal(|| None::<(usize, String)>);
    let mut skill = use_signal(agent_setup::skill_installed);
    let binary = use_hook(cli_binary);
    let store = use_hook(token_store_kind);

    use_future(move || async move {
        loop {
            tokio::time::sleep(RECHECK).await;
            if let Ok(now) = tokio::task::spawn_blocking(agent_setup::existing_entries).await
                && *entries.peek() != now
            {
                entries.set(now);
            }
        }
    });

    let Some(bin) = binary else {
        return rsx! {
            div { class: "agent-setup-scrim", onclick: move |_| open.set(false),
                div { class: "dialog agent-setup agent-setup--narrow", role: "dialog", onclick: move |e| e.stop_propagation(),
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

    let agent = *selected.read();
    let slot = agent.slot();
    let wants_write = access.read()[slot];
    let list = entries.read().clone();
    let status = Status::of(agent, &list);
    let changed = status != Status::of(agent, &initial);
    let steps = Checklist::new(status, wants_write, changed);
    let setup = Setup::detect(bin.clone(), wants_write, store);
    let bin_str = bin.to_string_lossy().into_owned();
    // Configs, not list rows: Claude Code's two rows are one entry.
    let connected =
        [Agent::CodeTerminal, Agent::ClaudeDesktop].iter().filter(|a| Status::of(**a, &list).is_connected()).count();
    let config = agent.config();
    let config_file = config.as_ref().map(|(_, p)| tilde(p)).unwrap_or_default();
    let app = agent.app_name();
    let just_installed = installed.read().as_ref().filter(|(sl, _)| *sl == slot).map(|(_, m)| m.clone());

    // Add (or rewrite, for Update) Coo's entry in this agent's config file.
    let setup_for_install = setup.clone();
    let config_for_install = config.clone();
    let install = move |_| {
        let Some((client, path)) = config_for_install.clone() else { return };
        match agent_setup::install_config(&setup_for_install, &path, client) {
            Ok(r) => {
                let backup = if r.backup.is_some() { " · backup saved next to it" } else { "" };
                let verb = if r.replaced { "Updated" } else { "Added to" };
                installed.set(Some((slot, format!("{verb} {}{backup}.", tilde(&r.path)))));
            }
            Err(e) => s.local_toast(format!("Didn't change {}: {e}", tilde(&path)), true),
        }
        entries.set(agent_setup::existing_entries());
    };
    let update = install.clone();
    let bin_for_skill = bin.clone();
    let install_skill = move |_| match agent_setup::install_skill(None, &bin_for_skill, true) {
        Ok(_) => skill.set(true),
        Err(e) => s.local_toast(format!("Couldn't install the skill: {e}"), true),
    };
    let skill_dir = agent_setup::default_skill_dir().map(|d| tilde(&d)).unwrap_or_default();
    let json_client = config.as_ref().map(|(c, _)| *c).unwrap_or(Client::ClaudeCodeDesktop);
    let config_json = setup.config_json(json_client);
    let json_open = *show_json.read();

    rsx! {
        div { class: "agent-setup-scrim", onclick: move |_| open.set(false),
            div { class: "dialog agent-setup", role: "dialog", "aria-label": "Connect an AI agent",
                onclick: move |e| e.stop_propagation(),
                div { class: "agent-setup__head",
                    div {
                        div { class: "dialog__title", "Connect an AI agent" }
                        div { class: "agent-setup__sub",
                            match connected {
                                0 => "Let an agent search and read your mail and calendar through Coo, on this Mac.".to_string(),
                                1 => "1 agent connected.".to_string(),
                                n => format!("{n} agents connected."),
                            }
                        }
                    }
                    button { class: "icon-btn icon-btn--sm", "aria-label": "Close (Esc)", onclick: move |_| open.set(false), Icon { name: "x" } }
                }

                div { class: "agent-setup__split",
                    nav { class: "agent-setup__nav", "aria-label": "Agents",
                        div { class: "agent-setup__label",
                            if connected > 0 { "Your agents" } else { "1 · Pick your agent" }
                        }
                        for a in Agent::ALL {
                            {
                                let st = Status::of(a, &list);
                                let w = access.read()[a.slot()];
                                rsx! {
                                    button { key: "{a.label()}", class: "agent-pick", r#type: "button",
                                        "aria-current": if a == agent { "true" } else { "false" },
                                        onclick: move |_| { selected.set(a); show_json.set(false); },
                                        Icon { name: agent_icon(a), class: "agent-pick__icon" }
                                        span { class: "agent-pick__text",
                                            span { class: "agent-pick__name", "{a.label()}" }
                                            span { class: chip_class(st, w), span { class: "agent-chip__dot" } "{st.label(w)}" }
                                        }
                                    }
                                }
                            }
                        }
                        div { class: "agent-setup__hint",
                            "Terminal and the desktop app share one setting, so they connect together."
                        }
                    }

                    div { class: "agent-setup__pane",
                        div { class: "agent-setup__pane-head",
                            div { class: "agent-setup__agent", "{agent.label()}" }
                            if status.is_connected() {
                                span { class: "agent-chip agent-chip--ok", span { class: "agent-chip__dot" }
                                    if let Status::Connected { allow_write: true } = status { "Connected · can send" } else { "Connected · read-only" }
                                }
                            }
                        }
                        ol { class: "agent-steps",
                            Item { n: 2, step: steps.access, title: "What may it do?",
                                div { class: "segmented agent-access", role: "radiogroup", "aria-label": "Access",
                                    button { role: "radio", r#type: "button", "aria-checked": "{!wants_write}",
                                        onclick: move |_| access.write()[slot] = false, "Read only" }
                                    button { role: "radio", r#type: "button", "aria-checked": "{wants_write}",
                                        onclick: move |_| access.write()[slot] = true, "Read, draft and send" }
                                }
                                div { class: "agent-step__sub",
                                    if wants_write {
                                        "Archive, flag, reply, RSVP, create events. Replies are saved as drafts unless the agent asks to send, and it's told to confirm with you first."
                                    } else {
                                        "Search and read mail and calendar. It can't send or change anything."
                                    }
                                }
                                if status.needs_update(wants_write) {
                                    div { class: "banner banner--warning", role: "status",
                                        Icon { name: "alert", class: "icon--sm" }
                                        span { class: "banner__text",
                                            if wants_write {
                                                "{app} is set up read-only. Update it to let it draft and send; it's told to confirm with you first."
                                            } else {
                                                "{app} can still send. Update it to make it read-only."
                                            }
                                        }
                                        button { class: "btn btn--primary btn--sm", r#type: "button", onclick: update, "Update" }
                                    }
                                }
                            }

                            Item { n: 3, step: steps.install,
                                title: match (agent, status.is_connected()) {
                                    (Agent::Other, _) => "Give your agent this MCP config".to_string(),
                                    (_, true) => format!("Coo is in {app}'s settings"),
                                    (Agent::CodeTerminal, false) => "Run this once in Terminal".to_string(),
                                    (_, false) => format!("Add Coo to {app}"),
                                },
                                if status.is_connected() {
                                    if let Some(msg) = just_installed.clone() {
                                        div { class: "agent-step__sub agent-step__sub--ok", "{msg}" }
                                    }
                                    div { class: "agent-step__sub",
                                        code { class: "t-mono", "{config_file}" } " · "
                                        button { class: "agent-link", r#type: "button", onclick: move |_| show_json.toggle(),
                                            if json_open { "Hide JSON" } else { "Show JSON" } }
                                    }
                                    if json_open {
                                        Snippet { text: config_json.clone(), what: "config" }
                                    }
                                } else {
                                    match agent {
                                        Agent::CodeTerminal => rsx! {
                                            Snippet { text: setup.claude_code_command(), what: "command", label: "Copy command", primary: true }
                                            div { class: "agent-step__sub",
                                                "Adds Coo for all your projects; the desktop app uses the same setting. This ticks itself once it's done."
                                            }
                                        },
                                        Agent::CodeDesktop | Agent::ClaudeDesktop => rsx! {
                                            div { class: "agent-step__action",
                                                button { class: "btn btn--primary agent-step__btn", r#type: "button", onclick: install,
                                                    Icon { name: "plus", class: "icon--sm" } "Add to {app}" }
                                                span { class: "agent-step__sub",
                                                    "Writes a \"coo\" entry to " code { class: "t-mono", "{config_file}" }
                                                    "; keeps your other servers, saves a backup."
                                                }
                                            }
                                            button { class: "agent-link", r#type: "button", onclick: move |_| show_json.toggle(),
                                                if json_open { "Hide JSON" } else { "Edit the file yourself (show JSON)" } }
                                            if json_open {
                                                if agent == Agent::ClaudeDesktop {
                                                    div { class: "agent-step__sub", "In Claude: Settings › Developer › Edit Config, and merge this in (keep your other \"mcpServers\")." }
                                                } else {
                                                    div { class: "agent-step__sub", "Merge this into that file (keep your other \"mcpServers\")." }
                                                }
                                                Snippet { text: config_json.clone(), what: "config" }
                                            }
                                        },
                                        Agent::Other => rsx! {
                                            div { class: "agent-step__sub",
                                                "Any MCP client: start " code { class: "t-mono", "{bin_str} mcp" }
                                                " over stdio with this config (absolute path and env included)."
                                            }
                                            Snippet { text: config_json.clone(), what: "config", label: "Copy config", primary: true }
                                            div { class: "agent-step__sub", "Agents without MCP can use the CLI. Tell them to run:" }
                                            Snippet { text: format!("{bin_str} agent-help"), what: "command" }
                                        },
                                    }
                                }
                            }

                            Item { n: 4, step: steps.restart,
                                title: match agent {
                                    Agent::ClaudeDesktop => "Quit and reopen Claude".to_string(),
                                    Agent::Other => "Restart your agent".to_string(),
                                    _ => "Restart Claude Code (or reload its window)".to_string(),
                                },
                                div { class: "agent-step__sub",
                                    if status.needs_update(wants_write) { "After Update. " }
                                    "Coo's tools appear in new sessions."
                                }
                            }

                            Item { n: 5, step: steps.prompt, title: "Tell it about Coo",
                                Snippet { text: setup.agent_prompt(), what: "prompt", label: "Copy prompt", prose: true,
                                    primary: steps.install == Step::Done }
                            }
                        }

                        for note in setup.notes() {
                            div { class: "banner banner--info", Icon { name: "alert", class: "icon--sm" } span { class: "banner__text", "{note}" } }
                        }
                    }
                }

                div { class: "agent-setup__foot",
                    if *skill.read() {
                        span { class: "agent-step__sub spacer", "Claude Code skill installed · {skill_dir}" }
                    } else {
                        button { class: "btn btn--ghost btn--sm", r#type: "button", onclick: install_skill,
                            title: "Lets Claude reach for Coo by itself for mail and calendar tasks. Installs to {skill_dir}.",
                            "Install the Claude Code skill (optional)" }
                        span { class: "spacer" }
                    }
                    button { class: if steps.all_done() { "btn btn--primary" } else { "btn btn--secondary" },
                        r#type: "button", onclick: move |_| open.set(false), "Done" }
                }

            }
        }
    }
}
