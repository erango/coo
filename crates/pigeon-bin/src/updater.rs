//! Auto-updates through Sparkle 2 (the standard macOS updater).
//!
//! Release builds ship Sparkle.framework in Coo.app/Contents/Frameworks; the
//! feed URL, EdDSA public key and check schedule live in Info.plist (see
//! scripts/bundle.sh). The framework is loaded at runtime and its classes are
//! looked up by name, so dev builds without it (`cargo app`) run normally and
//! simply have no updater.
//!
//! Everything here runs on the main thread (Dioxus desktop's UI thread).

#[cfg(target_os = "macos")]
mod imp {
    use std::cell::OnceCell;
    use std::ptr;

    use objc2::msg_send;
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject, Bool};
    use objc2_foundation::{NSBundle, NSString};

    thread_local! {
        // SPUStandardUpdaterController, alive for the whole run (None = no Sparkle).
        static CONTROLLER: OnceCell<Option<Retained<AnyObject>>> = const { OnceCell::new() };
    }

    fn load() -> Option<Retained<AnyObject>> {
        let fw = NSBundle::mainBundle().privateFrameworksPath()?.to_string();
        let path = NSString::from_str(&format!("{fw}/Sparkle.framework"));
        let bundle = NSBundle::bundleWithPath(&path)?;
        // SAFETY: loads our own, code-signed embedded framework.
        if !unsafe { bundle.load() } {
            tracing::warn!("Sparkle.framework is present but failed to load");
            return None;
        }
        let cls = AnyClass::get(c"SPUStandardUpdaterController")?;
        let none: *mut AnyObject = ptr::null_mut();
        // SAFETY: Sparkle 2 API, -initWithStartingUpdater:updaterDelegate:userDriverDelegate:
        // (BOOL, id, id) -> instancetype; nil delegates are allowed.
        let ctrl: Option<Retained<AnyObject>> = unsafe {
            let alloc: Allocated<AnyObject> = msg_send![cls, alloc];
            msg_send![alloc, initWithStartingUpdater: Bool::YES, updaterDelegate: none, userDriverDelegate: none]
        };
        if ctrl.is_some() {
            tracing::info!("Sparkle updater started");
        }
        ctrl
    }

    fn with<R>(f: impl FnOnce(&AnyObject) -> R) -> Option<R> {
        CONTROLLER.with(|c| c.get_or_init(load).as_deref().map(f))
    }

    fn updater(ctrl: &AnyObject) -> Option<Retained<AnyObject>> {
        // SAFETY: -[SPUStandardUpdaterController updater] -> SPUUpdater *
        unsafe { msg_send![ctrl, updater] }
    }

    pub fn start() {
        let _ = with(|_| ());
    }

    pub fn available() -> bool {
        with(|_| ()).is_some()
    }

    pub fn check_now() {
        let none: *mut AnyObject = ptr::null_mut();
        // SAFETY: -[SPUStandardUpdaterController checkForUpdates:(id)sender]
        let _ = with(|c| unsafe {
            let _: () = msg_send![c, checkForUpdates: none];
        });
    }

    pub fn automatic() -> bool {
        with(|c| {
            updater(c).is_some_and(|u| {
                // SAFETY: SPUUpdater BOOL property getter.
                let b: Bool = unsafe { msg_send![&*u, automaticallyDownloadsUpdates] };
                b.as_bool()
            })
        })
        .unwrap_or(false)
    }

    pub fn set_automatic(on: bool) {
        let _ = with(|c| {
            if let Some(u) = updater(c) {
                let v = Bool::new(on);
                // SAFETY: SPUUpdater BOOL property setters.
                unsafe {
                    let _: () = msg_send![&*u, setAutomaticallyChecksForUpdates: v];
                    let _: () = msg_send![&*u, setAutomaticallyDownloadsUpdates: v];
                }
            }
        });
    }

    /// (marketing version, build number) from the running bundle.
    pub fn version() -> (String, String) {
        let info = |k: &str| {
            NSBundle::mainBundle()
                .objectForInfoDictionaryKey(&NSString::from_str(k))
                .and_then(|o| o.downcast::<NSString>().ok())
                .map(|s| s.to_string())
        };
        (
            info("CFBundleShortVersionString").unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string()),
            info("CFBundleVersion").unwrap_or_else(|| "dev".into()),
        )
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn start() {}
    pub fn available() -> bool {
        false
    }
    pub fn check_now() {}
    pub fn automatic() -> bool {
        false
    }
    pub fn set_automatic(_on: bool) {}
    pub fn version() -> (String, String) {
        (env!("CARGO_PKG_VERSION").to_string(), "dev".into())
    }
}

pub use imp::{automatic, available, check_now, set_automatic, start, version};
