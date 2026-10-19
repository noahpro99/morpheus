//! Shared Dioxus UI for Morpheus.
//!
//! The desktop and mobile crates only pick a renderer and hand over an
//! [`AppConfig`] through `LaunchBuilder::with_context`. Everything visible
//! lives here so both apps stay pixel-consistent.

mod app;
mod attach;
pub mod autostart;
mod components;
pub mod device_prefs;
#[cfg(feature = "e2e")]
mod e2e;
#[cfg(not(feature = "e2e"))]
mod e2e {
    use dioxus::prelude::*;

    /// Stand-in when the test bridge is compiled out.
    #[component]
    pub fn E2eBridge() -> Element {
        rsx! {}
    }
}
mod format;
pub mod icons;
mod linkify;
mod media;
mod window;

pub use app::{App, AppCommand, AppCommands, AppConfig, OpenRoom, SessionHook};
pub use window::WindowControls;
