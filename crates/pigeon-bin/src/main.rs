//! Coo desktop app (Dioxus 0.7). Renders from signals fed by the engine
//! service; every action goes out as a `Cmd`.

mod about;
mod bridge;
mod ctxmenu;
mod dock;
mod dragout;
mod emoji;
mod fonts;
mod format;
mod keymon;
mod meeting;
mod menu;
mod menubar;
mod milestones;
mod notify;
mod perf;
mod prefs;
mod quicklook;
mod richtext;
mod shortcuts;
mod sound;
mod spell;
mod state;
mod syskeys;
mod themes;
mod ui;
mod updater;
mod week;

use std::sync::{Arc, Mutex, OnceLock};

use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
use pigeon_engine::Pigeon;
use pigeon_engine::service::{Cmd, Delta};
use tokio::sync::mpsc;

/// CSS straight from the design bundle (single source of truth).
const CSS: &[&str] = &[
    include_str!("../../../design/tokens.css"),
    include_str!("../../../design/base.css"),
    include_str!("../../../design/screens/shell.css"),
    include_str!("../../../design/screens/mail-list.css"),
    include_str!("../../../design/screens/reading.css"),
    include_str!("../../../design/screens/composer.css"),
    include_str!("../../../design/screens/calendar.css"),
    include_str!("../../../design/screens/event-editor.css"),
    include_str!("../../../design/screens/palette.css"),
    include_str!("../../../design/screens/search.css"),
    include_str!("../../../design/screens/onboarding.css"),
    include_str!("../../../design/screens/settings.css"),
    include_str!("../../../design/screens/states.css"),
    include_str!("app.css"),
];

static CMD: OnceLock<mpsc::UnboundedSender<Cmd>> = OnceLock::new();
/// Delta receiver, taken once by the UI pump.
static RX: Mutex<Option<mpsc::UnboundedReceiver<Delta>>> = Mutex::new(None);

pub fn send_cmd(cmd: Cmd) {
    if let Some(tx) = CMD.get() {
        let _ = tx.send(cmd);
    }
}

/// `<head>` for every window (main and compose): styles, so the first paint
/// is already styled, and the theme attributes before the body exists, so
/// there's no flash of the default palette.
pub fn head_html() -> String {
    let prefs = prefs::get();
    let platform_css = if cfg!(target_os = "macos") { ":root{--titlebar-traffic:62px}" } else { "" };
    let zoom_css = ui::zoom::head_css(prefs.font_scale);
    let list_css = ui::list_width_css(prefs.list_width);
    let sidebar_css = ui::sidebar_width_css(prefs.sidebar_width);
    let theme_mode = match prefs.theme_mode.as_str() {
        "dark" | "light" => prefs.theme_mode.as_str(),
        _ => "system",
    };
    let palette = if themes::is_known(&prefs.palette) { prefs.palette.as_str() } else { themes::DEFAULT };
    format!(
        "<style>{}\n{}\n{}\n{platform_css}\n{zoom_css}\n{list_css}\n{sidebar_css}</style><script>document.documentElement.dataset.theme={theme_mode:?};\
         document.documentElement.dataset.palette={palette:?};</script>",
        fonts::css(),
        CSS.join("\n"),
        themes::css()
    )
}

fn main() {
    perf::mark_start();
    // Crash reporting off in Settings > Privacy: telemetry::init honors this.
    if !prefs::get().crash_reports {
        // SAFETY: first thing in main, before any other thread exists.
        unsafe { std::env::set_var("PIGEON_TELEMETRY", "off") };
    }
    // Keep the guard for the whole process so queued reports flush on exit.
    let _telemetry = pigeon_engine::telemetry::init(
        concat!("coo-app@", env!("CARGO_PKG_VERSION")),
        "warn,pigeon=info,pigeon_engine=info",
    );

    // Load settings now so the first paint already uses them.
    let prefs = prefs::get();
    tracing::debug!(font_scale = prefs.font_scale, "prefs loaded");

    let app = match Pigeon::open() {
        Ok(app) => Arc::new(app),
        Err(e) => {
            eprintln!("pigeon: cannot open the local store: {e}");
            std::process::exit(1);
        }
    };

    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (delta_tx, delta_rx) = mpsc::unbounded_channel();
    CMD.set(cmd_tx).ok();
    *RX.lock().unwrap() = Some(delta_rx);

    // Engine on its own runtime, off the UI thread.
    std::thread::Builder::new()
        .name("pigeon-engine".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
            rt.block_on(pigeon_engine::service::run(app, cmd_rx, delta_tx));
        })
        .expect("engine thread");

    let mut window = WindowBuilder::new()
        .with_title("Coo")
        // Relaunched by a silent update while Coo was in the menu bar.
        .with_visible(!prefs::get().relaunch_hidden)
        .with_inner_size(LogicalSize::new(1360.0, 860.0))
        .with_min_inner_size(LogicalSize::new(960.0, 600.0));
    #[cfg(target_os = "macos")]
    {
        use dioxus::desktop::tao::platform::macos::WindowBuilderExtMacOS;
        window = window.with_titlebar_transparent(true).with_fullsize_content_view(true).with_title_hidden(true);
    }

    let head = head_html();

    dock::set_app_icon();
    notify::init();
    ctxmenu::install();
    spell::install();

    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            Config::new()
                .with_window(window)
                .with_custom_head(head)
                // A real menu bar: without the Edit menu macOS doesn't route
                // copy/paste/undo to the web view, and ⌘Q does nothing.
                .with_menu(menu::build())
                .with_background_color((17, 18, 21, 255)),
        )
        .launch(ui::App);
}
