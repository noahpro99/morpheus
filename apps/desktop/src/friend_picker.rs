//! The list of friends every invite screen has: a filter box over a row per
//! friend (picture, name, a label that says what clicking does and, once it
//! was done, that it was). A full Matrix ID typed in the filter shows as one
//! more row, so people who are not friends can be picked too. The picker only
//! says who was picked (`Picked`); the screen holding it does its own thing
//! (a Matrix invite, a message with a link) and reports back with
//! `set_state`. What matches the filter is `view_model::friends`.

use std::{collections::HashMap, sync::Arc};

use gpui::{
    AnyElement, Context, ElementId, Entity, EventEmitter, FontWeight, Image, IntoElement, Render,
    Window, div, prelude::*, px, rgb, rgba,
};
use morpheus_core::{OwnedUserId, Profile, Session};

use crate::{
    e2e::E2eExt as _,
    input::{Changed, TextInput},
    media::{self, Avatars, avatar},
    theme,
    view_model::{
        friends::{self, FriendRow},
        nicknames::NickMap,
    },
};

/// Someone's row was clicked.
pub struct Picked(pub OwnedUserId);

/// What became of the action for a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowState {
    Working,
    Done,
    Failed(String),
}

pub struct FriendPicker {
    session: Option<Session>,
    friends: Vec<OwnedUserId>,
    nicknames: NickMap,
    input: Entity<TextInput>,
    filter: String,
    profiles: HashMap<String, Option<Profile>>,
    avatars: Avatars,
    states: HashMap<String, RowState>,
    /// What clicking a row does ("Invite"), and what its row says when done
    /// ("Invited", "Sent").
    action: &'static str,
    done: &'static str,
}

impl EventEmitter<Picked> for FriendPicker {}

impl FriendPicker {
    /// `input_name` names the filter box (the e2e `name`); the box takes
    /// full Matrix IDs too, which the owner may read with `input`.
    pub fn new(
        session: Option<Session>,
        friends: Vec<OwnedUserId>,
        nicknames: NickMap,
        input_name: &'static str,
        placeholder: &str,
        action: &'static str,
        done: &'static str,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| TextInput::new(input_name, placeholder, false, "", cx).framed());
        cx.subscribe(&input, |this, _, _: &Changed, cx| {
            this.filter = this.input.read(cx).value().to_owned();
            cx.notify();
        })
        .detach();
        let mut this = Self {
            session,
            friends,
            nicknames,
            input,
            filter: String::new(),
            profiles: HashMap::new(),
            avatars: Avatars::default(),
            states: HashMap::new(),
            action,
            done,
        };
        this.fetch(cx);
        this
    }

    /// The filter box, to focus it or hear its Enter.
    pub fn input(&self) -> &Entity<TextInput> {
        &self.input
    }

    /// Say what became of the action for `user` (`None`: nothing, again
    /// clickable).
    pub fn set_state(&mut self, user: &OwnedUserId, state: Option<RowState>, cx: &mut Context<Self>) {
        match state {
            Some(state) => self.states.insert(user.to_string(), state),
            None => self.states.remove(user.as_str()),
        };
        cx.notify();
    }

    fn pick(&mut self, user: OwnedUserId, cx: &mut Context<Self>) {
        if matches!(
            self.states.get(user.as_str()),
            Some(RowState::Working | RowState::Done)
        ) {
            return;
        }
        self.states.insert(user.to_string(), RowState::Working);
        cx.emit(Picked(user));
        cx.notify();
    }

    /// Asks for each friend's profile and picture, once.
    fn fetch(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        for user_id in self.friends.clone() {
            let key = user_id.to_string();
            self.profiles.insert(key.clone(), None);
            let asking = session.profile(user_id);
            let session = session.clone();
            cx.spawn(async move |this, cx| {
                let profile = asking.await.ok();
                let media = profile.as_ref().and_then(|p| p.avatar.clone());
                this.update(cx, |this, cx| {
                    this.profiles.insert(key, profile);
                    cx.notify();
                })
                .ok();
                if let Some(media) = media {
                    let fetching = session.thumbnail(media.clone(), 64, 64);
                    let begun = this
                        .update(cx, |this, _| this.avatars.begin(&media, 32))
                        .unwrap_or(false);
                    if begun {
                        let image = fetching
                            .await
                            .ok()
                            .flatten()
                            .and_then(|bytes| media::decode(&bytes));
                        this.update(cx, |this, cx| {
                            this.avatars.finish(&media, 32, image);
                            cx.notify();
                        })
                        .ok();
                    }
                }
            })
            .detach();
        }
    }

    fn picture(&self, user_id: &str) -> Option<Arc<Image>> {
        let media = self.profiles.get(user_id)?.as_ref()?.avatar.clone()?;
        self.avatars.get(&media, 32)
    }

    fn row_el(&self, user: &OwnedUserId, row: FriendRow, cx: &mut Context<Self>) -> AnyElement {
        let key = user.to_string();
        let state = self.states.get(&key);
        let (label, tag) = match state {
            None => (self.action.to_owned(), "idle"),
            Some(RowState::Working) => ("…".to_owned(), "working"),
            Some(RowState::Done) => (self.done.to_owned(), "sent"),
            Some(RowState::Failed(_)) => ("Failed, retry".to_owned(), "failed"),
        };
        let clickable = !matches!(state, Some(RowState::Working | RowState::Done));
        let target = user.clone();
        let mut el = div()
            .id(ElementId::Name(format!("friend-pick-{key}").into()))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(8.0))
            .py(px(6.0))
            .rounded(px(theme::RADIUS))
            .when(clickable, |d| {
                d.cursor_pointer().hover(|s| s.bg(rgba(theme::bg_hover())))
            })
            .on_click(cx.listener(move |this, _, _, cx| this.pick(target.clone(), cx)))
            .child(avatar(self.picture(&key), &row.name, 32.0, None))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(row.name.clone())
                            .e2e("span", "friend-pick-name")
                            .text(row.name.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(0.8 * theme::REM))
                            .text_color(rgb(theme::text_muted()))
                            .child(key.clone())
                            .e2e("span", "friend-pick-id muted")
                            .text(key.clone()),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(0.85 * theme::REM))
                    .text_color(if tag == "sent" {
                        rgb(theme::ok())
                    } else if tag == "failed" {
                        rgb(theme::danger())
                    } else {
                        rgb(theme::text_muted())
                    })
                    .child(label.clone())
                    .e2e("span", "friend-pick-state")
                    .text(label.clone()),
            )
            .e2e("div", "friend-pick")
            .attr("data-user", key)
            .attr("data-state", tag);
        if let Some(RowState::Failed(why)) = state {
            el = el.attr("title", why.clone());
        }
        el.into_any_element()
    }
}

impl Render for FriendPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut listed: Vec<OwnedUserId> = Vec::new();
        for user in self.friends.clone() {
            let key = user.to_string();
            let profile = self.profiles.get(&key).and_then(|p| p.as_ref());
            let row = friends::friend_row(
                &self.nicknames,
                &key,
                profile.map(|p| (p.name.as_str(), p.status.as_deref())),
            );
            if friends::matches_filter(&row, &key, &self.filter) {
                rows.push(self.row_el(&user, row, cx));
                listed.push(user);
            }
        }
        // A full Matrix ID typed that is not a friend: one more row.
        if let Ok(typed) = self.filter.trim().parse::<OwnedUserId>()
            && !self.friends.contains(&typed)
        {
            let row = FriendRow { name: typed.to_string(), subtitle: String::new() };
            rows.push(self.row_el(&typed, row, cx));
        }
        let empty = rows.is_empty();
        let list = div()
            .id("friend-pick-list")
            .flex()
            .flex_col()
            .gap(px(2.0))
            .max_h(px(260.0))
            .min_h_0()
            .overflow_y_scroll()
            .children(rows)
            .when(empty, |d| {
                let text = if self.friends.is_empty() {
                    "No friends yet. Type a full Matrix ID like @name:server."
                } else {
                    "No friend matches. A full Matrix ID like @name:server works too."
                };
                d.child(
                    div()
                        .px(px(8.0))
                        .py(px(10.0))
                        .text_color(rgb(theme::text_muted()))
                        .child(text)
                        .e2e("p", "friend-pick-empty muted")
                        .text(text),
                )
            });
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .min_h_0()
            .child(self.input.clone())
            .child(list)
            .e2e("div", "friend-picker")
            .into_any_element()
    }
}
