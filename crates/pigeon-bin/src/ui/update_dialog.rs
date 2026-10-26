//! Coo › Check for Updates…: a small dialog that opens at once on
//! "Checking…" and turns into the outcome (up to date, downloading, ready to
//! restart, or failed), following the updater's status live.

use dioxus::prelude::*;

use super::Icon;
use crate::state::AppState;
use crate::updater::Status;

/// What the dialog's buttons do, besides closing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Only "OK" / "Close".
    None,
    Restart,
    Retry,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub icon: &'static str,
    /// "busy" (still working), "ok" / "error" tint the icon.
    pub tone: &'static str,
    pub title: String,
    pub body: String,
    pub action: Action,
    /// The release whose notes "What's new" opens.
    pub notes: Option<String>,
}

/// The dialog for `status`, running `version`. `composing`: restarting now
/// would close what's being written.
pub fn view(status: &Status, version: &str, composing: bool) -> View {
    let v = |icon, tone, title: &str, body: String, action| View {
        icon,
        tone,
        title: title.to_string(),
        body,
        action,
        notes: None,
    };
    match status {
        Status::Idle | Status::Checking => {
            v("sync", "busy", "Checking for updates…", format!("You have Coo {version}."), Action::None)
        }
        Status::UpToDate => {
            v("check", "ok", "Coo is up to date", format!("Version {version} is the newest version."), Action::None)
        }
        Status::Downloading(new) => View {
            notes: Some(new.clone()),
            ..v(
                "download",
                "busy",
                &format!("Downloading Coo {new}…"),
                "You can keep working. Coo will ask you to restart once it's ready.".into(),
                Action::None,
            )
        },
        Status::Ready(new) => View {
            notes: Some(new.clone()),
            ..v(
                "check",
                "ok",
                &format!("Coo {new} is ready"),
                if composing {
                    "Restart to finish updating. Restarting now closes the message you're writing.".into()
                } else {
                    "Restart to finish updating. It takes a few seconds.".into()
                },
                Action::Restart,
            )
        },
        Status::Failed => v(
            "alert",
            "error",
            "Couldn't check for updates",
            "Check your internet connection and try again.".into(),
            Action::Retry,
        ),
        Status::Unavailable => v(
            "alert",
            "neutral",
            "Updates are off",
            "This is a development build; it doesn't update itself.".into(),
            Action::None,
        ),
    }
}

/// Enter in the dialog: its primary button (Restart now, Try again, OK).
pub fn confirm(s: AppState) {
    let status = s.update_status.peek().clone();
    let (version, _) = crate::updater::version();
    match view(&status, &version, super::update_would_interrupt(s)).action {
        Action::Restart => super::restart_to_update(),
        Action::Retry => super::check_for_updates(s),
        Action::None => {
            let mut open = s.update_dialog;
            open.set(false);
        }
    }
}

#[component]
pub fn UpdateDialog() -> Element {
    let s = use_context::<AppState>();
    let mut open = s.update_dialog;
    let status = s.update_status.read().clone();
    let (version, _) = crate::updater::version();
    let d = view(&status, &version, super::update_would_interrupt(s));
    let notes_url = d.notes.as_ref().map(|n| format!("https://github.com/erango/coo/releases/tag/v{n}"));
    rsx! {
        div { class: "update-scrim", onclick: move |_| open.set(false),
            div { class: "dialog update-dialog", role: "alertdialog", "aria-live": "polite",
                "aria-label": "{d.title}", "data-tone": d.tone,
                onclick: move |e| e.stop_propagation(),
                div { class: "update-dialog__head",
                    span { class: "update-dialog__icon", "data-icon": d.icon, Icon { name: d.icon } }
                    div { class: "dialog__title", "{d.title}" }
                }
                div { class: "dialog__body", "{d.body}" }
                div { class: "dialog__actions",
                    if let Some(url) = notes_url {
                        button { class: "btn btn--ghost btn--sm update-dialog__notes", r#type: "button",
                            onclick: move |_| { let _ = open::that_detached(&url); },
                            "What's new"
                        }
                    }
                    match d.action {
                        Action::Restart => rsx! {
                            button { class: "btn btn--secondary btn--sm", r#type: "button",
                                onclick: move |_| open.set(false), "Later" }
                            button { class: "btn btn--primary btn--sm", r#type: "button", autofocus: true,
                                onclick: move |_| super::restart_to_update(), "Restart now" }
                        },
                        Action::Retry => rsx! {
                            button { class: "btn btn--secondary btn--sm", r#type: "button",
                                onclick: move |_| open.set(false), "Close" }
                            button { class: "btn btn--primary btn--sm", r#type: "button", autofocus: true,
                                onclick: move |_| super::check_for_updates(s), "Try again" }
                        },
                        Action::None => rsx! {
                            button { class: "btn btn--primary btn--sm", r#type: "button", autofocus: true,
                                onclick: move |_| open.set(false),
                                if d.tone == "busy" { "Close" } else { "OK" }
                            }
                        },
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_outcome_says_what_happened() {
        let checking = view(&Status::Checking, "0.1.7", false);
        assert_eq!((checking.title.as_str(), checking.tone), ("Checking for updates…", "busy"));
        assert_eq!(view(&Status::UpToDate, "0.1.7", false).body, "Version 0.1.7 is the newest version.");

        let down = view(&Status::Downloading("0.1.8".into()), "0.1.7", false);
        assert_eq!(down.title, "Downloading Coo 0.1.8…");
        assert_eq!(down.notes.as_deref(), Some("0.1.8"));

        let ready = view(&Status::Ready("0.1.8".into()), "0.1.7", false);
        assert_eq!((ready.title.as_str(), ready.action), ("Coo 0.1.8 is ready", Action::Restart));
        assert!(view(&Status::Ready("0.1.8".into()), "0.1.7", true).body.contains("closes the message"));

        assert_eq!(view(&Status::Failed, "0.1.7", false).action, Action::Retry);
    }
}
