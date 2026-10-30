//! A person's avatar: their photo when one is known (and photos are turned
//! on), else initials. Its color is the person's, from their address
//! (`hue_for`, one of the theme's hues), so someone writing to two of your
//! accounts looks the same in both. Photos come from the engine
//! (`Cmd::Photo`), cached on disk; the UI never fetches anything itself.

use dioxus::prelude::*;
use pigeon_engine::service::{Cmd, hue_for};

use crate::format;
use crate::send_cmd;
use crate::state::AppState;

#[component]
pub fn Avatar(
    email: String,
    /// Display name or address, for initials and the alt text.
    label: String,
    /// Extra classes: size (`avatar--32`), `avatar--ring`.
    #[props(default)]
    class: String,
) -> Element {
    let s = use_context::<AppState>();
    let prefs = crate::prefs::get();
    let gravatar = prefs.photos_gravatar;
    let google = prefs.photos_google && !s.google_contacts.read().is_empty();
    let enabled = gravatar || google;
    let key = email.trim().to_lowercase();
    let hue = hue_for(if key.is_empty() { &label } else { &key });
    let photo = if enabled { s.photos.read().get(&key).cloned().flatten() } else { None };
    // Ask once per address (the map remembers "asked", even with no photo;
    // `Delta::ContactPhotosChanged` forgets the misses).
    let ask = key.clone();
    let asked = s.photos.read().contains_key(&key);
    use_effect(use_reactive!(|(ask, enabled, asked)| {
        if enabled && !asked && !ask.is_empty() && !s.photos.peek().contains_key(&ask) {
            let mut photos = s.photos;
            photos.write().insert(ask.clone(), None);
            send_cmd(Cmd::Photo { email: ask, google, gravatar });
        }
    }));
    match photo {
        Some(uri) => {
            rsx! { img { class: "avatar {class}", "data-hue": "{hue}", src: "{uri}", alt: "{label}", draggable: "false" } }
        }
        None => rsx! { span { class: "avatar {class}", "data-hue": "{hue}", "{format::initials(&label)}" } },
    }
}
