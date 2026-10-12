//! Desktop entry point. Picks the renderer, locates the data directory and
//! hands everything else to `morpheus_ui`.

use std::path::PathBuf;

use std::rc::Rc;

use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
use dioxus::prelude::*;
use morpheus_ui::{AppConfig, WindowControls};
use tracing_subscriber::EnvFilter;

const DEFAULT_HOMESERVER: &str = "http://localhost:8008";

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "morpheus=debug,warn".into()),
        )
        .init();

    let config = AppConfig {
        data_dir: data_dir(),
        default_homeserver: std::env::var("MORPHEUS_HOMESERVER")
            .unwrap_or_else(|_| DEFAULT_HOMESERVER.into()),
    };
    tracing::info!(?config.data_dir, "starting morpheus desktop");

    // Test runs open many windows; they must not steal focus from whatever
    // the developer is typing into. A distinct title lets the e2e driver
    // (and a compositor rule) tell them apart from a real session.
    let under_test = std::env::var_os("MORPHEUS_E2E_PORT").is_some();
    let window = WindowBuilder::new()
        .with_title(if under_test { "Morpheus (test)" } else { "Morpheus" })
        .with_focused(!under_test)
        // The shell draws its own Discord-style bar; see `DesktopRoot`.
        .with_decorations(false)
        .with_inner_size(LogicalSize::new(1280.0, 800.0));

    LaunchBuilder::new()
        .with_context(config)
        .with_cfg(Config::new().with_window(window).with_menu(None))
        .launch(DesktopRoot);
}

/// Wraps the shared app with the window controls its title bar needs.
#[component]
fn DesktopRoot() -> Element {
    let window = dioxus::desktop::use_window();
    use_context_provider(|| WindowControls {
        drag: Rc::new({
            let window = window.clone();
            move || window.drag()
        }),
        minimize: Rc::new({
            let window = window.clone();
            move || window.window.set_minimized(true)
        }),
        toggle_maximize: Rc::new({
            let window = window.clone();
            move || window.window.set_maximized(!window.window.is_maximized())
        }),
        is_focused: Rc::new({
            let window = window.clone();
            move || window.window.is_focused()
        }),
        notify: Rc::new(notify),
        close: Rc::new(move || window.close()),
    });
    rsx! { morpheus_ui::App {} }
}

/// Raise an OS notification. Off the UI thread, since the D-Bus round trip
/// can stall when no notification daemon answers. Test runs never notify:
/// they would pile real pop-ups on the developer's desktop.
fn notify(title: String, body: String) {
    if std::env::var_os("MORPHEUS_E2E_PORT").is_some() {
        return;
    }
    std::thread::spawn(move || {
        if let Err(err) = notify_rust::Notification::new()
            .appname("Morpheus")
            .summary(&title)
            .body(&body)
            .show()
        {
            tracing::warn!(%err, "desktop notification failed");
        }
    });
}

/// `MORPHEUS_DATA_DIR` wins so several accounts can run side by side during
/// development (see `just desktop-as`). Otherwise the XDG data dir.
fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MORPHEUS_DATA_DIR") {
        return PathBuf::from(dir);
    }
    directories::ProjectDirs::from("dev", "morpheus", "Morpheus")
        .map(|dirs| dirs.data_dir().to_owned())
        .unwrap_or_else(|| std::env::temp_dir().join("morpheus"))
}
