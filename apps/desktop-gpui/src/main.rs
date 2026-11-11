//! Morpheus desktop, drawn with GPUI. Scaffold: a window with the theme's
//! chat background and the bundled fonts.

use std::borrow::Cow;

mod e2e;

use e2e::E2eExt as _;
use gpui::{
    App, Application, Bounds, Context, IntoElement, Render, SharedString, TitlebarOptions, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};

/// Theme tokens (dark), from `crates/ui/assets/theme.css`.
mod theme {
    pub const BG_CHAT: u32 = 0x1a1e29;
    pub const TEXT: u32 = 0xe6e9f2;
}

/// The brand label's text.
const BRAND: &str = "Morpheus";

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

struct Root;

impl Render for Root {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        e2e::frame(
            div()
                .size_full()
                .bg(rgb(theme::BG_CHAT))
                .flex()
                .items_center()
                .justify_center()
                .font_family("Inter")
                .text_color(rgb(theme::TEXT))
                .child(
                    div()
                        .font_family("Unbounded")
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_size(px(36.0))
                        .child(BRAND)
                        .e2e("h1", "brand")
                        .text(BRAND),
                )
                .e2e("div", "app"),
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

fn main() {
    Application::new().run(|cx: &mut App| {
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
                ..Default::default()
            },
            |window, cx| {
                window.set_window_title(TITLE);
                cx.new(|_| Root)
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
