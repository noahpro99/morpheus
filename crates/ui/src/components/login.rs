use dioxus::prelude::*;
use morpheus_core::{Core, LoginRequest, Session};

use crate::AppConfig;

#[component]
pub fn Login(restore_error: Option<String>) -> Element {
    let core = use_context::<Core>();
    let config = use_context::<AppConfig>();
    let mut session = use_context::<Signal<Option<Session>>>();

    let mut homeserver = use_signal(|| config.default_homeserver.clone());
    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut error = use_signal(|| restore_error);
    let mut busy = use_signal(|| false);

    let submit = move |evt: FormEvent| {
        evt.prevent_default();
        if busy() {
            return;
        }
        busy.set(true);
        error.set(None);

        let core = core.clone();
        let request = LoginRequest {
            homeserver: homeserver(),
            username: username(),
            password: password(),
        };
        spawn(async move {
            match core.login(request).await {
                Ok(active) => session.set(Some(active)),
                Err(err) => error.set(Some(err.to_string())),
            }
            busy.set(false);
        });
    };

    rsx! {
        main { class: "centered",
            form { class: "card login", onsubmit: submit,
                h1 { "Welcome back" }
                p { class: "muted", "Sign in to your Matrix account" }

                label { "Homeserver"
                    input { r#type: "url", value: "{homeserver}", oninput: move |e| homeserver.set(e.value()) }
                }
                label { "Username"
                    input { r#type: "text", autofocus: true, value: "{username}", oninput: move |e| username.set(e.value()) }
                }
                label { "Password"
                    input { r#type: "password", value: "{password}", oninput: move |e| password.set(e.value()) }
                }

                if let Some(message) = error() {
                    p { class: "error", "{message}" }
                }

                button { r#type: "submit", disabled: busy(),
                    if busy() { "Signing in…" } else { "Sign in" }
                }
            }
        }
    }
}
