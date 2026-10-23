use dioxus::prelude::*;
use pigeon_engine::service::{Cmd, Toast};

use super::Icon;
use crate::send_cmd;
use crate::state::AppState;

#[component]
pub fn Toasts() -> Element {
    let s = use_context::<AppState>();
    let list = s.toasts.read().clone();
    rsx! {
        div { class: "toast-stack",
            for t in list.into_iter().rev().take(3) {
                ToastItem { key: "{t.id}", toast: t }
            }
        }
    }
}

#[component]
fn ToastItem(toast: Toast) -> Element {
    let s = use_context::<AppState>();
    let mut toasts = s.toasts;
    let icon: &'static str = if toast.error {
        "alert"
    } else if toast.undo.is_some() {
        "archive"
    } else {
        "check"
    };
    let tone = if toast.error { "error" } else { "neutral" };
    let id = toast.id;
    let undo = toast.undo.clone();
    rsx! {
        div { class: "toast", role: "status", "data-tone": tone,
            Icon { name: icon }
            span { class: "toast__msg", dir: "auto", {toast.message.clone()} }
            if let Some((account_id, handle)) = undo {
                button { class: "btn btn--ghost btn--sm", r#type: "button", style: "color:var(--text-1)",
                    onclick: move |_| {
                        toasts.write().retain(|x| x.id != id);
                        send_cmd(Cmd::Undo { account_id: account_id.clone(), handle });
                    },
                    "Undo"
                    kbd { class: "kbd", "Z" }
                }
            }
        }
    }
}
