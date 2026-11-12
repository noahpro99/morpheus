//! Morpheus desktop, drawn with GPUI. The window, the bundled fonts and the
//! root view: a splash while the saved session restores, then the sign-in
//! screen or the room list.

use std::{borrow::Cow, path::PathBuf};

mod call;
mod call_ui;
mod chat;
mod chrome;
mod composer;
mod dialogs;
mod e2e;
mod emoji_picker;
mod friends;
mod hotkeys;
mod icons;
mod invites;
mod message;
mod input;
mod login;
mod members;
mod media;
mod member_actions;
mod menu;
mod mod_view;
mod model;
mod prefs;
mod profile;
mod profile_actions;
mod rail;
mod room_dialogs;
mod room_list;
mod room_menus;
mod search;
mod settings;
mod shell;
mod sounds;
mod switcher;
mod thread;
mod titlebar;
mod timeline;
mod theme;
mod user_bar;
// The view-models live in their own crate; the old module path stays valid.
use morpheus_view_model as view_model;

use e2e::E2eExt as _;
use gpui::{
    App, Application, AssetSource, Bounds, Context, Entity, IntoElement, Render, SharedString,
    TitlebarOptions, Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size, svg,
};
use morpheus_core::Core;

use login::{Login, SignedIn};
use shell::Shell;

/// Static instances of Inter and Unbounded (GPUI cannot read the woff2
/// variable fonts the Dioxus app uses), made from `crates/ui/assets/fonts`.
const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/Inter-400.ttf"),
    include_bytes!("../assets/fonts/Inter-600.ttf"),
    include_bytes!("../assets/fonts/Inter-700.ttf"),
    include_bytes!("../assets/fonts/Unbounded-400.ttf"),
    include_bytes!("../assets/fonts/Unbounded-600.ttf"),
    include_bytes!("../assets/fonts/Unbounded-700.ttf"),
];

/// Files `svg().path(..)` loads: the logo.
struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(match path {
            "logo.svg" => Some(Cow::Borrowed(
                include_bytes!("../../../crates/ui/assets/logo.svg").as_slice(),
            )),
            // The glasses only, as the site's wordmark crops them.
            "logo-tight.svg" => Some(Cow::Owned(
                include_str!("../../../crates/ui/assets/logo.svg")
                    .replacen("viewBox=\"0 0 512 512\"", "viewBox=\"48 182 416 154\"", 1)
                    .into_bytes(),
            )),
            other => icons::svg_for(other).map(|svg| Cow::Owned(svg.into_bytes())),
        })
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

enum Screen {
    /// The saved session is being restored.
    Splash,
    Login(Entity<Login>),
    Rooms(Entity<Shell>),
}

struct Root {
    core: Core,
    homeserver: String,
    screen: Screen,
}

impl Root {
    fn new(core: Core, homeserver: String, cx: &mut Context<Self>) -> Self {
        let restoring = core.restore_session();
        cx.spawn(async move |this, cx| {
            let result = restoring.await;
            cx.update(|cx| {
                this.update(cx, |this, cx| match result {
                    Ok(Some(session)) => this.signed_in(session, cx),
                    Ok(None) => this.show_login(None, cx),
                    Err(err) => this.show_login(Some(err.to_string()), cx),
                })
            })
            .ok();
        })
        .detach();
        Self {
            core,
            homeserver,
            screen: Screen::Splash,
        }
    }

    fn show_login(&mut self, restore_error: Option<String>, cx: &mut Context<Self>) {
        let (core, homeserver) = (self.core.clone(), self.homeserver.clone());
        let Some(window) = cx.windows().first().copied() else {
            return;
        };
        let login = window
            .update(cx, |_, window, cx| {
                let login = cx.new(|cx| Login::new(core, &homeserver, restore_error, window, cx));
                login
            })
            .ok();
        if let Some(login) = login {
            cx.subscribe(&login, |this: &mut Root, _, event: &SignedIn, cx| {
                this.signed_in(event.0.clone(), cx);
            })
            .detach();
            self.screen = Screen::Login(login);
        }
        cx.notify();
    }

    fn signed_in(&mut self, session: morpheus_core::Session, cx: &mut Context<Self>) {
        self.screen = Screen::Rooms(cx.new(|cx| Shell::new(session, cx)));
        cx.notify();
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let base = div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme::BG_CHAT))
            .font_family("Inter")
            .text_size(px(theme::REM))
            .text_color(rgb(theme::TEXT));
        e2e::frame(
            match &self.screen {
                // The logo, no name.
                Screen::Splash => base.child(chrome::window_bar()).child(
                    div().flex_1().flex().items_center().justify_center().child(
                        svg()
                            .path("logo.svg")
                            .size(px(160.0))
                            .text_color(rgb(theme::ACCENT))
                            .e2e("svg", "splash-logo"),
                    ),
                ),
                Screen::Login(login) => base
                    .child(chrome::window_bar())
                    .child(div().flex_1().min_h_0().child(login.clone())),
                Screen::Rooms(rooms) => base.child(div().flex_1().min_h_0().child(rooms.clone())),
            }
            .child(chrome::resize_edges(window))
            .e2e("body", "app"),
        )
    }
}

/// Test builds are titled as the Dioxus app's are, so the test setup's window
/// rules (unfocused, on a monitor of their own) apply to them.
const TITLE: &str = if cfg!(feature = "e2e") {
    "Morpheus (test)"
} else {
    "Morpheus"
};

/// 1100x720, or `MORPHEUS_E2E_SIZE=WxH` in a test build.
fn window_size() -> gpui::Size<gpui::Pixels> {
    let asked = cfg!(feature = "e2e")
        .then(|| std::env::var("MORPHEUS_E2E_SIZE").ok())
        .flatten()
        .and_then(|size| {
            let (w, h) = size.split_once('x')?;
            Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))
        });
    let (w, h) = asked.unwrap_or((1100.0, 720.0));
    size(px(w), px(h))
}

/// `MORPHEUS_DATA_DIR`, else its own platform data directory ("Morpheus GPUI"),
/// apart from the Dioxus app's so the two never share a store.
pub(crate) fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MORPHEUS_DATA_DIR") {
        return PathBuf::from(dir);
    }
    directories::ProjectDirs::from("dev", "morpheus", "Morpheus GPUI")
        .map(|dirs| dirs.data_dir().to_owned())
        .unwrap_or_else(|| std::env::temp_dir().join("morpheus-gpui"))
}

/// What the sign-in form suggests: `MORPHEUS_HOMESERVER`, else the dev server
/// in test builds and matrix.org otherwise.
fn default_homeserver() -> String {
    std::env::var("MORPHEUS_HOMESERVER").unwrap_or_else(|_| {
        if cfg!(feature = "e2e") {
            "http://localhost:8008".into()
        } else {
            "matrix.org".into()
        }
    })
}

fn main() {
    let core = match Core::new(data_dir()) {
        Ok(core) => core,
        Err(err) => {
            eprintln!("morpheus-gpui: starting the core failed: {err}");
            std::process::exit(1);
        }
    };
    Application::new()
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            if let Err(e) = cx
                .text_system()
                .add_fonts(FONTS.iter().map(|f| Cow::Borrowed(*f)).collect())
            {
                eprintln!("morpheus-gpui: loading fonts failed: {e}");
            }
            let bounds = Bounds::centered(None, window_size(), cx);
            let opened = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some(SharedString::from(TITLE)),
                        ..Default::default()
                    }),
                    app_id: Some("morpheus-gpui".into()),
                    // The app draws its own bar, buttons and resize edges.
                    window_decorations: Some(gpui::WindowDecorations::Client),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title(TITLE);
                    cx.new(|cx| Root::new(core, default_homeserver(), cx))
                },
            );
            if let Err(e) = opened {
                eprintln!("morpheus-gpui: opening the window failed: {e}");
                cx.quit();
            }
            #[cfg(feature = "e2e")]
            e2e::start(cx);
        });
}
