//! The native menu bar. On macOS the Edit menu is what routes ⌘C/⌘V/⌘X/⌘A/⌘Z
//! to text fields in the web view, and the app menu carries ⌘Q: launching
//! with no menu (`with_menu(None)`) silently disabled all of them.
//!
//! Key ownership: ⌘, is on the menu item (opening Settings twice is harmless
//! if the web view also sees it). ⌘+ / ⌘- / ⌘0 stay with the JS bridge, which
//! also handles them inside editors; the View items have no accelerators so
//! nothing fires twice.

use dioxus::desktop::muda::accelerator::{Accelerator, Code, Modifiers};
use dioxus::desktop::muda::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};

pub const SETTINGS: &str = "pigeon-settings";
pub const AGENT_SETUP: &str = "pigeon-agent-setup";
pub const ZOOM_IN: &str = "pigeon-zoom-in";
pub const ZOOM_OUT: &str = "pigeon-zoom-out";
pub const ZOOM_RESET: &str = "pigeon-zoom-reset";

pub fn build() -> Menu {
    let about = AboutMetadata {
        name: Some("Pigeon".into()),
        version: Some(env!("CARGO_PKG_VERSION").into()),
        ..Default::default()
    };
    let settings =
        MenuItem::with_id(SETTINGS, "Settings…", true, Some(Accelerator::new(Some(Modifiers::SUPER), Code::Comma)));
    let app = Submenu::with_items(
        "Pigeon",
        true,
        &[
            &PredefinedMenuItem::about(Some("About Pigeon"), Some(about)),
            &PredefinedMenuItem::separator(),
            &settings,
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::quit(None),
        ],
    )
    .expect("app menu");
    let edit = Submenu::with_items(
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(None),
            &PredefinedMenuItem::redo(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::cut(None),
            &PredefinedMenuItem::copy(None),
            &PredefinedMenuItem::paste(None),
            &PredefinedMenuItem::select_all(None),
        ],
    )
    .expect("edit menu");
    let view = Submenu::with_items(
        "View",
        true,
        &[
            &MenuItem::with_id(ZOOM_IN, "Bigger Text    ⌘+", true, None),
            &MenuItem::with_id(ZOOM_OUT, "Smaller Text   ⌘−", true, None),
            &MenuItem::with_id(ZOOM_RESET, "Actual Size    ⌘0", true, None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::fullscreen(None),
        ],
    )
    .expect("view menu");
    let window = Submenu::with_items(
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(None),
            &PredefinedMenuItem::maximize(None),
            &PredefinedMenuItem::close_window(None),
        ],
    )
    .expect("window menu");
    let help =
        Submenu::with_items("Help", true, &[&MenuItem::with_id(AGENT_SETUP, "Connect an AI Agent…", true, None)])
            .expect("help menu");

    let menu = Menu::new();
    menu.append_items(&[&app, &edit, &view, &window, &help]).expect("menu bar");
    #[cfg(target_os = "macos")]
    {
        window.set_as_windows_menu_for_nsapp();
        help.set_as_help_menu_for_nsapp();
    }
    menu
}
