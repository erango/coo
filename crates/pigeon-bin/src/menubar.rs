//! Coo in the menu bar. Closing the main window hides it instead of quitting
//! (Settings › General › Closing the window), so sync, notifications,
//! reminders and updates keep running; the Dock icon goes away while no
//! window is visible. A status item (always shown) reopens the window and has
//! New Message / Get New Mail / unread count / Settings / Quit. The first time
//! the window closes into the menu bar, a native popover under the status
//! item says so.
//!
//! Wiring: the main window's close behaviour is set to "hide" just before
//! each close request (from the current pref), so the Settings toggle needs
//! no restart. ⌘Q, the status item's Quit and Sparkle's relaunch go through
//! `NSApp terminate:`, which still quits. Reopening (status item, notification
//! click, launching Coo again from Finder/Dock: tao's `Reopen` event) shows
//! the window and restores the Dock icon. Compose windows stay open on their
//! own when the main window hides.

use std::cell::RefCell;

use dioxus::desktop::trayicon::menu::{Menu, MenuItem, PredefinedMenuItem};
use dioxus::desktop::trayicon::{Icon, TrayIcon, TrayIconBuilder};
use dioxus::desktop::{DesktopContext, WindowCloseBehaviour};

pub const OPEN: &str = "coo-tray-open";
pub const NEW_MESSAGE: &str = "coo-tray-new";
pub const SYNC: &str = "coo-tray-sync";
pub const SETTINGS: &str = "coo-tray-settings";

/// The pigeon mark (design/logo/mark-template.svg) as 39×36 RGBA, drawn at
/// 18 pt high (@2x); a template image, so macOS tints it for the menu bar.
const ICON_RGBA: &[u8] = include_bytes!("../assets/icon/menubar-39x36.rgba");
const ICON_W: u32 = 39;
const ICON_H: u32 = 36;

/// Things the status item's popover asks the UI to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    OpenSettings,
}

struct Tray {
    _icon: TrayIcon,
    menu: Menu,
    unread: MenuItem,
    /// "Restart to Update to x.y.z" and its separator, while shown.
    restart: Option<(MenuItem, PredefinedMenuItem)>,
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
    static MAIN: RefCell<Option<DesktopContext>> = const { RefCell::new(None) };
}

static ACTIONS: std::sync::OnceLock<tokio::sync::mpsc::UnboundedSender<Action>> = std::sync::OnceLock::new();
static ACTIONS_RX: std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<Action>>> = std::sync::Mutex::new(None);

/// Popover button actions for the UI to handle (taken once).
pub fn take_actions() -> Option<tokio::sync::mpsc::UnboundedReceiver<Action>> {
    ACTIONS_RX.lock().ok()?.take()
}

fn post(a: Action) {
    if let Some(tx) = ACTIONS.get() {
        let _ = tx.send(a);
    }
}

/// "N unread" for the status menu; None hides the count.
pub fn unread_label(n: u32) -> String {
    match n {
        0 => "No unread mail".into(),
        1 => "1 unread".into(),
        n => format!("{} unread", crate::dock::badge_label(n).unwrap_or_default()),
    }
}

/// Create the status item. Call once, from the main window, on the UI thread.
pub fn install(main: DesktopContext) {
    if ACTIONS.get().is_none() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let _ = ACTIONS.set(tx);
        if let Ok(mut slot) = ACTIONS_RX.lock() {
            *slot = Some(rx);
        }
    }
    MAIN.with(|m| *m.borrow_mut() = Some(main));
    if TRAY.with(|t| t.borrow().is_some()) {
        return;
    }
    let unread = MenuItem::new(unread_label(0), false, None);
    let menu = Menu::new();
    let items: [&dyn dioxus::desktop::trayicon::menu::IsMenuItem; 8] = [
        &MenuItem::with_id(OPEN, "Open Coo", true, None),
        &MenuItem::with_id(NEW_MESSAGE, "New Message", true, None),
        &MenuItem::with_id(SYNC, "Get New Mail", true, None),
        &PredefinedMenuItem::separator(),
        &unread,
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id(SETTINGS, "Settings…", true, None),
        &PredefinedMenuItem::quit(Some("Quit Coo")),
    ];
    if let Err(e) = menu.append_items(&items) {
        tracing::warn!(error = %e, "status menu");
        return;
    }
    let icon = match Icon::from_rgba(ICON_RGBA.to_vec(), ICON_W, ICON_H) {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!(error = %e, "status item icon");
            return;
        }
    };
    let built = TrayIconBuilder::new()
        .with_menu(Box::new(menu.clone()))
        .with_menu_on_left_click(true)
        .with_icon(icon)
        .with_icon_as_template(true)
        .with_tooltip("Coo")
        .build();
    match built {
        Ok(icon) => TRAY.with(|t| *t.borrow_mut() = Some(Tray { _icon: icon, menu, unread, restart: None })),
        Err(e) => tracing::warn!(error = %e, "could not create the status item"),
    }
}

/// Show (Some) or remove (None) "Restart to Update to x.y.z" at the top of
/// the status menu.
pub fn set_restart(version: Option<&str>) {
    TRAY.with(|t| {
        let mut t = t.borrow_mut();
        let Some(t) = t.as_mut() else { return };
        match (version, t.restart.as_ref()) {
            (Some(v), Some((item, _))) => item.set_text(crate::menu::restart_title(v)),
            (Some(v), None) => {
                let item = MenuItem::with_id(crate::menu::RESTART_TO_UPDATE, crate::menu::restart_title(v), true, None);
                let sep = PredefinedMenuItem::separator();
                if t.menu.insert(&item, 0).is_ok() && t.menu.insert(&sep, 1).is_ok() {
                    t.restart = Some((item, sep));
                }
            }
            (None, Some(_)) => {
                if let Some((item, sep)) = t.restart.take() {
                    let _ = t.menu.remove(&item);
                    let _ = t.menu.remove(&sep);
                }
            }
            (None, None) => {}
        }
    });
}

/// The main window is hidden (menu-bar mode).
pub fn main_hidden() -> bool {
    MAIN.with(|m| m.borrow().as_ref().is_some_and(|w| !w.is_visible()))
}

/// Start hidden in menu-bar mode (after a silent update relaunch).
pub fn start_hidden() {
    MAIN.with(|m| {
        if let Some(w) = m.borrow().as_ref() {
            w.set_visible(false);
        }
    });
    imp::set_dock_icon(false);
}

/// Keep the status menu's unread line current.
pub fn set_unread(n: u32) {
    TRAY.with(|t| {
        if let Some(t) = t.borrow().as_ref() {
            t.unread.set_text(unread_label(n));
        }
    });
}

/// The main window is about to handle a close request: hide (pref on) or
/// close (pref off). Returns true when it will hide.
pub fn before_close() -> bool {
    let hide = crate::prefs::get().close_to_menu_bar;
    MAIN.with(|m| {
        if let Some(w) = m.borrow().as_ref() {
            w.set_close_behavior(if hide {
                WindowCloseBehaviour::WindowHides
            } else {
                WindowCloseBehaviour::WindowCloses
            });
        }
    });
    hide
}

/// After the main window hid: drop the Dock icon unless a compose window is
/// still up, and show the one-time hint.
pub fn did_hide(compose_windows_open: bool) {
    if !compose_windows_open {
        imp::set_dock_icon(false);
    }
    if !crate::prefs::get().menu_bar_hint_shown {
        crate::prefs::update(|p| p.menu_bar_hint_shown = true);
        imp::show_hint();
    }
}

/// Bring the main window back (status item, notification, relaunch).
pub fn show_main() {
    imp::set_dock_icon(true);
    imp::close_hint();
    MAIN.with(|m| {
        if let Some(w) = m.borrow().as_ref() {
            w.set_visible(true);
            w.set_focus();
        }
    });
    imp::activate();
}

/// Hide the main window as a close would (debug hook for screenshots).
pub fn hide_main_for_demo() {
    MAIN.with(|m| {
        if let Some(w) = m.borrow().as_ref() {
            w.set_visible(false);
        }
    });
    imp::set_dock_icon(false);
    imp::show_hint();
}

#[cfg(target_os = "macos")]
mod imp {
    use std::cell::RefCell;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSButton, NSColor, NSFont, NSLayoutAttribute,
        NSPopover, NSPopoverBehavior, NSScreen, NSStackView, NSStatusWindowLevel, NSTextField,
        NSUserInterfaceLayoutOrientation, NSView, NSViewController, NSWindow, NSWindowStyleMask,
    };
    use objc2_foundation::{NSArray, NSEdgeInsets, NSPoint, NSRect, NSRectEdge, NSSize, NSString};

    use super::{Action, post};

    pub const TITLE: &str = "Coo is still running";
    pub const BODY: &str = "Closing the window keeps Coo in the menu bar. To quit instead, change this in Settings.";

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "CooMenuBarHintTarget"]
        struct HintTarget;

        impl HintTarget {
            #[unsafe(method(gotIt:))]
            fn got_it(&self, _sender: Option<&AnyObject>) {
                close_hint();
            }

            #[unsafe(method(openSettings:))]
            fn open_settings(&self, _sender: Option<&AnyObject>) {
                close_hint();
                post(Action::OpenSettings);
            }
        }
    );

    impl HintTarget {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(());
            // SAFETY: NSObject's init on a freshly allocated instance.
            unsafe { msg_send![super(this), init] }
        }
    }

    struct Hint {
        popover: Retained<NSPopover>,
        _target: Retained<HintTarget>,
        /// Invisible stand-in anchor when the status item is off-screen.
        stand_in: Option<Retained<NSWindow>>,
    }

    thread_local! {
        static HINT: RefCell<Option<Hint>> = const { RefCell::new(None) };
    }

    pub fn set_dock_icon(on: bool) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = NSApplication::sharedApplication(mtm);
        let policy = if on { NSApplicationActivationPolicy::Regular } else { NSApplicationActivationPolicy::Accessory };
        if app.activationPolicy() != policy {
            app.setActivationPolicy(policy);
            if on {
                // The Dock icon comes back generic for unbundled builds.
                crate::dock::set_app_icon();
            }
        }
    }

    pub fn activate() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    }

    /// The status item's button: its window is an NSStatusBarWindow (ours is
    /// the only status item this process owns).
    fn status_button(mtm: MainThreadMarker) -> Option<Retained<NSView>> {
        let app = NSApplication::sharedApplication(mtm);
        app.windows()
            .iter()
            .find(|w| w.class().name().to_str().is_ok_and(|n| n.contains("StatusBarWindow")))
            .and_then(|w| w.contentView())
    }

    pub fn show_hint() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        close_hint();
        // A full menu bar (more items than fit beside the notch) parks extra
        // status items off-screen; then point at the menu bar's right end.
        let mut stand_in = None;
        let anchor = match status_button(mtm) {
            Some(b) if b.window().is_some_and(|w| on_screen(mtm, w.frame())) => b,
            _ => {
                tracing::info!("status item not visible; anchoring the menu bar hint to the menu bar's right end");
                let Some(w) = stand_in_anchor(mtm) else { return };
                let Some(v) = w.contentView() else { return };
                stand_in = Some(w);
                v
            }
        };
        // Hiding the last window hands focus to another app; a transient
        // popover in an inactive app closes at the first click elsewhere, or
        // doesn't show, so take focus first (no Dock icon in this mode).
        activate();
        let target = HintTarget::new(mtm);

        let title = NSTextField::labelWithString(&NSString::from_str(TITLE), mtm);
        title.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
        let body = NSTextField::wrappingLabelWithString(&NSString::from_str(BODY), mtm);
        body.setFont(Some(&NSFont::systemFontOfSize(12.0)));
        body.setTextColor(Some(&NSColor::secondaryLabelColor()));
        body.setPreferredMaxLayoutWidth(250.0);

        // SAFETY: the target outlives the buttons (kept in HINT with the
        // popover) and implements both selectors.
        let settings = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Settings…"),
                Some(&target),
                Some(sel!(openSettings:)),
                mtm,
            )
        };
        let got_it = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Got it"),
                Some(&target),
                Some(sel!(gotIt:)),
                mtm,
            )
        };
        got_it.setKeyEquivalent(&NSString::from_str("\r"));

        let buttons = NSStackView::stackViewWithViews(
            &NSArray::from_retained_slice(&[into_view(settings), into_view(got_it)]),
            mtm,
        );
        buttons.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        buttons.setSpacing(8.0);

        let title_v = into_view(title);
        let body_v = into_view(body);
        let stack = NSStackView::stackViewWithViews(
            &NSArray::from_retained_slice(&[title_v.clone(), body_v.clone(), into_view(buttons)]),
            mtm,
        );
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        stack.setAlignment(NSLayoutAttribute::Leading);
        stack.setSpacing(6.0);
        stack.setCustomSpacing_afterView(12.0, &body_v);
        stack.setEdgeInsets(NSEdgeInsets { top: 14.0, left: 16.0, bottom: 14.0, right: 16.0 });

        let vc = NSViewController::new(mtm);
        vc.setView(&stack);
        let popover = NSPopover::new(mtm);
        popover.setBehavior(NSPopoverBehavior::Transient);
        popover.setContentViewController(Some(&vc));
        popover.showRelativeToRect_ofView_preferredEdge(anchor.bounds(), &anchor, NSRectEdge::MinY);
        HINT.with(|h| *h.borrow_mut() = Some(Hint { popover, _target: target, stand_in }));

        // Dismiss on its own after a while.
        dioxus::prelude::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            close_hint();
        });
    }

    pub fn close_hint() {
        if let Some(h) = HINT.with(|h| h.borrow_mut().take()) {
            h.popover.close();
            if let Some(w) = h.stand_in {
                w.orderOut(None);
            }
        }
    }

    fn on_screen(mtm: MainThreadMarker, r: NSRect) -> bool {
        NSScreen::screens(mtm).iter().any(|s| {
            let f = s.frame();
            r.origin.x + r.size.width > f.origin.x
                && r.origin.x < f.origin.x + f.size.width
                && r.origin.y + r.size.height > f.origin.y
                && r.origin.y < f.origin.y + f.size.height
        })
    }

    /// A tiny transparent window just under the menu bar's right end, where
    /// status items live, to hang the popover from.
    fn stand_in_anchor(mtm: MainThreadMarker) -> Option<Retained<NSWindow>> {
        let screen = NSScreen::mainScreen(mtm)?;
        let (full, visible) = (screen.frame(), screen.visibleFrame());
        let top = visible.origin.y + visible.size.height;
        let rect =
            NSRect::new(NSPoint::new(full.origin.x + full.size.width - 160.0, top - 2.0), NSSize::new(24.0, 2.0));
        // SAFETY: standard borderless window init on the main thread.
        let w = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: we keep the Retained ourselves; AppKit must not release it.
        unsafe { w.setReleasedWhenClosed(false) };
        w.setOpaque(false);
        w.setBackgroundColor(Some(&NSColor::clearColor()));
        w.setIgnoresMouseEvents(true);
        w.setLevel(NSStatusWindowLevel);
        w.orderFrontRegardless();
        Some(w)
    }

    fn into_view<T: Message + AsRef<NSView>>(v: Retained<T>) -> Retained<NSView> {
        let view: &NSView = (*v).as_ref();
        view.retain()
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn set_dock_icon(_on: bool) {}
    pub fn activate() {}
    pub fn show_hint() {}
    pub fn close_hint() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_asset_matches_its_size() {
        assert_eq!(ICON_RGBA.len() as u32, ICON_W * ICON_H * 4);
        assert!(ICON_RGBA.chunks(4).any(|px| px[3] > 0), "icon has visible pixels");
    }

    #[test]
    fn unread_labels() {
        assert_eq!(unread_label(0), "No unread mail");
        assert_eq!(unread_label(1), "1 unread");
        assert_eq!(unread_label(12), "12 unread");
        assert_eq!(unread_label(1500), "999+ unread");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn hint_copy_is_short() {
        assert!(imp::TITLE.len() < 40 && imp::BODY.len() < 110);
    }
}
