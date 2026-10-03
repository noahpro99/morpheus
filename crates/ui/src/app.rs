use std::path::PathBuf;

use dioxus::prelude::*;
use morpheus_core::{Core, Session};

use crate::{
    components::{Login, Shell, Splash},
    e2e::E2eBridge,
};

/// Provided by the platform crate via `LaunchBuilder::with_context`.
#[derive(Debug, Clone, PartialEq)]
pub struct AppConfig {
    pub data_dir: PathBuf,
    pub default_homeserver: String,
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

#[component]
fn Boot(core: Core) -> Element {
    use_context_provider(|| core.clone());
    let mut session: Signal<Option<Session>> = use_context_provider(|| Signal::new(None));

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
        (Some(_), Some(active)) => rsx! { Shell { session: active } },
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
