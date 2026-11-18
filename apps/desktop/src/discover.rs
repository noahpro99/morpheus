//! Discover: the compass under + in the server list. Browse the public
//! rooms and spaces of a Matrix server's room directory (your own at first;
//! any server's can be typed in, and a few are offered), search them, look
//! at one (name, topic, size, in the same card a matrix.to link gets) and
//! join it. What a row says is `view_model::discover`.

use std::collections::HashSet;

use gpui::{
    AnyElement, Context, ElementId, Entity, EventEmitter, FocusHandle, FontWeight, IntoElement,
    Render, SharedString, Window, div, prelude::*, px, rgb, rgba,
};
use morpheus_core::{PublicRoom, Session};

use crate::{
    dialogs::{Closed, dialog_box, heading, muted, overlay},
    e2e::E2eExt as _,
    input::{Changed, Submit, TextInput},
    media::{self, Avatars, avatar},
    room_dialogs::button,
    room_preview::Joined,
    theme,
    view_model::discover as vm,
};

/// A room's row was clicked: look at it before joining (the second is the
/// server whose directory it came from).
pub struct Preview(pub PublicRoom, pub String);

pub struct Discover {
    session: Session,
    /// The account's own server, whose directory is the first shown.
    home: String,
    /// The directory being browsed.
    server: String,
    server_input: Entity<TextInput>,
    search: Entity<TextInput>,
    rooms: Vec<PublicRoom>,
    next: Option<String>,
    loading: bool,
    /// The newest query; older answers arriving late are dropped.
    generation: u64,
    error: Option<String>,
    /// Rooms already joined (by id), and those being joined.
    joined: HashSet<String>,
    joining: HashSet<String>,
    join_error: Option<String>,
    avatars: Avatars,
    focus: FocusHandle,
    take_focus: bool,
}

impl EventEmitter<Closed> for Discover {}
impl EventEmitter<Joined> for Discover {}
impl EventEmitter<Preview> for Discover {}

impl Discover {
    pub fn new(session: Session, joined: HashSet<String>, cx: &mut Context<Self>) -> Self {
        let home = session
            .user_id()
            .map(|u| u.server_name().to_string())
            .unwrap_or_default();
        let server_input =
            cx.new(|cx| TextInput::new("discover-server", "Server, e.g. matrix.org", false, &home, cx).framed());
        let search = cx.new(|cx| {
            TextInput::new("discover-search", "Search public rooms and spaces", false, "", cx)
                .framed()
        });
        cx.subscribe(&search, |this, _, _: &Changed, cx| this.query(false, cx))
            .detach();
        cx.subscribe(&server_input, |this, _, _: &Submit, cx| {
            let typed = this.server_input.read(cx).value().to_owned();
            match vm::normalise_server(&typed) {
                Some(server) => this.show_server(server, cx),
                None => {
                    this.error = Some(format!("\"{}\" is not a server name", typed.trim()));
                    cx.notify();
                }
            }
        })
        .detach();
        let mut this = Self {
            session,
            server: home.clone(),
            home,
            server_input,
            search,
            rooms: Vec::new(),
            next: None,
            loading: false,
            generation: 0,
            error: None,
            joined,
            joining: HashSet::new(),
            join_error: None,
            avatars: Avatars::default(),
            focus: cx.focus_handle(),
            take_focus: true,
        };
        this.query(false, cx);
        this
    }

    fn show_server(&mut self, server: String, cx: &mut Context<Self>) {
        self.server = server.clone();
        self.server_input
            .update(cx, |input, cx| input.set_value(&server, cx));
        self.query(false, cx);
    }

    /// Ask the directory: the first page of what the search box says, or
    /// (`more`) the page after the one shown.
    fn query(&mut self, more: bool, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        let since = if more { self.next.clone() } else { None };
        if !more {
            self.rooms.clear();
            self.next = None;
        }
        self.loading = true;
        self.error = None;
        let term = self.search.read(cx).value().to_owned();
        let server = (self.server != self.home).then(|| self.server.clone());
        let asking = self.session.public_rooms(server, term, since);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = asking.await;
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                this.loading = false;
                match result {
                    Ok(page) => {
                        this.next = page.next;
                        this.rooms.extend(page.rooms);
                        this.fetch_pictures(cx);
                    }
                    Err(err) => this.error = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn fetch_pictures(&mut self, cx: &mut Context<Self>) {
        for room in self.rooms.clone() {
            let Some(media) = room.avatar.clone() else {
                continue;
            };
            if !self.avatars.begin(&media, 40) {
                continue;
            }
            let fetching = self.session.thumbnail(media.clone(), 80, 80);
            cx.spawn(async move |this, cx| {
                let image = media::fetched(cx, fetching, (80, 80), false).await;
                this.update(cx, |this, cx| {
                    this.avatars.finish(&media, 40, image);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    fn join(&mut self, room: &PublicRoom, cx: &mut Context<Self>) {
        let key = room.room_id.to_string();
        if self.joining.contains(&key) {
            return;
        }
        if self.joined.contains(&key) {
            cx.emit(Joined(room.room_id.clone()));
            return;
        }
        self.joining.insert(key.clone());
        self.join_error = None;
        let room_id = room.room_id.clone();
        let via = vec![self.server.clone()];
        let joining = self.session.join_room_via(room_id.clone(), via);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = joining.await;
            this.update(cx, |this, cx| {
                this.joining.remove(&key);
                match result {
                    Ok(()) => {
                        this.joined.insert(key);
                        cx.emit(Joined(room_id));
                    }
                    Err(err) => this.join_error = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn row(&self, room: &PublicRoom, cx: &mut Context<Self>) -> AnyElement {
        let name = crate::streamer::name(&vm::name(room));
        let key = room.room_id.to_string();
        let joined = self.joined.contains(&key);
        let joining = self.joining.contains(&key);
        let picture = room
            .avatar
            .as_ref()
            .and_then(|media| self.avatars.get(media, 40));
        let label = if joined {
            "Open"
        } else if joining {
            "Joining…"
        } else {
            "Join"
        };
        let (look, join) = (room.clone(), room.clone());
        let server = self.server.clone();
        let mut info = div()
            .id(ElementId::Name(format!("discover-look-{key}").into()))
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(10.0))
            .cursor_pointer()
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.emit(Preview(look.clone(), server.clone()));
            }))
            .child(avatar(picture, &name, 40.0, Some(theme::RADIUS)));
        let mut text = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(name.clone())
                    .e2e("span", "discover-name")
                    .text(name.clone()),
            );
        let size = if room.is_space {
            format!("Space, {}", vm::members_line(room))
        } else {
            vm::members_line(room)
        };
        text = text.child(
            div()
                .truncate()
                .text_size(px(0.8 * theme::REM))
                .text_color(rgb(theme::text_muted()))
                .child(size.clone())
                .e2e("span", "discover-size muted")
                .text(size),
        );
        if let Some(topic) = vm::topic(room) {
            text = text.child(
                div()
                    .truncate()
                    .text_size(px(0.85 * theme::REM))
                    .text_color(rgb(theme::text_muted()))
                    .child(topic.clone())
                    .e2e("span", "discover-topic muted")
                    .text(topic),
            );
        }
        info = info.child(text);
        div()
            .id(ElementId::Name(format!("discover-room-{key}").into()))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(8.0))
            .py(px(6.0))
            .rounded(px(theme::RADIUS))
            .hover(|s| s.bg(rgba(theme::bg_hover())))
            .child(info)
            .child(
                div()
                    .id(ElementId::Name(format!("discover-join-{key}").into()))
                    .flex_none()
                    .px(px(12.0))
                    .py(px(5.0))
                    .rounded(px(theme::RADIUS))
                    .bg(rgb(theme::accent_strong()))
                    .text_color(rgb(theme::on_danger()))
                    .cursor_pointer()
                    .when(joining, |d| d.opacity(0.6))
                    .on_click(cx.listener(move |this, _, _, cx| this.join(&join, cx)))
                    .child(SharedString::from(label))
                    .e2e("button", "discover-join")
                    .text(label),
            )
            .e2e("div", "discover-room")
            .attr("data-room", key)
            .attr("data-joined", joined.to_string())
            .into_any_element()
    }

    fn chip(&self, server: &str, cx: &mut Context<Self>) -> AnyElement {
        let target = server.to_owned();
        let current = self.server == server;
        div()
            .id(ElementId::Name(format!("discover-chip-{server}").into()))
            .flex_none()
            .px(px(10.0))
            .py(px(3.0))
            .rounded_full()
            .text_size(px(0.85 * theme::REM))
            .cursor_pointer()
            .bg(rgb(if current {
                theme::accent_strong()
            } else {
                theme::bg_elevated()
            }))
            .text_color(rgb(if current {
                theme::on_danger()
            } else {
                theme::text_muted()
            }))
            .on_click(cx.listener(move |this, _, _, cx| this.show_server(target.clone(), cx)))
            .child(server.to_owned())
            .e2e("button", "discover-chip")
            .attr("data-server", server.to_owned())
            .text(server.to_owned())
            .into_any_element()
    }
}

impl Render for Discover {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.take_focus) {
            self.search.read(cx).focus(window);
        }
        let mut chips = vec![self.chip(&self.home.clone(), cx)];
        for server in vm::SUGGESTED_SERVERS {
            if *server != self.home {
                chips.push(self.chip(server, cx));
            }
        }
        let rows: Vec<AnyElement> = self
            .rooms
            .clone()
            .iter()
            .map(|room| self.row(room, cx))
            .collect();
        let status = if let Some(error) = &self.error {
            Some((error.clone(), true))
        } else if let Some(error) = &self.join_error {
            Some((error.clone(), true))
        } else if self.loading {
            Some(("Loading…".to_owned(), false))
        } else if self.rooms.is_empty() {
            Some((format!("No public rooms found on {}.", self.server), false))
        } else {
            None
        };
        let list = div()
            .id("discover-list")
            .flex()
            .flex_col()
            .gap(px(2.0))
            .h(px(340.0))
            .min_h_0()
            .overflow_y_scroll()
            .children(rows)
            .when_some(status, |d, (text, bad)| {
                d.child(
                    div()
                        .px(px(8.0))
                        .py(px(10.0))
                        .text_color(rgb(if bad {
                            theme::danger()
                        } else {
                            theme::text_muted()
                        }))
                        .child(text.clone())
                        .e2e("p", if bad { "discover-status error" } else { "discover-status muted" })
                        .text(text),
                )
            })
            .when(self.next.is_some() && !self.loading, |d| {
                d.child(button(
                    "discover-more",
                    None,
                    "Load more",
                    "secondary",
                    false,
                    false,
                    cx.listener(|this, _, _, cx| this.query(true, cx)),
                ))
            });
        let boxed = dialog_box()
            .id("discover-dialog")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(Closed);
                }
            }))
            .w(px(560.0))
            .child(heading("Discover"))
            .child(muted(
                "Public rooms and spaces anyone can join. Pick a server or type one in and press Enter.",
                "muted",
            ))
            .child(div().flex().flex_wrap().gap(px(6.0)).children(chips))
            .child(self.server_input.clone())
            .child(self.search.clone())
            .child(list)
            .child(
                div()
                    .flex()
                    .justify_end()
                    .child(button(
                        "discover-close",
                        None,
                        "Close",
                        "secondary",
                        false,
                        false,
                        cx.listener(|_, _, _, cx| cx.emit(Closed)),
                    )),
            )
            .e2e("div", "message-dialog room-dialog discover")
            .focusable(&self.focus);
        let this = cx.weak_entity();
        overlay(window, cx, boxed.into_any_element(), move |_, cx| {
            this.update(cx, |_, cx| cx.emit(Closed)).ok();
        })
    }
}

impl crate::shell::Shell {
    /// The compass: browse public rooms to join.
    pub fn open_discover(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let joined: HashSet<String> = self.rooms.iter().map(|r| r.id.to_string()).collect();
        let dialog = cx.new(|cx| Discover::new(session, joined, cx));
        cx.subscribe(&dialog, |this, _, _: &Closed, cx| this.dialog_close(cx))
            .detach();
        cx.subscribe(&dialog, |this, _, joined: &Joined, cx| {
            this.dialog_close(cx);
            this.pending_dm = Some(joined.0.clone());
            this.open_pending_dm(cx);
        })
        .detach();
        cx.subscribe(&dialog, |this, _, look: &Preview, cx| {
            let room = morpheus_core::LinkedRoom {
                room_id: look.0.room_id.clone(),
                name: look.0.name.clone(),
                topic: look.0.topic.clone(),
                avatar: look.0.avatar.clone(),
                members: look.0.members,
                joined: false,
            };
            this.dialog_close(cx);
            this.preview_room(room, vec![look.1.clone()], cx);
        })
        .detach();
        self.dialog_open(crate::dialogs::Dialog::Discover(dialog), cx);
    }
}
