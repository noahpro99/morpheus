//! The settings pages that are not about keys: Notifications, Appearance,
//! Voice & Video, Startup and Help & Feedback, and what the search does
//! (`crates/ui/src/components/settings.rs`). What they offer and say is in
//! `view_model::settings`; this draws it and runs the calls.

use crate::view_model::i18n::t;
use std::time::Duration;

use gpui::{
    AnyElement, ClickEvent, Context, ElementId, FontWeight, IntoElement, MouseButton, SharedString,
    Window, canvas, div, prelude::*, px, rgb, rgba,
};
use morpheus_core::NotifyMode;

use crate::crop_view::{cropped_png, picked_from};
use super::{Kind, SettingsView, ThemeChosen, button, muted, note, section, section_classed};
use crate::{
    e2e::E2eExt as _,
    icons::icon,
    tooltip::TipExt as _,
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
    let label = t(label);
    let label = label.as_str();
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
    let legend = t(legend);
    let legend = legend.as_str();
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

    /// Shows one page.
    pub(crate) fn show_page(&mut self, page: crate::view_model::settings::SettingsPage, cx: &mut Context<Self>) {
        if page != crate::view_model::settings::SettingsPage::VoiceVideo {
            self.stop_video(cx);
        }
        self.page = page;
        cx.notify();
    }

    /// Flashes the setting `anchor` for a moment.
    pub(crate) fn flash(&mut self, anchor: &'static str, cx: &mut Context<Self>) {
        self.highlight = Some(anchor);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HIGHLIGHT).await;
            this.update(cx, |this, cx| {
                if this.highlight == Some(anchor) {
                    this.highlight = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
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
                    &t(content.label()),
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
                    theme::fill_elevated()
                })
                .bg(theme::fill_elevated())
                .cursor_pointer()
                .child(swatch)
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(t(theme_choice.label()))
                        .e2e("span", "theme-name")
                        .text(t(theme_choice.label())),
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
        }).collect::<Vec<_>>();
        let custom_tile = self.custom_tile(cx);
        let row = div()
            .flex()
            .flex_wrap()
            .gap(px(12.0))
            .children(options)
            .child(custom_tile)
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
        let spell_on = crate::spell::enabled();
        let spell = choice(
            "spell-check".into(),
            "spell-check",
            "",
            "Spell check",
            spell_on,
            true,
            false,
            cx.listener(|this, _, _, cx| {
                crate::spell::set_enabled(&this.data_dir, !crate::spell::enabled());
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
                spell,
                muted(
                    "Underlines misspelled words in the message box in red; right-click one for corrections, to add it to your dictionary or to ignore it. Checked on this computer, nothing is sent anywhere.",
                    "muted",
                )
                .into_any_element(),
            ],
            self.hl("keyboard"),
        );
        let previews_now = crate::link_previews::settings();
        let previews = choice(
            "link-previews".into(),
            "link-previews",
            "",
            "Link previews",
            previews_now.enabled,
            true,
            false,
            cx.listener(|_, _, _, cx| {
                crate::link_previews::set_enabled(!crate::link_previews::settings().enabled);
                cx.notify();
            }),
        );
        let previews_encrypted = choice(
            "link-previews-encrypted".into(),
            "link-previews-encrypted",
            "",
            "Also in encrypted rooms",
            previews_now.in_encrypted,
            true,
            false,
            cx.listener(|_, _, _, cx| {
                crate::link_previews::set_in_encrypted(!crate::link_previews::settings().in_encrypted);
                cx.notify();
            }),
        );
        let previews_section = section(
            "Link previews",
            "link-previews-section",
            vec![
                previews,
                muted(
                    "A card under a message with a link: the site, title, description and picture. They come through your homeserver, so this device never contacts the site; you can close a card with its x.",
                    "muted",
                )
                .into_any_element(),
                previews_encrypted,
                muted(
                    "In encrypted rooms the homeserver would learn the link, which end-to-end encryption otherwise keeps from it, so previews stay off there unless you turn this on.",
                    "muted",
                )
                .into_any_element(),
            ],
            self.hl("link-previews-section"),
        );
        let sections = vec![
            section(
                "Theme",
                "theme",
                vec![row.into_any_element()],
                self.hl("theme"),
            )
            .into_any_element(),
            self.custom_colour_section(cx).into_any_element(),
            self.language_section(cx).into_any_element(),
            self.glass_section(cx).into_any_element(),
            self.share_section(cx).into_any_element(),
            keyboard.into_any_element(),
            previews_section.into_any_element(),
        ];
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
        noise.push(crate::noise_model::picker(cx, |_: &mut Self, model, cx| {
            cx.emit(super::NoiseModelChosen(model))
        }));
        noise.extend(crate::noise::sensitivity(cx));
        noise.extend(crate::noise::advanced(
            cx,
            self.noise_advanced,
            |this: &mut Self, cx| {
                this.noise_advanced = !this.noise_advanced;
                cx.notify();
            },
        ));
        let video = self.video_section(cx);
        let graphics = self.graphics_section(cx);
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
            graphics,
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
            .bg(theme::fill_input())
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
                    .bg(theme::fill_elevated())
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
            .child(icon("bug", 16.0).text_color(rgb(theme::on_accent_solid())))
            .child(t("Report a bug"))
            .on_click(cx.listener(move |this, _, _, cx| {
                let url = morpheus_core::bug_report_url_with(
                    env!("CARGO_PKG_VERSION"),
                    &this.data_dir,
                    &report_text,
                );
                crate::shell::open_external(&url, cx);
            }))
            .e2e("a", "button primary report-bug")
            .text(t("Report a bug"));
        let copy = button("diag-copy", "Copy all", Kind::Secondary, "diag-copy")
            .child(icon("copy", 16.0).text_color(rgb(theme::text())))
            .child(t("Copy all"))
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
            }))
            .e2e("a", "button secondary diag-copy")
            .text(t("Copy all"));
        let folder = button("diag-open-folder", "Open folder", Kind::Secondary, "diag-open-folder")
            .child(icon("folder", 16.0).text_color(rgb(theme::text())))
            .child(t("Open folder"))
            .on_click(cx.listener(move |_, _, _, cx| {
                if std::env::var_os("MORPHEUS_E2E_PORT").is_none() {
                    cx.reveal_path(&data_dir);
                }
            }))
            .e2e("a", "button secondary diag-open-folder")
            .text(t("Open folder"));
        let mut diag_children: Vec<AnyElement> = vec![
            div()
                .flex()
                .gap(px(8.0))
                .child(copy)
                .child(folder)
                .into_any_element(),
        ];
        let diag_open = self.diag_open;
        let toggle = button("diag-toggle", "Show debug info", Kind::Secondary, "diag-toggle")
            .child(icon(if diag_open { "chevron-down" } else { "chevron-right" }, 16.0).text_color(rgb(theme::text())))
            .child(if diag_open { "Hide debug info" } else { "Show debug info" })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.diag_open = !this.diag_open;
                cx.notify();
            }))
            .e2e("button", "button secondary diag-toggle")
            .text(if diag_open { "Hide debug info" } else { "Show debug info" });
        diag_children.push(div().mt(px(10.0)).flex().child(toggle).into_any_element());
        for section in diagnostics.sections.iter().filter(|_| diag_open) {
            diag_children.push(
                div()
                    .mt(px(10.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t(&section.title))
                    .e2e("div", "diag-heading")
                    .text(t(&section.title))
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
        let notes_link = button("patch-notes-link", "View release on GitHub", Kind::Secondary, "patch-notes-link")
            .child(icon("link", 16.0).text_color(rgb(theme::text())))
            .child(t("View release on GitHub"))
            .on_click(cx.listener(|_, _, _, cx| {
                crate::shell::open_external(&crate::patch_notes::release_page(), cx);
            }))
            .e2e("a", "button secondary patch-notes-link")
            .text(t("View release on GitHub"));
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
            section(
                "Patch notes",
                "patch-notes",
                vec![
                    crate::patch_notes::help_notes(),
                    div().flex().child(notes_link).into_any_element(),
                ],
                self.hl("patch-notes"),
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
            switch(
                "remember-opened",
                "Don't ask again for channels you've already opened this stream",
                s.remember_opened,
                |s| s.remember_opened = !s.remember_opened,
            ),
        ];
        vec![section("Streamer mode", "streamer", children, self.hl("streamer")).into_any_element()]
    }
}

impl SettingsView {
    /// Security > Locked chats: how long an unlocked chat that is not looked
    /// at stays unlocked.
    pub(super) fn locked_chats_section(&self, cx: &mut Context<Self>) -> AnyElement {
        use crate::view_model::locks::{AutoLock, TIMEOUT_CHOICES, TimeUnit, describe_secs};
        let now = crate::locks::auto_lock();
        let custom = self.lock_custom_open || now.is_custom();
        let mut children = vec![
            muted(
                "A chat locked with a password (its menu, Lock with a password) opens again with the password only. Unlocked, it locks itself when you have not been looking at it for:",
                "muted",
            )
            .into_any_element(),
        ];
        let mut pick = |id: String, value: String, label: &str, checked: bool, to: Option<AutoLock>| {
            children.push(choice(
                id,
                "lock-timeout",
                &value,
                label,
                checked,
                false,
                false,
                cx.listener(move |this, _, _, cx| {
                    this.lock_custom_open = to.is_none();
                    this.lock_custom_error = None;
                    if let Some(to) = to {
                        crate::locks::set_auto_lock(to);
                    } else {
                        this.apply_lock_custom(cx);
                    }
                    cx.notify();
                }),
            ));
        };
        pick(
            "lock-timeout-now".into(),
            "now".into(),
            "Immediately (when you leave the chat)",
            !custom && now == AutoLock::Immediately,
            Some(AutoLock::Immediately),
        );
        for (secs, _) in TIMEOUT_CHOICES {
            pick(
                format!("lock-timeout-{secs}"),
                secs.to_string(),
                &describe_secs(secs),
                !custom && now == AutoLock::After(secs),
                Some(AutoLock::After(secs)),
            );
        }
        pick(
            "lock-timeout-never".into(),
            "never".into(),
            "Never (only on restart or Lock now)",
            !custom && now == AutoLock::Never,
            Some(AutoLock::Never),
        );
        let label = match now {
            AutoLock::After(secs) if now.is_custom() => crate::view_model::i18n::tf("Custom (%{time})", &[("time", &describe_secs(secs))]),
            _ => "Custom".to_owned(),
        };
        pick("lock-timeout-custom".into(), "custom".into(), &label, custom, None);
        if custom {
            let mut row = div().flex().items_center().gap(px(12.0)).child(
                div()
                    .w(px(90.0))
                    .child(self.lock_custom_input.clone())
                    .e2e("div", "lock-custom-number"),
            );
            for unit in TimeUnit::ALL {
                row = row.child(choice(
                    format!("lock-unit-{}", unit.label()),
                    "lock-unit",
                    unit.label(),
                    unit.label(),
                    self.lock_unit == unit,
                    false,
                    false,
                    cx.listener(move |this, _, _, cx| {
                        this.lock_unit = unit;
                        this.apply_lock_custom(cx);
                    }),
                ));
            }
            let mut block = div().flex().flex_col().gap(px(6.0)).child(row);
            if let Some(message) = &self.lock_custom_error {
                block = block.child(
                    div()
                        .text_color(rgb(theme::danger()))
                        .child(message.clone())
                        .e2e("div", "lock-custom-error")
                        .text(message.clone()),
                );
            }
            children.push(block.e2e("div", "lock-custom").into_any_element());
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
            filters: crate::view_model::room_dialogs::picture_filters(),
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
        let cut = cx.background_spawn(async move { cropped_png(&bytes, rect, crate::view_model::crop::OUTPUT_SIDE) });
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
            .child(t("Save"))
            .on_click(cx.listener(|this, _, _, cx| this.save_name(cx)))
            .e2e("button", "primary")
            .text(t("Save"))
            .into_any_element();
        let note = self.profile_note.as_ref().map(|note| match note {
            Ok(text) => super::note(text, "ok", theme::ok()).into_any_element(),
            Err(text) => super::note(text, "error", theme::danger()).into_any_element(),
        });
        let preview = match &self.picked {
            Some(picked) => crate::crop_view::crop_view(picked, |this: &mut SettingsView| this.picked.as_mut(), cx),
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
            .child(t("Choose picture…"))
            .on_click(cx.listener(|this, _, _, cx| this.pick_picture(cx)))
            .e2e("button", "secondary")
            .text(t("Choose picture…"));
        let set = button("set-picture", "Set as avatar", Kind::Primary, "")
            .when(busy || self.picked.is_none(), |d| d.opacity(0.5))
            .child(t("Set as avatar"))
            .on_click(cx.listener(|this, _, _, cx| this.set_picture(cx)))
            .e2e("button", "primary")
            .text(t("Set as avatar"));
        let remove = button("remove-picture", "Remove picture", Kind::Danger, "")
            .when(busy, |d| d.opacity(0.6))
            .child(t("Remove picture"))
            .on_click(cx.listener(|this, _, _, cx| this.remove_picture(cx)))
            .e2e("button", "danger")
            .text(t("Remove picture"));
        let cancel = button("cancel-picture", "Cancel", Kind::Secondary, "")
            .child(icon("x", 16.0).text_color(rgb(theme::text())))
                                        .child(t("Cancel"))
            .on_click(cx.listener(|this, _, _, cx| {
                this.picked = None;
                cx.notify();
            }))
            .e2e("button", "secondary")
            .text(t("Cancel"));
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
        let activity = self.activity_sections(cx);
        sections.into_iter().chain(note).chain(activity).collect()
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
        let value = track(vec![(0x000000, from_hsv(h, s, 1.0))]); // fixed: the value track starts at black
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(self.hsv_slider(0, "hue", "Hue", h / 360.0, format!("{}", h.round() as i32), rainbow, cx))
            .child(self.hsv_slider(1, "sat", "Saturation", s, format!("{}%", (s * 100.0).round() as i32), saturation, cx))
            .child(self.hsv_slider(2, "val", "Value", v, format!("{}%", (v * 100.0).round() as i32), value, cx))
            .into_any_element()
    }

    /// The colour menu's recent colours: the looks used lately, to restore or pin.
    fn history_row(&self, cx: &mut Context<Self>) -> AnyElement {
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
                    theme::fill_elevated()
                })
                .bg(theme::fill_elevated())
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
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(muted("Recent colours: click one to bring it back; pin the ones to keep.", "muted"))
            .child(body)
            .e2e("div", "theme-history")
            .into_any_element()
    }

    /// Appearance's language: the system's (the default) or one of ours, kept on this
    /// device and shown at once.
    fn language_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::view_model::i18n::{self, Choice};
        let system_now = i18n::from_system(&i18n::system_languages());
        let system_name = i18n::LANGUAGES
            .iter()
            .find(|l| l.code == system_now)
            .map(|l| l.name)
            .unwrap_or("English");
        let mut options: Vec<(Choice, String)> = vec![(
            Choice::System,
            format!("{} ({system_name})", t("System default")),
        )];
        options.extend(i18n::LANGUAGES.iter().map(|l| (Choice::Fixed(l.code), l.name.to_owned())));
        let choices: Vec<AnyElement> = options
            .into_iter()
            .map(|(option, label)| {
                // The language's own name is never translated: it is what its speakers look for.
                choice(
                    format!("language-{}", option.key()),
                    "language",
                    option.key(),
                    &label,
                    self.lang == option,
                    false,
                    false,
                    cx.listener(move |this, _, _, cx| {
                        this.lang = option;
                        if let Err(err) = i18n::save_choice(&this.data_dir, option) {
                            eprintln!("morpheus-desktop: saving the language: {err}");
                        }
                        i18n::set_language(option.resolve(&i18n::system_languages()));
                        cx.refresh_windows();
                        cx.notify();
                    }),
                )
            })
            .collect();
        section(
            "Language",
            "language",
            vec![
                muted(
                    "The language Morpheus is shown in. Text that is not translated yet stays in English.",
                    "muted",
                )
                .into_any_element(),
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(16.0))
                    .children(choices)
                    .e2e("div", "language-options")
                    .into_any_element(),
            ],
            self.hl("language"),
        )
    }

    /// Appearance's Transparent setting: a see-through window; only the window and panel
    /// backgrounds let the desktop show, by a slider whose lowest end is where the theme's
    /// text still reads over any backdrop.
    fn glass_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::view_model::glass::{self, Glass};
        use std::rc::Rc;
        let support = theme::support();
        let palette = self.custom.map(crate::view_model::theme::derive).unwrap_or_else(|| {
            crate::view_model::theme::Palette::of(self.theme)
        });
        let current = self.glass;
        let supported = support != glass::Support::None;
        let on = choice(
            "glass-on".into(),
            "glass",
            "on",
            "Transparent",
            current.on,
            true,
            !supported,
            cx.listener(move |this, _, _, cx| {
                this.glass.on = !this.glass.on;
                cx.emit(super::GlassChosen(this.glass));
                cx.notify();
            }),
        );
        let floor = glass::floor(&palette);
        let drawn = current.opacity_for(&palette);
        let readable = glass::readable_floor(&palette);
        let enabled = current.on && supported;
        let bar: Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>> = Rc::default();
        let at = {
            let bar = bar.clone();
            move |x: gpui::Pixels| -> Option<u8> {
                let bounds = bar.get();
                let width = f32::from(bounds.size.width);
                (width > 0.0).then(|| Glass::opacity_at(f32::from(x - bounds.origin.x) / width, floor))
            }
        };
        let move_at = at.clone();
        let slider = div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .w_full()
            .when(!enabled, |d| d.opacity(0.5))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(0.85 * theme::REM))
                    .text_color(rgb(theme::text_muted()))
                    .child(crate::view_model::i18n::t("Opacity"))
                    .child(div().text_color(rgb(theme::text_faint())).child(format!("{drawn}%"))),
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
                            if this.glass.on
                                && let Some(percent) = at(event.position.x)
                            {
                                this.glass.opacity = percent;
                                cx.emit(super::GlassChosen(this.glass));
                                cx.notify();
                            }
                        }),
                    )
                    .on_mouse_move(cx.listener(move |this, event: &gpui::MouseMoveEvent, _, cx| {
                        if event.pressed_button == Some(MouseButton::Left)
                            && this.glass.on
                            && let Some(percent) = move_at(event.position.x)
                            && percent != this.glass.opacity
                        {
                            this.glass.opacity = percent;
                            cx.emit(super::GlassChosen(this.glass));
                            cx.notify();
                        }
                    }))
                    .child(canvas(move |bounds, _, _| bar.set(bounds), |_, _, _, _| {}).absolute().inset_0())
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
                                    .w(gpui::relative(Glass::fraction_of(drawn, floor)))
                                    .rounded_full()
                                    .bg(rgb(theme::accent())),
                            )
                            .child(crate::menu::slider_handle(Glass::fraction_of(drawn, floor), 3.0)),
                    )
                    .e2e("div", "glass-slider")
                    .attr("data-value", drawn.to_string())
                    .attr("data-min", floor.to_string()),
            );
        let mut body = vec![
            muted(
                "Makes the window and its panels see-through. Text, icons, buttons, menus and pictures stay solid.",
                "muted",
            )
            .into_any_element(),
            on,
            slider.into_any_element(),
            div().child(muted(support.note(), "muted")).e2e("div", "glass-support").attr(
                "data-support",
                match support {
                    glass::Support::Blur => "blur",
                    glass::Support::Translucent => "translucent",
                    glass::Support::None => "none",
                },
            )
            .into_any_element(),
        ];
        if current.on && supported && drawn < readable {
            body.push(
                muted(
                    "Text can be hard to read over a bright window behind this one at this opacity.",
                    "glass-readable",
                )
                .into_any_element(),
            );
        }
        body.shrink_to_fit();
        section("Transparent", "glass", body, self.hl("glass"))
    }

    /// The look as it is now, as a shareable code.
    fn shared_now(&self) -> crate::view_model::theme_share::SharedTheme {
        crate::view_model::theme_share::SharedTheme {
            theme: self.theme,
            colour: self.custom,
            glass: self.glass,
        }
    }

    /// What was typed or pasted into the import box, read: a preview or the reason not.
    pub(super) fn read_import(&mut self, cx: &mut Context<Self>) {
        let typed = self.import_input.read(cx).value().to_owned();
        self.import = (!typed.trim().is_empty())
            .then(|| crate::view_model::theme_share::SharedTheme::decode(&typed));
        self.share_note = None;
        cx.notify();
    }

    /// Shows `text` (a code from a message) in the import box with its preview.
    pub fn show_theme_code(&mut self, text: &str, cx: &mut Context<Self>) {
        let code = crate::view_model::theme_share::find_in_message(text)
            .map(|t| t.encode())
            .unwrap_or_else(|| text.to_owned());
        self.import_input.update(cx, |input, cx| input.set_value(&code, cx));
        self.read_import(cx);
        self.redraw_inputs(cx);
    }

    fn cancel_import(&mut self, cx: &mut Context<Self>) {
        self.import_input.update(cx, |input, cx| input.set_value("", cx));
        self.import = None;
        self.redraw_inputs(cx);
        cx.notify();
    }

    fn apply_import(&mut self, cx: &mut Context<Self>) {
        if let Some(Ok(shared)) = self.import.take() {
            self.import_input.update(cx, |input, cx| input.set_value("", cx));
            self.redraw_inputs(cx);
            cx.emit(super::ThemeImported(shared));
            cx.notify();
        }
    }

    fn open_theme_file(&mut self, cx: &mut Context<Self>) {
        // Tests cannot drive the system's file dialog: they name the file.
        #[cfg(feature = "e2e")]
        if let Ok(path) = std::env::var("MORPHEUS_E2E_PICK") {
            self.theme_file_picked(std::path::PathBuf::from(path), cx);
            return;
        }
        let picking = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open a theme".into()),
            filters: crate::view_model::room_dialogs::theme_filters(),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picking.await else {
                return;
            };
            if let Some(path) = paths.into_iter().next() {
                this.update(cx, |this, cx| this.theme_file_picked(path, cx)).ok();
            }
        })
        .detach();
    }

    fn theme_file_picked(&mut self, path: std::path::PathBuf, cx: &mut Context<Self>) {
        use crate::view_model::theme_share::{ThemeCodeError, from_file_text};
        // A theme file is a line of text: a huge file is not one.
        let read = match std::fs::metadata(&path) {
            Ok(meta) if meta.len() <= 4096 => std::fs::read_to_string(&path).ok(),
            _ => None,
        };
        let parsed = match read {
            Some(text) => from_file_text(&text),
            None => Err(ThemeCodeError::NotATheme),
        };
        let code = parsed.as_ref().map(|t| t.encode()).unwrap_or_default();
        self.import_input.update(cx, |input, cx| input.set_value(&code, cx));
        self.import = Some(parsed);
        self.share_note = None;
        self.redraw_inputs(cx);
        cx.notify();
    }

    fn save_theme_file(&mut self, cx: &mut Context<Self>) {
        let text = crate::view_model::theme_share::file_text(&self.shared_now());
        let name = format!("my-theme.{}", crate::view_model::theme_share::FILE_EXTENSION);
        #[cfg(feature = "e2e")]
        if let Ok(path) = std::env::var("MORPHEUS_E2E_SAVE") {
            self.theme_file_saved(std::fs::write(path, text).map_err(|e| e.to_string()), cx);
            return;
        }
        let dir = dirs_home();
        let picking = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = picking.await else {
                return;
            };
            let result = std::fs::write(path, text).map_err(|e| e.to_string());
            this.update(cx, |this, cx| this.theme_file_saved(result, cx)).ok();
        })
        .detach();
    }

    fn theme_file_saved(&mut self, result: Result<(), String>, cx: &mut Context<Self>) {
        self.share_note = Some(match result {
            Ok(()) => crate::view_model::i18n::t("Saved. Anyone can open it from Settings > Appearance."),
            Err(err) => crate::view_model::i18n::tf("Could not save the file: %{err}", &[("err", &err.to_string())]),
        });
        cx.notify();
    }

    /// Appearance's sharing: the look as a code to copy or save, a box to paste one, a
    /// button to open a file, and the preview of what was read, applied only on request.
    fn share_section(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let code = self.shared_now().encode();
        let copied = code.clone();
        let code_row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(10.0))
            .child(
                div()
                    .font_family(theme::MONO)
                    .px(px(10.0))
                    .py(px(6.0))
                    .rounded(px(theme::RADIUS_SM))
                    .bg(theme::fill_input())
                    .child(code.clone())
                    .e2e("code", "theme-code")
                    .text(&code),
            )
            .child(
                button("theme-copy", "Copy", Kind::Secondary, "")
                    .child(icon("copy", 16.0).text_color(rgb(theme::text())))
                    .child(t("Copy"))
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(copied.clone()))
                    })
                    .e2e("button", "secondary theme-copy")
                    .text(t("Copy")),
            )
            .child(
                button("theme-save", "Save to file", Kind::Secondary, "")
                    .child(icon("download", 16.0).text_color(rgb(theme::text())))
                    .child(t("Save to file"))
                    .on_click(cx.listener(|this, _, _, cx| this.save_theme_file(cx)))
                    .e2e("button", "secondary theme-save")
                    .text(t("Save to file")),
            );
        let import_row = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(10.0))
            .child(div().w(px(320.0)).child(self.import_input.clone()))
            .child(
                button("theme-open", "Open file", Kind::Secondary, "")
                    .child(icon("folder-open", 16.0).text_color(rgb(theme::text())))
                    .child(t("Open file"))
                    .on_click(cx.listener(|this, _, _, cx| this.open_theme_file(cx)))
                    .e2e("button", "secondary theme-open")
                    .text(t("Open file")),
            )
            .e2e("div", "theme-import");
        let mut children = vec![
            muted(
                "Your theme as a code (the theme, custom colour and glass). Send it to a friend in a message or a file; they can paste it below, or choose Apply this theme on the message.",
                "muted",
            )
            .into_any_element(),
            code_row.into_any_element(),
            import_row.into_any_element(),
        ];
        if let Some(note) = &self.share_note {
            children.push(muted(note, "muted theme-note").into_any_element());
        }
        match &self.import {
            None => {}
            Some(Err(err)) => children.push(
                note(&err.message(), "theme-import-error", theme::danger()).into_any_element(),
            ),
            Some(Ok(shared)) => {
                let p = shared.palette();
                let summary = shared.summary();
                children.push(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .child(muted(&crate::view_model::i18n::tf("Preview: %{summary}. Nothing changes until you apply it.", &[("summary", &summary.to_string())]), "muted"))
                        .child(
                            div()
                                .child(preview(p))
                                .e2e("div", "theme-import-preview")
                                .attr("data-bg", format!("{:06x}", p.bg_chat)),
                        )
                        .child(
                            div()
                                .flex()
                                .gap(px(10.0))
                                .child(
                                    button("theme-apply", "Apply", Kind::Primary, "")
                                        .child(icon("check", 16.0).text_color(rgb(theme::on_accent_solid())))
                                        .child(t("Apply"))
                                        .on_click(cx.listener(|this, _, _, cx| this.apply_import(cx)))
                                        .e2e("button", "primary theme-apply")
                                        .text(t("Apply")),
                                )
                                .child(
                                    button("theme-cancel", "Cancel", Kind::Secondary, "")
                                        .child(t("Cancel"))
                                        .on_click(cx.listener(|this, _, _, cx| this.cancel_import(cx)))
                                        .e2e("button", "secondary theme-cancel")
                                        .text(t("Cancel")),
                                ),
                        )
                        .into_any_element(),
                );
            }
        }
        section("Share a theme", "theme-share", children, self.hl("theme-share"))
    }

    /// The Custom tile (#354): same size as the theme tiles, with a large palette icon. It opens
    /// the colour menu under it and is selected while a custom colour is in use.
    fn custom_tile(&self, cx: &mut Context<Self>) -> AnyElement {
        use crate::view_model::theme::on_colour;
        let selected = self.custom.is_some();
        let fill = self.custom.map(crate::view_model::theme::derive).map(|p| p.bg_sidebar);
        let ink = fill.map(on_colour).unwrap_or_else(theme::text_muted);
        let swatch = div()
            .flex()
            .items_center()
            .justify_center()
            .w(px(120.0))
            .h(px(72.0))
            .rounded(px(theme::RADIUS))
            .bg(match fill {
                Some(c) => rgb(c).into(),
                None => theme::fill_elevated(),
            })
            .border_1()
            .border_color(rgba(theme::edge()))
            .child(
                div()
                    .id("theme-custom-icon")
                    .child(icon("palette", 40.0).text_color(rgb(ink)))
                    .tip(super::colour_field::TIP)
                    .e2e("div", "custom-palette-icon")
                    .attr("data-tip", super::colour_field::TIP),
            );
        let tile = div()
            .id("theme-custom")
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(8.0))
            .rounded(px(theme::RADIUS_LG))
            .border_2()
            .border_color(if selected { rgb(theme::accent()) } else { theme::fill_elevated() })
            .bg(theme::fill_elevated())
            .cursor_pointer()
            .child(swatch)
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t("Custom"))
                    .e2e("span", "theme-name")
                    .text(t("Custom")),
            )
            .on_click(cx.listener(|this, event: &gpui::ClickEvent, _, cx| {
                // The press that closed the menu (outside it, on this tile) is this click's
                // own and must not open it again. Any later click is a fresh one (#359: a
                // 250 ms window swallowed real clicks).
                let closed_by_this = this.colour_closed_at.take().is_some_and(|at| {
                    matches!(event, gpui::ClickEvent::Mouse(click) if click.up.position == at)
                });
                let open = !this.colour_open && !closed_by_this;
                this.set_colour_open(open, cx);
            }))
            .e2e(
                "button",
                if selected {
                    "theme-option custom colour-field-button selected"
                } else {
                    "theme-option custom colour-field-button"
                },
            )
            .attr("data-theme", "custom");
        let popover = self.colour_open.then(|| {
            let grid = self.colour_grid_view(cx);
            let content = div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(div().w(px(180.0)).child(self.colour_input.clone()))
                .child(self.hsv_sliders(cx))
                .child(grid)
                .child(self.history_row(cx))
                .into_any_element();
            let this = cx.entity().downgrade();
            let closer = this.clone();
            super::colour_field::popover(
                content,
                &self.colour_focus,
                self.colour_pressed_outside.clone(),
                move |_, cx| {
                    this.update(cx, |this, cx| this.set_colour_open(false, cx)).ok();
                },
                move |event, _, cx| {
                    closer
                        .update(cx, |this, cx| {
                            this.colour_closed_at = Some(event.position);
                            this.set_colour_open(false, cx);
                        })
                        .ok();
                },
            )
        });
        div().relative().child(tile).children(popover).into_any_element()
    }

    /// The picker's grid of colours.
    fn colour_grid_view(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .children(colour_grid().into_iter().map(|row| {
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
            }))
            .into_any_element()
    }

    /// Appearance's custom colour: a hex box, a grid to pick from and the preview.
    fn custom_colour_section(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        let palette = self
            .custom
            .map(crate::view_model::theme::derive)
            .unwrap_or_else(theme::palette);
        section(
            "Preview",
            "theme-preview",
            vec![
                muted(
                    "Custom in the theme row sets one colour for the whole app: type a hex colour or pick one. Text turns black on a light colour.",
                    "muted",
                )
                .into_any_element(),
                preview(palette).into_any_element(),
            ],
            self.hl("theme-preview"),
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
                .child(t("Message #general")),
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

/// Where a save dialog opens.
fn dirs_home() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}
