//! The signed-in shell (`crates/ui/src/components/shell.rs`): the rail, the
//! sidebar with its footer, and the chat area, over the rooms the core syncs.

use std::sync::Arc;

use gpui::{Context, Entity, IntoElement, Render, ScrollHandle, Window, div, prelude::*, px, rgb};
use morpheus_core::{MediaRef, OwnedRoomId, Profile, RoomSummary, Session, SpaceRoom, VectorDiff};

use crate::{
    chat,
    composer::Composer,
    e2e::E2eExt as _,
    media::{self, Avatars},
    model::{Scope, rooms_in_scope},
    prefs::Prefs,
    rail, room_list,
    theme,
    timeline::{LinkClicked, TimelineEvent, TimelineView},
    user_bar,
};

pub struct Shell {
    /// Absent only in widget tests, which feed `rooms` themselves.
    pub session: Option<Session>,
    pub rooms: Vec<RoomSummary>,
    pub scope: Scope,
    pub selected: Option<OwnedRoomId>,
    pub prefs: Prefs,
    pub open_folders: Vec<String>,
    pub profile: Option<Profile>,
    /// The rooms the selected server lists, joined or not.
    pub directory: Vec<SpaceRoom>,
    pub avatars: Avatars,
    /// The open room's messages.
    pub timeline: Option<(OwnedRoomId, Entity<TimelineView>)>,
    /// The open room's message box.
    pub composer: Option<(OwnedRoomId, Entity<Composer>)>,
    /// The sidebar's room list scrolling.
    pub room_scroll: ScrollHandle,
    /// Links handed to the browser (the test bridge's `.link-log`).
    pub opened_links: Vec<String>,
}

impl Shell {
    pub fn new(session: Session, cx: &mut Context<Self>) -> Self {
        let mut shell = Self::bare(Some(session.clone()));
        let subscribed = session.room_list();
        cx.spawn(async move |this, cx| {
            let Ok(mut updates) = subscribed.await else {
                return;
            };
            while let Some(diffs) = updates.next().await {
                let alive = this
                    .update(cx, |this, cx| {
                        for diff in diffs {
                            apply(&mut this.rooms, diff);
                        }
                        crate::model::adopt_children(&mut this.rooms);
                        cx.notify();
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();
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
        let prefs = session.ui_prefs();
        cx.spawn(async move |this, cx| {
            if let Ok(value) = prefs.await {
                this.update(cx, |this, cx| {
                    this.prefs = Prefs::from_json(&value);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        shell.scope = Scope::Rooms;
        shell
    }

    /// A shell with no account behind it, for widget tests.
    pub fn bare(session: Option<Session>) -> Self {
        Self {
            session,
            rooms: Vec::new(),
            scope: Scope::default(),
            selected: None,
            prefs: Prefs::default(),
            open_folders: Vec::new(),
            profile: None,
            directory: Vec::new(),
            avatars: Avatars::default(),
            timeline: None,
            composer: None,
            room_scroll: ScrollHandle::new(),
            opened_links: Vec::new(),
        }
    }

    /// The open room, if it is one we have joined.
    pub fn current(&self) -> Option<&RoomSummary> {
        let id = self.selected.as_ref()?;
        self.rooms.iter().find(|r| &r.id == id)
    }

    /// What a rail badge does: list another scope.
    pub fn set_scope(&mut self, scope: Scope, cx: &mut Context<Self>) {
        self.scope = scope.clone();
        self.directory.clear();
        if let (Some(session), Scope::Space(id)) = (&self.session, &scope) {
            let listing = session.space_rooms(id.clone());
            cx.spawn(async move |this, cx| {
                let list = listing.await.unwrap_or_default();
                this.update(cx, |this, cx| {
                    if this.scope == scope {
                        this.directory = list;
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    pub fn open_room(&mut self, id: OwnedRoomId, cx: &mut Context<Self>) {
        if self.session.is_some() && self.timeline.as_ref().is_none_or(|(room, _)| room != &id) {
            let session = self.session.clone();
            let room = id.clone();
            let timeline = cx.new(|cx| TimelineView::new(session, room.clone(), cx));
            cx.subscribe(&timeline, |this: &mut Self, _, event: &LinkClicked, cx| {
                this.link_clicked(&event.0, cx)
            })
            .detach();
            let (name, is_dm) = self
                .rooms
                .iter()
                .find(|r| r.id == id)
                .map(|r| (r.name.clone(), r.is_dm))
                .unwrap_or_default();
            let composer = cx.new(|cx| Composer::new(self.session.clone(), room.clone(), name, is_dm, cx));
            let target = composer.clone();
            cx.subscribe(&timeline, move |_, _, event: &TimelineEvent, cx| {
                target.update(cx, |composer, cx| match event {
                    TimelineEvent::Opened(handle) => composer.opened(handle.clone(), cx),
                    TimelineEvent::Reply(message) => composer.reply_to(message, cx),
                    TimelineEvent::Edit(message) => composer.edit(message, cx),
                    TimelineEvent::LastOwn(message) => composer.set_last_own(message.clone()),
                })
            })
            .detach();
            // The old one is dropped here: leaving a room reads it.
            self.timeline = Some((id.clone(), timeline));
            self.composer = Some((id.clone(), composer));
        }
        self.selected = Some(id);
        cx.notify();
    }

    /// A link in a message: Matrix links open in the app (not yet ported),
    /// others in the browser; test runs only log the hand-off.
    fn link_clicked(&mut self, url: &str, cx: &mut Context<Self>) {
        if morpheus_core::parse_matrix_link(url).is_some() {
            return;
        }
        if std::env::var_os("MORPHEUS_E2E_PORT").is_some() {
            self.opened_links.push(url.to_owned());
            cx.notify();
        } else {
            cx.open_url(url);
        }
    }

    /// Starts fetching every picture the frame will draw.
    fn fetch_avatars(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let mut wanted: Vec<(MediaRef, u32)> = Vec::new();
        for room in &self.rooms {
            let size = if room.is_space { 48 } else { 32 };
            wanted.extend(room.avatar.clone().map(|m| (m, size)));
        }
        wanted.extend(
            self.profile
                .as_ref()
                .and_then(|p| p.avatar.clone())
                .map(|m| (m, 34)),
        );
        wanted.extend(
            self.rooms
                .iter()
                .filter(|r| r.is_space)
                .filter_map(|r| r.avatar.clone())
                .flat_map(|m| [(m.clone(), 40), (m, 18)]),
        );
        wanted.extend(
            self.current()
                .and_then(|r| r.avatar.clone())
                .map(|m| (m, 24)),
        );
        for (media, size) in wanted {
            if !self.avatars.begin(&media, size) {
                continue;
            }
            let fetching = session.thumbnail(media.clone(), size * 2, size * 2);
            cx.spawn(async move |this, cx| {
                let image = fetching
                    .await
                    .ok()
                    .flatten()
                    .and_then(|bytes| media::decode(&bytes));
                this.update(cx, |this, cx| {
                    this.avatars.finish(&media, size, image);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    /// The picture for `media` at `size`, if fetched.
    pub fn picture(&self, media: &Option<MediaRef>, size: u32) -> Option<Arc<gpui::Image>> {
        media.as_ref().and_then(|m| self.avatars.get(m, size))
    }
}

/// One batch change of the core's list, applied to a plain vector.
pub fn apply(rooms: &mut Vec<RoomSummary>, diff: VectorDiff<RoomSummary>) {
    use VectorDiff::*;
    match diff {
        Append { values } => rooms.extend(values),
        Clear => rooms.clear(),
        PushFront { value } => rooms.insert(0, value),
        PushBack { value } => rooms.push(value),
        PopFront => {
            if !rooms.is_empty() {
                rooms.remove(0);
            }
        }
        PopBack => {
            rooms.pop();
        }
        Insert { index, value } => rooms.insert(index.min(rooms.len()), value),
        Set { index, value } => {
            if let Some(slot) = rooms.get_mut(index) {
                *slot = value;
            }
        }
        Remove { index } => {
            if index < rooms.len() {
                rooms.remove(index);
            }
        }
        Truncate { length } => rooms.truncate(length),
        Reset { values } => *rooms = values.into_iter().collect(),
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.fetch_avatars(cx);
        // The Rooms scope is empty of rooms at first; nothing else to wait for.
        let _ = rooms_in_scope;
        let left = div()
            .flex()
            .flex_col()
            .w(px(theme::RAIL_W + theme::SIDEBAR_W))
            .flex_none()
            .h_full()
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(rail::render(self, cx))
                    .child(room_list::render(self, cx)),
            )
            .child(user_bar::render(self, cx));
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme::BG_CHAT))
            .child(
                div()
                    .size_0()
                    .children(
                        self.opened_links
                            .iter()
                            .map(|url| div().size_0().e2e("li", "").text(url.clone())),
                    )
                    .e2e("ul", "link-log"),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(left)
                    .child(chat::render(self, cx))
                    .e2e("div", "shell-body"),
            )
            .e2e("div", "shell")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::room;

    #[test]
    fn diffs_apply_in_order() {
        let mut rooms = vec![room("!a:x", "a")];
        apply(
            &mut rooms,
            VectorDiff::PushBack {
                value: room("!b:x", "b"),
            },
        );
        apply(
            &mut rooms,
            VectorDiff::Insert {
                index: 0,
                value: room("!c:x", "c"),
            },
        );
        apply(&mut rooms, VectorDiff::Remove { index: 1 });
        let names: Vec<_> = rooms.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["c", "b"]);
        apply(&mut rooms, VectorDiff::Clear);
        assert!(rooms.is_empty());
    }
}

/// Widget tests: the shell drawn on gpui's test platform, headless, with
/// rooms fed in by hand. They read the frame's tagged tree and click at
/// element centres, so hit-testing is covered too.
#[cfg(all(test, feature = "e2e"))]
mod widget_tests {
    use super::*;
    use crate::e2e::{self, query};
    use crate::model::tests::room;
    use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point};

    /// The window's root: the shell in a fresh tree per frame.
    struct Harness(Entity<Shell>);

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            e2e::frame(div().size_full().child(self.0.clone()))
        }
    }

    fn open(
        cx: &mut TestAppContext,
        rooms: Vec<RoomSummary>,
    ) -> (Entity<Shell>, &mut VisualTestContext) {
        let (harness, cx) = cx.add_window_view(|_, cx| Harness(cx.new(|_| Shell::bare(None))));
        let shell = harness.read_with(cx, |h, _| h.0.clone());
        shell.update(cx, |s, cx| {
            s.rooms = rooms;
            cx.notify();
        });
        cx.run_until_parked();
        cx.draw(
            point(px(0.0), px(0.0)),
            gpui::size(px(1100.0), px(720.0)),
            |_, _| div(),
        );
        cx.run_until_parked();
        (shell, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &str) {
        let (_, (x, y)) = query(selector)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no {selector}"));
        cx.simulate_click(point(px(x), px(y)), Modifiers::none());
        cx.run_until_parked();
    }

    fn texts(selector: &str) -> Vec<String> {
        query(selector).into_iter().map(|(t, _)| t).collect()
    }

    fn seeded() -> Vec<RoomSummary> {
        let mut space = room("!s:x", "Engineering");
        space.is_space = true;
        let mut dev = room("!dev:x", "dev");
        dev.parent_spaces = vec![space.id.clone()];
        let mut bob = room("!bob:x", "bob");
        bob.is_dm = true;
        let mut random = room("!r:x", "random");
        random.unread_messages = 2;
        random.topic = Some("anything goes".into());
        vec![room("!g:x", "general"), random, dev, space, bob]
    }

    #[gpui::test]
    fn the_rail_selects_what_the_sidebar_lists(cx: &mut TestAppContext) {
        let (_shell, cx) = open(cx, seeded());
        assert_eq!(texts(".sidebar-header h2"), ["Rooms"]);
        assert_eq!(texts(".room .room-name"), ["general", "random"]);
        assert_eq!(query(".rail-item.selected[title=\"Rooms\"]").len(), 1);
        assert_eq!(query(".rail-item.selected").len(), 1);

        click(cx, ".rail-item[title=\"Engineering\"]");
        assert_eq!(texts(".sidebar-header h2"), ["Engineering"]);
        assert_eq!(texts(".room .room-name"), ["dev"]);
        assert_eq!(query(".rail-item.selected[title=\"Engineering\"]").len(), 1);

        click(cx, ".rail-item[title=\"Direct messages\"]");
        assert_eq!(texts(".sidebar-header h2"), ["Direct messages"]);
        assert_eq!(texts(".room.dm .room-name"), ["bob"]);
        assert!(
            query(".create-room-button").is_empty(),
            "no create button in DMs"
        );
    }

    #[gpui::test]
    fn opening_a_room_puts_its_name_in_the_header(cx: &mut TestAppContext) {
        let (shell, cx) = open(cx, seeded());
        assert_eq!(texts(".empty-state h2"), ["Pick a room"]);
        assert!(query(".chat-header").is_empty());

        click(cx, ".room[data-room=\"!r:x\"]");
        assert_eq!(texts(".chat-header h2"), ["random"]);
        assert_eq!(texts(".chat-header .topic"), ["anything goes"]);
        assert_eq!(query(".room.selected").len(), 1);
        assert!(query(".empty-state").is_empty());
        shell.read_with(cx, |s, _| {
            assert_eq!(s.current().map(|r| r.name.as_str()), Some("random"))
        });
    }

    #[gpui::test]
    fn rooms_arriving_later_appear_and_unread_ones_are_marked(cx: &mut TestAppContext) {
        let (shell, cx) = open(cx, seeded());
        assert_eq!(query(".room.unread").len(), 1);
        shell.update(cx, |s, cx| {
            s.rooms.push(room("!n:x", "new-room"));
            cx.notify();
        });
        cx.run_until_parked();
        cx.draw(
            point(px(0.0), px(0.0)),
            gpui::size(px(1100.0), px(720.0)),
            |_, _| div(),
        );
        assert!(texts(".room .room-name").contains(&"new-room".to_owned()));
    }

    #[gpui::test]
    fn the_footer_shows_a_name_not_an_id(cx: &mut TestAppContext) {
        let (shell, _cx) = open(cx, Vec::new());
        assert_eq!(texts(".sidebar-footer .profile-chip .user"), [""]);
        shell.read_with(_cx, |s, _| assert!(s.profile.is_none()));
        assert_eq!(texts(".sidebar .room-list .muted"), ["No rooms here."]);
    }
}
