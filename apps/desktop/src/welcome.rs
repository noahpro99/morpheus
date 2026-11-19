//! A server's welcome screen: shown in the chat area the first time a server
//! is opened (and when you join one), and again from its menu's "Server
//! overview": its picture, name, topic, member count, channels (text and
//! voice) and a "Start chatting" button that opens the first text channel.
//! Which servers have been welcomed is kept in `welcomed_servers.json` in the
//! data directory (`view_model::last_place`).

use gpui::{AnyElement, Context, FontWeight, IntoElement, div, prelude::*, px, rgb};
use morpheus_core::{OwnedRoomId, RoomSummary, SpaceRoom};

use crate::tooltip::TipExt as _;
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

/// A channel being joined from the overview.
#[derive(Clone, Debug, PartialEq)]
pub enum JoinState {
    Joining,
    Joined,
    Failed(String),
}

impl Shell {
    /// Joins a channel from the overview, in place: the overview stays and the row
    /// shows the progress, then "Joined" (or why it could not).
    fn join_from_welcome(&mut self, room: SpaceRoom, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        if self.welcome_join.get(&room.id) == Some(&JoinState::Joining) {
            return;
        }
        let id = room.id.clone();
        self.welcome_join.insert(id.clone(), JoinState::Joining);
        let joining = session.join_room_via(room.id, room.via);
        cx.spawn(async move |this, cx| {
            let result = joining.await;
            this.update(cx, |this, cx| {
                let state = match result {
                    Ok(()) => JoinState::Joined,
                    Err(err) => JoinState::Failed(err.to_string()),
                };
                this.welcome_join.insert(id, state);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

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
        if self.welcome_members.as_ref().is_none_or(|(room, _)| room != &space) {
            let members = self.new_members_view(space.clone(), cx);
            self.welcome_members = Some((space.clone(), members));
        }
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
    open: bool,
    key: String,
    voice: bool,
    name: &str,
    topic: Option<&str>,
    status: AnyElement,
    error: Option<String>,
    on_click: impl Fn(&mut Shell, &mut Context<Shell>) + 'static,
) -> AnyElement {
    let mut label = div().flex().flex_col().min_w_0().child(
        div()
            .font_weight(FontWeight::SEMIBOLD)
            .child(name.to_owned()),
    );
    let has_topic = topic.is_some_and(|t| !t.is_empty());
    if let Some(topic) = topic.filter(|t| !t.is_empty()) {
        // One line, cut off with an ellipsis, until opened.
        label = label.child(
            div()
                .text_size(px(0.85 * theme::REM))
                .text_color(rgb(theme::text_muted()))
                .when(!open, |d| d.truncate())
                .child(topic.to_owned())
                .e2e("div", "welcome-topic")
                .attr("data-open", open.to_string())
                .text(topic.to_owned()),
        );
    }
    if let Some(error) = error {
        label = label.child(
            div()
                .text_size(px(0.85 * theme::REM))
                .text_color(rgb(theme::danger()))
                .child(error.clone())
                .e2e("div", "welcome-error")
                .text(error),
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
        .child(crate::room_list::channel_glyph(voice))
        .child(div().flex_1().min_w_0().child(label))
        .children(has_topic.then(|| {
            let tip = if open { "Collapse the description" } else { "Show the full description" };
            let toggled = key.clone();
            div()
                .id(gpui::SharedString::from(format!("welcome-topic-toggle-{key}")))
                .flex_none()
                .size(px(24.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(theme::RADIUS_SM))
                .text_color(rgb(theme::text_muted()))
                .hover(|s| s.text_color(rgb(theme::text())))
                .cursor_pointer()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    if !this.welcome_topics_open.remove(&toggled) {
                        this.welcome_topics_open.insert(toggled.clone());
                    }
                    cx.notify();
                }))
                .child(icon(if open { "chevron-up" } else { "chevron-down" }, 16.0))
                .tip(tip)
                .e2e("button", "welcome-topic-toggle")
                .attr("data-open", open.to_string())
                .attr("title", tip)
        }))
        .child(status)
        .e2e("div", if voice { "welcome-channel voice" } else { "welcome-channel" })
        .text(name.to_owned())
        .into_any_element()
}

/// The welcome screen of `space`, in place of the chat.
pub fn render(shell: &Shell, cx: &mut Context<Shell>, space: &RoomSummary) -> AnyElement {
    let (joined, unjoined) = welcome_channels(shell, &space.id);
    let mut channels = div().flex().flex_col().gap(px(6.0));
    let pending: Vec<SpaceRoom> = unjoined
        .iter()
        .filter(|r| {
            !matches!(
                shell.welcome_join.get(&r.id),
                Some(JoinState::Joining | JoinState::Joined)
            )
        })
        .cloned()
        .collect();
    if pending.len() >= 2 {
        let all = card_button("welcome-join-all", "Join all", false)
            .on_click(cx.listener(move |this, _, _, cx| {
                for room in pending.clone() {
                    this.join_from_welcome(room, cx);
                }
            }))
            .e2e("button", "welcome-join-all")
            .text("Join all");
        channels = channels.child(div().flex().justify_end().child(all));
    }
    for room in joined {
        let (id, scope) = (room.id.clone(), Scope::Space(space.id.clone()));
        channels = channels.child(row(
            cx,
            shell.welcome_topics_open.contains(&room.id.to_string()),
            room.id.to_string(),
            room.is_voice,
            &room.name,
            room.topic.as_deref(),
            joined_mark(),
            None,
            move |this, cx| this.open_room_in(scope.clone(), id.clone(), cx),
        ));
    }
    for room in unjoined {
        let target: SpaceRoom = room.clone();
        let state = shell.welcome_join.get(&room.id).cloned();
        let (status, error) = match state {
            Some(JoinState::Joined) => (joined_mark(), None),
            Some(JoinState::Joining) => (joining_mark(), None),
            other => {
                let error = match other {
                    Some(JoinState::Failed(why)) => Some(why),
                    _ => None,
                };
                let join_target = target.clone();
                let button = card_button(
                    gpui::SharedString::from(format!("welcome-join-{}", room.id)),
                    "Join",
                    true,
                )
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.join_from_welcome(join_target.clone(), cx)
                }))
                .e2e("button", "welcome-join")
                .text("Join");
                (button.into_any_element(), error)
            }
        };
        channels = channels.child(row(
            cx,
            shell.welcome_topics_open.contains(&room.id.to_string()),
            room.id.to_string(),
            room.is_voice,
            &room.name,
            room.topic.as_deref(),
            status,
            error,
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
        .flex_none()
        .w(px(480.0))
        .max_w_full()
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
    // One column as tall as its content (at least the area), the card
    // centred in it: a short card sits in the middle, a tall one scrolls from
    // its top to its bottom (#197, #205).
    div()
        .id("server-welcome-scroll")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .overflow_y_scroll()
        .track_scroll(&shell.welcome_scroll)
        .child(
            div()
                .flex_none()
                .min_h_full()
                .flex()
                .flex_col()
                .justify_center()
                .items_center()
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
                ),
        )
        .e2e("div", "server-welcome-scroll")
        .scrollable(std::rc::Rc::new(crate::e2e::HandleProbe(
            shell.welcome_scroll.clone(),
        )))
        .into_any_element()
}

/// "Joined" with a check: the status of a channel you are in.
fn joined_mark() -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(4.0))
        .text_size(px(0.85 * theme::REM))
        .text_color(rgb(theme::text_muted()))
        .child(icon("check", 14.0))
        .child("Joined")
        .e2e("div", "welcome-joined")
        .text("Joined")
        .into_any_element()
}

/// A turning arrow and "Joining…": the status of a channel being joined.
fn joining_mark() -> AnyElement {
    use gpui::{Animation, AnimationExt as _};
    div()
        .flex()
        .items_center()
        .gap(px(4.0))
        .text_size(px(0.85 * theme::REM))
        .text_color(rgb(theme::text_muted()))
        .child(icon("refresh-cw", 14.0).with_animation(
            "welcome-joining-spinner",
            Animation::new(std::time::Duration::from_millis(900)).repeat(),
            |icon, delta| icon.rotated(delta),
        ))
        .child("Joining…")
        .e2e("div", "welcome-joining")
        .text("Joining…")
        .into_any_element()
}
