use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use dioxus::prelude::*;
use morpheus_core::{Core, OwnedRoomId, Session};

use crate::{
    components::{Login, Shell, Splash},
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

/// Something the platform asks the UI to do, from outside its runtime (the
/// tray menu).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppCommand {
    Acknowledgements,
    CheckForUpdates,
    /// Show this room (its Matrix id), e.g. from a tapped notification.
    OpenRoom(String),
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

/// Root component. Boots the core, restores a session, and routes between
/// the login screen and the main shell.
#[component]
pub fn App() -> Element {
    let config = use_context::<AppConfig>();
    let core = use_hook(|| Core::new(&config.data_dir).map_err(|e| e.to_string()));

    rsx! {
        document::Stylesheet { href: THEME }
        // No-op unless built with the `e2e` feature.
        E2eBridge {}
        match core {
            Err(message) => rsx! { FatalError { message } },
            Ok(core) => rsx! { Boot { core } },
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
        (None, _) => rsx! { Splash {} },
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
        (Some(result), None) => rsx! { Login { restore_error: result.clone().err() } },
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
