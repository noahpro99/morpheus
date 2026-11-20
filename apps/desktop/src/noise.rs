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
use morpheus_call::{NoiseSuppression, NoiseTuning};

use crate::{e2e::E2eExt as _, icons::icon, shell::Shell, theme, view_model::calls::VoiceFlags};

const FILE: &str = "noise-suppression.json";

/// The mode kept from last time (Strong if none).
pub fn read() -> NoiseSuppression {
    std::fs::read_to_string(crate::data_dir().join(FILE))
        .map(|json| NoiseSuppression::from_json(&json))
        .unwrap_or_default()
}

/// The advanced settings kept from last time (the defaults if none).
pub fn read_tuning() -> NoiseTuning {
    let mut tuning = std::fs::read_to_string(crate::data_dir().join(FILE))
        .ok()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .map(|file| NoiseTuning::from_value(&file["tuning"]))
        .unwrap_or_default();
    // The low-delay model chosen, its file gone: the built-in one.
    if tuning.model == morpheus_call::NoiseModel::DeepFilterNet3LowDelay
        && !crate::noise_model::ready()
    {
        tuning.model = morpheus_call::NoiseModel::default();
    }
    tuning
}

/// Keeps the mode and the advanced settings now in use:
/// `{"mode":"strong","tuning":{...}}`.
fn write() {
    let file = serde_json::json!({
        "mode": morpheus_call::noise_suppression().key(),
        "tuning": morpheus_call::noise_tuning().to_value(),
    });
    if let Err(err) = std::fs::write(crate::data_dir().join(FILE), file.to_string()) {
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
    write();
}

/// Use the advanced settings `f` makes from the ones in use, from now on (a
/// call or mic test going takes them within a frame), and keep them.
pub fn tune(f: impl FnOnce(NoiseTuning) -> NoiseTuning) {
    morpheus_call::set_noise_tuning(f(morpheus_call::noise_tuning()));
    write();
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

    pub(crate) fn refresh_settings(&self, cx: &mut Context<Self>) {
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
        // Strong given way to Standard (too slow here) is settled too.
        let settled = morpheus_call::applied_noise_suppression() == expected
            || morpheus_call::strong_too_slow().is_some();
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
            .text_color(if active {
                rgb(theme::on_accent_solid())
            } else {
                rgb(theme::text_muted())
            })
            .when(!active, |d| {
                d.hover(|s| {
                    s.bg(rgba(theme::bg_selected()))
                        .text_color(rgb(theme::text()))
                })
            })
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
            .attr(
                "data-color",
                if active {
                    "#ffffff".to_owned()
                } else {
                    format!("#{:06x}", theme::text_muted())
                },
            )
            .attr(
                "data-bg",
                if active {
                    format!("#{:06x}", theme::accent_solid())
                } else {
                    String::new()
                },
            )
            .attr("title", mode.about())
            .text(mode.label())
            .into_any_element()
    });
    let about = div()
        .text_size(px(0.8 * theme::REM))
        .text_color(rgb(theme::text_muted()))
        .child(current.about())
        .into_any_element();
    // DeepFilterNet could not keep up on this computer, so the microphone
    // gets Standard (#101).
    let too_slow = morpheus_call::strong_too_slow()
        .filter(|_| current == NoiseSuppression::Strong)
        .map(|why| {
            let text = format!("Strong is too slow on this computer ({why}), so Standard is used.");
            div()
                .text_size(px(0.8 * theme::REM))
                .text_color(rgb(theme::danger()))
                .child(text.clone())
                .e2e("div", "noise-too-slow")
                .text(&text)
                .into_any_element()
        });
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
            rgb(theme::accent_solid())
        })
        .text_color(rgb(theme::on(if testing { theme::danger() } else { theme::accent_solid() })))
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
        .attr("data-color", "ffffff")
        .attr(
            "data-bg",
            format!(
                "{:06x}",
                if testing {
                    theme::danger()
                } else {
                    theme::accent_solid()
                }
            ),
        )
        .text(label);
    let meter = div()
        .flex_1()
        .h(px(8.0))
        .rounded(px(4.0))
        .bg(theme::fill_input())
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
    let mut controls = vec![
        div().flex().gap(px(6.0)).children(modes).into_any_element(),
        about,
    ];
    controls.extend(too_slow);
    controls.extend([
        div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(test_button)
            .child(meter)
            .into_any_element(),
        note,
    ]);
    controls
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
        .bg(theme::fill_rail())
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
        .child(
            div()
                .id("noise-settings")
                .flex()
                .items_center()
                .gap(px(6.0))
                .px(px(8.0))
                .py(px(6.0))
                .rounded(px(theme::RADIUS_SM))
                .cursor_pointer()
                .text_color(rgb(theme::text()))
                .hover(|s| s.bg(gpui::rgba(theme::bg_selected())))
                .child(icon("settings", 14.0))
                .child("Noise suppression settings…")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.close_noise_card(cx);
                    this.open_settings(
                        crate::view_model::keys::SettingsFocus::NoiseSuppression,
                        cx,
                    );
                }))
                .e2e("button", "noise-settings-button"),
        )
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

/// A setting's number as shown and kept: no trailing zeros (`35`, `1.5`).
fn shown(value: f32) -> String {
    format!("{}", (value * 1000.0).round() / 1000.0)
}

/// A square − or + that steps a number setting.
fn step_button<V: 'static>(
    cx: &mut Context<V>,
    key: &'static str,
    up: bool,
    bump: impl Fn(&mut V, &mut Context<V>) + 'static,
) -> AnyElement {
    div()
        .id(gpui::SharedString::from(format!(
            "noise-step-{key}-{}",
            if up { "up" } else { "down" }
        )))
        .size(px(24.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_SM))
        .bg(rgba(theme::bg_selected()))
        .text_color(rgb(theme::text()))
        .cursor_pointer()
        .hover(|s| s.opacity(0.8))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _, _, cx| bump(this, cx)))
        .child(if up { "+" } else { "\u{2212}" })
        .e2e("button", "noise-step")
        .attr("data-dir", if up { "up" } else { "down" })
        .text(if up { "+" } else { "-" })
        .into_any_element()
}

/// A line under a setting's name saying what it does.
fn about_line(text: &str) -> AnyElement {
    div()
        .text_size(px(0.75 * theme::REM))
        .text_color(rgb(theme::text_muted()))
        .child(text.to_owned())
        .e2e("div", "noise-about")
        .text(text)
        .into_any_element()
}

/// One number setting: its name, a line on what it does, and − value + .
fn number_row<V: 'static>(cx: &mut Context<V>, param: morpheus_call::NoiseParam) -> AnyElement {
    let now = (param.get)(&morpheus_call::noise_tuning());
    let key = param.key;
    let change = move |up: bool| {
        move |_: &mut V, cx: &mut Context<V>| {
            let delta = if up { param.step } else { -param.step };
            tune(|t| t.with_param(key, (param.get)(&t) + delta));
            cx.notify();
        }
    };
    let value = if param.unit.is_empty() {
        shown(now)
    } else {
        format!("{} {}", shown(now), param.unit)
    };
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .min_w_0()
                .child(div().child(param.label))
                .child(about_line(param.about)),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(8.0))
                .child(step_button(cx, key, false, change(false)))
                .child(
                    div()
                        .min_w(px(84.0))
                        .flex()
                        .justify_center()
                        .text_color(rgb(theme::text_muted()))
                        .child(value),
                )
                .child(step_button(cx, key, true, change(true))),
        )
        .e2e("div", "noise-adv")
        .attr("data-key", key)
        .attr("data-value", shown(now))
        .into_any_element()
}

/// One on/off setting.
fn switch_row<V: 'static>(cx: &mut Context<V>, switch: morpheus_call::NoiseSwitch) -> AnyElement {
    let on = (switch.get)(&morpheus_call::noise_tuning());
    let key = switch.key;
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .min_w_0()
                .child(div().child(switch.label))
                .child(about_line(switch.about)),
        )
        .child(
            div()
                .id(gpui::SharedString::from(format!("noise-switch-{key}")))
                .flex_none()
                .w(px(38.0))
                .h(px(20.0))
                .rounded_full()
                .p(px(2.0))
                .flex()
                .when(on, |d| d.justify_end())
                .bg(if on {
                    rgb(theme::accent_solid())
                } else {
                    theme::fill_input()
                })
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |_: &mut V, _, _, cx| {
                    tune(|t| t.with_switch(key, !(switch.get)(&t)));
                    cx.notify();
                }))
                .child(div().size(px(16.0)).rounded_full().bg(rgb(theme::on(if on { theme::accent_solid() } else { theme::bg_input() }))))
                .e2e("button", "noise-switch")
                .attr("data-key", key)
                .attr("data-on", on.to_string()),
        )
        .into_any_element()
}

fn heading(text: &str) -> AnyElement {
    div()
        .mt(px(6.0))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_size(px(0.85 * theme::REM))
        .text_color(rgb(theme::text_muted()))
        .child(text.to_owned())
        .into_any_element()
}

/// The Advanced section of Settings' noise suppression: a toggle that
/// folds the tunables in and out (`open`, `toggle`), and when open each
/// setting with its explanation, the latency the current setting adds, and
/// Reset to defaults. A change is kept and used at once, by a running mic
/// test or call too.
pub fn advanced<V: 'static>(
    cx: &mut Context<V>,
    open: bool,
    toggle: impl Fn(&mut V, &mut Context<V>) + 'static,
) -> Vec<AnyElement> {
    let header = div()
        .id("noise-advanced-toggle")
        .flex()
        .items_center()
        .gap(px(6.0))
        .cursor_pointer()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(move |this, _, _, cx| toggle(this, cx)))
        .child(if open { "\u{25BE}" } else { "\u{25B8}" })
        .child("Advanced")
        .e2e("button", "noise-advanced-toggle")
        .attr("data-open", open.to_string())
        .text("Advanced")
        .into_any_element();
    if !open {
        return vec![header];
    }
    let tuning = morpheus_call::noise_tuning();
    let param = |key: &str| {
        NoiseTuning::PARAMS
            .into_iter()
            .find(|p| p.key == key)
            .expect("a known setting")
    };
    let switch = |key: &str| {
        NoiseTuning::SWITCHES
            .into_iter()
            .find(|s| s.key == key)
            .expect("a known switch")
    };
    let mode = morpheus_call::noise_suppression();
    let ms = tuning.added_latency_ms(mode, true);
    let applied = morpheus_call::applied_noise_tuning()
        .map(|t| shown(t.atten_limit_db))
        .unwrap_or_default();
    let model = div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(div().child("Model"))
                .child(about_line(
                    "The neural model Strong runs; a low-latency one will be listed here when there is one.",
                )),
        )
        .child(
            div()
                .text_color(rgb(theme::text_muted()))
                .child(tuning.model.label()),
        )
        .e2e("div", "noise-adv")
        .attr("data-key", "model")
        .attr("data-value", tuning.model.key())
        .into_any_element();
    let mut rows = vec![heading("Strong (DeepFilterNet)"), model];
    for key in [
        "atten_limit_db",
        "min_snr_db",
        "max_erb_snr_db",
        "max_df_snr_db",
    ] {
        rows.push(number_row(cx, param(key)));
    }
    rows.push(switch_row(cx, switch("post_filter")));
    rows.push(number_row(cx, param("post_filter_beta")));
    rows.push(heading("Smoothing on top"));
    rows.push(number_row(cx, param("floor_margin_db")));
    rows.push(number_row(cx, param("max_fall_db")));
    rows.push(heading("Standard (WebRTC), in every mode"));
    for key in ["echo_cancellation", "auto_gain", "high_pass"] {
        rows.push(switch_row(cx, switch(key)));
    }
    rows.push(about_line(
        "WebRTC's noise suppression level is fixed by the library and cannot be changed here.",
    ));
    let latency = div()
        .flex()
        .items_center()
        .justify_between()
        .child(div().child("Added latency"))
        .child(
            div()
                .text_color(rgb(theme::text_muted()))
                .child(format!("{ms:.0} ms")),
        )
        .e2e("div", "noise-latency")
        .attr("data-ms", format!("{ms:.0}"))
        .into_any_element();
    let reset = div()
        .id("noise-reset")
        .flex_none()
        .px(px(14.0))
        .py(px(7.0))
        .rounded(px(theme::RADIUS_SM))
        .bg(rgba(theme::bg_selected()))
        .cursor_pointer()
        .hover(|s| s.opacity(0.85))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|_: &mut V, _, _, cx| {
            tune(|_| NoiseTuning::default());
            cx.notify();
        }))
        .child("Reset to defaults")
        .e2e("button", "noise-reset")
        .text("Reset to defaults")
        .into_any_element();
    let body = div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .children(rows)
        .child(latency)
        .child(div().flex().child(reset))
        .e2e("div", "noise-advanced")
        .attr("data-applied-atten", applied)
        .into_any_element();
    vec![header, body]
}
