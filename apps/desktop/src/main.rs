//! Desktop entry point. Picks the renderer, locates the data directory and
//! hands everything else to `morpheus_ui`.

use std::path::PathBuf;

use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
use dioxus::prelude::*;
use morpheus_ui::AppConfig;
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

    let window = WindowBuilder::new()
        .with_title("Morpheus")
        .with_inner_size(LogicalSize::new(1280.0, 800.0));

    LaunchBuilder::new()
        .with_context(config)
        .with_cfg(Config::new().with_window(window).with_menu(None))
        .launch(morpheus_ui::App);
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
