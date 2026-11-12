//! The Friends page (`crates/ui/src/components/friends.rs`), at the top of
//! the DMs and taking the chat area: your friends with a way to message them
//! (the whole row opens the DM), and New Message, which opens a DM with
//! anyone by username. What it decides is in `view_model::friends`.

use std::{collections::HashMap, sync::Arc};

use gpui::{
    AnyElement, Context, ElementId, Entity, EventEmitter, FontWeight, Image, IntoElement, Render,
    SharedString, Window, div, prelude::*, px, rgb, rgba,
};
use morpheus_core::{OwnedRoomId, OwnedUserId, Profile, Session};

use crate::{
    e2e::E2eExt as _,
    icons::icon,
    input::{Changed, Submit, TextInput},
    media::{self, Avatars, avatar},
    theme,
    view_model::{
        friends::{self, NewMessage, Tab},
        nicknames::NickMap,
    },
};

/// What the page asks of the shell.
pub enum FriendsEvent {
    /// Open the direct message with someone (it may not be listed yet).
    OpenDm(OwnedRoomId),
    /// Take someone off the list.
    Remove(OwnedUserId),
    /// A background action failed: what it was, and why.
    Failed(&'static str, String),
}

impl EventEmitter<FriendsEvent> for FriendsPage {}

pub struct FriendsPage {
    session: Option<Session>,
    friends: Vec<OwnedUserId>,
    nicknames: NickMap,
    picked: Option<Tab>,
    form: NewMessage,
    input: Entity<TextInput>,
    /// Friends' profiles, asked for once each (`None` while loading or if
    /// it failed).
    profiles: HashMap<String, Option<Profile>>,
    avatars: Avatars,
}

impl FriendsPage {
    pub fn new(
        session: Option<Session>,
        friends: Vec<OwnedUserId>,
        nicknames: NickMap,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            TextInput::new("new-message-name", friends::NEW_MESSAGE_PLACEHOLDER, false, "", cx)
                .bare("input", "")
        });
        cx.subscribe(&input, |this, _, _: &Changed, cx| {
            let text = this.input.read(cx).value().to_owned();
            this.form.typed(&text);
            cx.notify();
        })
        .detach();
        cx.subscribe(&input, |this, _, _: &Submit, cx| this.submit(cx))
            .detach();
        Self {
            session,
            friends,
            nicknames,
            picked: None,
            form: NewMessage::default(),
            input,
            profiles: HashMap::new(),
            avatars: Avatars::default(),
        }
    }

    /// The list changed (added from a menu, removed here).
    pub fn set_friends(&mut self, friends: Vec<OwnedUserId>, cx: &mut Context<Self>) {
        if self.friends != friends {
            self.friends = friends;
            cx.notify();
        }
    }

    pub fn set_nicknames(&mut self, nicknames: NickMap, cx: &mut Context<Self>) {
        self.nicknames = nicknames;
        cx.notify();
    }

    /// Enter or the Message button: find who was typed, make or find the DM.
    fn submit(&mut self, cx: &mut Context<Self>) {
        let Some(text) = self.form.begin() else {
            return;
        };
        let Some(session) = self.session.clone() else {
            self.form.finish(Err("Not signed in.".into()));
            return;
        };
        let finding = session.find_user(text);
        let own = session.user_id();
        cx.spawn(async move |this, cx| {
            let opened = async {
                let user_id = finding.await.map_err(|err| err.to_string())?;
                friends::check_target(own.as_ref().map(|u| u.as_str()), user_id.as_str())?;
                session.dm_with(user_id).await.map_err(|err| err.to_string())
            }
            .await;
            this.update(cx, |this, cx| {
                match opened {
                    Ok(dm) => {
                        this.form.finish(Ok(()));
                        this.input.update(cx, |input, cx| input.set_value("", cx));
                        cx.emit(FriendsEvent::OpenDm(dm));
                    }
                    Err(reason) => this.form.finish(Err(reason)),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// The row's "Message" (or the row itself).
    fn message(&mut self, user_id: OwnedUserId, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let opening = session.dm_with(user_id);
        cx.spawn(async move |this, cx| {
            let result = opening.await;
            this.update(cx, |_, cx| match result {
                Ok(dm) => cx.emit(FriendsEvent::OpenDm(dm)),
                Err(err) => cx.emit(FriendsEvent::Failed("opening a DM", err.to_string())),
            })
            .ok();
        })
        .detach();
    }

    /// Asks for the profiles and pictures the rows need, once each.
    fn fetch(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        for user_id in self.friends.clone() {
            let key = user_id.to_string();
            if !self.profiles.contains_key(&key) {
                self.profiles.insert(key.clone(), None);
                let asking = session.profile(user_id);
                let task_key = key.clone();
                cx.spawn(async move |this, cx| {
                    let profile = asking.await.ok();
                    this.update(cx, |this, cx| {
                        this.profiles.insert(task_key, profile);
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
            }
            let media = self
                .profiles
                .get(&key)
                .and_then(|p| p.as_ref())
                .and_then(|p| p.avatar.clone());
            if let Some(media) = media
                && self.avatars.begin(&media, 40)
            {
                let fetching = session.thumbnail(media.clone(), 80, 80);
                cx.spawn(async move |this, cx| {
                    let image = fetching
                        .await
                        .ok()
                        .flatten()
                        .and_then(|bytes| media::decode(&bytes));
                    this.update(cx, |this, cx| {
                        this.avatars.finish(&media, 40, image);
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
            }
        }
    }

    fn picture(&self, user_id: &str) -> Option<Arc<Image>> {
        let media = self.profiles.get(user_id)?.as_ref()?.avatar.clone()?;
        self.avatars.get(&media, 40)
    }

    fn tab_button(
        &self,
        tab: Tab,
        current: Tab,
        class: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = tab == current;
        let new_message = tab == Tab::NewMessage;
        let mut button = div()
            .id(SharedString::from(class))
            .px(px(10.0))
            .py(px(4.0))
            .rounded(px(theme::RADIUS_SM))
            .font_weight(FontWeight::SEMIBOLD)
            .whitespace_nowrap()
            .cursor_pointer()
            .child(tab.label());
        button = match (new_message, active) {
            (true, false) => button.bg(rgb(theme::green())).text_color(rgb(0xffffff)),
            (true, true) => button.text_color(rgb(theme::green())),
            (false, true) => button
                .bg(rgba(theme::bg_selected()))
                .text_color(rgb(theme::text())),
            (false, false) => button
                .text_color(rgb(theme::text_muted()))
                .hover(|s| s.bg(rgba(theme::bg_hover())).text_color(rgb(theme::text()))),
        };
        button
            .on_click(cx.listener(move |this, _, _, cx| {
                this.picked = Some(tab);
                cx.notify();
            }))
            .e2e(
                "button",
                &if active {
                    format!("friends-tab {class} active")
                } else {
                    format!("friends-tab {class}")
                },
            )
            .text(tab.label())
            .into_any_element()
    }

    fn new_message_el(&self, cx: &mut Context<Self>) -> AnyElement {
        let can = self.form.can_submit();
        let label = self.form.button_label();
        let mut button = div()
            .id("new-message-send")
            .flex_none()
            .px(px(16.0))
            .py(px(8.0))
            .rounded(px(theme::RADIUS_SM))
            .whitespace_nowrap()
            .bg(rgb(theme::accent()))
            .text_color(rgb(0xffffff))
            .font_weight(FontWeight::SEMIBOLD)
            .child(label);
        button = if can {
            button
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| this.submit(cx)))
        } else {
            button.opacity(0.5)
        };
        let mut button = button
            .e2e("button", "primary")
            .attr("type", "submit")
            .text(label);
        if !can {
            button = button.attr("disabled", "");
        }
        let form = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .pl(px(14.0))
            .pr(px(6.0))
            .py(px(2.0))
            .rounded(px(theme::RADIUS))
            .bg(rgb(theme::bg_input()))
            .border_1()
            .border_color(rgba(theme::bg_selected()))
            .child(div().flex_1().min_w_0().child(self.input.clone()))
            .child(button)
            .e2e("form", "new-message");
        let mut panel = div()
            .flex()
            .flex_col()
            .px(px(24.0))
            .py(px(20.0))
            .border_b_1()
            .border_color(rgba(theme::bg_selected()))
            .child(
                div()
                    .mb(px(4.0))
                    .font_weight(FontWeight::BOLD)
                    .child(friends::NEW_MESSAGE_TITLE.to_uppercase())
                    .e2e("h3", "")
                    .text(friends::NEW_MESSAGE_TITLE),
            )
            .child(
                div()
                    .mb(px(16.0))
                    .text_size(px(0.9 * theme::REM))
                    .text_color(rgb(theme::text_muted()))
                    .child(friends::NEW_MESSAGE_HINT)
                    .e2e("p", "muted")
                    .text(friends::NEW_MESSAGE_HINT),
            )
            .child(form);
        if let Some(reason) = &self.form.failed {
            panel = panel.child(
                div()
                    .mt(px(10.0))
                    .text_size(px(0.9 * theme::REM))
                    .text_color(rgb(theme::danger()))
                    .child(reason.clone())
                    .e2e("p", "new-message-result error")
                    .text(reason.clone()),
            );
        }
        panel.e2e("div", "new-message").into_any_element()
    }

    fn list_el(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.friends.is_empty() {
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.0))
                .child(
                    div()
                        .font_weight(FontWeight::BOLD)
                        .child(friends::EMPTY_TITLE)
                        .e2e("h2", "")
                        .text(friends::EMPTY_TITLE),
                )
                .child(
                    div()
                        .text_color(rgb(theme::text_muted()))
                        .child(friends::EMPTY_HINT)
                        .e2e("p", "muted")
                        .text(friends::EMPTY_HINT),
                )
                .e2e("div", "empty-state")
                .into_any_element();
        }
        let heading = friends::count_heading(self.friends.len());
        let mut list = div()
            .id("friend-list")
            .flex()
            .flex_col()
            .gap(px(2.0))
            .px(px(16.0))
            .py(px(8.0))
            .min_h_0()
            .overflow_y_scroll();
        for (ix, user_id) in self.friends.iter().enumerate() {
            let key = user_id.to_string();
            let profile = self.profiles.get(&key).and_then(|p| p.as_ref());
            let row = friends::friend_row(
                &self.nicknames,
                &key,
                profile.map(|p| (p.name.as_str(), p.status.as_deref())),
            );
            let (open, remove, talk) = (user_id.clone(), user_id.clone(), user_id.clone());
            let mut item = div()
                .id(ElementId::Name(format!("friend-{key}").into()))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(12.0))
                .px(px(8.0))
                .py(px(10.0))
                .rounded(px(theme::RADIUS))
                .cursor_pointer()
                .hover(|s| s.bg(rgba(theme::bg_hover())))
                .on_click(cx.listener(move |this, _, _, cx| this.message(open.clone(), cx)));
            if ix > 0 {
                item = item.border_t_1().border_color(rgb(theme::bg_elevated()));
            }
            list = list.child(
                item.child(avatar(self.picture(&key), &row.name, 40.0, None))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(row.name.clone())
                                    .e2e("span", "friend-name")
                                    .text(row.name.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(0.85 * theme::REM))
                                    .text_color(rgb(theme::text_muted()))
                                    .child(row.subtitle.clone())
                                    .e2e("span", "friend-status muted")
                                    .text(row.subtitle.clone()),
                            )
                            .e2e("div", "friend-text"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .id(ElementId::Name(format!("message-{key}").into()))
                                    .px(px(12.0))
                                    .py(px(6.0))
                                    .rounded(px(theme::RADIUS))
                                    .bg(rgb(theme::bg_elevated()))
                                    .cursor_pointer()
                                    .child("Message")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.message(talk.clone(), cx)
                                    }))
                                    .e2e("button", "secondary")
                                    .text("Message"),
                            )
                            .child(
                                div()
                                    .id(ElementId::Name(format!("remove-{key}").into()))
                                    .px(px(12.0))
                                    .py(px(6.0))
                                    .rounded(px(theme::RADIUS))
                                    .text_color(rgb(theme::text_muted()))
                                    .hover(|s| s.text_color(rgb(theme::text())))
                                    .child("Remove")
                                    .on_click(cx.listener(move |_, _, _, cx| {
                                        // Not also the row's click.
                                        cx.stop_propagation();
                                        cx.emit(FriendsEvent::Remove(remove.clone()))
                                    }))
                                    .e2e("button", "ghost")
                                    .text("Remove"),
                            )
                            .e2e("div", "friend-actions"),
                    )
                    .e2e("li", "friend")
                    .attr("role", "button"),
            );
        }
        div()
            .flex()
            .flex_col()
            .min_h_0()
            .child(
                div()
                    .px(px(24.0))
                    .pt(px(16.0))
                    .pb(px(4.0))
                    .text_size(px(0.7 * theme::REM))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(theme::text_faint()))
                    .child(heading.to_uppercase())
                    .e2e("h4", "section friends-count")
                    .text(heading),
            )
            .child(list.e2e("ul", "friend-list"))
            .into_any_element()
    }
}


impl Render for FriendsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.fetch(cx);
        let tab = friends::effective_tab(self.picked, self.friends.len());
        let header = div()
            .h(px(48.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(12.0))
            .px(px(16.0))
            .border_b_1()
            .border_color(rgba(0x0000004d))
            .child(icon("users", 18.0).text_color(rgb(theme::text())))
            .child(
                div()
                    .flex_none()
                    .font_weight(FontWeight::BOLD)
                    .child("Friends")
                    .e2e("h2", "")
                    .text("Friends"),
            )
            .child(
                div()
                    .flex_none()
                    .w(px(1.0))
                    .h(px(24.0))
                    .bg(rgba(theme::bg_selected()))
                    .e2e("span", "friends-divider"),
            )
            .child(self.tab_button(Tab::All, tab, "all-friends-tab", cx))
            .child(self.tab_button(Tab::NewMessage, tab, "new-message-tab", cx))
            .e2e("header", "chat-header friends-header");
        let body = match tab {
            Tab::NewMessage => self.new_message_el(cx),
            Tab::All => self.list_el(cx),
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .line_height(gpui::relative(theme::LINE_HEIGHT))
            .child(header)
            .child(body)
            .e2e("section", "friends-page")
    }
}
