//! Mobile entry point. Same UI as desktop; only the renderer and the
//! storage location differ.

use std::path::PathBuf;

use dioxus::prelude::*;
use morpheus_ui::AppConfig;
use tracing_subscriber::EnvFilter;

const DEFAULT_HOMESERVER: &str = "http://10.0.2.2:8008"; // host machine from the Android emulator

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

    LaunchBuilder::new()
        .with_context(config)
        .launch(morpheus_ui::App);
}

/// PLAN.md phase 5: resolve the platform's app-private files directory via
/// the Android `Context` / iOS `NSFileManager`. Until then this uses an
/// override or a writable temp location so the app boots.
fn data_dir() -> PathBuf {
    std::env::var_os("MORPHEUS_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("morpheus"))
}
