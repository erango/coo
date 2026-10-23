use dioxus::prelude::*;
use pigeon_core::model::ProviderKind;
use pigeon_engine::service::Cmd;

use super::Icon;
use crate::send_cmd;
use crate::state::AppState;

#[component]
pub fn Onboarding() -> Element {
    let s = use_context::<AppState>();
    let mut signing_in = s.signing_in;
    let waiting = *signing_in.read();
    let mut login = move |p: ProviderKind| {
        signing_in.set(true);
        send_cmd(Cmd::Login(p));
    };

    rsx! {
        div { class: "onboard",
            div { class: "onboard__chrome" }
            div { class: "onboard__stage",
                if waiting {
                    div { class: "onboard__col onboard__col--center",
                        Icon { name: "browser", class: "icon--xl" }
                        div { class: "onboard__title onboard__title--sm", "Finish signing in in your browser" }
                        div { class: "onboard__lede", "Pigeon opened your browser to sign in. Come back here when it says you're done." }
                        button { class: "btn btn--ghost", r#type: "button", onclick: move |_| signing_in.set(false), "Cancel" }
                    }
                } else {
                    div { class: "onboard__col",
                        svg { class: "icon", style: "width:44px;height:44px", r#use { href: "#i-pigeon" } }
                        div { class: "onboard__title", "Welcome to Pigeon" }
                        div { class: "onboard__lede", "Add your first account. You can add as many as you like later; they all land in one inbox." }
                        div { style: "display:flex;flex-direction:column;gap:10px",
                            button { class: "btn btn--secondary", r#type: "button", style: "height:44px;justify-content:flex-start",
                                onclick: move |_| login(ProviderKind::Microsoft),
                                Icon { name: "building" } "Microsoft 365 / Outlook"
                            }
                            button { class: "btn btn--secondary", r#type: "button", style: "height:44px;justify-content:flex-start",
                                onclick: move |_| login(ProviderKind::Google),
                                Icon { name: "at" } "Google"
                            }
                            button { class: "btn btn--secondary", r#type: "button", style: "height:44px;justify-content:flex-start", disabled: true,
                                Icon { name: "cloud" } "iCloud (coming soon)"
                            }
                        }
                        div { class: "onboard__fine", Icon { name: "lock", class: "icon--sm" } "You sign in on the provider's own page. Pigeon stores tokens in your Keychain." }
                    }
                }
            }
        }
    }
}
