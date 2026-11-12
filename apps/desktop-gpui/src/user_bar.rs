//! The pill along the bottom of the rail and sidebar
//! (`crates/ui/src/components/user_bar.rs`): your avatar and name, mute,
//! deafen and the settings gear. Not ported yet: the account menu behind the
//! chip, what the buttons do (calls and settings come with their screens).

use gpui::{Context, FontWeight, IntoElement, div, linear_gradient, prelude::*, px, rgb};

use crate::{
    e2e::E2eExt as _, icons::icon, media::avatar, shell::Shell, theme,
    view_model::keys::SettingsFocus,
};

fn ghost(
    id: &'static str,
    glyph: &str,
    title: &str,
    classes: &str,
    on_click: Option<Box<dyn Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App)>>,
) -> impl IntoElement {
    div()
        .id(id)
        .when_some(on_click, |d, f| d.on_click(f))
        .size(px(32.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_SM))
        .text_color(rgb(theme::text_muted()))
        .cursor_pointer()
        .hover(|s| {
            s.bg(gpui::rgba(theme::bg_hover()))
                .text_color(rgb(theme::text()))
        })
        .child(icon(glyph, 18.0).text_color(rgb(theme::text_muted())))
        .e2e("button", classes)
        .attr("title", title)
}

pub fn render(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement {
    let user_id = shell
        .session
        .as_ref()
        .and_then(|s| s.user_id())
        .map(|id| id.to_string())
        .unwrap_or_default();
    // A display name, never the raw Matrix ID.
    let (name, media) = match &shell.profile {
        Some(profile) => (profile.name.clone(), profile.avatar.clone()),
        None => (user_id.trim_start_matches('@').to_owned(), None),
    };
    let chip = div()
        .id("profile-chip")
        .flex_1()
        .min_w_0()
        .h(px(40.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(4.0))
        .rounded(px(theme::RADIUS_SM))
        .cursor_pointer()
        .hover(|s| s.bg(gpui::rgba(theme::bg_hover())))
        // Click (or right-click) on your own chip: account actions.
        .on_click(cx.listener(|this, event: &gpui::ClickEvent, _, cx| {
            this.open_account_menu(event.position(), cx)
        }))
        .on_mouse_down(
            gpui::MouseButton::Right,
            cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.open_account_menu(event.position, cx)
            }),
        )
        .child(avatar(shell.picture(&media, 34), &name, 34.0, None))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(0.85 * theme::REM))
                .child(name.clone())
                .e2e("span", "user")
                .text(name),
        )
        .e2e("button", "profile-chip")
        .attr("title", user_id);
    // The mic and its arrow are one thing: red together when muted.
    let voice = shell.calls.voice;
    let pair = |id: &'static str,
                glyph: &'static str,
                title: &'static str,
                classes: &'static str,
                active: bool,
                toggle: fn(&mut Shell, &mut Context<Shell>),
                speakers: bool,
                arrow_title: &'static str,
                cx: &mut Context<Shell>| {
        let colour = if active { theme::danger() } else { theme::text_muted() };
        let tint = |alpha: u32| gpui::rgba((theme::danger() << 8) | alpha);
        let main = div()
            .id(id)
            .size(px(32.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_l(px(theme::RADIUS_SM))
            .cursor_pointer()
            .when(active, |d| d.bg(tint(0x2e)))
            .hover(|s| if active { s.bg(tint(0x47)) } else { s.bg(gpui::rgba(theme::bg_hover())) })
            .child(icon(glyph, 18.0).text_color(rgb(colour)))
            .on_click(cx.listener(move |this, _, _, cx| toggle(this, cx)))
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.open_audio_menu(speakers, event.position, cx)
                }),
            )
            .e2e("button", classes)
            .attr("data-color", if active { "#ff6b6b" } else { "#8a90a2" })
            .attr("data-bg", if active { "#ff6b6b2e" } else { "" })
            .attr("title", title);
        let arrow = div()
            .id(gpui::SharedString::from(format!("{id}-menu")))
            .w(px(16.0))
            .h(px(32.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_r(px(theme::RADIUS_SM))
            .cursor_pointer()
            .when(active, |d| d.bg(tint(0x2e)))
            .hover(|s| if active { s.bg(tint(0x47)) } else { s.bg(gpui::rgba(theme::bg_hover())) })
            .child(icon("chevron-down", 12.0).text_color(rgb(colour)))
            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                this.open_audio_menu(speakers, event.position(), cx)
            }))
            .e2e("button", "ghost device-menu-button")
            .attr("data-color", if active { "#ff6b6b" } else { "#8a90a2" })
            .attr("data-bg", if active { "#ff6b6b2e" } else { "" })
            .attr("title", arrow_title);
        div()
            .flex()
            .gap(px(1.0))
            .child(main)
            .child(arrow)
            .e2e("div", if active { "device-pair active" } else { "device-pair" })
    };
    let mute = pair(
        "mute",
        if voice.mic_off() { "mic-off" } else { "mic" },
        voice.mute_title(),
        if voice.mic_off() { "ghost mute-button active" } else { "ghost mute-button" },
        voice.mic_off(),
        Shell::toggle_mute,
        false,
        "Microphone",
        cx,
    );
    let deafen = pair(
        "deafen",
        if voice.deafened { "headphone-off" } else { "headphones" },
        voice.deafen_title(),
        if voice.deafened { "ghost deafen-button active" } else { "ghost deafen-button" },
        voice.deafened,
        Shell::toggle_deafen,
        true,
        "Speaker",
        cx,
    );
    let row = div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .h(px(theme::BOTTOM_H))
        .pl(px(4.0))
        .pr(px(6.0))
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap(px(8.0))
                .child(chip),
        )
        .child(mute)
        .child(deafen)
        .child(ghost(
            "settings",
            "settings",
            "Settings (Ctrl+,)",
            "ghost settings-button",
            Some(Box::new(cx.listener(|this, _, _, cx| {
                this.open_settings(SettingsFocus::None, cx)
            }))),
        ))
        .e2e("div", "user-row");
    // In a call, its controls sit at the top of this card.
    let pill = div()
        .w_full()
        .flex()
        .flex_col()
        .rounded(px(theme::RADIUS_LG))
        .bg(rgb(theme::bg_elevated()))
        .children(crate::call_ui::voice_panel(shell, cx))
        .child(row)
        .e2e("div", "user-pill");
    // The band keeps the rail's and the sidebar's colours; the pill floats on it.
    div()
        .w_full()
        .flex_none()
        .flex()
        .flex_col()
        .px(px(8.0))
        .pb(px(theme::BOTTOM_GAP))
        .bg(linear_gradient(
            90.0,
            gpui::linear_color_stop(rgb(theme::bg_rail()), 0.0),
            gpui::linear_color_stop(rgb(theme::bg_sidebar()), 1.0),
        ))
        .child(pill)
        .e2e("footer", "sidebar-footer user-bar")
}

impl Shell {
    /// The footer chip's menu: your profile and its editors, the accounts on
    /// this device, sign out.
    pub fn open_account_menu(&mut self, at: gpui::Point<gpui::Pixels>, cx: &mut Context<Self>) {
        use crate::view_model::menu::{MenuTarget, account_menu};
        let Some(own) = self.session.as_ref().and_then(|s| s.user_id()) else {
            return;
        };
        let items = account_menu(&own, &self.accounts);
        self.open_menu(crate::menu::OpenMenu::new(at, items, false, MenuTarget::Other), cx);
    }

    /// Reads your own name, picture and status again (they were edited).
    pub fn reload_own_profile(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let profile = session.own_profile();
        cx.spawn(async move |this, cx| {
            if let Ok(profile) = profile.await {
                this.update(cx, |this, cx| {
                    this.profile = Some(profile);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }
}
