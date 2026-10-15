use crate::icons::Logo;
use dioxus::prelude::*;
use morpheus_core::{Core, LoginFlows, LoginRequest, Session};

use crate::AppConfig;

/// Quiet time after the last keystroke in the homeserver field before asking
/// that server for its sign-in options.
const PROBE_DEBOUNCE_MS: u64 = 600;

#[component]
pub fn Login(restore_error: Option<String>) -> Element {
    let core = use_context::<Core>();
    let config = use_context::<AppConfig>();
    let open_url = config.open_url.clone();
    let mut session = use_context::<Signal<Option<Session>>>();

    let mut homeserver = use_signal(|| config.default_homeserver.clone());
    // Only probed when the field settles (change event), not on every keystroke.
    let mut probe_server = use_signal(|| config.default_homeserver.clone());
    let mut username = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut error = use_signal(|| restore_error);
    let mut busy = use_signal(|| None::<&'static str>);

    // Ask the server what it supports whenever the homeserver field settles:
    // on leaving the field, or a moment after typing stops (on a phone the
    // field is often never left before tapping a button). Until it answers,
    // assume a password form so the screen never goes blank.
    let mut typed = use_signal(|| 0u64);
    let flows = use_resource({
        let core = core.clone();
        move || {
            let core = core.clone();
            let server = probe_server();
            async move {
                if server.trim().is_empty() {
                    return Ok(LoginFlows::default());
                }
                core.login_flows(server.clone())
                    .await
                    .map_err(|err| format!("Could not reach {}: {err}", server.trim()))
            }
        }
    });
    let fallback = LoginFlows {
        password: true,
        sso: false,
    };
    let (flows_now, probe_error) = match flows() {
        Some(Ok(found)) => (found, None),
        Some(Err(err)) => (fallback, Some(err)),
        None => (fallback, None),
    };
    let probing = flows().is_none();

    let submit = {
        let core = core.clone();
        move |evt: FormEvent| {
            evt.prevent_default();
            if busy().is_some() {
                return;
            }
            busy.set(Some("Signing in…"));
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
                busy.set(None);
            });
        }
    };

    let sso = move |_| {
        if busy().is_some() {
            return;
        }
        busy.set(Some("Finish signing in in your browser…"));
        error.set(None);

        let core = core.clone();
        let server = homeserver();
        let open_url = open_url.clone();
        spawn(async move {
            match core.login_sso(server, open_url).await {
                Ok(active) => session.set(Some(active)),
                Err(err) => error.set(Some(err.to_string())),
            }
            busy.set(None);
        });
    };

    rsx! {
        main { class: "centered",
            form { class: "card login", onsubmit: submit,
                Logo { size: 48, class: "login-logo" }
                h1 { "Welcome back" }
                p { class: "muted", "Sign in to your Matrix account" }

                label { "Homeserver"
                    input {
                        r#type: "text",
                        name: "homeserver",
                        placeholder: "matrix.org",
                        value: "{homeserver}",
                        oninput: move |e| {
                            let value = e.value();
                            homeserver.set(value.clone());
                            let stamp = *typed.peek() + 1;
                            typed.set(stamp);
                            spawn(async move {
                                tokio::time::sleep(std::time::Duration::from_millis(PROBE_DEBOUNCE_MS)).await;
                                if *typed.peek() == stamp && *probe_server.peek() != value {
                                    probe_server.set(value);
                                }
                            });
                        },
                        onchange: move |e| {
                            if *probe_server.peek() != e.value() {
                                probe_server.set(e.value());
                            }
                        },
                    }
                }

                if flows_now.password {
                    label { "Username or email"
                        input { r#type: "text", name: "username", autofocus: true, value: "{username}", oninput: move |e| username.set(e.value()) }
                    }
                    label { "Password"
                        input { r#type: "password", name: "password", value: "{password}", oninput: move |e| password.set(e.value()) }
                    }
                }

                if let Some(message) = error() {
                    p { class: "error", "{message}" }
                } else if let Some(message) = probe_error {
                    p { class: "error", "{message}" }
                }
                if let Some(status) = busy() {
                    p { class: "muted status", "{status}" }
                }

                if flows_now.password {
                    button { r#type: "submit", disabled: busy().is_some(), "Sign in" }
                }
                if flows_now.sso {
                    button {
                        r#type: "button",
                        class: "secondary",
                        disabled: busy().is_some(),
                        onclick: sso,
                        "Sign in with single sign-on"
                    }
                }
                if probing {
                    p { class: "muted status", "Checking sign-in options…" }
                }
                if !probing && !flows_now.password && !flows_now.sso {
                    p { class: "error", "This server offers no sign-in method Morpheus supports yet." }
                }
            }
        }
    }
}
