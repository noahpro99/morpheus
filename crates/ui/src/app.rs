use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use dioxus::prelude::*;
use morpheus_core::{Core, OwnedRoomId, Session};

use crate::{
    components::{Login, Shell, Splash, WindowBar},
    e2e::E2eBridge,
};

/// Called with every session that becomes active (restored or freshly logged
/// in), so the platform can run work that must outlive the webview, such as
/// OS notifications while the window is hidden.
pub type SessionHook = Arc<dyn Fn(Session) + Send + Sync>;

/// The room on screen, shared with platform code outside the UI runtime.
pub type OpenRoom = Arc<Mutex<Option<OwnedRoomId>>>;

/// Ask for the microphone before a call; blocks until the user answers.
/// `true` when the app may record.
pub type MicrophoneAccess = Arc<dyn Fn() -> bool + Send + Sync>;

/// Send call audio to the loudspeaker (`true`) or the earpiece (phones).
pub type SpeakerRoute = Arc<dyn Fn(bool) + Send + Sync>;

/// Tells the platform about the call going: `Some((room id, room name))`
/// once connected, `None` when it ends (a phone's ongoing call notification).
pub type CallPresence = Arc<dyn Fn(Option<(String, String)>) + Send + Sync>;

/// Something the platform asks the UI to do, from outside its runtime (the
/// tray menu).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppCommand {
    Acknowledgements,
    CheckForUpdates,
    /// Show this room (its Matrix id), e.g. from a tapped notification.
    OpenRoom(String),
    /// Hang up the call going (the phone's call notification).
    HangUp,
    /// Answer the call ringing in this room (the phone's incoming call).
    AnswerCall {
        room_id: String,
    },
    /// Decline the ring (`notification`, its event id) in this room.
    DeclineCall {
        room_id: String,
        notification: String,
    },
}

/// A queue of [`AppCommand`]s any thread can push to; the shell drains it.
/// Commands sent before sign-in wait for the shell.
#[derive(Clone, Default)]
pub struct AppCommands {
    queue: Arc<Mutex<Vec<AppCommand>>>,
    wake: Arc<tokio::sync::Notify>,
}

impl AppCommands {
    pub fn send(&self, command: AppCommand) {
        if let Ok(mut queue) = self.queue.lock() {
            queue.push(command);
        }
        self.wake.notify_one();
    }

    pub(crate) async fn next(&self) -> AppCommand {
        loop {
            let waiting = self
                .queue
                .lock()
                .ok()
                .and_then(|mut queue| (!queue.is_empty()).then(|| queue.remove(0)));
            if let Some(command) = waiting {
                return command;
            }
            self.wake.notified().await;
        }
    }
}

/// Provided by the platform crate via `LaunchBuilder::with_context`.
#[derive(Clone)]
pub struct AppConfig {
    pub data_dir: PathBuf,
    pub default_homeserver: String,
    pub on_session: Option<SessionHook>,
    pub open_room: OpenRoom,
    pub commands: AppCommands,
    /// How to show a URL in the browser (single sign-on). `None` uses the
    /// desktop opener; Android supplies an intent.
    pub open_url: Option<morpheus_core::UrlOpener>,
    /// Runtime permission for the microphone (Android). `None`: no need.
    pub microphone: Option<MicrophoneAccess>,
    /// Loudspeaker or earpiece for calls (phones). `None`: the system decides.
    pub speakerphone: Option<SpeakerRoute>,
    /// The platform's own record of the call going. `None`: nothing to tell.
    pub call_presence: Option<CallPresence>,
    /// A single sign-on that came back to the page: `(homeserver, login
    /// token)` (the web app).
    pub sso_return: Option<(String, String)>,
}

impl AppConfig {
    pub fn new(data_dir: PathBuf, default_homeserver: String) -> Self {
        Self {
            data_dir,
            default_homeserver,
            on_session: None,
            open_room: OpenRoom::default(),
            commands: AppCommands::default(),
            open_url: None,
            microphone: None,
            speakerphone: None,
            call_presence: None,
            sso_return: None,
        }
    }
}

impl std::fmt::Debug for AppConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppConfig")
            .field("data_dir", &self.data_dir)
            .field("default_homeserver", &self.default_homeserver)
            .finish_non_exhaustive()
    }
}

const THEME: Asset = asset!("/assets/theme.css");

// The site's fonts (morpheuschat.org, the same files), bundled so the app
// looks the same everywhere instead of taking the system's: Inter for text,
// Unbounded for the wordmark and headings. Variable, Latin and Latin
// Extended (accented names). SIL OFL, licences beside them in assets/fonts.
const INTER_LATIN: Asset = asset!("/assets/fonts/inter-latin-wght-normal.woff2");
const INTER_LATIN_EXT: Asset = asset!("/assets/fonts/inter-latin-ext-wght-normal.woff2");
const UNBOUNDED_LATIN: Asset = asset!("/assets/fonts/unbounded-latin-wght-normal.woff2");
const UNBOUNDED_LATIN_EXT: Asset = asset!("/assets/fonts/unbounded-latin-ext-wght-normal.woff2");
const LATIN: &str = "U+0000-00FF,U+0131,U+0152-0153,U+02BB-02BC,U+02C6,U+02DA,U+02DC,U+0304,U+0308,U+0329,U+2000-206F,U+20AC,U+2122,U+2191,U+2193,U+2212,U+2215,U+FEFF,U+FFFD";
const LATIN_EXT: &str = "U+0100-02BA,U+02BD-02C5,U+02C7-02CC,U+02CE-02D7,U+02DD-02FF,U+0304,U+0308,U+0329,U+1D00-1DBF,U+1E00-1E9F,U+1EF2-1EFF,U+2020,U+20A0-20AB,U+20AD-20C0,U+2113,U+2C60-2C7F,U+A720-A7FF";

/// The `@font-face` rules, with the bundled files' served paths.
fn font_faces() -> String {
    let face = |family: &str, weights: &str, file: Asset, range: &str| {
        format!(
            "@font-face {{ font-family: '{family}'; font-style: normal; font-display: swap; \
             font-weight: {weights}; src: url({file}) format('woff2'); unicode-range: {range}; }}"
        )
    };
    [
        face("Inter Variable", "100 900", INTER_LATIN_EXT, LATIN_EXT),
        face("Inter Variable", "100 900", INTER_LATIN, LATIN),
        face(
            "Unbounded Variable",
            "200 900",
            UNBOUNDED_LATIN_EXT,
            LATIN_EXT,
        ),
        face("Unbounded Variable", "200 900", UNBOUNDED_LATIN, LATIN),
    ]
    .join("\n")
}

/// Root component. Boots the core, restores a session, and routes between
/// the login screen and the main shell.
#[component]
pub fn App() -> Element {
    let config = use_context::<AppConfig>();
    let core = use_hook(|| Core::new(&config.data_dir).map_err(|e| e.to_string()));

    rsx! {
        document::Style { {font_faces()} }
        document::Stylesheet { href: THEME }
        // No-op unless built with the `e2e` feature.
        E2eBridge {}
        if crate::platform::IS_WEB {
            // A browser tab: its own bar (no window to manage) with the way
            // to the desktop app, above everything.
            div { class: "web-root",
                WebBar {}
                div { class: "web-content",
                    match core {
                        Err(message) => rsx! { FatalError { message } },
                        Ok(core) => rsx! { Boot { core } },
                    }
                }
            }
        } else {
            match core {
                Err(message) => rsx! { FatalError { message } },
                Ok(core) => rsx! { Boot { core } },
            }
        }
    }
}

/// Bumped to rebuild the whole interface (Ctrl+R, as in Discord): the shell
/// is keyed by it, so everything under it starts afresh on the same session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reload(pub u32);
pub type Reloads = Signal<Reload>;

#[component]
fn Boot(core: Core) -> Element {
    use_context_provider(|| core.clone());
    let mut session: Signal<Option<Session>> = use_context_provider(|| Signal::new(None));
    let reloads: Reloads = use_context_provider(|| Signal::new(Reload(0)));
    // Hand each new session to the platform (see `SessionHook`).
    let hook = use_context::<AppConfig>().on_session;
    use_effect(move || {
        if let (Some(hook), Some(active)) = (&hook, session()) {
            hook(active);
        }
    });

    let restore = use_resource(move || {
        let core = core.clone();
        async move {
            match core.restore_session().await {
                Ok(Some(restored)) => {
                    session.set(Some(restored));
                    Ok(())
                }
                Ok(None) => Ok(()),
                Err(err) => Err(err.to_string()),
            }
        }
    });

    match (&*restore.read(), session()) {
        (None, _) => rsx! {
            div { class: "window-screen", WindowBar {} Splash {} }
        },
        // Keyed by account: switching accounts builds a fresh shell around
        // the new session instead of keeping the old one in its context.
        // (Keys only count in a list, hence the list of one.)
        (Some(_), Some(active)) => rsx! {
            for active in [active] {
                Shell {
                    key: "{active.user_id().map(|id| id.to_string()).unwrap_or_default()}-{reloads().0}",
                    session: active,
                }
            }
        },
        (Some(result), None) => rsx! {
            div { class: "window-screen",
                WindowBar {}
                Login { restore_error: result.clone().err() }
            }
        },
    }
}

#[component]
fn FatalError(message: String) -> Element {
    rsx! {
        main { class: "centered",
            div { class: "card",
                h1 { "Morpheus could not start" }
                p { class: "error", "{message}" }
            }
        }
    }
}

/// Where the desktop app is downloaded.
pub const DESKTOP_DOWNLOAD_URL: &str = "https://morpheuschat.org/download";

/// The web app's bar: only the way to the desktop app (the browser has the
/// rest of what a window bar holds).
#[component]
fn WebBar() -> Element {
    rsx! {
        div { class: "web-bar",
            a {
                class: "get-desktop",
                href: DESKTOP_DOWNLOAD_URL,
                target: "_blank",
                rel: "noopener",
                crate::icons::Icon { glyph: crate::icons::DOWNLOAD, size: 14 }
                "Get Morpheus Desktop"
            }
        }
    }
}
