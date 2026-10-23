//! App icon at runtime. A bundled Coo.app gets its icon from
//! Info.plist/Coo.icns; an unbundled binary (`cargo app`) would show the
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

/// Unread count on the Dock icon (unified inbox); 0 clears it.
#[cfg(target_os = "macos")]
pub fn set_badge(count: u32) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    use objc2_foundation::NSString;

    let Some(mtm) = MainThreadMarker::new() else { return };
    let tile = NSApplication::sharedApplication(mtm).dockTile();
    let label = badge_label(count).map(|l| NSString::from_str(&l));
    tile.setBadgeLabel(label.as_deref());
}

#[cfg(not(target_os = "macos"))]
pub fn set_badge(_count: u32) {}

/// Dock badge text: none for 0, "999+" past that.
pub fn badge_label(count: u32) -> Option<String> {
    match count {
        0 => None,
        1..=999 => Some(count.to_string()),
        _ => Some("999+".into()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn badge_labels() {
        assert_eq!(super::badge_label(0), None);
        assert_eq!(super::badge_label(7).as_deref(), Some("7"));
        assert_eq!(super::badge_label(1500).as_deref(), Some("999+"));
    }
}
