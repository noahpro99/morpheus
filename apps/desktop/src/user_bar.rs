//! The pill along the bottom of the rail an sidebar
//! (`crates/ui/src/components/user_bar.rs`): your avatar and name, mute,
//! deafen and the settings gear. Not ported yet: the account menu behind the
//! chip, what the buttons do (calls and settings come with their screens).

use crate::view_model::i18n::ts;
use gpui::{Context, FontWeight, IntoElement, div, prelude::*, px, rgb};

use crate::tooltip::TipExt as _;
use crate::{
    e2e::E2eExt as _, icons::icon, media::avatar, shell::Shell, theme,
    view_model::keys::SettingsFocus,
};

/// The user pill's row is a rounded rectangle: the profile chip and the buttons are inset evenly in it (#505).
const CHIP_H: f32 = 40.0;
const GHOST: f32 = 32.0;
/// Each is inset evenly in the 50 px row, so its corner is the pill's less that inset.
const CHIP_R: f32 = theme::RADIUS_LG - (theme::BOTTOM_H - CHIP_H) / 2.0;
const GHOST_R: f32 = theme::RADIUS_LG - (theme::BOTTOM_H - GHOST) / 2.0;

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
        .size(px(GHOST))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(GHOST_R))
        .text_color(rgb(theme::text_muted()))
        .cursor_pointer()
        .hover(|s| {
            s.bg(gpui::rgba(theme::bg_hover()))
                .text_color(rgb(theme::text()))
        })
        .child(icon(glyph, 18.0).text_color(rgb(theme::text_muted())))
        .tip(title.to_owned())
        .e2e("button", classes)
        .attr("title", title)
        .attr("radius", theme::radius_attr(GHOST_R))
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
    let picture = shell.picture(&media, 34);
    if !user_id.is_empty() && (media.is_none() || picture.is_some()) {
        looks::remember(&user_id, theme::accent_solid(), picture.as_ref());
    }
    let (name, user_id) = (crate::streamer::name(&name), crate::streamer::id(&user_id));
    let chip = div()
        .id("profile-chip")
        .flex_1()
        .min_w_0()
        .h(px(CHIP_H))
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(theme::inset_in(CHIP_H, 34.0)))
        .rounded(px(CHIP_R))
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
        .child(avatar(picture, &name, 34.0, None))
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
        .attr("title", user_id)
        .attr("radius", theme::radius_attr(CHIP_R));
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
        let colour = if active {
            theme::danger()
        } else {
            theme::text_muted()
        };
        let tint = |alpha: u32| gpui::rgba((theme::danger() << 8) | alpha);
        let main = div()
            .id(id)
            .size(px(32.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_l(px(GHOST_R))
            .cursor_pointer()
            .when(active, |d| d.bg(tint(0x2e)))
            .hover(|s| {
                if active {
                    s.bg(tint(0x47))
                } else {
                    s.bg(gpui::rgba(theme::bg_hover()))
                }
            })
            .child(icon(glyph, 18.0).text_color(rgb(colour)))
            .on_click(cx.listener(move |this, _, _, cx| toggle(this, cx)))
            .on_mouse_down(
                gpui::MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.open_audio_menu(speakers, false, event.position, cx)
                }),
            )
            .tip(title.to_owned())
            .e2e("button", classes)
            .attr("data-color", if active { "#ff6b6b" } else { "#8a90a2" })
            .attr("data-bg", if active { "#ff6b6b2e" } else { "" })
            .attr("title", title);
        let arrow = div()
            .id(gpui::SharedString::from(format!("{id}-menu")))
            .w(px(16.0))
            .h(px(GHOST))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_r(px(GHOST_R))
            .cursor_pointer()
            .when(active, |d| d.bg(tint(0x2e)))
            .hover(|s| {
                if active {
                    s.bg(tint(0x47))
                } else {
                    s.bg(gpui::rgba(theme::bg_hover()))
                }
            })
            .child(icon("chevron-down", 12.0).text_color(rgb(colour)))
            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                this.open_audio_menu(speakers, true, event.position(), cx)
            }))
            .tip(arrow_title.to_owned())
            .e2e("button", "ghost device-menu-button")
            .attr("data-color", if active { "#ff6b6b" } else { "#8a90a2" })
            .attr("data-bg", if active { "#ff6b6b2e" } else { "" })
            .attr("title", arrow_title);
        div().flex().gap(px(1.0)).child(main).child(arrow).e2e(
            "div",
            if active {
                "device-pair active"
            } else {
                "device-pair"
            },
        )
    };
    let mute = pair(
        "mute",
        if voice.mic_off() { "mic-off" } else { "mic" },
        voice.mute_title(),
        if voice.mic_off() {
            "ghost mute-button active"
        } else {
            "ghost mute-button"
        },
        voice.mic_off(),
        Shell::toggle_mute,
        false,
        ts("Microphone"),
        cx,
    );
    let deafen = pair(
        "deafen",
        if voice.deafened {
            "headphone-off"
        } else {
            "headphones"
        },
        voice.deafen_title(),
        if voice.deafened {
            "ghost deafen-button active"
        } else {
            "ghost deafen-button"
        },
        voice.deafened,
        Shell::toggle_deafen,
        true,
        ts("Speaker"),
        cx,
    );
    let row = div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .h(px(theme::BOTTOM_H))
        .pl(px(theme::inset_in(theme::BOTTOM_H, CHIP_H)))
        .pr(px(theme::inset_in(theme::BOTTOM_H, GHOST)))
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
            ts("Settings (Ctrl+,)"),
            "ghost settings-button",
            Some(Box::new(cx.listener(|this, _, _, cx| {
                this.open_settings(SettingsFocus::None, cx)
            }))),
        ))
        .e2e("div", "user-row");
    // In a call, its controls sit at the top of this card.
    let join_error = crate::call_ui::join_error(shell, cx);
    let voice_panel = crate::call_ui::voice_panel(shell, cx);
    let pill = div()
        .w_full()
        .flex()
        .flex_col()
        .rounded(px(theme::BAR_R))
        .bg(theme::fill_card())
        .children(join_error)
        .children(voice_panel)
        .child(row)
        .e2e("div", "user-pill")
        .attr("radius", theme::radius_attr(theme::BAR_R));
    // The band is the rail's fill under the rail and the sidebar's under the list: the
    // same flat fills as the columns above, so a see-through window shows no seam or
    // extra layer; the pill floats on it.
    div()
        .w_full()
        .flex_none()
        .relative()
        .flex()
        .flex_col()
        .px(px(8.0))
        .pb(px(theme::BOTTOM_GAP))
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .child(div().w(px(theme::RAIL_W)).h_full().flex_none().bg(theme::fill_rail()))
                .child(div().h_full().flex_1().bg(theme::fill_sidebar())),
        )
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
        let mut items = account_menu(&own, &self.accounts);
        // Each account on its picture and the colour of its theme.
        let mut pictures = std::collections::HashMap::new();
        for item in items.iter_mut().flat_map(|i| i.children.iter_mut()) {
            let Some(crate::view_model::menu::MenuAction::SwitchAccount(key)) = &item.action else {
                continue;
            };
            let Some(account) = self.accounts.iter().find(|a| &a.key == key) else {
                continue;
            };
            let look = looks::read(account.user_id.as_str());
            if let Some(accent) = look.accent {
                item.fill = Some(accent);
            }
            if let Some(picture) = look.picture {
                pictures.insert(key.clone(), picture);
            }
        }
        let mut menu = crate::menu::OpenMenu::new(at, items, false, MenuTarget::Other);
        menu.account_pictures = pictures;
        self.open_menu(menu, cx);
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

/// What an account looks like in the account switcher: its picture and the
/// colour of its theme, remembered on this device as each account is used, so
/// the others show at once (#275).
pub mod looks {
    use std::sync::{Arc, Mutex};

    use base64::Engine as _;

    #[derive(Clone, Default)]
    pub struct Look {
        pub accent: Option<u32>,
        pub picture: Option<Arc<gpui::Image>>,
    }

    fn file(user: &str) -> std::path::PathBuf {
        let stem: String = user.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
        crate::data_dir().join("account-looks").join(format!("{stem}.json"))
    }

    pub fn read(user: &str) -> Look {
        let Ok(text) = std::fs::read_to_string(file(user)) else {
            return Look::default();
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
            return Look::default();
        };
        let picture = (|| {
            let format = gpui::ImageFormat::from_mime_type(json["mime"].as_str()?)?;
            let bytes = base64::engine::general_purpose::STANDARD.decode(json["picture"].as_str()?).ok()?;
            Some(Arc::new(gpui::Image::from_bytes(format, bytes)))
        })();
        Look { accent: json["accent"].as_u64().map(|a| a as u32), picture }
    }

    /// Remember the signed-in account's look (written when it changes).
    pub fn remember(user: &str, accent: u32, picture: Option<&Arc<gpui::Image>>) {
        static LAST: Mutex<Option<(String, u32, usize)>> = Mutex::new(None);
        let seen = (user.to_owned(), accent, picture.map_or(0, |p| Arc::as_ptr(p) as usize));
        let Ok(mut last) = LAST.lock() else {
            return;
        };
        if last.as_ref() == Some(&seen) {
            return;
        }
        *last = Some(seen);
        let path = file(user);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut json = serde_json::json!({ "accent": accent });
        if let Some(picture) = picture {
            json["mime"] = picture.format.mime_type().into();
            json["picture"] = base64::engine::general_purpose::STANDARD.encode(&picture.bytes).into();
        }
        let _ = std::fs::write(path, json.to_string());
    }
}
