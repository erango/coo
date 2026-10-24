//! Coo › About Coo: the standard macOS About panel with our own credits
//! ("Built with love by @erango", the handle linking to GitHub) and the
//! update status. Opening it also checks for updates in the background; the
//! status line refreshes while it's open.

pub const AUTHOR: &str = "@erango";
pub const AUTHOR_URL: &str = "https://github.com/erango";

/// The credits text and the (UTF-16 offset, length) of the linked handle.
const LEAD: &str = "Built with love by ";

pub fn credits(status_line: &str) -> (String, (usize, usize)) {
    let text = format!("{LEAD}{AUTHOR}\n\n{status_line}");
    let start = LEAD.encode_utf16().count();
    (text, (start, AUTHOR.encode_utf16().count()))
}

/// Open (or refresh) the About panel, and check for updates.
pub fn show() {
    imp::show(&crate::updater::status().line());
    crate::updater::check_background(true);
}

/// Refresh the status line if the panel is open.
pub fn refresh() {
    if imp::is_open() {
        imp::show(&crate::updater::status().line());
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{MainThreadMarker, Message, msg_send};
    use objc2_app_kit::{
        NSAboutPanelOptionApplicationName, NSAboutPanelOptionApplicationVersion, NSAboutPanelOptionCredits,
        NSAboutPanelOptionVersion, NSApplication, NSColor, NSFont, NSFontAttributeName, NSForegroundColorAttributeName,
        NSLinkAttributeName, NSMutableParagraphStyle, NSParagraphStyleAttributeName, NSTextAlignment, NSView,
    };
    use objc2_foundation::{NSDictionary, NSMutableAttributedString, NSRange, NSString, NSURL};

    /// The open About panel's credits text view (found by its text).
    fn credits_view(mtm: MainThreadMarker) -> Option<Retained<NSView>> {
        fn find(v: &NSView) -> Option<Retained<NSView>> {
            if v.class().name().to_str().is_ok_and(|n| n.contains("TextView")) {
                // SAFETY: NSText -string -> NSString *.
                let text: Option<Retained<NSString>> = unsafe { msg_send![v, string] };
                if text.is_some_and(|t| t.to_string().starts_with(super::LEAD)) {
                    return Some(v.retain());
                }
            }
            v.subviews().iter().find_map(|c| find(&c))
        }
        NSApplication::sharedApplication(mtm)
            .windows()
            .iter()
            .filter(|w| w.isVisible())
            .find_map(|w| w.contentView().and_then(|c| find(&c)))
    }

    pub fn is_open() -> bool {
        MainThreadMarker::new().is_some_and(|mtm| credits_view(mtm).is_some())
    }

    pub fn show(status_line: &str) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let s = attributed(status_line);
        // Already open: swap the credits in place (AppKit keeps the panel and
        // ignores new options while it's on screen).
        if let Some(view) = credits_view(mtm) {
            // SAFETY: NSTextView -textStorage -> NSTextStorage * (an
            // NSMutableAttributedString); -setAttributedString: replaces it.
            unsafe {
                let storage: Option<Retained<AnyObject>> = msg_send![&*view, textStorage];
                if let Some(storage) = storage {
                    let _: () = msg_send![&*storage, setAttributedString: &*s];
                }
            }
            return;
        }
        present(mtm, &s);
    }

    fn attributed(status_line: &str) -> Retained<NSMutableAttributedString> {
        let (text, (start, len)) = super::credits(status_line);
        // SAFETY: plain init with a string, no attributes.
        let s: Retained<NSMutableAttributedString> = unsafe {
            NSMutableAttributedString::initWithString_attributes(
                objc2::AllocAnyThread::alloc(),
                &NSString::from_str(&text),
                None,
            )
        };
        let all = NSRange::new(0, s.length());
        let para = NSMutableParagraphStyle::new();
        para.setAlignment(NSTextAlignment::Center);
        let font = NSFont::systemFontOfSize(11.0);
        let muted = NSColor::secondaryLabelColor();
        // SAFETY: attribute values have the types AppKit expects for each key.
        unsafe {
            s.addAttribute_value_range(NSParagraphStyleAttributeName, &para, all);
            s.addAttribute_value_range(NSFontAttributeName, &font, all);
            s.addAttribute_value_range(NSForegroundColorAttributeName, &muted, all);
            if let Some(url) = NSURL::URLWithString(&NSString::from_str(super::AUTHOR_URL)) {
                s.addAttribute_value_range(NSLinkAttributeName, &url, NSRange::new(start, len));
            }
        }
        s
    }

    fn present(mtm: MainThreadMarker, s: &NSMutableAttributedString) {
        let (version, build) = crate::updater::version();
        // SAFETY: AppKit's About panel option key constants (immutable NSStrings).
        let keys = unsafe {
            [
                NSAboutPanelOptionApplicationName,
                NSAboutPanelOptionApplicationVersion,
                NSAboutPanelOptionVersion,
                NSAboutPanelOptionCredits,
            ]
        };
        let name = NSString::from_str("Coo");
        let version = NSString::from_str(&version);
        let build = NSString::from_str(&format!("build {build}"));
        let values: [&AnyObject; 4] = [&name, &version, &build, s];
        let opts = NSDictionary::from_slices(&keys, &values);
        let app = NSApplication::sharedApplication(mtm);
        // SAFETY: option values have the documented types (NSString, NSAttributedString).
        unsafe { app.orderFrontStandardAboutPanelWithOptions(&opts) };
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn is_open() -> bool {
        false
    }
    pub fn show(_status_line: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credits_link_covers_the_handle() {
        let (text, (start, len)) = credits("Coo is up to date");
        assert!(text.starts_with("Built with love by @erango"));
        let utf16: Vec<u16> = text.encode_utf16().collect();
        assert_eq!(String::from_utf16(&utf16[start..start + len]).unwrap(), "@erango");
        assert!(text.ends_with("Coo is up to date"));
    }
}
