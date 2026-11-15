//! A server's welcome screen: shown in the chat area the first time a server
//! is opened (and when you join one), and again from its menu's "Server
//! overview": its picture, name, topic, member count, channels (text and
//! voice) and a "Start chatting" button that opens the first text channel.
//! Which servers have been welcomed is kept in `welcomed_servers.json` in the
//! data directory (`view_model::last_place`).

use gpui::{AnyElement, Context, FontWeight, IntoElement, div, prelude::*, px, rgb};
use morpheus_core::{OwnedRoomId, RoomSummary, SpaceRoom};

use crate::{
    e2e::E2eExt as _,
    icons::icon,
    invites::card_button,
    media::avatar,
    model::Scope,
    room_list::welcome_channels,
    shell::Shell,
    theme,
    view_model::{invites as vm_invites, last_place},
};

impl Shell {
    pub fn is_welcomed(&self, space: &OwnedRoomId) -> bool {
        self.place_user.is_empty()
            || last_place::is_welcomed(&crate::data_dir(), &self.place_user, space.as_str())
    }

    /// Remembers the server as welcomed (opening it will not show the welcome).
    pub fn mark_welcomed(&self, space: &OwnedRoomId) {
        if self.place_user.is_empty() {
            return;
        }
        if let Err(err) =
            last_place::mark_welcomed(&crate::data_dir(), &self.place_user, space.as_str())
        {
            eprintln!("morpheus-desktop: welcomed servers not kept: {err}");
        }
    }

    /// Fills the chat area with the server's welcome screen.
    pub fn show_welcome(&mut self, space: OwnedRoomId, cx: &mut Context<Self>) {
        self.mark_welcomed(&space);
        self.friends_open = false;
        self.set_scope(Scope::Space(space.clone()), cx);
        self.welcome = Some(space);
        cx.notify();
    }

    /// "Start chatting": the first text channel you are in, else the first
    /// one you could join.
    fn start_chatting(&mut self, space: OwnedRoomId, cx: &mut Context<Self>) {
        let (joined, unjoined) = welcome_channels(self, &space);
        let scope = Scope::Space(space);
        if let Some(room) = joined.into_iter().find(|r| !r.is_voice) {
            self.open_room_in(scope, room.id, cx);
        } else if let Some(room) = unjoined.into_iter().find(|r| !r.is_voice) {
            self.open_unjoined(room, cx);
        } else {
            self.welcome = None;
            cx.notify();
        }
    }
}

fn row(
    cx: &mut Context<Shell>,
    key: String,
    voice: bool,
    name: &str,
    topic: Option<&str>,
    on_click: impl Fn(&mut Shell, &mut Context<Shell>) + 'static,
) -> AnyElement {
    let mut label = div().flex().flex_col().min_w_0().child(
        div()
            .font_weight(FontWeight::SEMIBOLD)
            .child(name.to_owned()),
    );
    if let Some(topic) = topic.filter(|t| !t.is_empty()) {
        label = label.child(
            div()
                .text_size(px(0.85 * theme::REM))
                .text_color(rgb(theme::text_muted()))
                .child(topic.to_owned()),
        );
    }
    div()
        .id(gpui::SharedString::from(format!("welcome-channel-{key}")))
        .flex()
        .items_center()
        .gap(px(10.0))
        .px(px(12.0))
        .py(px(8.0))
        .rounded(px(theme::RADIUS))
        .bg(rgb(theme::bg_elevated()))
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, _, cx| on_click(this, cx)))
        .child(if voice {
            icon("volume-2", 18.0)
                .text_color(rgb(theme::text_faint()))
                .into_any_element()
        } else {
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(theme::text_faint()))
                .child("#")
                .into_any_element()
        })
        .child(label)
        .e2e("div", if voice { "welcome-channel voice" } else { "welcome-channel" })
        .text(name.to_owned())
        .into_any_element()
}

/// The welcome screen of `space`, in place of the chat.
pub fn render(shell: &Shell, cx: &mut Context<Shell>, space: &RoomSummary) -> AnyElement {
    let (joined, unjoined) = welcome_channels(shell, &space.id);
    let mut channels = div().flex().flex_col().gap(px(6.0)).w_full();
    for room in joined {
        let (id, scope) = (room.id.clone(), Scope::Space(space.id.clone()));
        channels = channels.child(row(
            cx,
            room.id.to_string(),
            room.is_voice,
            &room.name,
            room.topic.as_deref(),
            move |this, cx| this.open_room_in(scope.clone(), id.clone(), cx),
        ));
    }
    for room in unjoined {
        let target: SpaceRoom = room.clone();
        channels = channels.child(row(
            cx,
            room.id.to_string(),
            room.is_voice,
            &room.name,
            room.topic.as_deref(),
            move |this, cx| this.open_unjoined(target.clone(), cx),
        ));
    }
    let id = space.id.clone();
    let start = card_button("welcome-start", "Start chatting", true)
        .on_click(cx.listener(move |this, _, _, cx| this.start_chatting(id.clone(), cx)))
        .e2e("button", "primary welcome-start")
        .text("Start chatting");
    let line = vm_invites::members_label(space.member_count);
    let mut card = div()
        .m_auto()
        .w_full()
        .max_w(px(480.0))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(10.0))
        .p(px(32.0))
        .text_center()
        .child(avatar(
            shell.picture(&space.avatar, 96),
            &space.name,
            96.0,
            None,
        ))
        .child(
            div()
                .mt(px(4.0))
                .text_size(px(1.5 * theme::REM))
                .font_weight(FontWeight::BOLD)
                .child(format!("Welcome to {}", space.name))
                .e2e("h2", "")
                .text(space.name.clone()),
        )
        .child(
            div()
                .text_color(rgb(theme::text_muted()))
                .child(line.clone())
                .e2e("p", "muted")
                .text(line),
        );
    if let Some(topic) = space.topic.as_deref().filter(|t| !t.is_empty()) {
        card = card.child(
            div()
                .text_color(rgb(theme::text_muted()))
                .child(topic.to_owned())
                .e2e("p", "topic")
                .text(topic.to_owned()),
        );
    }
    div()
        .id("server-welcome-scroll")
        .flex_1()
        .min_h_0()
        .flex()
        .overflow_y_scroll()
        .child(
            card.child(
                div()
                    .mt(px(8.0))
                    .w_full()
                    .text_left()
                    .child(channels)
                    .e2e("div", "welcome-channels"),
            )
            .child(div().mt(px(12.0)).child(start))
            .e2e("section", "server-welcome"),
        )
        .into_any_element()
}
