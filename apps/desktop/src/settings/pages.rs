//! The settings pages that are not about keys: Notifications, Appearance,
//! Voice & Video, Startup and Help & Feedback, and what the search does
//! (`crates/ui/src/components/settings.rs`). What they offer and say is in
//! `view_model::settings`; this draws it and runs the calls.

use std::time::Duration;

use gpui::{
    AnyElement, ClickEvent, Context, ElementId, FontWeight, IntoElement, MouseButton, SharedString,
    Window, canvas, div, prelude::*, px, rgb, rgba,
};
use morpheus_core::NotifyMode;

use super::{Kind, SettingsView, ThemeChosen, button, muted, note, section, section_classed};
use crate::{
    e2e::E2eExt as _,
    icons::icon,
    theme,
    view_model::settings::{
        self as vm, Autostart, DESKTOP_NOTIFICATION_NOTE, NOTIFY_DEFAULT_CHOICES,
        NotificationContent, ROOM_DEFAULTS_BLURB, SettingEntry, Theme,
    },
};

/// How long a setting opened by the search flashes.
const HIGHLIGHT: Duration = Duration::from_millis(2500);

/// A radio button or checkbox with its words: `.notify-choice`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn choice(
    id: String,
    group: &str,
    value: &str,
    label: &str,
    checked: bool,
    checkbox: bool,
    disabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let ring = rgb(if checked {
        theme::accent()
    } else {
        theme::text_muted()
    });
    let mark = div()
        .size(px(18.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .border_2()
        .border_color(ring)
        .map(|d| {
            if checkbox {
                d.rounded(px(3.0))
            } else {
                d.rounded_full()
            }
        })
        .when(checked, |d| {
            d.child(
                div()
                    .size(px(8.0))
                    .map(|d| {
                        if checkbox {
                            d.rounded(px(3.0))
                        } else {
                            d.rounded_full()
                        }
                    })
                    .bg(rgb(theme::accent())),
            )
        })
        .e2e("input", "")
        .attr("type", if checkbox { "checkbox" } else { "radio" })
        .attr("name", group)
        .attr("value", value);
    let mark = if checked {
        mark.attr("checked", "true")
    } else {
        mark
    };
    let mark = if disabled {
        mark.attr("disabled", "true")
    } else {
        mark
    };
    div()
        .id(ElementId::from(SharedString::from(id)))
        .flex()
        .items_center()
        .gap(px(8.0))
        .when(disabled, |d| d.opacity(0.4))
        .when(!disabled, |d| d.cursor_pointer().on_click(on_click))
        .child(mark)
        .child(div().child(label.to_owned()).e2e("span", "").text(label))
        .e2e("label", "notify-choice")
        .into_any_element()
}

/// A group of choices under a legend (`fieldset.notify-default`).
fn group(
    legend: &str,
    kind: &str,
    anchor: Option<&'static str>,
    highlighted: bool,
    children: Vec<AnyElement>,
) -> AnyElement {
    let el = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x(px(16.0))
        .gap_y(px(6.0))
        .mt(px(12.0))
        .mb(px(4.0))
        .child(
            div()
                .w_full()
                .font_weight(FontWeight::SEMIBOLD)
                .child(legend.to_owned())
                .e2e("legend", "")
                .text(legend),
        )
        .children(children)
        .when(highlighted, |d| d.bg(gpui::rgba(theme::accent_tint())))
        .e2e(
            "fieldset",
            match (anchor.is_some(), highlighted) {
                (true, true) => "notify-default notification-content setting-highlight",
                (true, false) => "notify-default notification-content",
                _ => "notify-default",
            },
        );
    let el = if kind.is_empty() {
        el
    } else {
        el.attr("data-kind", kind)
    };
    match anchor {
        Some(anchor) => el.attr("data-setting", anchor).into_any_element(),
        None => el.into_any_element(),
    }
}

impl SettingsView {
    /// Whether `anchor` is the setting the search just opened.
    pub(super) fn hl(&self, anchor: &str) -> bool {
        self.highlight == Some(anchor)
    }

    /// Opens a page on the setting a search found, flashing it a moment.
    pub(crate) fn show_setting(&mut self, entry: &'static SettingEntry, cx: &mut Context<Self>) {
        if entry.page != crate::view_model::settings::SettingsPage::VoiceVideo {
            self.stop_video(cx);
        }
        self.page = entry.page;
        self.highlight = Some(entry.anchor);
        self.search.update(cx, |input, cx| input.set_value("", cx));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HIGHLIGHT).await;
            this.update(cx, |this, cx| {
                if this.highlight == Some(entry.anchor) {
                    this.highlight = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// The server's defaults for direct messages and group rooms.
    pub(super) fn load_notify_defaults(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        for (slot, dm) in [(0usize, true), (1, false)] {
            let reading = session.default_notify_mode(dm);
            cx.spawn(async move |this, cx| {
                if let Ok(mode) = reading.await {
                    this.update(cx, |this, cx| {
                        this.notify_defaults[slot] = Some(mode);
                        cx.notify();
                    })
                    .ok();
                }
            })
            .detach();
        }
    }

    fn set_notify_default(&mut self, slot: usize, mode: NotifyMode, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        self.notify_defaults[slot] = Some(mode);
        self.notify_notes[slot] = None;
        let saving = session.set_default_notify_mode(slot == 0, mode);
        cx.spawn(async move |this, cx| {
            let result = saving.await;
            this.update(cx, |this, cx| {
                this.notify_notes[slot] = Some(match result {
                    Ok(()) => Ok("Saved"),
                    Err(err) => Err(err.to_string()),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn notify_default(&self, slot: usize, cx: &mut Context<Self>) -> AnyElement {
        let (kind, title) = vm::notify_default_kind(slot == 0);
        let mut children: Vec<AnyElement> = NOTIFY_DEFAULT_CHOICES
            .iter()
            .map(|(mode, value, label)| {
                let mode = *mode;
                choice(
                    format!("notify-{kind}-{value}"),
                    &format!("notify-{kind}"),
                    value,
                    label,
                    self.notify_defaults[slot] == Some(mode),
                    false,
                    false,
                    cx.listener(move |this, _, _, cx| this.set_notify_default(slot, mode, cx)),
                )
            })
            .collect();
        match &self.notify_notes[slot] {
            Some(Ok(text)) => children.push(
                div()
                    .text_color(rgb(theme::ok()))
                    .child(*text)
                    .e2e("span", "ok")
                    .text(*text)
                    .into_any_element(),
            ),
            Some(Err(text)) => children.push(
                div()
                    .text_color(rgb(theme::danger()))
                    .child(text.clone())
                    .e2e("span", "error")
                    .text(text.clone())
                    .into_any_element(),
            ),
            None => {}
        }
        group(title, kind, None, false, children)
    }

    pub(super) fn notifications_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let defaults = section(
            "Default for rooms",
            "room-defaults",
            vec![
                muted(ROOM_DEFAULTS_BLURB, "muted").into_any_element(),
                self.notify_default(0, cx),
                self.notify_default(1, cx),
            ],
            self.hl("room-defaults"),
        );
        let on_here = self.on_here;
        let sounds_here = self.sounds_here;
        let mut device = vec![
            choice(
                "device-notifications".into(),
                "device-notifications",
                "",
                "Notifications on this device",
                on_here,
                true,
                false,
                cx.listener(move |this, _, _, cx| {
                    let want = !this.on_here;
                    match vm::set_notifications_enabled(&this.data_dir, want) {
                        Ok(()) => this.on_here = want,
                        Err(err) => {
                            eprintln!("morpheus-desktop: saving the notification setting: {err}")
                        }
                    }
                    cx.notify();
                }),
            ),
            choice(
                "message-sounds".into(),
                "message-sounds",
                "",
                "Message sounds",
                sounds_here,
                true,
                false,
                cx.listener(move |this, _, _, cx| {
                    let want = !this.sounds_here;
                    match vm::set_message_sounds_enabled(&this.data_dir, want) {
                        Ok(()) => this.sounds_here = want,
                        Err(err) => {
                            eprintln!("morpheus-desktop: saving the message sound setting: {err}")
                        }
                    }
                    cx.notify();
                }),
            ),
            muted(DESKTOP_NOTIFICATION_NOTE, "device-notifications muted").into_any_element(),
        ];
        let shows = NotificationContent::ALL
            .into_iter()
            .map(|content| {
                choice(
                    format!("content-{}", content.key()),
                    "notification-content",
                    content.key(),
                    content.label(),
                    self.content == content,
                    false,
                    false,
                    cx.listener(move |this, _, _, cx| {
                        match vm::set_notification_content(&this.data_dir, content) {
                            Ok(()) => this.content = content,
                            Err(err) => {
                                eprintln!("morpheus-desktop: saving what notifications show: {err}")
                            }
                        }
                        cx.notify();
                    }),
                )
            })
            .collect();
        device.push(group(
            "What notifications show",
            "",
            Some("notification-content"),
            self.hl("notification-content"),
            shows,
        ));
        let device = section(
            "This device",
            "device-notifications",
            device,
            self.hl("device-notifications") || self.hl("notification-content"),
        );
        vec![defaults.into_any_element(), device.into_any_element()]
    }

    pub(super) fn appearance_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let options = Theme::ALL.into_iter().map(|theme_choice| {
            let selected = theme_choice == self.theme && self.custom.is_none();
            let (side, chat) = theme::swatch_of(theme_choice);
            let swatch = div()
                .flex()
                .w(px(120.0))
                .h(px(72.0))
                .rounded(px(theme::RADIUS))
                .overflow_hidden()
                .child(div().w(relative_36()).h_full().bg(rgb(side)))
                .child(div().flex_1().h_full().bg(rgb(chat)));
            div()
                .id(ElementId::from(SharedString::from(format!(
                    "theme-{}",
                    theme_choice.key()
                ))))
                .flex()
                .flex_col()
                .gap(px(8.0))
                .p(px(8.0))
                .rounded(px(theme::RADIUS_LG))
                .border_2()
                .border_color(if selected {
                    rgb(theme::accent())
                } else {
                    rgb(theme::bg_elevated())
                })
                .bg(rgb(theme::bg_elevated()))
                .cursor_pointer()
                .child(swatch)
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(theme_choice.label())
                        .e2e("span", "theme-name")
                        .text(theme_choice.label()),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.theme = theme_choice;
                    this.set_custom_shown(None, cx);
                    cx.emit(ThemeChosen(theme_choice));
                    cx.notify();
                }))
                .e2e(
                    "button",
                    if selected {
                        "theme-option selected"
                    } else {
                        "theme-option"
                    },
                )
                .attr("data-theme", theme_choice.key())
        });
        let row = div()
            .flex()
            .flex_wrap()
            .gap(px(12.0))
            .children(options)
            .e2e("div", "theme-options");
        let vim_on = crate::vim::enabled();
        let vim = choice(
            "vim-mode".into(),
            "vim-mode",
            "",
            "Vim mode",
            vim_on,
            true,
            false,
            cx.listener(|this, _, _, cx| {
                crate::vim::set(&this.data_dir, !crate::vim::enabled());
                cx.emit(super::StreamerChanged);
                cx.notify();
            }),
        );
        let keyboard = section(
            "Keyboard",
            "keyboard",
            vec![
                vim,
                muted(
                    "Modal editing as in vim. The message box starts in insert mode; Esc goes to normal mode (h j k l, w b e, 0 ^ $, gg G, x, dd, D, C, cw, ciw, yy, p, u, Ctrl+R, i a I A o O, v, counts and . work), and Enter sends from there. With no box focused, j and k move through the messages, r replies, e edits, dd deletes, + reacts, y copies, o opens the thread, / searches and i goes back to the box.",
                    "muted",
                )
                .into_any_element(),
            ],
            self.hl("keyboard"),
        );
        let mut sections = vec![
            section(
                "Theme",
                "theme",
                vec![row.into_any_element()],
                self.hl("theme"),
            )
            .into_any_element(),
            self.custom_colour_section(cx).into_any_element(),
            self.history_section(cx).into_any_element(),
            keyboard.into_any_element(),
        ];
        // Linux draws with Vulkan on the GPU picked at start (macOS: Metal's).
        if cfg!(target_os = "linux") {
            let dedicated = choice(
                "dedicated-gpu".into(),
                "dedicated-gpu",
                "",
                "Use the dedicated graphics card",
                crate::graphics::dedicated(),
                true,
                false,
                cx.listener(|this, _, _, cx| {
                    crate::graphics::set(&this.data_dir, !crate::graphics::dedicated());
                    cx.notify();
                }),
            );
            sections.push(
                section(
                    "Graphics",
                    "graphics",
                    vec![
                        dedicated,
                        muted(
                            "On a computer with two graphics cards, draw with the dedicated one (faster, uses more power) rather than the one built into the processor. Takes effect the next time Morpheus starts.",
                            "muted",
                        )
                        .into_any_element(),
                    ],
                    self.hl("graphics"),
                )
                .into_any_element(),
            );
        }
        sections
    }

    pub(super) fn voice_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        use crate::view_model::menu::DeviceKind;
        let mut children: Vec<AnyElement> = Vec::new();
        match &self.voice {
            None => children.push(muted("Looking for devices…", "muted").into_any_element()),
            Some(found) if found.microphones.is_empty() && found.speakers.is_empty() => {
                children.push(muted(vm::VOICE_NO_DEVICES, "muted").into_any_element())
            }
            Some(found) => {
                children.push(self.device_picker(
                    DeviceKind::Microphone,
                    "microphone",
                    "mic",
                    "Microphone",
                    &found.microphones,
                    self.voice_prefs.microphone.as_deref(),
                    cx,
                ));
                children.push(self.volume_slider(
                    "Input Volume",
                    "input-volume",
                    self.voice_prefs.input_volume,
                    true,
                    cx,
                ));
                children.push(self.device_picker(
                    DeviceKind::Speaker,
                    "speaker",
                    "volume-2",
                    "Speaker",
                    &found.speakers,
                    self.voice_prefs.speaker.as_deref(),
                    cx,
                ));
                children.push(self.volume_slider(
                    "Output Volume",
                    "output-volume",
                    self.voice_prefs.output_volume,
                    false,
                    cx,
                ));
                if !found.cameras.is_empty() {
                    children.push(self.device_picker(
                        DeviceKind::Camera,
                        "camera",
                        "video",
                        "Camera",
                        &found.cameras,
                        self.voice_prefs.camera.as_deref(),
                        cx,
                    ));
                }
                children.push(
                    muted(
                        "Used for calls on this device; a call going now switches over.",
                        "muted",
                    )
                    .into_any_element(),
                );
            }
        }
        let mut noise = crate::noise::controls(
            cx,
            |_: &mut Self, mode, cx| cx.emit(super::NoiseChosen(mode)),
            |_: &mut Self, on, cx| cx.emit(super::MicTest(on)),
        );
        noise.extend(crate::noise::advanced(
            cx,
            self.noise_advanced,
            |this: &mut Self, cx| {
                this.noise_advanced = !this.noise_advanced;
                cx.notify();
            },
        ));
        let video = self.video_section(cx);
        let screen_share = self.screen_share_section(cx);
        vec![
            section_classed(
                "Devices",
                "audio-devices",
                children,
                self.hl("audio-devices"),
                "voice-devices",
            )
            .into_any_element(),
            section_classed(
                "Noise suppression",
                "noise-suppression",
                noise,
                self.hl("noise-suppression"),
                "voice-noise",
            )
            .into_any_element(),
            video,
            screen_share,
        ]
    }

    /// One device list: the current choice, and the others below it when
    /// opened. Drawn by the app rather than the system.
    #[allow(clippy::too_many_arguments)]
    fn device_picker(
        &self,
        kind: crate::view_model::menu::DeviceKind,
        name: &'static str,
        glyph: &'static str,
        label: &'static str,
        devices: &[(String, String)],
        current: Option<&str>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rows = crate::view_model::calls::device_rows(
            devices,
            current.filter(|id| devices.iter().any(|(d, _)| d == id)),
        );
        let shown = rows
            .iter()
            .find(|row| row.checked)
            .map_or_else(|| "System default".to_owned(), |row| row.name.clone());
        let open = self.picker_open == Some(name);
        let current_id = current.unwrap_or_default().to_owned();
        let head = div()
            .id(SharedString::from(format!("device-current-{name}")))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .px(px(12.0))
            .py(px(8.0))
            .rounded(px(theme::RADIUS_SM))
            .bg(rgb(theme::bg_input()))
            .cursor_pointer()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.picker_open = if this.picker_open == Some(name) {
                    None
                } else {
                    Some(name)
                };
                cx.notify();
            }))
            .child(div().child(shown.clone()).e2e("span", "").text(shown))
            .child(icon("chevron-down", 16.0).text_color(rgb(theme::text_muted())))
            .e2e(
                "button",
                if open {
                    "device-current open"
                } else {
                    "device-current"
                },
            )
            .attr("data-id", current_id);
        let mut choice = div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .text_color(rgb(theme::text_muted()))
                    .child(icon(glyph, 16.0))
                    .child(label)
                    .e2e("span", "device-label")
                    .text(label),
            )
            .child(head);
        if open {
            let options = rows.iter().map(|row| {
                let id = row.id.clone();
                let selected = row.checked;
                div()
                    .id(SharedString::from(format!(
                        "device-option-{name}-{}",
                        row.id.clone().unwrap_or_default()
                    )))
                    .px(px(12.0))
                    .py(px(8.0))
                    .rounded(px(theme::RADIUS_SM))
                    .cursor_pointer()
                    .text_color(rgb(theme::text()))
                    .when(selected, |d| d.bg(rgba(theme::bg_selected())))
                    .hover(|s| s.bg(rgba(theme::bg_hover())))
                    .child(row.name.clone())
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.pick_voice(kind, id.clone(), cx)),
                    )
                    .e2e(
                        "button",
                        if selected {
                            "device-option selected"
                        } else {
                            "device-option"
                        },
                    )
                    .attr("data-id", row.id.clone().unwrap_or_default())
                    .attr("data-color", "#e6e8ee")
                    .text(row.name.clone())
            });
            choice = choice.child(
                div()
                    .flex()
                    .flex_col()
                    .p(px(4.0))
                    .rounded(px(theme::RADIUS_SM))
                    .bg(rgb(theme::bg_elevated()))
                    .children(options)
                    .e2e("div", "device-options")
                    .attr("role", "listbox")
                    .attr("data-bg", "#222736"),
            );
        }
        choice
            .e2e("div", "device-choice")
            .attr("data-kind", name)
            .into_any_element()
    }

    /// An input or output volume slider in Settings.
    fn volume_slider(
        &self,
        label: &'static str,
        class: &'static str,
        volume: f32,
        is_input: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use std::rc::Rc;
        let max = morpheus_call::MAX_VOLUME;
        let percent = (volume * 100.0).round() as u32;
        let bar: Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>> = Rc::default();
        let set_at = {
            let bar = bar.clone();
            move |x: gpui::Pixels| -> Option<f32> {
                let bounds = bar.get();
                let width = f32::from(bounds.size.width);
                (width > 0.0)
                    .then(|| (f32::from(x - bounds.origin.x) / width).clamp(0.0, 1.0) * max)
            }
        };
        let (down_set, move_set) = (set_at.clone(), set_at);

        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .w_full()
            .mt(px(2.0))
            .mb(px(8.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .text_size(px(0.85 * theme::REM))
                    .text_color(rgb(theme::text_muted()))
                    .child(label)
                    .child(
                        div()
                            .text_color(rgb(theme::text_faint()))
                            .child(format!("{percent}%"))
                            .e2e("span", "volume-percent"),
                    ),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(20.0))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            if let Some(vol) = (if event.click_count >= 2 {
                                Some(1.0)
                            } else {
                                down_set(event.position.x)
                            }) {
                                if is_input {
                                    this.voice_prefs.input_volume = vol;
                                    cx.emit(super::InputVolumeChanged(vol));
                                } else {
                                    this.voice_prefs.output_volume = vol;
                                    cx.emit(super::OutputVolumeChanged(vol));
                                }
                                cx.notify();
                            }
                        }),
                    )
                    .on_mouse_move(
                        cx.listener(move |this, event: &gpui::MouseMoveEvent, _, cx| {
                            if event.pressed_button == Some(MouseButton::Left)
                                && let Some(vol) = move_set(event.position.x)
                            {
                                if is_input {
                                    this.voice_prefs.input_volume = vol;
                                    cx.emit(super::InputVolumeChanged(vol));
                                } else {
                                    this.voice_prefs.output_volume = vol;
                                    cx.emit(super::OutputVolumeChanged(vol));
                                }
                                cx.notify();
                            }
                        }),
                    )
                    .child(
                        canvas(move |bounds, _, _| bar.set(bounds), |_, _, _, _| {})
                            .absolute()
                            .inset_0(),
                    )
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(6.0))
                            .rounded_full()
                            .bg(rgba(theme::bg_selected()))
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .left_0()
                                    .h_full()
                                    .w(gpui::relative((volume / max).clamp(0.0, 1.0)))
                                    .rounded_full()
                                    .bg(rgb(theme::accent())),
                            )
                            .child(crate::menu::slider_handle(volume / max, 3.0)),
                    )
                    .e2e("div", &format!("volume-slider {class}"))
                    .attr("data-value", percent.to_string()),
            )
            .into_any_element()
    }

    /// A device picked in Settings: kept, used by the next call (a call
    /// going now switches too, as the shell hears).
    fn pick_voice(
        &mut self,
        kind: crate::view_model::menu::DeviceKind,
        id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        use crate::view_model::menu::DeviceKind;
        self.picker_open = None;
        match kind {
            DeviceKind::Microphone => self.voice_prefs.microphone = id.clone(),
            DeviceKind::Speaker => self.voice_prefs.speaker = id.clone(),
            DeviceKind::Camera => {
                self.voice_prefs.camera = id.clone();
                morpheus_call::choose_camera(id);
            }
        }
        if let Err(err) =
            crate::view_model::calls::write_device_prefs(&self.data_dir, &self.voice_prefs)
        {
            eprintln!("morpheus-desktop: saving the audio devices: {err}");
        }
        cx.emit(super::DevicesChosen(kind));
        cx.notify();
    }

    /// Looks for the microphones, speakers and cameras the page lists.
    pub(super) fn load_voice(&mut self, cx: &mut Context<Self>) {
        self.voice = None;
        self.picker_open = None;
        self.voice_prefs = crate::view_model::calls::read_device_prefs(&self.data_dir);
        let mode = morpheus_call::AudioMode::from_env();
        let cameras: Vec<(String, String)> = morpheus_call::cameras(mode)
            .into_iter()
            .map(|c| (c.id, c.name))
            .collect();
        let finding = morpheus_call::audio_devices(mode);
        cx.spawn(async move |this, cx| {
            let found = finding.await;
            this.update(cx, |this, cx| {
                this.voice = Some(super::VoiceLists {
                    microphones: found
                        .microphones
                        .into_iter()
                        .map(|d| (d.id, d.name))
                        .collect(),
                    speakers: found.speakers.into_iter().map(|d| (d.id, d.name)).collect(),
                    cameras,
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn set_autostart(&mut self, wanted: Autostart, cx: &mut Context<Self>) {
        match vm::write_autostart(wanted) {
            Ok(()) => {
                self.autostart = vm::read_autostart();
                self.startup_note = None;
            }
            Err(err) => self.startup_note = Some(err.to_string()),
        }
        cx.notify();
    }

    pub(super) fn startup_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut children: Vec<AnyElement> = Vec::new();
        let keep = crate::tray::keeps_running();
        match self.autostart {
            None => children.push(muted(vm::STARTUP_UNAVAILABLE, "muted").into_any_element()),
            Some(state) => {
                children.push(choice(
                    "autostart".into(),
                    "autostart",
                    "",
                    "Open Morpheus",
                    state.enabled,
                    true,
                    false,
                    cx.listener(move |this, _, _, cx| {
                        this.set_autostart(
                            Autostart {
                                enabled: !state.enabled,
                                hidden: state.hidden,
                            },
                            cx,
                        )
                    }),
                ));
                children.push(choice(
                    "start-hidden".into(),
                    "start-hidden",
                    "",
                    "Start hidden in the tray",
                    state.hidden,
                    true,
                    !state.hidden_selectable_with(keep),
                    cx.listener(move |this, _, _, cx| {
                        this.set_autostart(
                            Autostart {
                                enabled: state.enabled,
                                hidden: !state.hidden,
                            },
                            cx,
                        )
                    }),
                ));
            }
        }
        if let Some(text) = &self.startup_note {
            children.push(note(text, "error", theme::danger()).into_any_element());
        }
        if !keep && self.autostart.is_some() {
            children.push(muted(vm::START_HIDDEN_NEEDS_BACKGROUND, "muted").into_any_element());
        }
        let background = section(
            "In the background",
            "background",
            vec![
                choice(
                    "keep-running".into(),
                    "keep-running",
                    "",
                    "Keep running in the background",
                    keep,
                    true,
                    false,
                    cx.listener(move |this, _, _, cx| {
                        crate::tray::set_keep_running(&this.data_dir, !keep);
                        cx.notify();
                    }),
                ),
                muted(vm::KEEP_RUNNING_HINT, "muted").into_any_element(),
            ],
            self.hl("background"),
        )
        .into_any_element();
        vec![
            background,
            section(
                "When you sign in to your computer",
                "startup",
                children,
                self.hl("startup"),
            )
            .into_any_element(),
        ]
    }

    pub(super) fn help_page(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let version = vm::version_line();
        let encryption = self.status.as_ref().and_then(|s| s.as_ref().ok()).cloned();
        let inputs = crate::diagnostics::Inputs {
            session: self.session.as_ref(),
            encryption: encryption.as_ref(),
        };
        let diagnostics = crate::diagnostics::full(window, cx, &inputs);
        if !self.diag_timer {
            self.diag_timer = true;
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(crate::diagnostics::REFRESH).await;
                    let keep = this
                        .update(cx, |this, cx| {
                            let showing = this.open && this.page == crate::view_model::settings::SettingsPage::Help;
                            if showing {
                                cx.notify();
                            } else {
                                this.diag_timer = false;
                            }
                            showing
                        })
                        .unwrap_or(false);
                    if !keep {
                        break;
                    }
                }
            })
            .detach();
        }
        let text = diagnostics.text();
        let report_text = diagnostics.text_within(2200);
        let data_dir = self.data_dir.clone();
        let report = button("report-bug", "Report a bug", Kind::Primary, "report-bug")
            .child(icon("bug", 16.0).text_color(rgb(theme::text())))
            .child("Report a bug")
            .on_click(cx.listener(move |this, _, _, cx| {
                let url = morpheus_core::bug_report_url_with(
                    env!("CARGO_PKG_VERSION"),
                    &this.data_dir,
                    &report_text,
                );
                crate::shell::open_external(&url, cx);
            }))
            .e2e("a", "button primary report-bug")
            .text("Report a bug");
        let copy = button("diag-copy", "Copy all", Kind::Secondary, "diag-copy")
            .child("Copy all")
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
            }))
            .e2e("a", "button secondary diag-copy")
            .text("Copy all");
        let folder = button("diag-open-folder", "Open folder", Kind::Secondary, "diag-open-folder")
            .child("Open folder")
            .on_click(cx.listener(move |_, _, _, cx| {
                if std::env::var_os("MORPHEUS_E2E_PORT").is_none() {
                    cx.reveal_path(&data_dir);
                }
            }))
            .e2e("a", "button secondary diag-open-folder")
            .text("Open folder");
        let mut diag_children: Vec<AnyElement> = vec![
            div()
                .flex()
                .gap(px(8.0))
                .child(copy)
                .child(folder)
                .into_any_element(),
        ];
        for section in &diagnostics.sections {
            diag_children.push(
                div()
                    .mt(px(10.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(section.title.clone())
                    .e2e("div", "diag-heading")
                    .text(section.title.clone())
                    .into_any_element(),
            );
            for (label, value) in &section.lines {
                diag_children.push(crate::diagnostics::line_element(
                    label,
                    value,
                    theme::text_muted(),
                ));
            }
        }
        vec![
            section(
                "Report a bug",
                "report-bug",
                vec![
                    muted(vm::REPORT_BUG_BLURB, "muted").into_any_element(),
                    div().flex().child(report).into_any_element(),
                ],
                self.hl("report-bug"),
            )
            .into_any_element(),
            section(
                "Version",
                "version",
                vec![
                    div()
                        .child(version.clone())
                        .e2e("p", "")
                        .text(version)
                        .into_any_element(),
                    muted(&morpheus_core::device(), "muted").into_any_element(),
                ],
                self.hl("version"),
            )
            .into_any_element(),
            section("Diagnostics", "diagnostics", diag_children, self.hl("diagnostics"))
                .into_any_element(),
        ]
    }
}

/// 36% of the swatch: the side list's share.
fn relative_36() -> gpui::DefiniteLength {
    gpui::relative(0.36)
}

impl SettingsView {
    /// Keep a change to the streamer mode choices and tell the shell.
    fn save_streamer(
        &mut self,
        settings: crate::view_model::streamer::StreamerSettings,
        cx: &mut Context<Self>,
    ) {
        if let Err(err) = settings.save(&self.data_dir) {
            eprintln!("morpheus-desktop: saving the streamer mode setting: {err}");
        }
        crate::streamer::set_settings(settings);
        cx.emit(super::StreamerChanged);
        cx.notify();
    }

    pub(super) fn streamer_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        use crate::view_model::streamer::StreamerSettings;
        let s = crate::streamer::settings();
        let switch =
            |name: &'static str, label: &str, checked: bool, change: fn(&mut StreamerSettings)| {
                choice(
                    format!("streamer-{name}"),
                    &format!("streamer-{name}"),
                    "",
                    label,
                    checked,
                    true,
                    false,
                    cx.listener(move |this, _, _, cx| {
                        let mut now = crate::streamer::settings();
                        change(&mut now);
                        this.save_streamer(now, cx);
                    }),
                )
            };
        let children = vec![
            muted(
                "Hides what could give you or others away while you share your screen, as in Discord: names are shortened to a first letter, IDs and links are dotted out, notifications say nothing.",
                "muted",
            )
            .into_any_element(),
            switch("auto", "Turn on by itself while I share my screen", s.auto, |s| s.auto = !s.auto),
            choice(
                "streamer-now".to_owned(),
                "streamer-now",
                "",
                "Turn on now",
                crate::streamer::active(),
                true,
                false,
                cx.listener(|this, _, _, cx| {
                    let next = if crate::streamer::active() {
                        crate::streamer::turn_off()
                    } else {
                        crate::streamer::turn_on()
                    };
                    this.save_streamer(next, cx);
                }),
            ),
            group(
                "What to hide",
                "",
                None,
                false,
                vec![
                    switch("names", "People's names (shortened to a letter)", s.names, |s| s.names = !s.names),
                    switch("notifications", "What notifications say", s.notifications, |s| {
                        s.notifications = !s.notifications
                    }),
                    switch("ids", "IDs, e-mail addresses, phone numbers and links", s.ids, |s| s.ids = !s.ids),
                ],
            ),
            switch("ask", "Ask before opening another channel while I share", s.ask, |s| s.ask = !s.ask),
        ];
        vec![section("Streamer mode", "streamer", children, self.hl("streamer")).into_any_element()]
    }
}

impl SettingsView {
    /// Security > Locked chats: how long an unlocked chat that is not looked
    /// at stays unlocked.
    pub(super) fn locked_chats_section(&self, cx: &mut Context<Self>) -> AnyElement {
        use crate::view_model::locks::TIMEOUT_CHOICES;
        let now = crate::locks::timeout_secs();
        let mut children = vec![
            muted(
                "A chat locked with a password (its menu, Lock with a password) opens again with the password only. Unlocked, it locks itself when you have not been looking at it for:",
                "muted",
            )
            .into_any_element(),
        ];
        for (secs, label) in TIMEOUT_CHOICES {
            children.push(choice(
                format!("lock-timeout-{secs}"),
                "lock-timeout",
                &secs.to_string(),
                label,
                now == secs,
                false,
                false,
                cx.listener(move |_, _, _, cx| {
                    crate::locks::set_timeout_secs(secs);
                    cx.notify();
                }),
            ));
        }
        section(
            "Locked chats",
            "locked-chats",
            children,
            self.hl("locked-chats"),
        )
        .into_any_element()
    }
}

// ---- Edit Profile -----------------------------------------------------------------

/// The picture in `bytes`, if it is a format GPUI draws.
fn picture_from(bytes: Vec<u8>) -> Option<std::sync::Arc<gpui::Image>> {
    let format = match image::guess_format(&bytes).ok()? {
        image::ImageFormat::Png => gpui::ImageFormat::Png,
        image::ImageFormat::Jpeg => gpui::ImageFormat::Jpeg,
        image::ImageFormat::Gif => gpui::ImageFormat::Gif,
        image::ImageFormat::WebP => gpui::ImageFormat::Webp,
        _ => return None,
    };
    Some(std::sync::Arc::new(gpui::Image::from_bytes(format, bytes)))
}

/// Side of the crop view's square frame, in pixels.
const CROP_FRAME: f32 = 280.0;

/// A picture chosen for the avatar and being cropped: what it looks like,
/// where the frame looks at it, the drag in progress and the zoom slider's
/// bounds (found when it is drawn).
pub(crate) struct Picked {
    image: std::sync::Arc<gpui::Image>,
    crop: crate::view_model::crop::Crop,
    drag: Option<gpui::Point<gpui::Pixels>>,
    bar: std::rc::Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
}

/// The chosen picture, if it is a format Morpheus can draw and cut.
fn picked_from(bytes: Vec<u8>) -> Option<Picked> {
    let (width, height) = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    let image = picture_from(bytes)?;
    Some(Picked {
        image,
        crop: crate::view_model::crop::Crop::new(width, height),
        drag: None,
        bar: Default::default(),
    })
}

/// The square part of the picture `bytes` that `rect` names, as a PNG of the size an avatar is uploaded at.
fn cropped_png(bytes: &[u8], rect: crate::view_model::crop::SourceRect) -> Result<Vec<u8>, String> {
    use crate::view_model::crop::OUTPUT_SIDE;
    let picture = image::load_from_memory(bytes).map_err(|err| err.to_string())?;
    let square = picture
        .crop_imm(rect.x, rect.y, rect.side, rect.side)
        .resize_exact(OUTPUT_SIDE, OUTPUT_SIDE, image::imageops::FilterType::Lanczos3);
    let mut out = std::io::Cursor::new(Vec::new());
    square
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|err| err.to_string())?;
    Ok(out.into_inner())
}

impl SettingsView {
    /// Puts the account's current display name in the name box.
    pub(super) fn load_own_name(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let managing = session.account_management_url();
        cx.spawn(async move |this, cx| {
            let url = managing.await.ok().flatten();
            this.update(cx, |this, cx| {
                this.manage_url = url;
                cx.notify();
            })
            .ok();
        })
        .detach();
        self.picked = None;
        self.profile_note = None;
        let profile = session.own_profile();
        cx.spawn(async move |this, cx| {
            if let Ok(profile) = profile.await {
                this.update(cx, |this, cx| {
                    let name = profile.name;
                    this.name_input
                        .update(cx, |input, cx| input.set_value(&name, cx));
                })
                .ok();
            }
        })
        .detach();
    }

    /// Runs a profile call; its note says how it went and the shell reads the profile again.
    fn run_profile(
        &mut self,
        call: impl std::future::Future<Output = Result<(), String>> + 'static,
        done: &'static str,
        cx: &mut Context<Self>,
    ) {
        if self.profile_busy {
            return;
        }
        self.profile_busy = true;
        self.profile_note = None;
        cx.spawn(async move |this, cx| {
            let result = call.await;
            this.update(cx, |this, cx| {
                this.profile_busy = false;
                match result {
                    Ok(()) => {
                        this.profile_note = Some(Ok(done.to_owned()));
                        cx.emit(super::OwnProfileChanged);
                    }
                    Err(err) => this.profile_note = Some(Err(err)),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn save_name(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let name = self.name_input.read(cx).value().to_owned();
        let call = session.set_display_name(name);
        self.run_profile(
            async move { call.await.map_err(|err| err.to_string()) },
            "Saved.",
            cx,
        );
    }

    fn pick_picture(&mut self, cx: &mut Context<Self>) {
        // Tests cannot drive the system's file dialog: they name the file.
        #[cfg(feature = "e2e")]
        if let Ok(path) = std::env::var("MORPHEUS_E2E_PICK") {
            self.picture_picked(std::path::PathBuf::from(path), cx);
            return;
        }
        let picking = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a picture".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picking.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |this, cx| this.picture_picked(path, cx))
                .ok();
        })
        .detach();
    }

    /// Shows the picked file as a preview; nothing changes until "Set as avatar".
    fn picture_picked(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        match std::fs::read(&path).ok().and_then(picked_from) {
            Some(picked) => {
                self.picked = Some(picked);
                self.profile_note = None;
            }
            None => {
                self.picked = None;
                self.profile_note = Some(Err(
                    "That file is not a picture Morpheus can show.".to_owned()
                ));
            }
        }
        cx.notify();
    }

    fn set_picture(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let Some(picked) = self.picked.take() else {
            return;
        };
        let rect = picked.crop.source_rect();
        let bytes = picked.image.bytes.clone();
        let cut = cx.background_spawn(async move { cropped_png(&bytes, rect) });
        self.run_profile(
            async move {
                let png = cut.await?;
                let path = std::env::temp_dir().join(format!(
                    "morpheus-avatar-{}-{}.png",
                    std::process::id(),
                    rect.x ^ rect.y
                ));
                std::fs::write(&path, png).map_err(|err| err.to_string())?;
                let done = session.set_avatar(path.clone()).await;
                let _ = std::fs::remove_file(&path);
                done.map_err(|err| err.to_string())
            },
            "Picture updated.",
            cx,
        );
    }

    /// The crop view: the picture in a square frame under a circular mask,
    /// dragged to position, zoomed by the wheel or the slider.
    fn crop_view(&self, picked: &Picked, cx: &mut Context<Self>) -> AnyElement {
        let frame = CROP_FRAME;
        let crop = picked.crop;
        let (left, top) = crop.offset(frame);
        let (width, height) = crop.shown_size(frame);
        let shade = div()
            .absolute()
            .left(px(-frame / 2.0))
            .top(px(-frame / 2.0))
            .size(px(frame * 2.0))
            .border(px(frame / 2.0))
            .border_color(rgba(0x000000aa))
            .rounded(px(frame));
        let ring = div()
            .absolute()
            .inset_0()
            .rounded_full()
            .border_1()
            .border_color(rgba(0xffffffcc));
        let area = div()
            .id("crop-frame")
            .relative()
            .flex_none()
            .size(px(frame))
            .overflow_hidden()
            .rounded(px(theme::RADIUS_SM))
            .bg(rgb(0x000000))
            .cursor_pointer()
            .child(
                gpui::img(crate::media::drawn(picked.image.clone()))
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(width))
                    .h(px(height))
                    .object_fit(gpui::ObjectFit::Fill),
            )
            .child(shade)
            .child(ring)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _, _| {
                    if let Some(picked) = this.picked.as_mut() {
                        picked.drag = Some(event.position);
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                let Some(picked) = this.picked.as_mut() else {
                    return;
                };
                if event.pressed_button != Some(MouseButton::Left) {
                    picked.drag = None;
                    return;
                }
                if let Some(last) = picked.drag {
                    let moved = event.position - last;
                    picked
                        .crop
                        .pan_by(f32::from(moved.x), f32::from(moved.y), CROP_FRAME);
                    picked.drag = Some(event.position);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseUpEvent, _, _| {
                    if let Some(picked) = this.picked.as_mut() {
                        picked.drag = None;
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &gpui::MouseUpEvent, _, _| {
                    if let Some(picked) = this.picked.as_mut() {
                        picked.drag = None;
                    }
                }),
            )
            .on_scroll_wheel(cx.listener(|this, event: &gpui::ScrollWheelEvent, _, cx| {
                if let Some(picked) = this.picked.as_mut() {
                    // Wheel up zooms in.
                    let up = f32::from(event.delta.pixel_delta(px(20.0)).y);
                    picked.crop.zoom_by((up * 0.005).exp());
                    cx.notify();
                }
                cx.stop_propagation();
            }))
            .e2e("div", "crop-frame")
            .attr("data-zoom", format!("{:.2}", crop.zoom()))
            .attr("data-crop", format!("{:?}", crop.source_rect()));

        let fraction = crop.zoom_fraction();
        let bar = picked.bar.clone();
        let set_at = {
            let bar = bar.clone();
            move |x: gpui::Pixels| -> Option<f32> {
                let bounds = bar.get();
                let width = f32::from(bounds.size.width);
                (width > 0.0).then(|| (f32::from(x - bounds.origin.x) / width).clamp(0.0, 1.0))
            }
        };
        let move_set = set_at.clone();
        let slider = div()
            .id("crop-zoom")
            .relative()
            .w(px(frame))
            .h(px(14.0))
            .flex()
            .items_center()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    if let (Some(picked), Some(at)) = (this.picked.as_mut(), set_at(event.position.x)) {
                        picked.crop.set_zoom_fraction(at);
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(cx.listener(move |this, event: &gpui::MouseMoveEvent, _, cx| {
                if event.pressed_button == Some(MouseButton::Left)
                    && let (Some(picked), Some(at)) = (this.picked.as_mut(), move_set(event.position.x))
                {
                    picked.crop.set_zoom_fraction(at);
                    cx.notify();
                }
            }))
            .child(
                canvas(move |bounds, _, _| bar.set(bounds), |_, _, _, _| {})
                    .absolute()
                    .inset_0(),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(4.0))
                    .rounded_full()
                    .bg(rgba(theme::bg_selected()))
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .h_full()
                            .w(gpui::relative(fraction))
                            .rounded_full()
                            .bg(rgb(theme::accent())),
                    )
                    .child(crate::menu::slider_handle(fraction, 4.0)),
            )
            .e2e("div", "crop-zoom")
            .attr("data-value", format!("{:.2}", crop.zoom()));

        div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(10.0))
            .child(muted("Drag to position, scroll or use the slider to zoom.", "muted"))
            .child(area)
            .child(slider)
            .e2e("div", "avatar-crop")
            .into_any_element()
    }

    fn remove_picture(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        self.picked = None;
        let call = session.remove_avatar();
        self.run_profile(
            async move { call.await.map_err(|err| err.to_string()) },
            "Picture removed.",
            cx,
        );
    }

    pub(super) fn edit_profile_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let busy = self.profile_busy;
        let name_box = div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(muted("Display name", "muted"))
            .child(self.name_input.clone())
            .into_any_element();
        let save = button("save-name", "Save", Kind::Primary, "")
            .when(busy, |d| d.opacity(0.6))
            .child("Save")
            .on_click(cx.listener(|this, _, _, cx| this.save_name(cx)))
            .e2e("button", "primary")
            .text("Save")
            .into_any_element();
        let note = self.profile_note.as_ref().map(|note| match note {
            Ok(text) => super::note(text, "ok", theme::ok()).into_any_element(),
            Err(text) => super::note(text, "error", theme::danger()).into_any_element(),
        });
        let preview = match &self.picked {
            Some(picked) => self.crop_view(picked, cx),
            None => div()
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(crate::media::avatar(self.own.1.clone(), &self.own.0, 72.0, None))
                .child(muted("Your current picture", "muted"))
                .e2e("div", "profile-preview")
                .into_any_element(),
        };
        let pick = button("pick-picture", "Choose picture…", Kind::Secondary, "")
            .child(icon("image", 16.0).text_color(rgb(theme::text())))
            .child("Choose picture…")
            .on_click(cx.listener(|this, _, _, cx| this.pick_picture(cx)))
            .e2e("button", "secondary")
            .text("Choose picture…");
        let set = button("set-picture", "Set as avatar", Kind::Primary, "")
            .when(busy || self.picked.is_none(), |d| d.opacity(0.5))
            .child("Set as avatar")
            .on_click(cx.listener(|this, _, _, cx| this.set_picture(cx)))
            .e2e("button", "primary")
            .text("Set as avatar");
        let remove = button("remove-picture", "Remove picture", Kind::Danger, "")
            .when(busy, |d| d.opacity(0.6))
            .child("Remove picture")
            .on_click(cx.listener(|this, _, _, cx| this.remove_picture(cx)))
            .e2e("button", "danger")
            .text("Remove picture");
        let cancel = button("cancel-picture", "Cancel", Kind::Secondary, "")
            .child("Cancel")
            .on_click(cx.listener(|this, _, _, cx| {
                this.picked = None;
                cx.notify();
            }))
            .e2e("button", "secondary")
            .text("Cancel");
        let name_section = section(
            "Display name",
            "profile-name",
            vec![name_box, save],
            self.hl("profile-name"),
        )
        .into_any_element();
        let picture_section = section(
            "Profile picture",
            "profile-picture",
            vec![
                preview,
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(pick)
                    .child(set)
                    .when(self.picked.is_some(), |d| d.child(cancel))
                    .child(remove)
                    .into_any_element(),
            ],
            self.hl("profile-picture"),
        )
        .into_any_element();
        // While a picture is being cropped it comes first, so the frame is in view.
        let sections = if self.picked.is_some() {
            [picture_section, name_section]
        } else {
            [name_section, picture_section]
        };
        sections.into_iter().chain(note).collect()
    }
}

/// One colour of the picker grid: hue across, lightness down.
fn colour_grid() -> Vec<Vec<u32>> {
    use crate::view_model::theme::from_hsl;
    [0.25, 0.4, 0.55, 0.7, 0.85]
        .into_iter()
        .map(|l| (0..12).map(|i| from_hsl(i as f32 * 30.0, 0.6, l)).collect())
        .collect()
}

impl SettingsView {
    /// One of Appearance's colour sliders (`which` 0 hue, 1 saturation, 2 value): a
    /// gradient `track`, a handle at `fraction`, dragged to change the colour live
    /// (kept on release) and reset by a double click.
    fn hsv_slider(
        &self,
        which: usize,
        name: &'static str,
        label: &'static str,
        fraction: f32,
        shown: String,
        track: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use std::rc::Rc;
        let bar: Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>> = Rc::default();
        let at = {
            let bar = bar.clone();
            move |x: gpui::Pixels| -> Option<f32> {
                let bounds = bar.get();
                let width = f32::from(bounds.size.width);
                (width > 0.0).then(|| f32::from(x - bounds.origin.x) / width)
            }
        };
        let move_at = at.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .w_full()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(0.85 * theme::REM))
                    .text_color(rgb(theme::text_muted()))
                    .child(label)
                    .child(div().text_color(rgb(theme::text_faint())).child(shown)),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(20.0))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            if event.click_count >= 2 {
                                this.drag_hsv(which, None, true, cx);
                            } else if let Some(f) = at(event.position.x) {
                                this.drag_hsv(which, Some(f), false, cx);
                            }
                        }),
                    )
                    .on_mouse_move(
                        cx.listener(move |this, event: &gpui::MouseMoveEvent, _, cx| {
                            if event.pressed_button == Some(MouseButton::Left)
                                && let Some(f) = move_at(event.position.x)
                            {
                                this.drag_hsv(which, Some(f), false, cx);
                            }
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _: &gpui::MouseUpEvent, _, cx| this.keep_custom(cx)),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(move |this, _: &gpui::MouseUpEvent, _, cx| this.keep_custom(cx)),
                    )
                    .child(
                        canvas(move |bounds, _, _| bar.set(bounds), |_, _, _, _| {})
                            .absolute()
                            .inset_0(),
                    )
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(10.0))
                            .child(track)
                            .child(crate::menu::slider_handle(fraction, 1.0)),
                    )
                    .e2e("div", &format!("hsv-slider {name}"))
                    .attr("data-value", {
                        let scale = if which == 0 { 360.0 } else { 100.0 };
                        ((fraction * scale).round() as i32).to_string()
                    }),
            )
            .into_any_element()
    }

    /// The three HSV sliders for the colour being made.
    fn hsv_sliders(&self, cx: &mut Context<Self>) -> AnyElement {
        use crate::view_model::theme::from_hsv;
        let (h, s, v) = self.hsv;
        let track = |stops: Vec<(u32, u32)>| -> AnyElement {
            div()
                .absolute()
                .inset_0()
                .flex()
                .rounded_full()
                .overflow_hidden()
                .children(stops.into_iter().map(|(from, to)| {
                    div().h_full().flex_1().bg(gpui::linear_gradient(
                        90.0,
                        gpui::linear_color_stop(rgb(from), 0.0),
                        gpui::linear_color_stop(rgb(to), 1.0),
                    ))
                }))
                .into_any_element()
        };
        let rainbow = track(
            (0..6)
                .map(|i| {
                    (
                        from_hsv(i as f32 * 60.0, 1.0, 1.0),
                        from_hsv((i + 1) as f32 * 60.0, 1.0, 1.0),
                    )
                })
                .collect(),
        );
        let saturation = track(vec![(from_hsv(h, 0.0, v), from_hsv(h, 1.0, v))]);
        let value = track(vec![(0x000000, from_hsv(h, s, 1.0))]);
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(self.hsv_slider(0, "hue", "Hue", h / 360.0, format!("{}", h.round() as i32), rainbow, cx))
            .child(self.hsv_slider(1, "sat", "Saturation", s, format!("{}%", (s * 100.0).round() as i32), saturation, cx))
            .child(self.hsv_slider(2, "val", "Value", v, format!("{}%", (v * 100.0).round() as i32), value, cx))
            .into_any_element()
    }

    /// Appearance's history: the looks used lately, to restore or pin.
    fn history_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let items = self.history.iter().map(|entry| {
            let look = entry.look;
            let (side, chat) = match look.colour {
                Some(c) => {
                    let p = crate::view_model::theme::derive(c);
                    (p.bg_sidebar, p.bg_chat)
                }
                None => theme::swatch_of(look.theme),
            };
            let key = look
                .colour
                .map(|c| format!("{c:06x}"))
                .unwrap_or_else(|| look.theme.key().to_owned());
            let swatch = div()
                .flex()
                .w(px(72.0))
                .h(px(44.0))
                .rounded(px(theme::RADIUS_SM))
                .overflow_hidden()
                .child(div().w(relative_36()).h_full().bg(rgb(side)))
                .child(div().flex_1().h_full().bg(rgb(chat)));
            let pin_label = if entry.pinned { "Pinned" } else { "Pin" };
            let pin = div()
                .id(ElementId::from(SharedString::from(format!("history-pin-{key}"))))
                .px(px(6.0))
                .rounded(px(theme::RADIUS_SM))
                .text_size(px(11.0))
                .cursor_pointer()
                .text_color(rgb(if entry.pinned {
                    theme::accent()
                } else {
                    theme::text_muted()
                }))
                .child(pin_label)
                .on_click(cx.listener(move |_, _, _, cx| {
                    cx.emit(super::HistoryPin(look));
                    cx.stop_propagation();
                }))
                .e2e("button", "history-pin")
                .text(pin_label);
            div()
                .id(ElementId::from(SharedString::from(format!("history-{key}"))))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(4.0))
                .p(px(6.0))
                .rounded(px(theme::RADIUS))
                .border_2()
                .border_color(if entry.pinned {
                    rgb(theme::accent())
                } else {
                    rgb(theme::bg_elevated())
                })
                .bg(rgb(theme::bg_elevated()))
                .cursor_pointer()
                .child(swatch)
                .child(pin)
                .on_click(cx.listener(move |_, _, _, cx| cx.emit(super::HistoryRestore(look))))
                .e2e(
                    "button",
                    if entry.pinned {
                        "history-item pinned"
                    } else {
                        "history-item"
                    },
                )
                .attr("data-colour", key)
                .attr("data-theme", look.theme.key())
        });
        let body: AnyElement = if self.history.is_empty() {
            muted("Themes and colours you use show up here.", "muted").into_any_element()
        } else {
            div()
                .flex()
                .flex_wrap()
                .gap(px(10.0))
                .children(items)
                .e2e("div", "history-items")
                .into_any_element()
        };
        section(
            "Recently used",
            "theme-history",
            vec![
                muted(
                    "The last ten themes and colours you have used, so a tweak does not lose one you liked. Click one to bring it back; pin the ones to keep.",
                    "muted",
                )
                .into_any_element(),
                body,
            ],
            false,
        )
    }

    /// Appearance's custom colour: a hex box, a grid to pick from and the preview.
    fn custom_colour_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self
            .custom
            .map(crate::view_model::theme::derive)
            .unwrap_or_else(theme::palette);
        let grid = div().flex().flex_col().gap(px(4.0)).children(colour_grid().into_iter().map(|row| {
            div().flex().gap(px(4.0)).children(row.into_iter().map(|colour| {
                div()
                    .id(ElementId::from(SharedString::from(format!("colour-{colour:06x}"))))
                    .size(px(24.0))
                    .rounded(px(theme::RADIUS_SM))
                    .bg(rgb(colour))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| this.pick_colour(colour, cx)))
                    .e2e("button", "colour-swatch")
                    .attr("data-colour", format!("{colour:06x}"))
            }))
        }));
        let picker = div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(muted(
                "One colour themes the whole app: type a hex colour or pick one. Text turns black on a light colour.",
                "muted",
            ))
            .child(div().w(px(180.0)).child(self.colour_input.clone()))
            .child(self.hsv_sliders(cx))
            .child(grid)
            .e2e("div", "custom-colour");
        section(
            "Custom colour",
            "custom-colour",
            vec![
                picker.into_any_element(),
                preview(palette).into_any_element(),
            ],
            false,
        )
    }
}

/// Sample room list and messages (one in a thread) drawn with `p`: the live preview.
fn preview(p: crate::view_model::theme::Palette) -> impl IntoElement {
    let side = div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .w(px(150.0))
        .p(px(8.0))
        .bg(rgb(p.bg_sidebar))
        .child(
            div()
                .px(px(8.0))
                .py(px(5.0))
                .rounded(px(theme::RADIUS_SM))
                .bg(rgba(p.bg_selected))
                .text_color(rgb(p.text))
                .child("# general")
                .e2e("div", "preview-room selected")
                .text("# general"),
        )
        .child(
            div()
                .px(px(8.0))
                .py(px(5.0))
                .text_color(rgb(p.text_muted))
                .child("# random")
                .e2e("div", "preview-room")
                .text("# random"),
        );
    let message = |name: &'static str, body: &'static str, class: &'static str| {
        div()
            .flex()
            .gap(px(8.0))
            .child(div().size(px(28.0)).flex_none().rounded_full().bg(rgb(p.accent)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().font_weight(FontWeight::SEMIBOLD).text_color(rgb(p.own)).child(name))
                    .child(div().text_color(rgb(p.text)).child(body)),
            )
            .e2e("div", class)
            .text(body)
    };
    let thread = div()
        .ml(px(36.0))
        .pl(px(8.0))
        .border_l_2()
        .border_color(rgb(p.accent))
        .text_color(rgb(p.accent))
        .child("2 replies in a thread")
        .e2e("div", "preview-thread")
        .text("2 replies in a thread");
    let chat = div()
        .flex()
        .flex_col()
        .flex_1()
        .gap(px(8.0))
        .p(px(10.0))
        .child(message("alice", "Morning! Shall we start?", "preview-message"))
        .child(thread)
        .child(message("you", "Yes, give me a minute.", "preview-message"))
        .child(
            div()
                .px(px(10.0))
                .py(px(6.0))
                .rounded(px(theme::RADIUS))
                .bg(rgb(p.bg_input))
                .text_color(rgb(p.text_faint))
                .child("Message #general"),
        );
    div()
        .flex()
        .mt(px(12.0))
        .rounded(px(theme::RADIUS_LG))
        .overflow_hidden()
        .bg(rgb(p.bg_chat))
        .border_1()
        .border_color(rgba(p.edge))
        .child(side)
        .child(chat)
        .e2e("div", "custom-preview")
        .attr("data-bg", format!("{:06x}", p.bg_chat))
        .attr("data-color", format!("{:06x}", p.text))
}
