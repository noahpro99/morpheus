//! The pill along the bottom of the rail and sidebar
//! (`crates/ui/src/components/user_bar.rs`): your avatar and name, mute,
//! deafen and the settings gear. Not ported yet: the account menu behind the
//! chip, what the buttons do (calls and settings come with their screens).

use gpui::{Context, FontWeight, IntoElement, div, linear_gradient, prelude::*, px, rgb};

use crate::{e2e::E2eExt as _, icons::icon, media::avatar, shell::Shell, theme};

fn ghost(id: &'static str, glyph: &str, title: &str, classes: &str) -> impl IntoElement {
    div()
        .id(id)
        .size(px(32.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_SM))
        .text_color(rgb(theme::TEXT_MUTED))
        .cursor_pointer()
        .hover(|s| {
            s.bg(gpui::rgba(theme::BG_HOVER))
                .text_color(rgb(theme::TEXT))
        })
        .child(icon(glyph, 18.0))
        .e2e("button", classes)
        .attr("title", title)
}

pub fn render(shell: &Shell, _cx: &mut Context<Shell>) -> impl IntoElement {
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
        .hover(|s| s.bg(gpui::rgba(theme::BG_HOVER)))
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
    let pill = div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .h(px(theme::BOTTOM_H))
        .pl(px(4.0))
        .pr(px(6.0))
        .rounded(px(theme::RADIUS_LG))
        .bg(rgb(theme::BG_ELEVATED))
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap(px(8.0))
                .child(chip),
        )
        .child(ghost(
            "mute",
            "mic",
            "Mute (Ctrl+Shift+M)",
            "ghost mute-button",
        ))
        .child(ghost(
            "deafen",
            "headphones",
            "Deafen (Ctrl+Shift+D)",
            "ghost deafen-button",
        ))
        .child(ghost(
            "settings",
            "settings",
            "Settings (Ctrl+,)",
            "ghost settings-button",
        ))
        .e2e("div", "user-pill");
    // The band keeps the rail's and the sidebar's colours; the pill floats on it.
    div()
        .w_full()
        .flex_none()
        .px(px(8.0))
        .pb(px(theme::BOTTOM_GAP))
        .bg(linear_gradient(
            90.0,
            gpui::linear_color_stop(rgb(theme::BG_RAIL), 0.0),
            gpui::linear_color_stop(rgb(theme::BG_SIDEBAR), 1.0),
        ))
        .child(pill)
        .e2e("footer", "sidebar-footer user-bar")
}
