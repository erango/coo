//! The "starting now" panel: a small native window over every app (and
//! full-screen spaces), top center, when a meeting starts
//! (meeting_alerts.rs decides when). It doesn't take focus from the app
//! you're in. Join opens the meeting; Dismiss ends its alerts; ✕ puts it
//! away (it comes back once, if that's on).

use crate::meeting_alerts::Shown;

/// Show the panel for `m` ("Next, starts now" when `m.after` is set).
pub fn show(m: &Shown, again: bool, hotkey: &str) {
    crate::meeting_alerts::with(|a| {
        a.panel_open = Some(m.key.clone());
        *a.panel_shown.entry(m.key.clone()).or_insert(0) += 1;
    });
    imp::show(m, again, hotkey);
}

/// Close the panel (its meeting was joined or dismissed elsewhere).
pub fn close() {
    crate::meeting_alerts::with(|a| a.panel_open = None);
    imp::close();
}

/// Join the meeting that's on (or about to be) from any app: the global
/// shortcut's action.
pub fn join_from_anywhere() {
    if let Some(url) = crate::meeting_alerts::link_to_join(pigeon_core::now_ms()) {
        crate::meeting::join(&url);
    }
}

/// (Re)register the join shortcut from the current pref. Call from a
/// component (it needs the desktop context).
#[cfg(target_os = "macos")]
pub fn apply_hotkey() {
    use std::cell::RefCell;
    use std::str::FromStr;

    use dioxus::desktop::{HotKeyState, ShortcutHandle};
    thread_local! {
        static HANDLE: RefCell<Option<ShortcutHandle>> = const { RefCell::new(None) };
    }
    if let Some(h) = HANDLE.with(|h| h.borrow_mut().take()) {
        h.remove();
    }
    let keys = crate::prefs::get().meeting_join_hotkey;
    if keys.is_empty() {
        return;
    }
    let Ok(hotkey) = global_hotkey::hotkey::HotKey::from_str(&keys) else {
        tracing::warn!("join shortcut not understood");
        return;
    };
    match dioxus::desktop::window().create_shortcut(hotkey, |state| {
        if state == HotKeyState::Pressed {
            join_from_anywhere();
        }
    }) {
        Ok(h) => HANDLE.with(|slot| *slot.borrow_mut() = Some(h)),
        Err(e) => tracing::warn!(error = ?e, "could not register the join shortcut"),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn apply_hotkey() {}

/// The panel's kicker line.
pub fn kicker(m: &Shown, again: bool, now: i64) -> String {
    if again {
        let mins = ((now - m.start_at) / 60_000).max(1);
        format!("STARTED {mins} MIN AGO")
    } else if m.after.is_some() {
        "NEXT, STARTS NOW".into()
    } else {
        "STARTING NOW".into()
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::cell::RefCell;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
    use objc2_app_kit::{
        NSBackingStoreType, NSBox, NSBoxType, NSButton, NSColor, NSControlSize, NSFont, NSImage, NSLayoutAttribute,
        NSPanel, NSScreen, NSStackView, NSStatusWindowLevel, NSTextField, NSTitlePosition,
        NSUserInterfaceLayoutOrientation, NSView, NSWindowButton, NSWindowCollectionBehavior, NSWindowStyleMask,
        NSWindowTitleVisibility,
    };
    use objc2_foundation::{NSArray, NSEdgeInsets, NSPoint, NSRect, NSSize, NSString};

    use crate::meeting_alerts::{Shown, initials, mark_done};

    const WIDTH: f64 = 340.0;

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "CooMeetingPanelTarget"]
        struct Target;

        impl Target {
            #[unsafe(method(join:))]
            fn join(&self, _sender: Option<&AnyObject>) {
                let cur = CURRENT.with(|c| c.borrow().as_ref().map(|p| (p.key.clone(), p.url.clone())));
                if let Some((key, url)) = cur {
                    mark_done(&key);
                    super::close();
                    if let Some(url) = url {
                        crate::meeting::join(&url);
                    }
                }
            }

            #[unsafe(method(dismiss:))]
            fn dismiss(&self, _sender: Option<&AnyObject>) {
                if let Some(key) = CURRENT.with(|c| c.borrow().as_ref().map(|p| p.key.clone())) {
                    mark_done(&key);
                }
                super::close();
            }

            #[unsafe(method(later:))]
            fn later(&self, _sender: Option<&AnyObject>) {
                super::close();
            }
        }
    );

    impl Target {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(());
            // SAFETY: NSObject's init on a freshly allocated instance.
            unsafe { msg_send![super(this), init] }
        }
    }

    struct Panel {
        window: Retained<NSPanel>,
        _target: Retained<Target>,
        key: String,
        url: Option<String>,
    }

    thread_local! {
        static CURRENT: RefCell<Option<Panel>> = const { RefCell::new(None) };
        // Closed panels, kept until the next one shows: a button's action
        // may close its own panel, and its target must outlive the call.
        static CLOSED: RefCell<Vec<Panel>> = const { RefCell::new(Vec::new()) };
    }

    pub fn close() {
        if let Some(p) = CURRENT.with(|c| c.borrow_mut().take()) {
            p.window.orderOut(None);
            CLOSED.with(|c| c.borrow_mut().push(p));
        }
    }

    fn label(text: &str, size: f64, bold: bool, color: Retained<NSColor>, mtm: MainThreadMarker) -> Retained<NSView> {
        let l = NSTextField::labelWithString(&NSString::from_str(text), mtm);
        let font = if bold { NSFont::boldSystemFontOfSize(size) } else { NSFont::systemFontOfSize(size) };
        l.setFont(Some(&font));
        l.setTextColor(Some(&color));
        l.setPreferredMaxLayoutWidth(WIDTH - 32.0);
        l.setLineBreakMode(objc2_app_kit::NSLineBreakMode::ByTruncatingTail);
        into_view(l)
    }

    /// A round face with initials.
    fn face(name: &str, i: usize, mtm: MainThreadMarker) -> Retained<NSView> {
        let fills = [
            NSColor::systemBlueColor(),
            NSColor::systemOrangeColor(),
            NSColor::systemGreenColor(),
            NSColor::systemPurpleColor(),
            NSColor::systemTealColor(),
        ];
        let b = NSBox::new(mtm);
        b.setBoxType(NSBoxType::Custom);
        b.setTitlePosition(NSTitlePosition::NoTitle);
        b.setCornerRadius(13.0);
        b.setBorderWidth(0.0);
        b.setFillColor(&fills[i % fills.len()]);
        b.setContentViewMargins(NSSize::new(0.0, 0.0));
        let t = NSTextField::labelWithString(&NSString::from_str(&initials(name)), mtm);
        t.setFont(Some(&NSFont::boldSystemFontOfSize(10.0)));
        t.setTextColor(Some(&NSColor::whiteColor()));
        t.setAlignment(objc2_app_kit::NSTextAlignment::Center);
        t.setFrame(NSRect::new(NSPoint::new(0.0, 6.0), NSSize::new(26.0, 14.0)));
        b.addSubview(&t);
        let v = into_view(b);
        v.setTranslatesAutoresizingMaskIntoConstraints(false);
        v.widthAnchor().constraintEqualToConstant(26.0).setActive(true);
        v.heightAnchor().constraintEqualToConstant(26.0).setActive(true);
        v
    }

    fn row(views: Vec<Retained<NSView>>, spacing: f64, mtm: MainThreadMarker) -> Retained<NSStackView> {
        let r = NSStackView::stackViewWithViews(&NSArray::from_retained_slice(&views), mtm);
        r.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        r.setSpacing(spacing);
        r
    }

    pub fn show(m: &Shown, again: bool, hotkey: &str) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        close();
        CLOSED.with(|c| c.borrow_mut().clear());
        let target = Target::new(mtm);
        let now = pigeon_core::now_ms();

        // Kicker and ✕.
        let kicker_color = if again { NSColor::systemOrangeColor() } else { NSColor::controlAccentColor() };
        let kicker = label(&super::kicker(m, again, now), 11.0, true, kicker_color, mtm);
        // SAFETY: the target lives as long as the panel (kept together) and
        // implements the selectors.
        let x = unsafe {
            let img = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str("xmark"),
                Some(&NSString::from_str("Put away")),
            );
            match img {
                Some(img) => NSButton::buttonWithImage_target_action(&img, Some(&target), Some(sel!(later:)), mtm),
                None => NSButton::buttonWithTitle_target_action(
                    &NSString::from_str("✕"),
                    Some(&target),
                    Some(sel!(later:)),
                    mtm,
                ),
            }
        };
        x.setBordered(false);
        x.setToolTip(Some(&NSString::from_str("Put away (it comes back once if you haven't joined)")));
        let spacer = NSView::new(mtm);
        spacer.setContentHuggingPriority_forOrientation(1.0, objc2_app_kit::NSLayoutConstraintOrientation::Horizontal);
        let top = row(vec![kicker, spacer, into_view(x)], 6.0, mtm);
        top.setDistribution(objc2_app_kit::NSStackViewDistribution::Fill);

        let mut views: Vec<Retained<NSView>> = vec![into_view(top)];
        if let Some(prev) = &m.after {
            views.push(label(&format!("{prev} ends now"), 12.0, false, NSColor::secondaryLabelColor(), mtm));
        }
        views.push(label(&m.title, 17.0, true, NSColor::labelColor(), mtm));
        let when = format!(
            "{}–{}{}",
            crate::format::hhmm(m.start_at),
            crate::format::hhmm(m.end_at),
            if m.service.is_empty() { String::new() } else { format!(" · {}", m.service) }
        );
        views.push(label(&when, 13.0, false, NSColor::secondaryLabelColor(), mtm));
        if !m.people.is_empty() {
            let mut faces: Vec<Retained<NSView>> =
                m.people.iter().take(4).enumerate().map(|(i, n)| face(n, i, mtm)).collect();
            faces.push(label(&m.people_line(), 12.0, false, NSColor::secondaryLabelColor(), mtm));
            views.push(into_view(row(faces, 4.0, mtm)));
        }

        // Join (with its shortcut) and Dismiss.
        let mut buttons: Vec<Retained<NSView>> = Vec::new();
        if m.url.is_some() {
            let title = if hotkey.is_empty() { m.join_label() } else { format!("{}   {hotkey}", m.join_label()) };
            // SAFETY: as above.
            let join = unsafe {
                NSButton::buttonWithTitle_target_action(
                    &NSString::from_str(&title),
                    Some(&target),
                    Some(sel!(join:)),
                    mtm,
                )
            };
            // A filled accent button: the panel never becomes key, so a
            // default push button wouldn't show its color.
            join.setBordered(false);
            join.setAlignment(objc2_app_kit::NSTextAlignment::Center);
            join.setContentTintColor(Some(&NSColor::whiteColor()));
            join.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
            join.setKeyEquivalent(&NSString::from_str("\r"));
            let fill = NSBox::new(mtm);
            fill.setBoxType(NSBoxType::Custom);
            fill.setTitlePosition(NSTitlePosition::NoTitle);
            fill.setCornerRadius(8.0);
            fill.setBorderWidth(0.0);
            fill.setFillColor(&NSColor::controlAccentColor());
            fill.setContentViewMargins(NSSize::new(0.0, 0.0));
            let jv = into_view(join);
            jv.setTranslatesAutoresizingMaskIntoConstraints(false);
            fill.addSubview(&jv);
            let fv = into_view(fill);
            fv.setTranslatesAutoresizingMaskIntoConstraints(false);
            fv.heightAnchor().constraintEqualToConstant(32.0).setActive(true);
            jv.leadingAnchor().constraintEqualToAnchor(&fv.leadingAnchor()).setActive(true);
            jv.trailingAnchor().constraintEqualToAnchor(&fv.trailingAnchor()).setActive(true);
            jv.centerYAnchor().constraintEqualToAnchor(&fv.centerYAnchor()).setActive(true);
            fv.setContentHuggingPriority_forOrientation(1.0, objc2_app_kit::NSLayoutConstraintOrientation::Horizontal);
            buttons.push(fv);
        }
        // SAFETY: as above.
        let dismiss = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(if m.url.is_some() { "Dismiss" } else { "OK" }),
                Some(&target),
                Some(sel!(dismiss:)),
                mtm,
            )
        };
        dismiss.setControlSize(NSControlSize::Large);
        dismiss.setKeyEquivalent(&NSString::from_str("\u{1b}"));
        buttons.push(into_view(dismiss));
        let actions = row(buttons, 8.0, mtm);
        actions.setDistribution(objc2_app_kit::NSStackViewDistribution::Fill);
        let actions_v = into_view(actions);
        views.push(actions_v.clone());

        let stack = NSStackView::stackViewWithViews(&NSArray::from_retained_slice(&views), mtm);
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        stack.setAlignment(NSLayoutAttribute::Leading);
        stack.setSpacing(6.0);
        stack.setEdgeInsets(NSEdgeInsets { top: 14.0, left: 16.0, bottom: 16.0, right: 12.0 });
        stack.setTranslatesAutoresizingMaskIntoConstraints(false);
        stack.widthAnchor().constraintEqualToConstant(WIDTH).setActive(true);
        // The kicker row and the buttons span the width.
        for v in [&views[0], &actions_v] {
            v.widthAnchor().constraintEqualToConstant(WIDTH - 28.0).setActive(true);
        }
        stack.layoutSubtreeIfNeeded();
        let size = stack.fittingSize();

        let Some(screen) = NSScreen::mainScreen(mtm) else { return };
        let vis = screen.visibleFrame();
        let origin = NSPoint::new(
            vis.origin.x + (vis.size.width - size.width) / 2.0,
            vis.origin.y + vis.size.height - size.height - 12.0,
        );
        let style =
            NSWindowStyleMask::Titled | NSWindowStyleMask::FullSizeContentView | NSWindowStyleMask::NonactivatingPanel;
        let window = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            NSRect::new(origin, size),
            style,
            NSBackingStoreType::Buffered,
            false,
        );
        // SAFETY: we keep the Retained ourselves; AppKit must not release it.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitlebarAppearsTransparent(true);
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        for b in [NSWindowButton::CloseButton, NSWindowButton::MiniaturizeButton, NSWindowButton::ZoomButton] {
            if let Some(b) = window.standardWindowButton(b) {
                b.setHidden(true);
            }
        }
        window.setFloatingPanel(true);
        window.setHidesOnDeactivate(false);
        window.setMovableByWindowBackground(true);
        window.setLevel(NSStatusWindowLevel);
        window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );
        window.setContentView(Some(&stack));
        window.setFrame_display(NSRect::new(origin, size), true);
        window.orderFrontRegardless();
        CURRENT.with(|c| {
            *c.borrow_mut() = Some(Panel { window, _target: target, key: m.key.clone(), url: m.url.clone() })
        });
    }

    fn into_view<T: Message + AsRef<NSView>>(v: Retained<T>) -> Retained<NSView> {
        let view: &NSView = (*v).as_ref();
        view.retain()
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn show(_m: &super::Shown, _again: bool, _hotkey: &str) {}
    pub fn close() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kickers() {
        let m = Shown {
            key: "k".into(),
            title: "Design review".into(),
            start_at: 0,
            end_at: 1_800_000,
            url: None,
            service: String::new(),
            people: vec![],
            after: None,
        };
        assert_eq!(kicker(&m, false, 0), "STARTING NOW");
        assert_eq!(kicker(&m, true, 150_000), "STARTED 2 MIN AGO");
        assert_eq!(kicker(&Shown { after: Some("Standup".into()), ..m }, false, 0), "NEXT, STARTS NOW");
    }
}
