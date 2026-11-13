//! Share screen's first question, as in Discord's Go Live: a small popover
//! over the button with the resolution (720p, 1080p, Source) and frame rate
//! (15, 30, 60) to send it at, and Go Live to start. The choice is kept in
//! the data directory (`screen-quality.json`) for the next time. What the
//! choices mean for the encoder is `morpheus_call::ScreenQuality`.

use gpui::{
    AnyElement, Context, IntoElement, MouseButton, Pixels, Point, anchored, deferred, div,
    prelude::*, px, rgb, rgba,
};
use morpheus_call::{SCREEN_FPS, ScreenQuality, ScreenResolution};

use crate::{
    e2e::E2eExt as _,
    shell::Shell,
    theme,
};

const FILE: &str = "screen-quality.json";

/// The choice kept from last time.
pub fn read() -> ScreenQuality {
    std::fs::read_to_string(crate::data_dir().join(FILE))
        .map(|json| ScreenQuality::from_json(&json))
        .unwrap_or_default()
}

fn write(quality: ScreenQuality) {
    let _ = std::fs::write(crate::data_dir().join(FILE), quality.to_json());
}

impl Shell {
    /// Share screen was pressed at `at`: asks how, before sharing.
    pub fn ask_screen_quality(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.calls.screen_popover = Some(at);
        cx.notify();
    }

    pub fn close_screen_quality(&mut self, cx: &mut Context<Self>) {
        if self.calls.screen_popover.take().is_some() {
            cx.notify();
        }
    }

    fn choose_screen_quality(&mut self, quality: ScreenQuality, cx: &mut Context<Self>) {
        self.calls.screen_quality = quality;
        write(quality);
        cx.notify();
    }

    /// Go Live: share, at the quality chosen.
    fn go_live(&mut self, cx: &mut Context<Self>) {
        self.calls.screen_popover = None;
        self.set_screen_share(true, cx);
    }
}

/// A row of choices: a label, then a button each.
fn row<T: Copy + 'static>(
    cx: &mut Context<Shell>,
    label: &str,
    class: &'static str,
    choices: Vec<(T, String, String, bool)>,
    pick: impl Fn(&mut Shell, T, &mut Context<Shell>) + Clone + 'static,
) -> AnyElement {
    let buttons = choices.into_iter().map(|(value, value_key, text, active)| {
        let pick = pick.clone();
        div()
            .id(gpui::SharedString::from(format!("{class}-{value_key}")))
            .flex_1()
            .py(px(6.0))
            .flex()
            .justify_center()
            .rounded(px(theme::RADIUS_SM))
            .cursor_pointer()
            .bg(if active {
                rgb(theme::accent())
            } else {
                rgb(theme::bg_elevated())
            })
            .text_color(if active {
                rgb(0xffffff)
            } else {
                rgb(theme::text())
            })
            .when(!active, |d| d.hover(|s| s.bg(rgba(theme::bg_selected()))))
            .child(text.clone())
            .on_click(cx.listener(move |this, _, _, cx| pick(this, value, cx)))
            .e2e("button", &if active { format!("{class} active") } else { class.to_owned() })
            .attr("data-value", value_key)
            .text(text)
            .into_any_element()
    });
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            div()
                .text_size(px(0.75 * theme::REM))
                .text_color(rgb(theme::text_muted()))
                .child(label.to_owned()),
        )
        .child(div().flex().gap(px(4.0)).children(buttons))
        .into_any_element()
}

/// The popover, over everything, while it is open.
pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let at = shell.calls.screen_popover?;
    let quality = shell.calls.screen_quality;
    let resolutions = ScreenResolution::ALL
        .into_iter()
        .map(|r| {
            (
                r,
                r.key().to_owned(),
                r.label().to_owned(),
                r == quality.resolution,
            )
        })
        .collect();
    let rates = SCREEN_FPS
        .into_iter()
        .map(|fps| (fps, fps.to_string(), fps.to_string(), fps == quality.fps))
        .collect();
    let panel = div()
        .id("screen-quality")
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .occlude()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .w(px(260.0))
        .p(px(12.0))
        .rounded(px(theme::RADIUS_LG))
        .bg(rgb(theme::bg_rail()))
        .border_1()
        .border_color(rgba(theme::edge()))
        .shadow_lg()
        .text_size(px(0.9 * theme::REM))
        .text_color(rgb(theme::text()))
        .child(
            div()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("Share your screen"),
        )
        .child(row(cx, "Resolution", "screen-res", resolutions, move |this, r, cx| {
            let q = this.calls.screen_quality;
            this.choose_screen_quality(ScreenQuality { resolution: r, ..q }, cx);
        }))
        .child(row(cx, "Frame rate", "screen-fps", rates, move |this, fps, cx| {
            let q = this.calls.screen_quality;
            this.choose_screen_quality(ScreenQuality { fps, ..q }, cx);
        }))
        .child(
            div()
                .id("screen-go")
                .flex()
                .justify_center()
                .py(px(8.0))
                .rounded(px(theme::RADIUS_SM))
                .bg(rgb(theme::green()))
                .text_color(rgb(0xffffff))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .cursor_pointer()
                .hover(|s| s.opacity(0.9))
                .child("Go Live")
                .on_click(cx.listener(|this, _, _, cx| this.go_live(cx)))
                .e2e("button", "screen-go")
                .text("Go Live"),
        )
        .e2e("div", "screen-quality");
    let backdrop = div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_screen_quality(cx)))
        .on_mouse_down(MouseButton::Right, cx.listener(|this, _, _, cx| this.close_screen_quality(cx)))
        .child(
            anchored()
                .position(at)
                .anchor(gpui::Corner::BottomLeft)
                .snap_to_window_with_margin(px(8.0))
                .child(panel),
        )
        .e2e("div", "screen-quality-backdrop");
    Some(deferred(backdrop).with_priority(30).into_any_element())
}
