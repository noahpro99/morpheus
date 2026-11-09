//! Shared Dioxus UI for Morpheus.
//!
//! The desktop and mobile crates only pick a renderer and hand over an
//! [`AppConfig`] through `LaunchBuilder::with_context`. Everything visible
//! lives here so both apps stay pixel-consistent.

mod app;
pub mod attach;
pub mod autostart;
mod components;
pub mod device_prefs;
#[cfg(feature = "e2e")]
mod e2e;
#[cfg(all(feature = "web-e2e", not(feature = "e2e")))]
#[path = "e2e_web.rs"]
mod e2e;
#[cfg(not(any(feature = "e2e", feature = "web-e2e")))]
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
pub mod notices;
pub mod platform;
mod sounds;
mod window;

pub use app::{
    App, AppCommand, AppCommands, AppConfig, CallPresence, MicrophoneAccess, OpenRoom, SessionHook,
    SpeakerRoute,
};
pub use window::WindowControls;
