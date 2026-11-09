//! Morpheus in a browser tab (app.morpheuschat.org).
//!
//! The browser is the window: there is no window bar, no tray and no
//! updates, and a link to the desktop app sits at the top (see the shell).
//! The page's storage holds the session (`localStorage`) and the Matrix
//! stores (IndexedDB).
//!
//! `?homeserver=` sets the server the sign-in page starts on. A single
//! sign-on comes back here with `?loginToken=` (and `?sso_homeserver=`).

use std::path::PathBuf;

use dioxus::prelude::*;
use morpheus_ui::AppConfig;

/// Where the page keeps its files (names in its storage, not on a disk).
const DATA_DIR: &str = "/morpheus";

fn main() {
    let params = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .and_then(|search| web_sys::UrlSearchParams::new_with_str(&search).ok());
    let param = |name: &str| params.as_ref().and_then(|p| p.get(name));
    let homeserver = param("homeserver")
        .or_else(|| param("sso_homeserver"))
        .unwrap_or_else(|| "matrix.org".into());
    let mut config = AppConfig::new(PathBuf::from(DATA_DIR), homeserver.clone());
    if let (Some(token), Some(server)) = (param("loginToken"), param("sso_homeserver")) {
        config.sso_return = Some((server, token));
        // Not left in the address bar (or in history) for a reload to reuse.
        if let Some(history) = web_sys::window().and_then(|w| w.history().ok()) {
            let _ = history.replace_state_with_url(
                &wasm_bindgen_null(),
                "",
                Some(&format!("/?homeserver={homeserver}")),
            );
        }
    }
    tracing::info!("starting morpheus web");
    LaunchBuilder::web()
        .with_context(config)
        .launch(morpheus_ui::App);
}

fn wasm_bindgen_null() -> web_sys::wasm_bindgen::JsValue {
    web_sys::wasm_bindgen::JsValue::NULL
}
