//! WebKit's right-click menu, trimmed to what works in an app: Copy Image,
//! Copy Link, Look Up and the like stay; items that open a browser window,
//! download (there's no download handler) or reload the page go.
//!
//! macOS asks the view `willOpenMenu:withEvent:` just before showing its
//! menu; WKWebView's implementation is wrapped once at startup to remove
//! those items by their WebKit identifiers.

/// WebKit menu item identifiers that do nothing useful (or nothing) here.
pub const DROPPED: [&str; 13] = [
    "WKMenuItemIdentifierOpenImageInNewWindow",
    "WKMenuItemIdentifierOpenLinkInNewWindow",
    "WKMenuItemIdentifierOpenFrameInNewWindow",
    "WKMenuItemIdentifierOpenMediaInNewWindow",
    "WKMenuItemIdentifierDownloadImage",
    "WKMenuItemIdentifierDownloadLinkedFile",
    "WKMenuItemIdentifierDownloadMedia",
    "WKMenuItemIdentifierReload",
    "WKMenuItemIdentifierGoBack",
    "WKMenuItemIdentifierGoForward",
    "WKMenuItemIdentifierInspectElement",
    "WKMenuItemIdentifierShowHideMediaControls",
    "WKMenuItemIdentifierToggleFullScreen",
];

/// Which of `items` (identifier, is separator) to keep: drop the listed
/// identifiers, then separators left at the ends or next to each other.
pub fn keep(items: &[(Option<String>, bool)]) -> Vec<bool> {
    let mut keep: Vec<bool> =
        items.iter().map(|(id, _)| !id.as_deref().is_some_and(|id| DROPPED.contains(&id))).collect();
    let mut prev_kept_is_sep = true; // a leading separator goes
    for (i, (_, sep)) in items.iter().enumerate() {
        if !keep[i] {
            continue;
        }
        if *sep && prev_kept_is_sep {
            keep[i] = false;
            continue;
        }
        prev_kept_is_sep = *sep;
    }
    // A trailing separator goes too.
    if let Some(last) = (0..items.len()).rev().find(|&i| keep[i])
        && items[last].1
    {
        keep[last] = false;
    }
    keep
}

#[cfg(target_os = "macos")]
mod imp {
    use std::sync::OnceLock;

    use objc2::ffi;
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
    use objc2::sel;
    use objc2_foundation::NSString;

    type WillOpen = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject);

    static ORIGINAL: OnceLock<WillOpen> = OnceLock::new();

    unsafe extern "C-unwind" fn will_open_menu(
        this: *mut AnyObject,
        cmd: Sel,
        menu: *mut AnyObject,
        event: *mut AnyObject,
    ) {
        if let Some(original) = ORIGINAL.get() {
            // SAFETY: the implementation this one replaced, same signature.
            unsafe { original(this, cmd, menu, event) };
        }
        if menu.is_null() {
            return;
        }
        // SAFETY: `menu` is the NSMenu AppKit is about to show; NSMenu /
        // NSMenuItem methods with their documented signatures.
        unsafe {
            let menu = &*menu;
            let n: isize = msg_send![menu, numberOfItems];
            let items: Vec<(Option<String>, bool)> = (0..n)
                .map(|i| {
                    let item: *mut AnyObject = msg_send![menu, itemAtIndex: i];
                    let id: Option<Retained<NSString>> = msg_send![item, identifier];
                    let sep: Bool = msg_send![item, isSeparatorItem];
                    (id.map(|s| s.to_string()), sep.as_bool())
                })
                .collect();
            for (i, keep) in super::keep(&items).into_iter().enumerate().rev() {
                if !keep {
                    let _: () = msg_send![menu, removeItemAtIndex: i as isize];
                }
            }
        }
    }

    /// Wrap WKWebView's `willOpenMenu:withEvent:` (once).
    pub fn install() {
        if ORIGINAL.get().is_some() {
            return;
        }
        let Some(cls) = AnyClass::get(c"WKWebView") else { return };
        let sel = sel!(willOpenMenu:withEvent:);
        // SAFETY: Objective-C runtime calls on a real class and selector.
        // The original implementation (WKWebView's own or the inherited
        // NSView one) is saved first; the new one is added to WKWebView
        // itself, or replaces WKWebView's own, so NSView is never touched.
        unsafe {
            let method = ffi::class_getInstanceMethod(cls, sel);
            let Some(original) = (!method.is_null()).then(|| ffi::method_getImplementation(method)).flatten() else {
                return;
            };
            let _ = ORIGINAL.set(std::mem::transmute::<Imp, WillOpen>(original));
            let new: Imp = std::mem::transmute::<WillOpen, Imp>(will_open_menu);
            let cls_mut = cls as *const AnyClass as *mut AnyClass;
            if !ffi::class_addMethod(cls_mut, sel, new, c"v@:@@".as_ptr()).as_bool() {
                ffi::method_setImplementation(method, new);
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn install() {}
}

pub use imp::install;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_window_and_download_items_and_tidies_separators() {
        let item = |id: &str| (Some(id.to_string()), false);
        let sep = (None, true);
        let menu = vec![
            item("WKMenuItemIdentifierOpenImageInNewWindow"),
            item("WKMenuItemIdentifierDownloadImage"),
            sep.clone(),
            item("WKMenuItemIdentifierCopyImage"),
            sep.clone(),
            item("WKMenuItemIdentifierReload"),
            sep.clone(),
        ];
        assert_eq!(keep(&menu), [false, false, false, true, false, false, false]);
        // Nothing left of a frame's menu: an empty menu doesn't show.
        let frame = vec![item("WKMenuItemIdentifierReload"), sep.clone(), item("WKMenuItemIdentifierInspectElement")];
        assert!(keep(&frame).iter().all(|k| !k));
        // Items without identifiers (Look Up, Share…) stay.
        let other = vec![(None, false), sep, item("WKMenuItemIdentifierCopyLink")];
        assert_eq!(keep(&other), [true, true, true]);
    }
}
