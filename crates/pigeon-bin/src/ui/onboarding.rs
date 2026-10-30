use dioxus::prelude::*;
use pigeon_core::model::ProviderKind;
use pigeon_engine::service::Cmd;

use super::Icon;
use super::add_account::{FlowView, Opening, Step, use_flow};
use crate::send_cmd;
use crate::state::AppState;

#[component]
pub fn Onboarding() -> Element {
    let s = use_context::<AppState>();
    let mut signing_in = s.signing_in;
    // The "Add account" flow, embedded; a finished sign-in shows up in the
    // connected list, and the flow starts over for the next account.
    let opening = Opening {
        step: if super::icloud::debug_open("icloud") { Step::ICloud } else { Step::Start },
        ..Default::default()
    };
    let flow = use_flow(opening, use_callback(|()| {}));
    let in_icloud = *flow.step.read() == Step::ICloud;
    // The browser wait screen is for OAuth only (also "Try again" below).
    let waiting = flow.browser_wait().is_some() || (*signing_in.read() && !in_icloud);
    let mut login_error = s.login_error;
    let consent = login_error
        .read()
        .as_ref()
        .filter(|(p, _)| *p == ProviderKind::Microsoft)
        .and_then(|(_, e)| pigeon_core::auth::admin_consent_code(e).map(|c| (c, e.clone())));
    let accounts = s.accounts.read().clone();
    let done = accounts.iter().filter(|a| a.first_sync_done).count();
    let mut first_run = s.first_run;

    rsx! {
        div { class: "onboard",
            div { class: "onboard__chrome" }
            div { class: "onboard__stage",
                if let Some((code, _)) = consent {
                    AdminConsent { code, on_back: move |_| login_error.set(None) }
                } else if waiting {
                    div { class: "onboard__col onboard__col--center",
                        Icon { name: "browser", class: "icon--xl" }
                        div { class: "onboard__title onboard__title--sm", "Finish signing in in your browser" }
                        div { class: "onboard__lede", "Coo opened your browser to sign in. Come back here when it says you're done." }
                        div { class: "onboard__fine", style: "justify-content:center",
                            "Adding Gmail? If Google says it hasn't verified Coo, click Advanced, then Go to Coo." }
                        button { class: "btn btn--ghost", r#type: "button",
                            onclick: move |_| { flow.cancel_wait(); signing_in.set(false); }, "Cancel" }
                    }
                } else if in_icloud {
                    div { class: "onboard__col", FlowView { flow, embedded: true } }
                } else if accounts.is_empty() {
                    div { class: "onboard__col",
                        super::bird::Bird { pose: super::bird::Pose::Awake, style: "width:52px;height:48px" }
                        div { class: "onboard__title", "Welcome to Coo" }
                        div { class: "onboard__lede", "Add your accounts: work, personal, as many as you like. They all land in one inbox and one calendar. Coo works with Google, Microsoft 365 and iCloud." }
                        FlowView { flow, embedded: true }
                        div { class: "onboard__fine", Icon { name: "lock", class: "icon--sm" } "Microsoft and Google sign in on their own page; iCloud uses an app-specific password. Coo keeps credentials in your Keychain." }
                    }
                } else {
                    div { class: "onboard__col",
                        div { class: "onboard__title onboard__title--sm",
                            if accounts.len() == 1 { "You're connected" } else { "{accounts.len()} accounts connected" }
                        }
                        div { class: "onboard__lede", "Add the rest now, so everything is in one place from the start. Recent mail comes first; older messages fill in while you work." }
                        ConnectedAccounts {}
                        div { class: "t-label", "Add another account" }
                        FlowView { flow, embedded: true }
                        div { style: "display:flex;align-items:center;gap:12px",
                            button { class: "btn btn--primary", r#type: "button", style: "height:40px;padding:0 18px",
                                onclick: move |_| first_run.set(false), "Continue to Coo" }
                            span { class: "t-caption", "{done} of {accounts.len()} synced · the rest keeps going in the background" }
                        }
                    }
                }
            }
        }
    }
}

/// The text an admin needs: what to approve and where.
pub fn admin_instructions(client_id: Option<&str>) -> String {
    let link = client_id
        .map(pigeon_core::auth::admin_consent_url)
        .unwrap_or_else(|| "Entra admin center → Enterprise applications → Coo → Permissions".into());
    format!(
        "Hi, I'd like to use Coo (a mail and calendar app for macOS) with my work account. \
         Microsoft 365 needs an admin to approve it for our organization once.\n\n\
         Approve it here: {link}\n\n\
         It asks for: read and write mail, send mail, read and write calendars, and \
         offline access (so it stays signed in). Coo keeps mail on my Mac; there is no Coo server."
    )
}

#[component]
fn AdminConsent(code: &'static str, on_back: EventHandler<()>) -> Element {
    let s = use_context::<AppState>();
    let text = admin_instructions(pigeon_core::auth::microsoft_client_id().as_deref());
    let mut copied = use_signal(|| false);
    let copy_text = text.clone();
    let (copy_icon, copy_label) = if *copied.read() { ("check", "Copied") } else { ("copy", "Copy note") };
    rsx! {
        div { class: "onboard__col",
            div { class: "alert-mark", Icon { name: "lock" } }
            div { class: "onboard__title onboard__title--sm", "Your organization needs to approve Coo" }
            div { class: "onboard__lede",
                "Your Microsoft 365 admin has to allow Coo before anyone can sign in with a work account. \
                 Send them the note below; once they approve, sign in again."
            }
            div { class: "quote-box",
                div { style: "display:flex;align-items:center",
                    span { class: "t-label", "For your IT admin" }
                    span { class: "error-code", "{code}" }
                }
                div { style: "white-space:pre-wrap", "{text}" }
            }
            div { style: "display:flex;gap:8px",
                button { class: "btn btn--primary", r#type: "button",
                    onclick: move |_| {
                        let js = format!("navigator.clipboard.writeText({:?})", copy_text);
                        document::eval(&js);
                        copied.set(true);
                    },
                    Icon { name: copy_icon, class: "icon--sm" }
                    "{copy_label}"
                }
                button { class: "btn btn--secondary", r#type: "button",
                    onclick: move |_| {
                        on_back.call(());
                        let mut si = s.signing_in;
                        si.set(true);
                        send_cmd(Cmd::Login { provider: ProviderKind::Microsoft, login_hint: None });
                    },
                    "Try again"
                }
                button { class: "btn btn--ghost", r#type: "button", onclick: move |_| on_back.call(()), "Use another account" }
            }
        }
    }
}

/// After the first sign-in: per-account first-sync progress until each
/// account has its first page of mail.
/// Accounts added so far, each with its first-sync progress.
#[component]
fn ConnectedAccounts() -> Element {
    let s = use_context::<AppState>();
    let accounts = s.accounts.read().clone();
    rsx! {
        div { class: "sync-steps",
            for a in accounts.iter() {
                {
                    let messages: u32 = a.folders.iter().map(|f| f.total).sum();
                    let meta = if a.first_sync_done {
                        format!("{messages} messages")
                    } else if messages > 0 {
                        format!("{messages} so far…")
                    } else {
                        "Starting…".to_string()
                    };
                    rsx! {
                        div { class: "sync-step", key: "{a.account.id}", "data-hue": "{a.hue}",
                            "data-done": a.first_sync_done.then_some("true"),
                            div { class: "sync-step__line",
                                span { class: "sync-step__dot",
                                    if a.first_sync_done { Icon { name: "check", class: "icon--xs" } } else { span { class: "dot" } }
                                }
                                span { class: "sync-step__label", dir: "auto", "{a.account.email}" }
                                span { class: "sync-step__meta", "{meta}" }
                            }
                            if !a.first_sync_done {
                                div { class: "progress", span { style: "width:40%;animation:pg-indeterminate 1.4s var(--ease-standard) infinite" } }
                            }
                        }
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
    fn admin_note_has_the_approval_link() {
        let t = admin_instructions(Some("abc-123"));
        assert!(t.contains("adminconsent?client_id=abc-123") && t.contains("Coo"));
        assert!(admin_instructions(None).contains("Enterprise applications"));
    }
}
