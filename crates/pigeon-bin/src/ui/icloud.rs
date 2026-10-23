//! iCloud sign-in: Apple has no third-party OAuth for mail, so the user
//! makes an app-specific password and pastes it here. Used by onboarding
//! and Settings › Accounts.

use dioxus::prelude::*;
use pigeon_core::model::ProviderKind;
use pigeon_engine::service::{Cmd, Secret};

use super::Icon;
use crate::send_cmd;
use crate::state::AppState;

/// Where app-specific passwords are made (Sign-In and Security →
/// App-Specific Passwords).
pub const APPLE_ACCOUNT_URL: &str = "https://account.apple.com";

/// Debug builds: `PIGEON_DEBUG_OPEN=icloud` (onboarding) or `settings:icloud`
/// opens the form on launch, for screenshots.
pub fn debug_open(want: &str) -> bool {
    cfg!(debug_assertions) && std::env::var("PIGEON_DEBUG_OPEN").is_ok_and(|v| v == want)
}

/// Pure form check: an error to show, or None when it can be submitted.
pub fn validate(email: &str, password: &str) -> Option<&'static str> {
    let email = email.trim();
    let pw: String = password.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    if !email.contains('@') || email.starts_with('@') || email.ends_with('@') {
        return Some("Enter your full iCloud address, like name@icloud.com");
    }
    if pw.len() != 16 || !pw.chars().all(|c| c.is_ascii_alphabetic()) {
        return Some("App-specific passwords look like abcd-efgh-ijkl-mnop (16 letters)");
    }
    None
}

#[component]
pub fn ICloudForm(on_cancel: EventHandler<()>) -> Element {
    let s = use_context::<AppState>();
    let mut email = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut touched = use_signal(|| false);
    let mut signing_in = s.signing_in;
    // Toasts newer than the submit belong to this attempt.
    let mut after_toast = use_signal(|| None::<u64>);
    let busy = *signing_in.read();
    let problem = validate(&email.read(), &password.read());
    let server_error = after_toast
        .read()
        .and_then(|after| s.toasts.read().iter().rev().find(|t| t.error && t.id >= after).map(|t| t.message.clone()));

    let mut submit = move |_| {
        touched.set(true);
        if validate(&email.read(), &password.read()).is_some() || *signing_in.read() {
            return;
        }
        let next_id = s.toasts.read().iter().map(|t| t.id + 1).max().unwrap_or(0);
        after_toast.set(Some(next_id));
        signing_in.set(true);
        send_cmd(Cmd::LoginPassword {
            provider: ProviderKind::ICloud,
            email: email.read().trim().to_string(),
            password: Secret(password.read().clone()),
        });
    };

    rsx! {
        div { class: "icloud-form",
            div { class: "icloud-form__steps",
                div { class: "icloud-form__step",
                    span { class: "icloud-form__num", "1" }
                    div {
                        div { class: "icloud-form__step-title", "Create an app-specific password" }
                        div { class: "icloud-form__step-body",
                            "Apple doesn't let other apps use your Apple Account password. At account.apple.com, open "
                            b { "Sign-In and Security → App-Specific Passwords" }
                            ", create one named “Pigeon”, and copy it."
                        }
                        button { class: "btn btn--secondary btn--sm", r#type: "button", style: "margin-top:8px",
                            onclick: move |_| { let _ = open::that_detached(APPLE_ACCOUNT_URL); },
                            Icon { name: "browser", class: "icon--sm" } "Open account.apple.com"
                        }
                    }
                }
                div { class: "icloud-form__step",
                    span { class: "icloud-form__num", "2" }
                    div { style: "flex:1;display:flex;flex-direction:column;gap:10px",
                        div { class: "icloud-form__step-title", "Sign in" }
                        label { class: "field",
                            "iCloud address"
                            input { class: "input", r#type: "email", autocomplete: "username", placeholder: "name@icloud.com",
                                value: "{email}", disabled: busy,
                                oninput: move |e| email.set(e.value()),
                            }
                        }
                        label { class: "field",
                            "App-specific password"
                            input { class: "input", r#type: "password", autocomplete: "off", placeholder: "abcd-efgh-ijkl-mnop",
                                "aria-invalid": "{*touched.read() && problem.is_some()}",
                                value: "{password}", disabled: busy,
                                oninput: move |e| password.set(e.value()),
                                onkeydown: move |e| if e.key() == Key::Enter { e.prevent_default(); submit(()) },
                            }
                            if *touched.read() && let Some(p) = problem { span { class: "field__error", "{p}" } }
                        }
                        if let Some(err) = server_error.filter(|_| !busy) {
                            div { class: "banner banner--error", role: "alert", Icon { name: "alert", class: "icon--sm" } span { dir: "auto", "{err}" } }
                        }
                        div { style: "display:flex;gap:8px;align-items:center",
                            button { class: "btn btn--primary", r#type: "button", disabled: busy, onclick: move |_| submit(()),
                                if busy { "Checking…" } else { "Sign in to iCloud" }
                            }
                            button { class: "btn btn--ghost", r#type: "button", disabled: busy, onclick: move |_| on_cancel.call(()), "Cancel" }
                        }
                    }
                }
            }
            div { class: "onboard__fine", Icon { name: "lock", class: "icon--sm" }
                "Pigeon checks the password with iCloud Mail and Calendar, then keeps it in your Keychain. You can revoke it any time at account.apple.com."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::validate;

    #[test]
    fn form_validation() {
        assert!(validate("name@icloud.com", "abcd-efgh-ijkl-mnop").is_none());
        assert!(validate("name@icloud.com", "abcd efgh ijkl mnop").is_none());
        assert!(validate("name", "abcd-efgh-ijkl-mnop").is_some());
        assert!(validate("name@icloud.com", "my-apple-pass1").is_some());
    }
}
