//! Coo as the Mac's email app. Info.plist declares the `mailto` scheme
//! (scripts/bundle.sh), so once Coo is the default, an email link clicked
//! anywhere arrives here as tao's `Opened` event and becomes a new message.
//! Settings › General and the welcome tour offer to make Coo the default.

use std::sync::{LazyLock, Mutex};

use dioxus::desktop::tao::event::Event;
use tokio::sync::Notify;

static OPENED: Mutex<Vec<String>> = Mutex::new(Vec::new());
static ARRIVED: LazyLock<Notify> = LazyLock::new(Notify::new);

/// From the launch config's event handler, which sees events before the
/// first render: an email link that launched Coo arrives that early.
pub fn on_event<T>(event: &Event<'_, T>) {
    if let Event::Opened { urls } = event {
        let links: Vec<String> = urls.iter().filter(|u| u.scheme() == "mailto").map(|u| u.to_string()).collect();
        if !links.is_empty() {
            OPENED.lock().unwrap_or_else(|p| p.into_inner()).extend(links);
            ARRIVED.notify_one();
        }
    }
}

/// The email links opened from other apps, as they come.
pub async fn next() -> Vec<String> {
    loop {
        let links = std::mem::take(&mut *OPENED.lock().unwrap_or_else(|p| p.into_inner()));
        if !links.is_empty() {
            return links;
        }
        ARRIVED.notified().await;
    }
}

/// Whether Coo is the app email links open in; `None` when Coo can't be
/// (a development build, not a bundled Coo.app).
pub fn is_default() -> Option<bool> {
    imp::is_default()
}

/// The app email links open in now ("Mail"), when it isn't Coo.
pub fn current_app() -> Option<String> {
    imp::current_app()
}

/// Ask macOS to open email links in Coo. macOS may ask the user to confirm,
/// so the answer comes later: watch `is_default`.
pub fn make_default() {
    imp::make_default()
}

#[cfg(target_os = "macos")]
mod imp {
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSBundle, NSError, NSString, NSURL};

    const BUNDLE_ID: &str = "il.co.frost-media.pigeon";

    fn ours() -> Option<Retained<NSURL>> {
        let main = NSBundle::mainBundle();
        let id = main.bundleIdentifier()?;
        let url = main.bundleURL();
        let is_app = url.path().is_some_and(|p| p.to_string().ends_with(".app"));
        (id.to_string() == BUNDLE_ID && is_app).then_some(url)
    }

    fn handler() -> Option<Retained<NSURL>> {
        let probe = NSURL::URLWithString(&NSString::from_str("mailto:someone@example.com"))?;
        NSWorkspace::sharedWorkspace().URLForApplicationToOpenURL(&probe)
    }

    pub fn is_default() -> Option<bool> {
        ours()?;
        let id = handler().and_then(|url| NSBundle::bundleWithURL(&url)).and_then(|b| b.bundleIdentifier());
        Some(id.is_some_and(|id| id.to_string() == BUNDLE_ID))
    }

    pub fn current_app() -> Option<String> {
        let name = handler()?.lastPathComponent()?.to_string();
        Some(name.trim_end_matches(".app").to_string())
    }

    pub fn make_default() {
        let Some(app) = ours() else { return };
        let done = RcBlock::new(|error: *mut NSError| {
            // SAFETY: AppKit passes a valid NSError or nil.
            if let Some(e) = unsafe { error.as_ref() } {
                tracing::warn!(code = e.code(), "couldn't make Coo the default email app");
            }
        });
        NSWorkspace::sharedWorkspace().setDefaultApplicationAtURL_toOpenURLsWithScheme_completionHandler(
            &app,
            &NSString::from_str("mailto"),
            Some(&done),
        );
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn is_default() -> Option<bool> {
        None
    }
    pub fn current_app() -> Option<String> {
        None
    }
    pub fn make_default() {}
}
