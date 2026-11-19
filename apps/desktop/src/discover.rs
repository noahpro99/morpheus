//! Discover: the compass under + in the server list. A full page to the right
//! of the rail (no channel list, chat or thread beside it) browsing the public
//! rooms and spaces of several Matrix servers' room directories at once: the
//! own server's and the well-known ones, each switched on or off (and
//! remembered), searched at the same time with their results merged, each
//! row saying which server it is from. A directory that fails shows its own
//! error on its chip; the others carry on. Back (the title bar's button,
//! Alt+Left, the mouse's back button) returns to where it was opened from.
//! What a row says and how results merge is `view_model::discover`.

use std::collections::HashSet;

use gpui::{
    AnyElement, Context, ElementId, Entity, EventEmitter, FocusHandle, FontWeight, IntoElement,
    Render, SharedString, Window, div, prelude::*, px, rgb, rgba,
};
use morpheus_core::{PublicRoom, Session};

use crate::{
    dialogs::Closed,
    e2e::E2eExt as _,
    input::{Changed, Submit, TextInput},
    media::{self, Avatars, avatar},
    room_dialogs::button,
    room_preview::Joined,
    theme,
    view_model::discover::{self as vm, Directories, Results, Status},
};

/// How long a search waits for the typing to pause.
const TYPING_PAUSE: std::time::Duration = std::time::Duration::from_millis(300);
/// How long one directory has to answer.
const ANSWER_WITHIN: std::time::Duration = std::time::Duration::from_secs(10);

/// A room's row was clicked: look at it before joining (the second is the
/// servers whose directories list it).
pub struct Preview(pub PublicRoom, pub Vec<String>);

/// The directories were switched on or off or one was added: keep the choice.
pub struct Choice(pub Directories);

pub struct Discover {
    session: Session,
    /// The account's own server, whose directory is `None` to the session.
    home: String,
    dirs: Directories,
    server_input: Entity<TextInput>,
    search: Entity<TextInput>,
    results: Results,
    input_error: Option<String>,
    /// Counts changes of the search box (a search waits for a pause).
    typed: u64,
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
impl EventEmitter<Choice> for Discover {}

impl Discover {
    pub fn new(
        session: Session,
        joined: HashSet<String>,
        saved: Option<serde_json::Value>,
        cx: &mut Context<Self>,
    ) -> Self {
        let home = session
            .user_id()
            .map(|u| u.server_name().to_string())
            .unwrap_or_default();
        let dirs = Directories::new(&home, saved.as_ref());
        let server_input = cx.new(|cx| {
            TextInput::new("discover-server", "Add a server, e.g. matrix.org", false, "", cx)
                .framed()
        });
        let search = cx.new(|cx| {
            TextInput::new("discover-search", "Search public rooms and spaces", false, "", cx)
                .framed()
        });
        cx.subscribe(&search, |this, _, _: &Changed, cx| {
            // Wait for the typing to pause: one search, not one per key.
            this.typed += 1;
            let typed = this.typed;
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(TYPING_PAUSE).await;
                this.update(cx, |this, cx| {
                    if this.typed == typed {
                        this.query(cx);
                    }
                })
                .ok();
            })
            .detach();
        })
            .detach();
        cx.subscribe(&server_input, |this, _, _: &Submit, cx| {
            let typed = this.server_input.read(cx).value().to_owned();
            match vm::normalise_server(&typed) {
                Some(server) => {
                    this.input_error = None;
                    this.dirs.add(&server);
                    this.server_input.update(cx, |input, cx| input.set_value("", cx));
                    this.chosen(cx);
                }
                None => {
                    this.input_error = Some(format!("\"{}\" is not a server name", typed.trim()));
                    cx.notify();
                }
            }
        })
        .detach();
        let mut this = Self {
            session,
            home,
            dirs,
            server_input,
            search,
            results: Results::default(),
            input_error: None,
            typed: 0,
            joined,
            joining: HashSet::new(),
            join_error: None,
            avatars: Avatars::default(),
            focus: cx.focus_handle(),
            take_focus: true,
        };
        this.query(cx);
        this
    }

    /// The choice of directories changed: keep it and ask again.
    fn chosen(&mut self, cx: &mut Context<Self>) {
        cx.emit(Choice(self.dirs.clone()));
        self.query(cx);
    }

    /// Ask every selected directory at once for the first page of what the
    /// search box says.
    fn query(&mut self, cx: &mut Context<Self>) {
        let servers = self.dirs.selected();
        let generation = self.results.start(&servers);
        for server in servers {
            self.ask(generation, server, None, cx);
        }
        cx.notify();
    }

    /// The page after the one shown, of every directory that has one.
    fn more(&mut self, cx: &mut Context<Self>) {
        let (generation, ask) = self.results.start_more();
        for (server, since) in ask {
            self.ask(generation, server, Some(since), cx);
        }
        cx.notify();
    }

    fn ask(&mut self, generation: u64, server: String, since: Option<String>, cx: &mut Context<Self>) {
        let term = self.search.read(cx).value().to_owned();
        let own = (server != self.home).then(|| server.clone());
        let asking = self.session.public_rooms(own, term, since);
        cx.spawn(async move |this, cx| {
            // A slow directory gives up on its own; the others are not waited for.
            let timeout = cx.background_executor().timer(ANSWER_WITHIN);
            let answer = match futures::future::select(asking, timeout).await {
                futures::future::Either::Left((answer, _)) => answer
                    .map(|page| (page.rooms, page.next))
                    .map_err(|err| err.to_string()),
                futures::future::Either::Right(_) => Err("no answer in time".to_owned()),
            };
            this.update(cx, |this, cx| {
                if this.results.finish(generation, &server, answer) {
                    this.fetch_pictures(cx);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn fetch_pictures(&mut self, cx: &mut Context<Self>) {
        for entry in self.results.merged() {
            let Some(media) = entry.room.avatar.clone() else {
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

    fn join(&mut self, room: &PublicRoom, servers: Vec<String>, cx: &mut Context<Self>) {
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
        let joining = self.session.join_room_via(room_id.clone(), servers);
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

    fn row(&self, entry: &vm::Entry, cx: &mut Context<Self>) -> AnyElement {
        let room = &entry.room;
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
        let (via_look, via_join) = (entry.servers.clone(), entry.servers.clone());
        let mut info = div()
            .id(ElementId::Name(format!("discover-look-{key}").into()))
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(10.0))
            .cursor_pointer()
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.emit(Preview(look.clone(), via_look.clone()));
            }))
            .child(avatar(picture, &name, 40.0, Some(theme::RADIUS)));
        let mut text = div().flex_1().min_w_0().flex().flex_col().child(
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
        let from = entry.label();
        text = text.child(
            div()
                .flex()
                .gap(px(8.0))
                .text_size(px(0.8 * theme::REM))
                .text_color(rgb(theme::text_muted()))
                .child(
                    div()
                        .truncate()
                        .child(size.clone())
                        .e2e("span", "discover-size muted")
                        .text(size),
                )
                .child(
                    div()
                        .truncate()
                        .child(from.clone())
                        .e2e("span", "discover-server muted")
                        .text(from),
                ),
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
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.join(&join, via_join.clone(), cx)),
                    )
                    .child(SharedString::from(label))
                    .e2e("button", "discover-join")
                    .text(label),
            )
            .e2e("div", "discover-room")
            .attr("data-room", key)
            .attr("data-joined", joined.to_string())
            .into_any_element()
    }

    /// A directory's switch: on is filled; a failing one says so on itself.
    fn chip(&self, server: &str, cx: &mut Context<Self>) -> AnyElement {
        let target = server.to_owned();
        let on = self.dirs.is_on(server);
        let failed = on.then(|| self.results.error_of(server)).flatten().is_some();
        let loading = on
            && self
                .results
                .listings
                .iter()
                .any(|l| l.server == server && l.status == Status::Loading);
        let mut chip = div()
            .id(ElementId::Name(format!("discover-chip-{server}").into()))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(10.0))
            .py(px(3.0))
            .rounded_full()
            .text_size(px(0.85 * theme::REM))
            .cursor_pointer()
            .bg(rgb(if on {
                theme::accent_strong()
            } else {
                theme::bg_elevated()
            }))
            .text_color(rgb(if on {
                theme::on_danger()
            } else {
                theme::text_muted()
            }))
            .when(loading, |d| d.opacity(0.7))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.dirs.toggle(&target);
                this.chosen(cx);
            }))
            .child(server.to_owned());
        if failed {
            chip = chip.child(
                div()
                    .px(px(6.0))
                    .rounded_full()
                    .bg(rgb(theme::danger()))
                    .text_color(rgb(theme::on_danger()))
                    .text_size(px(0.75 * theme::REM))
                    .child("Error")
                    .e2e("span", "discover-chip-error")
                    .text("Error"),
            );
        }
        chip.e2e("button", "discover-chip")
            .attr("data-server", server.to_owned())
            .attr("data-on", on.to_string())
            .text(server.to_owned())
            .into_any_element()
    }
}

impl Render for Discover {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.take_focus) {
            self.search.read(cx).focus(window);
        }
        let chips: Vec<AnyElement> = self
            .dirs
            .all()
            .iter()
            .map(|server| self.chip(server, cx))
            .collect();
        let entries = self.results.merged();
        let rows: Vec<AnyElement> = entries.iter().map(|entry| self.row(entry, cx)).collect();
        // One line per problem: a directory's own error, the typed name's, a join's.
        let mut problems: Vec<(Option<String>, String)> = self
            .results
            .errors()
            .into_iter()
            .filter(|(server, _)| self.dirs.is_on(server))
            .map(|(server, err)| (Some(server.to_owned()), format!("{server}: {err}")))
            .collect();
        problems.extend(self.input_error.clone().map(|e| (None, e)));
        problems.extend(self.join_error.clone().map(|e| (None, e)));
        let selected = self.dirs.selected();
        let info = if selected.is_empty() {
            Some("Switch on a server above to see its public rooms.".to_owned())
        } else if self.results.loading() && entries.is_empty() {
            Some("Loading…".to_owned())
        } else if entries.is_empty() && problems.is_empty() {
            Some("No public rooms found.".to_owned())
        } else {
            None
        };
        let list = div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .children(rows)
            .children(problems.into_iter().map(|(server, text)| {
                div()
                    .px(px(8.0))
                    .py(px(6.0))
                    .text_color(rgb(theme::danger()))
                    .child(text.clone())
                    .e2e("p", "discover-status error")
                    .attr("data-server", server.unwrap_or_default())
                    .text(text)
            }))
            .when_some(info, |d, text| {
                d.child(
                    div()
                        .px(px(8.0))
                        .py(px(10.0))
                        .text_color(rgb(theme::text_muted()))
                        .child(text.clone())
                        .e2e("p", "discover-status muted")
                        .text(text),
                )
            })
            .when(self.results.has_more() && !self.results.loading(), |d| {
                d.child(button(
                    "discover-more",
                    None,
                    "Load more",
                    "secondary",
                    false,
                    false,
                    cx.listener(|this, _, _, cx| this.more(cx)),
                ))
            });
        div()
            .id("discover-page")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(Closed);
                }
            }))
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .bg(rgb(theme::bg_chat()))
            .child(
                div()
                    .id("discover-scroll")
                    .w_full()
                    .h_full()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(760.0))
                            .flex()
                            .flex_col()
                            .gap(px(12.0))
                            .p(px(24.0))
                            .child(
                                div()
                                    .text_size(px(1.5 * theme::REM))
                                    .font_weight(FontWeight::BOLD)
                                    .child("Discover")
                                    .e2e("h1", "discover-title")
                                    .text("Discover"),
                            )
                            .child(
                                div()
                                    .text_color(rgb(theme::text_muted()))
                                    .child("Public rooms and spaces anyone can join, from the servers switched on below."),
                            )
                            .child(self.search.clone())
                            .child(div().flex().flex_wrap().gap(px(6.0)).children(chips))
                            .child(self.server_input.clone())
                            .child(list),
                    ),
            )
            .e2e("div", "discover")
            .focusable(&self.focus)
    }
}

impl crate::shell::Shell {
    /// The compass: browse public rooms to join, on a page of its own.
    pub fn open_discover(&mut self, cx: &mut Context<Self>) {
        if self.discover.is_some() {
            return self.close_discover(cx);
        }
        let Some(session) = self.session.clone() else {
            return;
        };
        let joined: HashSet<String> = self.rooms.iter().map(|r| r.id.to_string()).collect();
        let saved = self.prefs.extra.get("discover_directories").cloned();
        let page = cx.new(|cx| Discover::new(session, joined, saved, cx));
        cx.subscribe(&page, |this, _, _: &Closed, cx| this.close_discover(cx))
            .detach();
        cx.subscribe(&page, |this, _, choice: &Choice, cx| {
            this.prefs
                .extra
                .insert("discover_directories".into(), choice.0.to_json());
            this.save_prefs(cx);
        })
        .detach();
        cx.subscribe(&page, |this, _, joined: &Joined, cx| {
            this.close_discover(cx);
            this.pending_dm = Some(joined.0.clone());
            this.open_pending_dm(cx);
        })
        .detach();
        cx.subscribe(&page, |this, _, look: &Preview, cx| {
            let room = morpheus_core::LinkedRoom {
                room_id: look.0.room_id.clone(),
                name: look.0.name.clone(),
                topic: look.0.topic.clone(),
                avatar: look.0.avatar.clone(),
                members: look.0.members,
                joined: false,
            };
            // The page stays under the card: closing it comes back here.
            this.preview_room(room, look.1.clone(), cx);
        })
        .detach();
        self.discover_place = Some(self.place());
        self.discover = Some(page);
        cx.notify();
    }

    /// Back from Discover to exactly where it was opened from: nothing of the
    /// room, scope or scroll behind it was touched.
    pub fn close_discover(&mut self, cx: &mut Context<Self>) {
        if self.discover.take().is_some() {
            self.discover_place = None;
            cx.notify();
        }
    }

    /// Where the user is, as far as Discover cares: leaving it by going
    /// anywhere else (a server in the rail, a notification, a hotkey) ends it.
    pub fn place(&self) -> Place {
        (
            self.scope.clone(),
            self.selected.clone(),
            self.welcome.clone(),
            self.friends_open,
        )
    }
}

/// Scope, open room, open server overview and whether Friends shows.
pub type Place = (
    crate::model::Scope,
    Option<morpheus_core::OwnedRoomId>,
    Option<morpheus_core::OwnedRoomId>,
    bool,
);
