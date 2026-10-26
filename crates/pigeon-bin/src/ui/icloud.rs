//! iCloud sign-in: Apple has no third-party OAuth for mail, so the user
//! makes an app-specific password and pastes it here. A step of the
//! "Add account" flow (ui/add_account.rs).

use dioxus::prelude::*;
use pigeon_core::compose::{is_icloud_mail, is_plain_address};
use pigeon_core::model::ProviderKind;
use pigeon_engine::service::{Cmd, Secret};

use super::Icon;
use super::add_account::Flow;
use crate::send_cmd;

/// Where app-specific passwords are made (Sign-In and Security →
/// App-Specific Passwords).
pub const APPLE_ACCOUNT_URL: &str = "https://account.apple.com/account/manage";

/// Debug builds: `PIGEON_DEBUG_OPEN=icloud` opens onboarding on the iCloud
/// step, for screenshots (the sheet's own hook is `add-account:icloud|…`).
pub fn debug_open(want: &str) -> bool {
    cfg!(debug_assertions) && std::env::var("PIGEON_DEBUG_OPEN").is_ok_and(|v| v == want)
}

/// The Apple ID is an address at another provider (a Gmail address, say):
/// iCloud Mail then sends from the account's own iCloud address, which
/// the form asks for.
pub fn needs_mail_address(apple_id: &str) -> bool {
    let id = apple_id.trim();
    is_plain_address(id) && !is_icloud_mail(id)
}

/// Pure form check: an error to show, or None when it can be submitted.
/// `mail` is the optional iCloud Mail address.
pub fn validate(email: &str, password: &str, mail: &str) -> Option<&'static str> {
    let email = email.trim();
    let mail = mail.trim();
    let pw: String = password.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    if !email.contains('@') || email.starts_with('@') || email.ends_with('@') {
        return Some("Enter your full iCloud address, like name@icloud.com");
    }
    if pw.len() != 16 || !pw.chars().all(|c| c.is_ascii_alphabetic()) {
        return Some("App-specific passwords look like abcd-efgh-ijkl-mnop (16 letters)");
    }
    if needs_mail_address(email) && !mail.is_empty() && !is_plain_address(mail) {
        return Some("Enter your iCloud Mail address, like name@icloud.com, or leave it empty");
    }
    None
}

/// The "Add account" iCloud step: the Apple ID (prefilled from the sheet's
/// address), the app-specific password, and how to make one.
#[component]
pub fn ICloudStep(flow: Flow) -> Element {
    let mut email = flow.email;
    let mut password = use_signal(String::new);
    let mut mail = use_signal(String::new);
    let mut touched = use_signal(|| false);
    // A full address came from the start step: show it, don't ask again.
    let mut editing = use_signal(|| !is_plain_address(flow.email.peek().trim()));
    let busy = *flow.waiting.read() == Some(ProviderKind::ICloud);
    let problem = validate(&email.read(), &password.read(), &mail.read());
    let ask_mail = needs_mail_address(&email.read());
    let error = flow.error.read().clone();

    let submit = move || {
        touched.set(true);
        if validate(&email.peek(), &password.peek(), &mail.peek()).is_some() || flow.waiting.peek().is_some() {
            return;
        }
        let mut err = flow.error;
        err.set(None);
        flow.begin_attempt(ProviderKind::ICloud);
        let apple_id = email.peek().trim().to_string();
        send_cmd(Cmd::LoginPassword {
            provider: ProviderKind::ICloud,
            mail_address: needs_mail_address(&apple_id)
                .then(|| mail.peek().trim().to_string())
                .filter(|m| !m.is_empty()),
            email: apple_id,
            password: Secret(password.peek().clone()),
        });
    };
    let mut submit_enter = submit;

    rsx! {
        div { class: "add-account__head add-account__head--row",
            button { class: "add-account__back", r#type: "button", "aria-label": "Back", title: "Back", disabled: busy,
                onclick: move |_| flow.back(),
                Icon { name: "chev-left", class: "icon--sm" }
            }
            h1 { class: "add-account__title", "iCloud Mail" }
        }
        if *editing.read() {
            label { class: "add-account__field",
                "Apple ID"
                input { class: "input", r#type: "email", autocomplete: "username", placeholder: "name@icloud.com",
                    value: "{email}", disabled: busy, spellcheck: "false",
                    onmounted: move |e| async move { let _ = e.set_focus(true).await; },
                    oninput: move |e| email.set(e.value()),
                }
            }
        } else {
            div { class: "add-account__id",
                span { class: "provider-logo", style: "--logo:18px", "aria-hidden": "true",
                    dangerous_inner_html: super::sidebar::provider_svg(ProviderKind::ICloud) }
                span { class: "add-account__id-email", dir: "auto", "{email}" }
                button { class: "link-btn", r#type: "button", disabled: busy, onclick: move |_| editing.set(true), "Change" }
            }
        }
        label { class: "add-account__field",
            "App-specific password"
            input { class: "input add-account__password", r#type: "password", autocomplete: "off", placeholder: "abcd-efgh-ijkl-mnop",
                "aria-invalid": "{*touched.read() && problem.is_some()}",
                value: "{password}", disabled: busy,
                onmounted: move |e| async move { if !*editing.peek() { let _ = e.set_focus(true).await; } },
                oninput: move |e| password.set(e.value()),
                onkeydown: move |e| if e.key() == Key::Enter { e.prevent_default(); submit_enter() },
            }
            if *touched.read() && let Some(p) = problem { span { class: "field__error", "{p}" } }
            span { class: "add-account__field-hint",
                "Apple doesn't let other apps use your Apple ID password. Make a password just for Coo, then paste it here."
            }
        }
        button { class: "link-btn add-account__apple-link", r#type: "button",
            onclick: move |_| { let _ = open::that_detached(APPLE_ACCOUNT_URL); },
            "Create one at account.apple.com"
            Icon { name: "popout", class: "icon--xs" }
        }
        ol { class: "add-account__steps",
            li { "Sign in, open Sign-In and Security, then App-Specific Passwords." }
            li { "Name it “Coo” and copy the password it shows." }
        }
        if ask_mail {
            label { class: "add-account__field",
                span { "iCloud Mail address " span { class: "t-3", "(optional)" } }
                input { class: "input", r#type: "email", autocomplete: "off", placeholder: "name@icloud.com",
                    value: "{mail}", disabled: busy, spellcheck: "false",
                    oninput: move |e| mail.set(e.value()),
                }
                span { class: "add-account__field-hint",
                    "{super::settings::ICLOUD_SEND_HINT} Add more aliases later in Settings › Accounts."
                }
            }
        }
        if let Some(err) = error.filter(|_| !busy) {
            div { class: "banner banner--error add-account__error", role: "alert",
                Icon { name: "alert", class: "icon--sm" } span { dir: "auto", "{err}" } }
        }
        button { class: "btn btn--primary add-account__primary", r#type: "button", disabled: busy,
            onclick: move |_| { let mut go = submit; go() },
            if busy { "Checking…" } else { "Sign in" }
        }
        p { class: "add-account__hint",
            Icon { name: "lock", class: "icon--xs" }
            " Coo checks it with iCloud Mail and Calendar, then keeps it in your Keychain."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{needs_mail_address, validate};

    #[test]
    fn form_validation() {
        assert!(validate("name@icloud.com", "abcd-efgh-ijkl-mnop", "").is_none());
        assert!(validate("name@icloud.com", "abcd efgh ijkl mnop", "").is_none());
        assert!(validate("name", "abcd-efgh-ijkl-mnop", "").is_some());
        assert!(validate("name@icloud.com", "my-apple-pass1", "").is_some());
        assert!(validate("me@gmail.com", "abcd-efgh-ijkl-mnop", "").is_none(), "the iCloud address is optional");
        assert!(validate("me@gmail.com", "abcd-efgh-ijkl-mnop", "me@icloud.com").is_none());
        assert!(validate("me@gmail.com", "abcd-efgh-ijkl-mnop", "me@").is_some());
    }

    #[test]
    fn asks_for_the_mail_address_only_for_a_foreign_apple_id() {
        assert!(needs_mail_address("me@gmail.com"));
        assert!(!needs_mail_address("me@icloud.com"));
        assert!(!needs_mail_address("me@me.com"));
        assert!(!needs_mail_address("me@gmail"), "not until it's a full address");
    }
}
