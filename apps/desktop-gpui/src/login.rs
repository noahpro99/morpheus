//! The sign-in screen (`crates/ui/src/components/login.rs`): homeserver,
//! username and password, the error and status lines, the single sign-on
//! button and the way back to a stored account. Signs in through
//! `morpheus_core`; the root view takes the [`Session`] from [`SignedIn`].

use std::time::Duration;

use gpui::{
    Context, Entity, EventEmitter, FontWeight, IntoElement, MouseButton, Render, SharedString,
    Window, div, prelude::*, px, rgb, svg,
};
use morpheus_core::{Core, LoginFlows, LoginRequest, Session};

use crate::e2e::E2eExt as _;
use crate::input::{Changed, Submit, TextInput};
use crate::theme;

/// Quiet time after the last keystroke in the homeserver field before asking
/// that server for its sign-in options.
const PROBE_DEBOUNCE: Duration = Duration::from_millis(600);

/// Signing in worked.
pub struct SignedIn(pub Session);

impl EventEmitter<SignedIn> for Login {}

pub struct Login {
    core: Core,
    homeserver: Entity<TextInput>,
    username: Entity<TextInput>,
    password: Entity<TextInput>,
    /// Newest typing in the homeserver field, to settle its probe.
    typed: u64,
    probed: String,
    flows: Option<Result<LoginFlows, String>>,
    error: Option<String>,
    busy: Option<&'static str>,
    /// An account already on this device (added another from its menu).
    stored: Option<String>,
}

impl Login {
    pub fn new(
        core: Core,
        default_homeserver: &str,
        restore_error: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let homeserver =
            cx.new(|cx| TextInput::new("homeserver", "matrix.org", false, default_homeserver, cx));
        let username = cx.new(|cx| TextInput::new("username", "", false, "", cx));
        let password = cx.new(|cx| TextInput::new("password", "", true, "", cx));
        username.read(cx).focus(window);
        cx.subscribe(&homeserver, |this, _, _: &Changed, cx| {
            this.homeserver_typed(cx)
        })
        .detach();
        for input in [&homeserver, &username, &password] {
            cx.subscribe(input, |this, _, _: &Submit, cx| this.submit(cx))
                .detach();
        }
        let mut this = Self {
            core,
            homeserver,
            username,
            password,
            typed: 0,
            probed: default_homeserver.to_owned(),
            flows: None,
            error: restore_error,
            busy: None,
            stored: None,
        };
        this.probe(cx);
        this.load_stored(cx);
        this
    }

    fn load_stored(&mut self, cx: &mut Context<Self>) {
        let accounts = self.core.accounts();
        cx.spawn(async move |this, cx| {
            let accounts = accounts.await.unwrap_or_default();
            let stored = accounts
                .into_iter()
                .find(|account| account.active)
                .map(|account| account.user_id.to_string());
            this.update(cx, |this, cx| {
                this.stored = stored;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Ask the server what it supports.
    fn probe(&mut self, cx: &mut Context<Self>) {
        let server = self.probed.clone();
        self.flows = None;
        let core = self.core.clone();
        cx.spawn(async move |this, cx| {
            let found = if server.trim().is_empty() {
                Ok(LoginFlows::default())
            } else {
                core.login_flows(server.clone())
                    .await
                    .map_err(|err| format!("Could not reach {}: {err}", server.trim()))
            };
            this.update(cx, |this, cx| {
                if this.probed == server {
                    this.flows = Some(found);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn homeserver_typed(&mut self, cx: &mut Context<Self>) {
        self.typed += 1;
        let stamp = self.typed;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PROBE_DEBOUNCE).await;
            this.update(cx, |this, cx| {
                let value = this.homeserver.read(cx).value().to_owned();
                if this.typed == stamp && this.probed != value {
                    this.probed = value;
                    this.probe(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn finish(&mut self, result: Result<Session, String>, cx: &mut Context<Self>) {
        self.busy = None;
        match result {
            Ok(session) => cx.emit(SignedIn(session)),
            Err(message) => self.error = Some(message),
        }
        cx.notify();
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some("Signing in…");
        self.error = None;
        let request = LoginRequest {
            homeserver: self.homeserver.read(cx).value().to_owned(),
            username: self.username.read(cx).value().to_owned(),
            password: self.password.read(cx).value().to_owned(),
        };
        let signing_in = self.core.login(request);
        cx.spawn(async move |this, cx| {
            let result = signing_in.await.map_err(|err| err.to_string());
            this.update(cx, |this, cx| this.finish(result, cx)).ok();
        })
        .detach();
        cx.notify();
    }

    fn sso(&mut self, cx: &mut Context<Self>) {
        if self.busy.is_some() {
            return;
        }
        self.busy = Some("Finish signing in in your browser…");
        self.error = None;
        let signing_in = self
            .core
            .login_sso(self.homeserver.read(cx).value().to_owned(), None);
        cx.spawn(async move |this, cx| {
            let result = signing_in.await.map_err(|err| err.to_string());
            this.update(cx, |this, cx| this.finish(result, cx)).ok();
        })
        .detach();
        cx.notify();
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        let restoring = self.core.restore_session();
        cx.spawn(async move |this, cx| {
            let result = restoring.await;
            this.update(cx, |this, cx| match result {
                Ok(Some(session)) => cx.emit(SignedIn(session)),
                Ok(None) => {}
                Err(err) => {
                    this.error = Some(err.to_string());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}

fn button(label: impl Into<SharedString>, primary: bool, busy: bool) -> gpui::Stateful<gpui::Div> {
    let label: SharedString = label.into();
    div()
        .id(label.clone())
        .flex()
        .justify_center()
        .p(px(12.0))
        .rounded(px(theme::RADIUS))
        .font_weight(FontWeight::SEMIBOLD)
        .when(busy, |d| d.opacity(0.6))
        .when(!busy, |d| d.cursor_pointer())
        .bg(rgb(if primary {
            theme::ACCENT_STRONG
        } else {
            theme::BG_INPUT
        }))
        .child(label)
}

fn label(text: &str, field: Entity<TextInput>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .text_size(px(0.75 * theme::REM))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(theme::TEXT_MUTED))
        .child(text.to_uppercase())
        .child(
            div()
                .text_size(px(theme::REM))
                .text_color(rgb(theme::TEXT))
                .child(field),
        )
        .e2e("label", "")
        .text(text)
}

impl Render for Login {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fallback = LoginFlows {
            password: true,
            sso: false,
        };
        let (flows, probe_error) = match &self.flows {
            Some(Ok(found)) => (found.clone(), None),
            Some(Err(err)) => (fallback, Some(err.clone())),
            None => (fallback, None),
        };
        let probing = self.flows.is_none();
        let busy = self.busy.is_some();

        let mut form = div()
            .w(px(420.0))
            .p(px(32.0))
            .flex()
            .flex_col()
            .gap(px(16.0))
            .rounded(px(16.0))
            .bg(rgb(theme::BG_SIDEBAR))
            .child(
                svg()
                    .path("logo.svg")
                    .size(px(48.0))
                    .text_color(rgb(theme::ACCENT)),
            )
            .child(
                div()
                    .font_family("Unbounded")
                    .font_weight(FontWeight::BOLD)
                    .text_size(px(1.5 * theme::REM))
                    .child("Welcome back")
                    .e2e("h1", "")
                    .text("Welcome back"),
            )
            .child(
                div()
                    .text_color(rgb(theme::TEXT_MUTED))
                    .child("Sign in to your Matrix account")
                    .e2e("p", "muted")
                    .text("Sign in to your Matrix account"),
            )
            .child(label("Homeserver", self.homeserver.clone()));
        if flows.password {
            form = form
                .child(label("Username or email", self.username.clone()))
                .child(label("Password", self.password.clone()));
        }
        let message = self.error.clone().or(probe_error);
        if let Some(message) = message {
            form = form.child(
                div()
                    .text_color(rgb(theme::DANGER))
                    .text_size(px(0.9 * theme::REM))
                    .child(message.clone())
                    .e2e("p", "error")
                    .text(message),
            );
        }
        if let Some(status) = self.busy {
            form = form.child(
                div()
                    .text_color(rgb(theme::TEXT_MUTED))
                    .child(status)
                    .e2e("p", "muted status")
                    .text(status),
            );
        }
        if flows.password {
            form = form.child(
                button("Sign in", true, busy)
                    .mt(px(8.0))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.submit(cx)),
                    )
                    .e2e("button", "")
                    .attr("type", "submit")
                    .attr("disabled", busy.to_string())
                    .text("Sign in"),
            );
        }
        if flows.sso {
            form = form.child(
                button("Sign in with single sign-on", false, busy)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.sso(cx)),
                    )
                    .e2e("button", "secondary")
                    .attr("type", "button")
                    .text("Sign in with single sign-on"),
            );
        }
        if let Some(account) = self.stored.clone() {
            let text = format!("Back to {account}");
            form = form.child(
                button(text.clone(), false, busy)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.back(cx)),
                    )
                    .e2e("button", "secondary back-to-account")
                    .attr("type", "button")
                    .text(text),
            );
        }
        if probing {
            form = form.child(
                div()
                    .text_color(rgb(theme::TEXT_MUTED))
                    .child("Checking sign-in options…")
                    .e2e("p", "muted status")
                    .text("Checking sign-in options…"),
            );
        }
        if !probing && !flows.password && !flows.sso {
            let text = "This server offers no sign-in method Morpheus supports yet.";
            form = form.child(
                div()
                    .text_color(rgb(theme::DANGER))
                    .child(text)
                    .e2e("p", "error")
                    .text(text),
            );
        }
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(theme::BG_RAIL))
            .child(form.e2e("form", "card login"))
            .e2e("main", "centered")
    }
}
