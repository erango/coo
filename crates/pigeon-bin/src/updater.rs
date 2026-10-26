//! Auto-updates through Sparkle 2 (the standard macOS updater).
//!
//! Release builds ship Sparkle.framework in Coo.app/Contents/Frameworks; the
//! feed URL, EdDSA public key and check schedule live in Info.plist (see
//! scripts/bundle.sh). The framework is loaded at runtime and its classes are
//! looked up by name, so dev builds without it (`cargo app`) run normally and
//! simply have no updater.
//!
//! Updates download silently (`SUAutomaticallyUpdate`) and Sparkle installs
//! them when the app terminates. Coo rarely terminates (closing the window
//! keeps it in the menu bar), so the updater delegate takes over at
//! `updater:willInstallUpdateOnQuit:immediateInstallationBlock:`: it keeps the
//! install block, the UI shows "Update ready · Restart" (sidebar pill, Coo
//! menu, status menu), and while the window is hidden Coo installs on its own
//! once nothing is being written or sent (see `silent_install_due`).
//!
//! Besides Sparkle's own schedule, Coo checks in the background when it comes
//! to the foreground (at most every 10 minutes, `check_due`: the appcast is
//! one small request) and whenever About opens.
//!
//! Everything here runs on the main thread (Dioxus desktop's UI thread).

use std::time::{Duration, SystemTime};

/// Foreground checks run at most this often (any check resets the clock).
pub const FOREGROUND_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// A downloaded update installs by itself, in menu-bar mode, after this long.
pub const SILENT_INSTALL_AFTER: Duration = Duration::from_secs(10 * 60);

/// What the updater is doing, for the About box and Settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// No Sparkle (dev builds).
    Unavailable,
    /// No check has finished since launch.
    Idle,
    Checking,
    UpToDate,
    /// A newer version was found and is downloading.
    Downloading(String),
    /// Downloaded and ready: restarting installs it.
    Ready(String),
    Failed,
}

impl Status {
    /// The About box / Settings status line.
    pub fn line(&self) -> String {
        match self {
            Status::Unavailable => "Updates are off in development builds".into(),
            Status::Idle => "Coo checks for updates automatically".into(),
            Status::Checking => "Checking for updates…".into(),
            Status::UpToDate => "Coo is up to date".into(),
            Status::Downloading(v) => format!("Downloading version {v}…"),
            Status::Ready(v) => format!("Version {v} is ready · Restart to update"),
            Status::Failed => "Couldn't check for updates right now".into(),
        }
    }
}

/// Should a foreground check run now? `last` is the latest check by anyone
/// (Coo or Sparkle's own schedule).
pub fn check_due(last: Option<SystemTime>, now: SystemTime) -> bool {
    match last {
        None => true,
        Some(t) => now.duration_since(t).map_or(true, |d| d >= FOREGROUND_INTERVAL),
    }
}

/// Install a ready update without asking? Only while the window is hidden,
/// the update has waited long enough, and nothing is being written or sent.
pub fn silent_install_due(ready_since: SystemTime, now: SystemTime, window_hidden: bool, busy: bool) -> bool {
    window_hidden && !busy && now.duration_since(ready_since).is_ok_and(|d| d >= SILENT_INSTALL_AFTER)
}

/// Status changes for the UI (taken once).
pub fn take_events() -> Option<tokio::sync::mpsc::UnboundedReceiver<Status>> {
    events::RX.lock().ok()?.take()
}

mod events {
    use super::Status;

    pub static TX: std::sync::OnceLock<tokio::sync::mpsc::UnboundedSender<Status>> = std::sync::OnceLock::new();
    pub static RX: std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<Status>>> = std::sync::Mutex::new(None);
    static CURRENT: std::sync::Mutex<Option<Status>> = std::sync::Mutex::new(None);

    pub fn init() {
        if TX.get().is_none() {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let _ = TX.set(tx);
            if let Ok(mut slot) = RX.lock() {
                *slot = Some(rx);
            }
        }
    }

    pub fn set(s: Status) {
        if let Ok(mut cur) = CURRENT.lock() {
            if cur.as_ref() == Some(&s) {
                return;
            }
            *cur = Some(s.clone());
        }
        if let Some(tx) = TX.get() {
            let _ = tx.send(s);
        }
    }

    pub fn get() -> Option<Status> {
        CURRENT.lock().ok()?.clone()
    }
}

/// The current status.
pub fn status() -> Status {
    if !available() {
        return Status::Unavailable;
    }
    events::get().unwrap_or(Status::Idle)
}

#[cfg(target_os = "macos")]
mod imp {
    use std::cell::{Cell, OnceCell, RefCell};
    use std::ptr;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use block2::{Block, RcBlock};
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject, Bool, NSObject};
    use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
    use objc2_foundation::{NSBundle, NSDate, NSError, NSString};

    use super::{Status, events};

    define_class!(
        // Sparkle's SPUUpdaterDelegate is an informal-enough protocol: it only
        // calls the selectors a delegate responds to.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "CooUpdaterDelegate"]
        struct Delegate;

        impl Delegate {
            #[unsafe(method(updater:didFindValidUpdate:))]
            fn did_find(&self, _updater: &AnyObject, item: &AnyObject) {
                if !matches!(events::get(), Some(Status::Ready(_))) {
                    events::set(Status::Downloading(display_version(item)));
                }
            }

            #[unsafe(method(updaterDidNotFindUpdate:))]
            fn not_found(&self, _updater: &AnyObject) {
                if !matches!(events::get(), Some(Status::Ready(_))) {
                    events::set(Status::UpToDate);
                }
            }

            #[unsafe(method(updater:willInstallUpdateOnQuit:immediateInstallationBlock:))]
            fn will_install_on_quit(&self, _updater: &AnyObject, item: &AnyObject, install: &Block<dyn Fn()>) -> Bool {
                let version = display_version(item);
                tracing::info!(version = %version, "update downloaded; ready to install");
                INSTALL.with(|b| *b.borrow_mut() = Some(install.copy()));
                events::set(Status::Ready(version));
                // We decide when to install (restart button, or silently while
                // hidden); Sparkle still installs on quit as a fallback.
                Bool::YES
            }

            #[unsafe(method(updater:didFinishUpdateCycleForUpdateCheck:error:))]
            fn cycle_done(&self, _updater: &AnyObject, _check: isize, error: Option<&NSError>) {
                CHECKING.with(|c| c.set(false));
                match events::get() {
                    Some(Status::Checking) => {
                        if let Some(e) = error {
                            // Sparkle reports "no update" as an error in some paths.
                            let code = e.code();
                            // SUNoUpdateError = 1001
                            events::set(if code == 1001 { Status::UpToDate } else { Status::Failed });
                        } else {
                            events::set(Status::UpToDate);
                        }
                    }
                    Some(Status::Downloading(_)) if error.is_some() => events::set(Status::Failed),
                    _ => {}
                }
            }
        }
    );

    impl Delegate {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(());
            // SAFETY: NSObject's init on a freshly allocated instance.
            unsafe { msg_send![super(this), init] }
        }
    }

    fn display_version(item: &AnyObject) -> String {
        // SAFETY: -[SUAppcastItem displayVersionString] -> NSString * (nonnull).
        let s: Option<Retained<NSString>> = unsafe { msg_send![item, displayVersionString] };
        s.map(|s| s.to_string()).unwrap_or_default()
    }

    struct Sparkle {
        controller: Retained<AnyObject>,
        // Sparkle keeps a weak reference to its delegate.
        _delegate: Retained<Delegate>,
    }

    thread_local! {
        // Alive for the whole run (None = no Sparkle).
        static SPARKLE: OnceCell<Option<Sparkle>> = const { OnceCell::new() };
        static INSTALL: RefCell<Option<RcBlock<dyn Fn()>>> = const { RefCell::new(None) };
        static LAST_CHECK: Cell<Option<SystemTime>> = const { Cell::new(None) };
        static CHECKING: Cell<bool> = const { Cell::new(false) };
    }

    fn load() -> Option<Sparkle> {
        let mtm = MainThreadMarker::new()?;
        let fw = NSBundle::mainBundle().privateFrameworksPath()?.to_string();
        let path = NSString::from_str(&format!("{fw}/Sparkle.framework"));
        let bundle = NSBundle::bundleWithPath(&path)?;
        // SAFETY: loads our own, code-signed embedded framework.
        if !unsafe { bundle.load() } {
            tracing::warn!("Sparkle.framework is present but failed to load");
            return None;
        }
        let cls = AnyClass::get(c"SPUStandardUpdaterController")?;
        events::init();
        let delegate = Delegate::new(mtm);
        let none: *mut AnyObject = ptr::null_mut();
        // SAFETY: Sparkle 2 API, -initWithStartingUpdater:updaterDelegate:userDriverDelegate:
        // (BOOL, id<SPUUpdaterDelegate>, id) -> instancetype; nil user driver delegate allowed.
        let ctrl: Option<Retained<AnyObject>> = unsafe {
            let alloc: Allocated<AnyObject> = msg_send![cls, alloc];
            msg_send![alloc, initWithStartingUpdater: Bool::YES, updaterDelegate: &*delegate, userDriverDelegate: none]
        };
        let controller = ctrl?;
        tracing::info!("Sparkle updater started");
        events::set(Status::Idle);
        Some(Sparkle { controller, _delegate: delegate })
    }

    fn with<R>(f: impl FnOnce(&AnyObject) -> R) -> Option<R> {
        SPARKLE.with(|c| c.get_or_init(load).as_ref().map(|s| f(&s.controller)))
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

    /// The user asked (menu item, Settings button): Sparkle's own UI.
    pub fn check_now() {
        LAST_CHECK.with(|c| c.set(Some(SystemTime::now())));
        let none: *mut AnyObject = ptr::null_mut();
        // SAFETY: -[SPUStandardUpdaterController checkForUpdates:(id)sender]
        let _ = with(|c| unsafe {
            let _: () = msg_send![c, checkForUpdates: none];
        });
    }

    /// The user asked (menu, Settings) with automatic updates on: look now,
    /// skipping the foreground debounce, and report through the status (Coo's
    /// update dialog shows it). Sparkle's own UI stayed silent when a
    /// background session was already running.
    pub fn check_user() {
        let _ = with(|c| {
            let Some(u) = updater(c) else { return };
            match events::get() {
                Some(Status::Ready(_)) => return,
                Some(Status::Downloading(_)) => return,
                _ => {}
            }
            LAST_CHECK.with(|c| c.set(Some(SystemTime::now())));
            CHECKING.with(|c| c.set(true));
            events::set(Status::Checking);
            // SAFETY: SPUUpdater BOOL property getter.
            let busy: Bool = unsafe { msg_send![&*u, sessionInProgress] };
            // A running session reports through the delegate like ours would.
            if !busy.as_bool() {
                // SAFETY: -[SPUUpdater checkForUpdatesInBackground]
                unsafe {
                    let _: () = msg_send![&*u, checkForUpdatesInBackground];
                }
            }
        });
    }

    /// Latest check by anyone: ours, or Sparkle's own schedule.
    fn last_check(u: &AnyObject) -> Option<SystemTime> {
        // SAFETY: -[SPUUpdater lastUpdateCheckDate] -> NSDate * (nullable).
        let date: Option<Retained<NSDate>> = unsafe { msg_send![u, lastUpdateCheckDate] };
        let sparkle = date.and_then(|d| {
            let secs = d.timeIntervalSince1970();
            (secs > 0.0).then(|| UNIX_EPOCH + Duration::from_secs_f64(secs))
        });
        let ours = LAST_CHECK.with(Cell::get);
        sparkle.max(ours)
    }

    /// A silent background check. `force` skips the foreground debounce (About).
    pub fn check_background(force: bool) {
        let _ = with(|c| {
            let Some(u) = updater(c) else { return };
            // An update is already staged: nothing to look for.
            if matches!(events::get(), Some(Status::Ready(_))) {
                return;
            }
            let now = SystemTime::now();
            if !force && !super::check_due(last_check(&u), now) {
                return;
            }
            // SAFETY: SPUUpdater BOOL property getter.
            let busy: Bool = unsafe { msg_send![&*u, sessionInProgress] };
            if busy.as_bool() {
                return;
            }
            LAST_CHECK.with(|c| c.set(Some(now)));
            CHECKING.with(|c| c.set(true));
            events::set(Status::Checking);
            // SAFETY: -[SPUUpdater checkForUpdatesInBackground]
            unsafe {
                let _: () = msg_send![&*u, checkForUpdatesInBackground];
            }
        });
    }

    /// Install the downloaded update now and relaunch. False when none is ready.
    pub fn install_now() -> bool {
        let Some(block) = INSTALL.with(|b| b.borrow().clone()) else { return false };
        tracing::info!("installing the update and relaunching");
        block.call(());
        true
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

    /// Debug builds: pretend an update is ready (screenshots of the pill).
    pub fn fake_ready(version: &str) {
        events::init();
        events::set(Status::Ready(version.to_string()));
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn start() {}
    pub fn available() -> bool {
        false
    }
    pub fn check_now() {}
    pub fn check_user() {}
    pub fn check_background(_force: bool) {}
    pub fn install_now() -> bool {
        false
    }
    pub fn automatic() -> bool {
        false
    }
    pub fn set_automatic(_on: bool) {}
    pub fn version() -> (String, String) {
        (env!("CARGO_PKG_VERSION").to_string(), "dev".into())
    }
    pub fn fake_ready(_version: &str) {}
}

pub use imp::{
    automatic, available, check_background, check_now, check_user, fake_ready, install_now, set_automatic, start,
    version,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreground_checks_are_every_ten_minutes() {
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        assert!(check_due(None, t0));
        assert!(!check_due(Some(t0), t0 + Duration::from_secs(9 * 60)));
        assert!(check_due(Some(t0), t0 + Duration::from_secs(10 * 60)));
        // A clock that went backwards doesn't block checks forever.
        assert!(check_due(Some(t0 + Duration::from_secs(3600)), t0));
    }

    #[test]
    fn silent_install_waits_and_never_interrupts() {
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let later = t0 + SILENT_INSTALL_AFTER;
        assert!(silent_install_due(t0, later, true, false));
        assert!(!silent_install_due(t0, t0 + Duration::from_secs(60), true, false), "too soon");
        assert!(!silent_install_due(t0, later, false, false), "window visible");
        assert!(!silent_install_due(t0, later, true, true), "composing or sending");
    }

    #[test]
    fn status_lines() {
        assert_eq!(Status::UpToDate.line(), "Coo is up to date");
        assert_eq!(Status::Ready("0.1.3".into()).line(), "Version 0.1.3 is ready · Restart to update");
        assert_eq!(Status::Checking.line(), "Checking for updates…");
    }
}
