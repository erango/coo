use dioxus::prelude::*;
use pigeon_engine::MailAction;
use pigeon_engine::service::{Cmd, Toast};

use super::Icon;
use crate::send_cmd;
use crate::shortcuts::{Do, hint};
use crate::state::AppState;

#[component]
pub fn Toasts() -> Element {
    let s = use_context::<AppState>();
    let list = s.toasts.read().clone();
    // An open composer's Send sits in this corner: stay above its footer.
    let composing = s.draft.read().is_some();
    rsx! {
        div { class: "toast-stack", "data-composing": composing.then_some("true"),
            for t in list.into_iter().rev().take(3) {
                ToastItem { key: "{t.id}", toast: t }
            }
        }
    }
}

/// The toast's icon: what went wrong, or the action it confirms.
fn icon(t: &Toast) -> &'static str {
    if t.error {
        return "alert";
    }
    match &t.action {
        Some(MailAction::Archive) => "archive",
        Some(MailAction::Trash) => "trash",
        Some(MailAction::Flag | MailAction::Unflag) => "flag",
        Some(MailAction::MoveTo(_)) => "folder",
        Some(MailAction::AddLabel(_)) => "tag",
        Some(MailAction::MarkRead) => "mailopen",
        Some(MailAction::MarkUnread) => "mail",
        Some(MailAction::Spam | MailAction::NotSpam) => "alert",
        Some(MailAction::Phishing) => "shield",
        None => "check",
    }
}

#[component]
fn ToastItem(toast: Toast) -> Element {
    let s = use_context::<AppState>();
    let mut toasts = s.toasts;
    let icon = icon(&toast);
    let tone = if toast.error { "error" } else { "neutral" };
    let id = toast.id;
    let undo = toast.undo.clone();
    let cancel_send = toast.cancel_send.clone();
    let reveal = toast.reveal.clone();
    rsx! {
        div { class: "toast", role: "status", "data-tone": tone,
            Icon { name: icon }
            span { class: "toast__msg", dir: "auto", {toast.message.clone()} }
            if let Some((account_id, handle)) = undo {
                button { class: "btn btn--ghost btn--sm", r#type: "button", style: "color:var(--text-1)",
                    onclick: move |_| {
                        toasts.write().retain(|x| x.id != id);
                        let mut stack = s.undo_stack;
                        stack.write().retain(|u| u.1 != handle);
                        send_cmd(Cmd::Undo { account_id: account_id.clone(), handle });
                    },
                    "Undo"
                    kbd { class: "kbd", {hint(Do::Undo)} }
                }
            }
            if let Some(path) = reveal {
                button { class: "icon-btn icon-btn--sm toast__reveal", r#type: "button",
                    "aria-label": "Show in Finder", title: "Show in Finder",
                    onclick: move |_| {
                        toasts.write().retain(|x| x.id != id);
                        crate::quicklook::reveal(&path);
                    },
                    Icon { name: "folder" }
                }
            }
            if let Some(draft_id) = cancel_send {
                button { class: "btn btn--ghost btn--sm", r#type: "button", style: "color:var(--text-1)",
                    onclick: move |_| {
                        toasts.write().retain(|x| x.id != id);
                        send_cmd(Cmd::CancelScheduled { draft_id: draft_id.clone() });
                    },
                    "Undo"
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_follow_the_action() {
        let t = |action| Toast { action, undo: Some(("a".into(), 1)), ..Default::default() };
        assert_eq!(icon(&t(Some(MailAction::Archive))), "archive");
        assert_eq!(icon(&t(Some(MailAction::Trash))), "trash");
        assert_eq!(icon(&t(Some(MailAction::MoveTo("f".into())))), "folder");
        assert_eq!(icon(&t(None)), "check");
        assert_eq!(icon(&Toast { error: true, action: Some(MailAction::Trash), ..Default::default() }), "alert");
    }
}
