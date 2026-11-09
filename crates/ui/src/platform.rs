//! What differs between a desktop, a phone and a browser, for the UI.

/// Show a web link: the desktop's browser, or a new tab of this one.
pub fn open_url(url: &str) -> std::io::Result<()> {
    #[cfg(not(target_arch = "wasm32"))]
    return open::that_detached(url);
    #[cfg(target_arch = "wasm32")]
    {
        let url = serde_json::to_string(url).unwrap_or_default();
        dioxus::prelude::document::eval(&format!("window.open({url}, '_blank', 'noopener');"));
        Ok(())
    }
}

/// The web app (app.morpheuschat.org): no window of its own to manage, no
/// tray, no updates; a link to the desktop app instead.
pub const IS_WEB: bool = cfg!(target_arch = "wasm32");
