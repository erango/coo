//! Help › Report a Bug…: a short form sent to the developer through Sentry
//! (`Cmd::ReportBug`). It lists exactly what goes with the description:
//! versions and a few settings, never mail content or addresses (other
//! than a reply address the user types in).

use dioxus::prelude::*;
use pigeon_engine::service::Cmd;
use pigeon_engine::telemetry::BugReport;

use super::Icon;
use crate::send_cmd;
use crate::state::AppState;

/// What the UI adds to a report (the engine adds account types and macOS).
fn details(s: &AppState) -> Vec<(String, String)> {
    let (version, build) = crate::updater::version();
    let p = crate::prefs::get();
    vec![
        ("version".into(), format!("{version} ({build})")),
        ("theme".into(), format!("{} · {}", p.theme_mode, p.palette)),
        ("list".into(), format!("{}{}", p.list_style, if p.list_avatars { " + avatars" } else { "" })),
        ("compose".into(), format!("{} · replies {}", p.compose_mode, p.reply_mode)),
        ("text size".into(), format!("{:.0}%", p.font_scale * 100.0)),
        ("mode".into(), format!("{:?}", *s.mode.peek())),
    ]
}

#[component]
pub fn BugReportDialog() -> Element {
    let s = use_context::<AppState>();
    let mut open = s.bug_report_open;
    let mut what = use_signal(String::new);
    let mut reply_to = use_signal(String::new);
    let mut show_details = use_signal(|| false);
    let available = pigeon_engine::telemetry::reports_available();
    let empty = what.read().trim().is_empty();
    let mut send = move |_: ()| {
        if what.peek().trim().is_empty() {
            return;
        }
        let r = reply_to.peek().trim().to_string();
        send_cmd(Cmd::ReportBug(BugReport {
            what: what.peek().trim().to_string(),
            reply_to: (!r.is_empty()).then_some(r),
            details: details(&s),
        }));
        open.set(false);
    };
    let listed = details(&s);
    rsx! {
        div { class: "update-scrim", onclick: move |_| open.set(false),
            form { class: "dialog bug-dialog", role: "dialog", "aria-label": "Report a bug",
                onclick: move |e| e.stop_propagation(),
                onsubmit: move |e| { e.prevent_default(); send(()); },
                onkeydown: move |e| if e.key() == Key::Escape { open.set(false) },
                div { class: "update-dialog__head",
                    span { class: "update-dialog__icon", Icon { name: "alert" } }
                    div { class: "dialog__title", "Report a bug" }
                }
                if available {
                    label { class: "bug-dialog__field",
                        span { "What happened?" }
                        textarea { rows: 6, autofocus: true, value: "{what}",
                            placeholder: "What you did, what you expected, and what happened instead.",
                            oninput: move |e| what.set(e.value()),
                            onkeydown: move |e| {
                                if e.key() == Key::Enter && e.modifiers().meta() {
                                    e.prevent_default();
                                    send(());
                                }
                            },
                        }
                    }
                    label { class: "bug-dialog__field",
                        span { "Email for a reply " span { class: "t-3", "(optional)" } }
                        input { r#type: "email", value: "{reply_to}", placeholder: "you@example.com",
                            oninput: move |e| reply_to.set(e.value()) }
                    }
                    div { class: "bug-dialog__sent",
                        button { class: "link-btn", r#type: "button", "aria-expanded": "{show_details}",
                            onclick: move |_| { let v = *show_details.peek(); show_details.set(!v); },
                            "What's sent with it"
                        }
                        if *show_details.read() {
                            ul {
                                for (k, v) in listed {
                                    li { span { class: "t-3", "{k}: " } "{v}" }
                                }
                                li { span { class: "t-3", "accounts: " } "the kinds connected (Google, Microsoft, iCloud)" }
                                li { span { class: "t-3", "macOS: " } "its version" }
                            }
                            p { class: "t-3", "No messages, subjects, names or addresses, other than an email you add above." }
                        }
                    }
                    div { class: "dialog__actions",
                        button { class: "btn btn--secondary btn--sm", r#type: "button", onclick: move |_| open.set(false), "Cancel" }
                        button { class: "btn btn--primary btn--sm", r#type: "submit", disabled: empty,
                            "Send report" kbd { class: "kbd", "⌘↵" } }
                    }
                } else {
                    div { class: "dialog__body", "Bug reports aren't set up in this build." }
                    div { class: "dialog__actions",
                        button { class: "btn btn--primary btn--sm", r#type: "button", onclick: move |_| open.set(false), "Close" }
                    }
                }
            }
        }
    }
}
