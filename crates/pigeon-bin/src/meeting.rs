//! One-click join: open a meeting link in its native app when installed
//! (Zoom, Teams), else the browser.

use std::path::Path;

use pigeon_core::meeting::{MeetingLink, Service, native_url, service_of};

fn app_installed(names: &[&str]) -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    names
        .iter()
        .any(|n| Path::new("/Applications").join(n).exists() || Path::new(&home).join("Applications").join(n).exists())
}

fn has_native_app(service: Service) -> bool {
    match service {
        Service::Zoom => app_installed(&["zoom.us.app"]),
        Service::Teams => app_installed(&[
            "Microsoft Teams.app",
            "Microsoft Teams (work or school).app",
            "Microsoft Teams classic.app",
        ]),
        _ => false,
    }
}

/// Join the meeting at `url`.
pub fn join(url: &str) {
    let link = MeetingLink { url: url.to_string(), service: service_of(url) };
    let target = native_url(&link).filter(|_| has_native_app(link.service)).unwrap_or_else(|| link.url.clone());
    if let Err(e) = open::that_detached(&target) {
        tracing::warn!(error = %e, "could not open meeting link");
        // Native scheme refused: fall back to the web link.
        if target != link.url {
            let _ = open::that_detached(&link.url);
        }
    }
}
