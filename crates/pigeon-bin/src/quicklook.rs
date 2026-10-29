//! Quick Look and Finder for files: an attachment's preview in macOS's
//! Quick Look panel, inside Coo (Space or Esc closes it), and "Show in
//! Finder". The panel is driven through its data source; if this macOS
//! doesn't have it, the system's own Quick Look window opens instead.

use std::path::Path;

/// Preview `path` in Quick Look.
pub fn preview(path: &Path) {
    if !mac::preview(path) {
        let _ = std::process::Command::new("/usr/bin/qlmanage")
            .arg("-p")
            .arg(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }
}

/// Select `path` in a Finder window.
pub fn reveal(path: &Path) {
    if !mac::reveal(path) {
        let _ = std::process::Command::new("/usr/bin/open").arg("-R").arg(path).spawn();
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::cell::RefCell;
    use std::path::Path;

    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, NSObject};
    use objc2::{AllocAnyThread, DefinedClass, MainThreadMarker, define_class, msg_send};
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSArray, NSString, NSURL};

    #[link(name = "QuickLookUI", kind = "framework")]
    unsafe extern "C" {}

    define_class!(
        // QLPreviewPanelDataSource: one item, the file (NSURL is a
        // QLPreviewItem).
        #[unsafe(super(NSObject))]
        #[name = "CooQuickLookSource"]
        #[ivars = Retained<NSURL>]
        struct Source;

        impl Source {
            #[unsafe(method(numberOfPreviewItemsInPreviewPanel:))]
            fn count(&self, _panel: *mut AnyObject) -> isize {
                1
            }

            #[unsafe(method(previewPanel:previewItemAtIndex:))]
            fn item(&self, _panel: *mut AnyObject, _index: isize) -> *mut AnyObject {
                Retained::as_ptr(self.ivars()).cast::<AnyObject>().cast_mut()
            }
        }
    );

    thread_local! {
        // The panel holds its data source weakly.
        static SOURCE: RefCell<Option<Retained<Source>>> = const { RefCell::new(None) };
    }

    fn url(path: &Path) -> Retained<NSURL> {
        NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
    }

    pub fn preview(path: &Path) -> bool {
        if MainThreadMarker::new().is_none() {
            return false;
        }
        let Some(cls) = AnyClass::get(c"QLPreviewPanel") else { return false };
        let source: Retained<Source> = {
            let this = Source::alloc().set_ivars(url(path));
            // SAFETY: NSObject's init on a freshly allocated instance.
            unsafe { msg_send![super(this), init] }
        };
        // SAFETY: QLPreviewPanel's documented class and instance methods,
        // on the main thread; the data source is kept alive in SOURCE.
        unsafe {
            let panel: *mut AnyObject = msg_send![cls, sharedPreviewPanel];
            if panel.is_null() {
                return false;
            }
            let _: () = msg_send![panel, setDataSource: &*source];
            let _: () = msg_send![panel, reloadData];
            let _: () = msg_send![panel, setCurrentPreviewItemIndex: 0isize];
            let none: *mut AnyObject = std::ptr::null_mut();
            let _: () = msg_send![panel, makeKeyAndOrderFront: none];
        }
        SOURCE.with(|s| *s.borrow_mut() = Some(source));
        true
    }

    pub fn reveal(path: &Path) -> bool {
        if MainThreadMarker::new().is_none() {
            return false;
        }
        NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url(path)]));
        true
    }
}

#[cfg(not(target_os = "macos"))]
mod mac {
    pub fn preview(_: &std::path::Path) -> bool {
        false
    }
    pub fn reveal(_: &std::path::Path) -> bool {
        false
    }
}
