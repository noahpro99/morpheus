//! Noise suppression and the mic test, as in Discord's Krisp button: in a
//! call, a button beside the camera shows the mode (Off, Standard, Strong)
//! and opens a card above it with the choice and a Mic Test; Settings →
//! Voice & Video has the same controls (its mic test also outside a call).
//! The mode is kept in the data directory (`noise-suppression.json`) and
//! what it does to the microphone is `morpheus_call`'s.
//!
//! The mic test deafens you in a call (others silent for you, shown on the
//! deafen button), plays your microphone back as the call sends it (after
//! suppression) and shows its level; stopping puts deafen back as it was.

use std::time::Duration;

use gpui::{
    AnyElement, Context, IntoElement, MouseButton, Pixels, Point, anchored, deferred, div,
    prelude::*, px, rgb, rgba,
};
use morpheus_call::NoiseSuppression;

use crate::{e2e::E2eExt as _, icons::icon, shell::Shell, theme, view_model::calls::VoiceFlags};

const FILE: &str = "noise-suppression.json";

/// The mode kept from last time (Strong if none).
pub fn read() -> NoiseSuppression {
    std::fs::read_to_string(crate::data_dir().join(FILE))
        .map(|json| NoiseSuppression::from_json(&json))
        .unwrap_or_default()
}

fn write(mode: NoiseSuppression) {
    if let Err(err) = std::fs::write(crate::data_dir().join(FILE), mode.to_json()) {
        eprintln!("morpheus-desktop: saving noise suppression: {err}");
    }
}

/// The icon for a mode: clean bars, a wave, the wave struck through.
pub fn glyph(mode: NoiseSuppression) -> &'static str {
    match mode {
        NoiseSuppression::Strong => "audio-lines",
        NoiseSuppression::Standard => "audio-waveform",
        NoiseSuppression::Off => "audio-waveform-off",
    }
}

/// Use `mode` from now on, and keep it.
pub fn choose(mode: NoiseSuppression) {
    morpheus_call::set_noise_suppression(mode);
    write(mode);
}

impl Shell {
    /// The noise button was pressed: its card, above `at` (the button's top
    /// left), or closed if open.
    pub fn toggle_noise_card(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.calls.noise_card = match self.calls.noise_card {
            Some(_) => None,
            None => Some(at),
        };
        cx.notify();
    }

    pub fn close_noise_card(&mut self, cx: &mut Context<Self>) {
        if self.calls.noise_card.take().is_some() {
            cx.notify();
        }
    }

    pub fn choose_noise(&mut self, mode: NoiseSuppression, cx: &mut Context<Self>) {
        choose(mode);
        self.refresh_settings(cx);
        cx.notify();
    }

    /// Start or stop the mic test. In a call it deafens you while it runs
    /// and puts deafen (and mute) back as they were when it stops.
    pub fn set_mic_test(&mut self, on: bool, cx: &mut Context<Self>) {
        let running = morpheus_call::mic_test_running();
        if on && !running {
            if self.calls.state.is_some() {
                let before = self.calls.voice;
                self.calls.mic_test_before = Some(before);
                self.set_voice(
                    VoiceFlags {
                        deafened: true,
                        ..before
                    },
                    cx,
                );
            }
            morpheus_call::start_mic_test();
        } else if !on && running {
            morpheus_call::stop_mic_test();
            if let Some(before) = self.calls.mic_test_before.take() {
                self.set_voice(before, cx);
            }
        }
        self.refresh_settings(cx);
        cx.notify();
    }

    fn refresh_settings(&self, cx: &mut Context<Self>) {
        if let Some(settings) = &self.settings {
            settings.update(cx, |_, cx| cx.notify());
        }
    }

    /// While the mic test runs (its meter) or the microphone is not yet
    /// processed as chosen, draw again shortly. Called while drawing.
    pub fn keep_mic_fresh(&self, cx: &mut Context<Self>) {
        let in_call = self.calls.state.as_ref().is_some_and(|c| c.connected);
        let testing = morpheus_call::mic_test_running();
        let expected = (in_call || testing).then(morpheus_call::noise_suppression);
        let settled = morpheus_call::applied_noise_suppression() == expected;
        if (testing || !settled) && !self.calls.mic_refresh.replace(true) {
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;
                this.update(cx, |this, cx| {
                    this.calls.mic_refresh.set(false);
                    this.refresh_settings(cx);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }
}

/// The call's noise button (beside the camera): the mode's icon.
pub fn button(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
    let mode = morpheus_call::noise_suppression();
    let open = shell.calls.noise_card.is_some();
    let title = format!("Noise suppression: {}", mode.label());
    div()
        .id("voice-noise")
        .size(px(32.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_SM))
        .cursor_pointer()
        .when(open, |d| d.bg(rgba(theme::bg_hover())))
        .hover(|s| s.bg(rgba(theme::bg_hover())))
        .child(
            icon(glyph(mode), 18.0).text_color(if mode == NoiseSuppression::Off {
                rgb(theme::text_muted())
            } else {
                rgb(theme::accent())
            }),
        )
        // Opening on press: the card's backdrop closes it on the next.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|this, _, _, cx| {
            // The card opens above the button, where it was last drawn.
            let at = this.calls.noise_button_at.get();
            this.toggle_noise_card(at, cx);
        }))
        .child({
            let place = shell.calls.noise_button_at.clone();
            gpui::canvas(
                |_, _, _| (),
                move |bounds, _, _, _| place.set(bounds.origin),
            )
            .absolute()
            .size_full()
        })
        .e2e(
            "button",
            if open {
                "ghost noise-button active"
            } else {
                "ghost noise-button"
            },
        )
        .attr("title", title)
        .attr("data-mode", mode.key())
        .into_any_element()
}

/// The modes and the mic test with its meter, for the card and Settings.
/// `pick` and `test` say what a click does in the view drawing them.
pub fn controls<V: 'static>(
    cx: &mut Context<V>,
    pick: impl Fn(&mut V, NoiseSuppression, &mut Context<V>) + Clone + 'static,
    test: impl Fn(&mut V, bool, &mut Context<V>) + 'static,
) -> Vec<AnyElement> {
    let current = morpheus_call::noise_suppression();
    let modes = NoiseSuppression::ALL.into_iter().map(|mode| {
        let pick = pick.clone();
        let active = mode == current;
        div()
            .id(gpui::SharedString::from(format!(
                "noise-mode-{}",
                mode.key()
            )))
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.0))
            .py(px(8.0))
            .rounded(px(theme::RADIUS_SM))
            .cursor_pointer()
            .when(active, |d| d.bg(rgb(theme::accent_solid())))
            .text_color(if active { rgb(0xffffff) } else { rgb(theme::text_muted()) })
            .when(!active, |d| d.hover(|s| s.bg(rgba(theme::bg_selected())).text_color(rgb(theme::text()))))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| pick(this, mode, cx)))
            .child(icon(glyph(mode), 18.0))
            .child(mode.label())
            .e2e(
                "button",
                if active {
                    "noise-mode active"
                } else {
                    "noise-mode"
                },
            )
            .attr("data-value", mode.key())
            .attr("data-color", if active { "#ffffff".to_owned() } else { format!("#{:06x}", theme::text_muted()) })
            .attr("data-bg", if active { format!("#{:06x}", theme::accent_solid()) } else { String::new() })
            .attr("title", mode.about())
            .text(mode.label())
            .into_any_element()
    });
    let about = div()
        .text_size(px(0.8 * theme::REM))
        .text_color(rgb(theme::text_muted()))
        .child(current.about())
        .into_any_element();
    let testing = morpheus_call::mic_test_running();
    let level = morpheus_call::mic_level();
    let looping = morpheus_call::mic_loopback();
    // The meter in dB: -60 dBFS empty, 0 full.
    let fill = if level > 0.0 {
        ((20.0 * level.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let label = if testing { "Stop Testing" } else { "Mic Test" };
    let test_button = div()
        .id("mic-test")
        .flex_none()
        .px(px(14.0))
        .py(px(7.0))
        .rounded(px(theme::RADIUS_SM))
        .cursor_pointer()
        .bg(if testing {
            rgb(theme::danger())
        } else {
            rgb(theme::accent())
        })
        .text_color(rgb(0xffffff))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .hover(|s| s.opacity(0.9))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _, _, cx| test(this, !testing, cx)))
        .child(label)
        .e2e(
            "button",
            if testing {
                "mic-test active"
            } else {
                "mic-test"
            },
        )
        .text(label);
    let meter = div()
        .flex_1()
        .h(px(8.0))
        .rounded(px(4.0))
        .bg(rgb(theme::bg_input()))
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(gpui::relative(fill))
                .rounded(px(4.0))
                .bg(rgb(theme::green())),
        )
        .e2e("div", "mic-meter")
        .attr("data-level", format!("{level:.3}"))
        .attr("data-loopback", if looping { "on" } else { "off" });
    let note = div()
        .text_size(px(0.75 * theme::REM))
        .text_color(rgb(theme::text_muted()))
        .child(if testing {
            "You hear yourself as others will. Deafened in a call while testing."
        } else {
            "Hear how you sound, after noise suppression."
        })
        .into_any_element();
    vec![
        div().flex().gap(px(6.0)).children(modes).into_any_element(),
        about,
        div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(test_button)
            .child(meter)
            .into_any_element(),
        note,
    ]
}

/// The card above the noise button, over everything, while it is open.
pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let at = shell.calls.noise_card?;
    let panel = div()
        .id("noise-card")
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .occlude()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .w(px(300.0))
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
                .child("Noise suppression"),
        )
        .children(controls(
            cx,
            |this: &mut Shell, mode, cx| this.choose_noise(mode, cx),
            |this: &mut Shell, on, cx| this.set_mic_test(on, cx),
        ))
        .e2e("div", "noise-card");
    let backdrop = div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.close_noise_card(cx)),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(|this, _, _, cx| this.close_noise_card(cx)),
        )
        .child(
            anchored()
                .position(at - gpui::point(px(0.0), px(8.0)))
                .anchor(gpui::Corner::BottomLeft)
                .snap_to_window_with_margin(px(8.0))
                .child(panel),
        )
        .e2e("div", "noise-card-backdrop");
    Some(deferred(backdrop).with_priority(30).into_any_element())
}
