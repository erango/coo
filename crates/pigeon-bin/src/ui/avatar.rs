//! A person's avatar: their photo when one is known (and photos are turned
//! on), else initials on the account hue. Photos come from the engine
//! (`Cmd::Photo`), cached on disk; the UI never fetches anything itself.

use dioxus::prelude::*;
use pigeon_engine::service::Cmd;

use crate::format;
use crate::send_cmd;
use crate::state::AppState;

#[component]
pub fn Avatar(
    email: String,
    /// Display name or address, for initials and the alt text.
    label: String,
    hue: u8,
    /// Extra classes: size (`avatar--32`), `avatar--ring`.
    #[props(default)]
    class: String,
) -> Element {
    let s = use_context::<AppState>();
    let enabled = crate::prefs::get().photos_gravatar;
    let key = email.trim().to_lowercase();
    let photo = if enabled { s.photos.read().get(&key).cloned().flatten() } else { None };
    // Ask once per address (the map remembers "asked", even with no photo).
    let ask = key.clone();
    use_effect(use_reactive!(|(ask, enabled)| {
        if enabled && !ask.is_empty() && !s.photos.peek().contains_key(&ask) {
            let mut photos = s.photos;
            photos.write().insert(ask.clone(), None);
            send_cmd(Cmd::Photo { email: ask });
        }
    }));
    match photo {
        Some(uri) => {
            rsx! { img { class: "avatar {class}", "data-hue": "{hue}", src: "{uri}", alt: "{label}", draggable: "false" } }
        }
        None => rsx! { span { class: "avatar {class}", "data-hue": "{hue}", "{format::initials(&label)}" } },
    }
}
