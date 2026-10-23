//! App icon at runtime. A bundled Pigeon.app gets its icon from
//! Info.plist/Pigeon.icns; an unbundled binary (`cargo app`) would show the
//! generic executable icon in the Dock, so set it explicitly.

#[cfg(target_os = "macos")]
pub fn set_app_icon() {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    const PNG: &[u8] = include_bytes!("../assets/icon/icon_512.png");
    let Some(mtm) = MainThreadMarker::new() else { return };
    let data = NSData::with_bytes(PNG);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else { return };
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: on the main thread (checked above) with a valid image.
    #[allow(unused_unsafe)]
    unsafe {
        app.setApplicationIconImage(Some(&image));
    }
}

#[cfg(not(target_os = "macos"))]
pub fn set_app_icon() {}
