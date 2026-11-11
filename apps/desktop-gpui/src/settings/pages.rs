//! The settings pages that are not about keys: Notifications, Appearance,
//! Voice & Video, Startup and Help & Feedback, and what the search does
//! (`crates/ui/src/components/settings.rs`). What they offer and say is in
//! `view_model::settings`; this draws it and runs the calls.

use std::time::Duration;

use gpui::{
    AnyElement, ClickEvent, Context, ElementId, FontWeight, IntoElement, SharedString, Window,
    div, prelude::*, px, rgb,
};
use morpheus_core::NotifyMode;

use super::{SettingsView, ThemeChosen, button, muted, note, section, Kind, OK};
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
fn choice(
    id: String,
    group: &str,
    value: &str,
    label: &str,
    checked: bool,
    checkbox: bool,
    disabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let ring = rgb(if checked { theme::ACCENT } else { theme::TEXT_MUTED });
    let mark = div()
        .size(px(18.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .border_2()
        .border_color(ring)
        .map(|d| if checkbox { d.rounded(px(3.0)) } else { d.rounded_full() })
        .when(checked, |d| {
            d.child(
                div()
                    .size(px(8.0))
                    .map(|d| if checkbox { d.rounded(px(3.0)) } else { d.rounded_full() })
                    .bg(rgb(theme::ACCENT)),
            )
        })
        .e2e("input", "")
        .attr("type", if checkbox { "checkbox" } else { "radio" })
        .attr("name", group)
        .attr("value", value);
    let mark = if checked { mark.attr("checked", "true") } else { mark };
    let mark = if disabled { mark.attr("disabled", "true") } else { mark };
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
fn group(legend: &str, kind: &str, anchor: Option<&'static str>, children: Vec<AnyElement>) -> AnyElement {
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
        .e2e("fieldset", if anchor.is_some() { "notify-default notification-content" } else { "notify-default" });
    let el = if kind.is_empty() { el } else { el.attr("data-kind", kind) };
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
    pub(super) fn show_setting(&mut self, entry: &'static SettingEntry, cx: &mut Context<Self>) {
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
                div().text_color(rgb(OK)).child(*text).e2e("span", "ok").text(*text).into_any_element(),
            ),
            Some(Err(text)) => children.push(
                div()
                    .text_color(rgb(theme::DANGER))
                    .child(text.clone())
                    .e2e("span", "error")
                    .text(text.clone())
                    .into_any_element(),
            ),
            None => {}
        }
        group(title, kind, None, children)
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
                        Err(err) => eprintln!("morpheus-gpui: saving the notification setting: {err}"),
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
                            Err(err) => eprintln!("morpheus-gpui: saving what notifications show: {err}"),
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
            let selected = theme_choice == self.theme;
            let (side, chat) = match theme_choice {
                Theme::Dark => (0x141720, 0x1a1e29),
                Theme::Midnight => (0x000000, 0x0a0a0c),
                Theme::Light => (0xeef0f5, 0xffffff),
            };
            let swatch = div()
                .flex()
                .w(px(120.0))
                .h(px(72.0))
                .rounded(px(theme::RADIUS))
                .overflow_hidden()
                .child(div().w(relative_36()).h_full().bg(rgb(side)))
                .child(div().flex_1().h_full().bg(rgb(chat)));
            div()
                .id(ElementId::from(SharedString::from(format!("theme-{}", theme_choice.key()))))
                .flex()
                .flex_col()
                .gap(px(8.0))
                .p(px(8.0))
                .rounded(px(theme::RADIUS_LG))
                .border_2()
                .border_color(if selected { rgb(theme::ACCENT) } else { rgb(theme::BG_ELEVATED) })
                .bg(rgb(theme::BG_ELEVATED))
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
                    cx.emit(ThemeChosen(theme_choice));
                    cx.notify();
                }))
                .e2e(
                    "button",
                    if selected { "theme-option selected" } else { "theme-option" },
                )
                .attr("data-theme", theme_choice.key())
        });
        let row = div().flex().flex_wrap().gap(px(12.0)).children(options).e2e("div", "theme-options");
        vec![section("Theme", "theme", vec![row.into_any_element()], self.hl("theme")).into_any_element()]
    }

    pub(super) fn voice_page(&self) -> Vec<AnyElement> {
        vec![
            section(
                "Devices",
                "audio-devices",
                vec![muted(vm::VOICE_NO_DEVICES, "muted").into_any_element()],
                self.hl("audio-devices"),
            )
            .into_any_element(),
        ]
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
                        this.set_autostart(Autostart { enabled: !state.enabled, hidden: state.hidden }, cx)
                    }),
                ));
                children.push(choice(
                    "start-hidden".into(),
                    "start-hidden",
                    "",
                    "Start hidden in the tray",
                    state.hidden,
                    true,
                    !state.hidden_selectable(),
                    cx.listener(move |this, _, _, cx| {
                        this.set_autostart(Autostart { enabled: state.enabled, hidden: !state.hidden }, cx)
                    }),
                ));
            }
        }
        if let Some(text) = &self.startup_note {
            children.push(note(text, "error", theme::DANGER).into_any_element());
        }
        vec![
            section(
                "When you sign in to your computer",
                "startup",
                children,
                self.hl("startup"),
            )
            .into_any_element(),
        ]
    }

    pub(super) fn help_page(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let version = vm::version_line();
        let report = button("report-bug", "Report a bug", Kind::Primary, "report-bug")
            .child(icon("bug", 16.0).text_color(rgb(theme::TEXT)))
            .child("Report a bug")
            .on_click(cx.listener(|this, _, _, cx| {
                let url = morpheus_core::bug_report_url(env!("CARGO_PKG_VERSION"), &this.data_dir);
                cx.open_url(&url);
            }))
            .e2e("a", "button primary report-bug")
            .text("Report a bug");
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
                    div().child(version.clone()).e2e("p", "").text(version).into_any_element(),
                    muted(&morpheus_core::device(), "muted").into_any_element(),
                ],
                self.hl("version"),
            )
            .into_any_element(),
        ]
    }
}

/// 36% of the swatch: the side list's share.
fn relative_36() -> gpui::DefiniteLength {
    gpui::relative(0.36)
}
