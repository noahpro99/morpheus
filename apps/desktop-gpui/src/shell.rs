//! The signed-in shell (`crates/ui/src/components/shell.rs`): the rail, the
//! sidebar with its footer, and the chat area, over the rooms the core syncs.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use gpui::{
    Context, Entity, FocusHandle, IntoElement, Render, ScrollHandle, Window, div, prelude::*, px,
    rgb,
};
use morpheus_core::{
    MediaRef, MessageId, OwnedEventId, OwnedRoomId, OwnedUserId, Profile, RoomSummary, Session, SpaceRoom,
    TimelineHandle,
    VectorDiff,
};

use crate::{
    chat,
    composer::Composer,
    dialogs::{self, Dialog, PendingUndo},
    e2e::E2eExt as _,
    media::{self, Avatars},
    members::{MemberEvent, MembersView},
    emoji_picker::EmojiPicker,
    menu::{self, OpenMenu},
    profile::ProfileCard,
    model::{Scope, rooms_in_scope},
    view_model::{marks::Marks, nicknames::NickMap, toast::ToastList},
    prefs::Prefs,
    rail, room_list,
    settings::{KeysChanged, SettingsView, ThemeChosen},
    theme,
    timeline::{LinkClicked, TimelineEvent, TimelineView},
    user_bar,
    view_model::keys::{KeyAccess, SettingsFocus, show_backup_prompt},
};

/// How often the lock state is re-read.
const KEYS_RECHECK: std::time::Duration = std::time::Duration::from_secs(30);

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
    /// The settings window, shown or not (absent only in widget tests).
    pub settings: Option<Entity<SettingsView>>,
    /// Whether this device can read encrypted history.
    pub keys: KeyAccess,
    /// "Later" on the Secure Backup offer, until the app next starts.
    pub backup_later: bool,
    /// The right-click menu, if open.
    pub menu: Option<OpenMenu>,
    pub menu_focus: Option<FocusHandle>,
    /// The dialog over the chat, if any.
    pub dialog: Option<Dialog>,
    pub dialog_focus: Option<FocusHandle>,
    pub take_dialog_focus: bool,
    pub toasts: ToastList,
    /// Deletions waiting out their Undo toast, by toast.
    pub pending: HashMap<u32, PendingUndo>,
    /// Rooms just left, still undoable: out of the lists.
    pub hidden_rooms: HashSet<String>,
    /// Private marks: pinned and closed DMs, ignored people, timed mutes.
    pub marks: Marks,
    /// The people you are friends with.
    pub friends: Vec<OwnedUserId>,
    /// The full emoji picker, and the message it reacts to.
    pub picker: Option<(Entity<EmojiPicker>, MessageId)>,
    /// The profile card (or full profile) that is open.
    pub card: Option<Entity<ProfileCard>>,
    /// Where the last menu opened, for what it opens in its place.
    pub menu_at: gpui::Point<gpui::Pixels>,
    /// The open room's member list.
    pub members: Option<(OwnedRoomId, Entity<MembersView>)>,
    /// Whether the member list shows (kept across rooms).
    pub members_open: bool,
    /// The member count the list was last read at.
    pub members_count: u64,
    /// The open room's timeline, to act on its messages.
    pub timeline_handle: Option<TimelineHandle>,
    /// The open room's pinned messages.
    pub pinned: Vec<OwnedEventId>,
    /// Your nicknames for people.
    pub nicknames: NickMap,
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
                        this.reopen_closed(cx);
                        this.sync_members(cx);
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
                    let theme = this.prefs.theme();
                    if let Some(settings) = &this.settings {
                        settings.update(cx, |settings, _| settings.theme = theme);
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        let nicknames = session.nicknames();
        cx.spawn(async move |this, cx| {
            if let Ok(names) = nicknames.await {
                this.update(cx, |this, cx| {
                    this.nicknames = names;
                    this.push_nicknames(cx);
                })
                .ok();
            }
        })
        .detach();
        shell.scope = Scope::Rooms;
        let settings = cx.new(|cx| SettingsView::new(Some(session), cx));
        cx.subscribe(&settings, |this, _, _: &KeysChanged, cx| this.refresh_keys(cx))
            .detach();
        cx.subscribe(&settings, |this, _, chosen: &ThemeChosen, cx| {
            this.prefs.set_theme(chosen.0);
            this.save_prefs(cx);
        })
        .detach();
        shell.settings = Some(settings);
        shell.refresh_keys(cx);
        shell.load_marks(cx);
        // Keys can arrive from elsewhere: look again now and then.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(KEYS_RECHECK).await;
                if this.update(cx, |this, cx| this.refresh_keys(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
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
            settings: None,
            keys: KeyAccess::default(),
            backup_later: false,
            menu: None,
            menu_focus: None,
            dialog: None,
            dialog_focus: None,
            take_dialog_focus: false,
            toasts: ToastList::default(),
            pending: HashMap::new(),
            hidden_rooms: HashSet::new(),
            marks: Marks::default(),
            friends: Vec::new(),
            picker: None,
            card: None,
            menu_at: gpui::Point::default(),
            members: None,
            members_open: true,
            members_count: 0,
            timeline_handle: None,
            pinned: Vec::new(),
            nicknames: NickMap::new(),
        }
    }

    /// Opens Settings, on the page `focus` asks for.
    pub fn open_settings(&mut self, focus: SettingsFocus, cx: &mut Context<Self>) {
        if let Some(settings) = &self.settings {
            settings.update(cx, |settings, cx| settings.open(focus, cx));
        }
    }

    /// Reads whether this device can read encrypted history, and tells the
    /// open timeline (its locked rows and banner follow).
    pub fn refresh_keys(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let status = session.encryption_status();
        cx.spawn(async move |this, cx| {
            if let Ok(status) = status.await {
                this.update(cx, |this, cx| {
                    let keys = KeyAccess::from_status(&status);
                    if keys != this.keys {
                        this.keys = keys;
                        if let Some((_, timeline)) = &this.timeline {
                            timeline.update(cx, |timeline, cx| timeline.set_keys(keys, cx));
                        }
                        cx.notify();
                    }
                })
                .ok();
            }
        })
        .detach();
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
                    TimelineEvent::OpenSettings(_)
                    | TimelineEvent::Menu(..)
                    | TimelineEvent::Picker(..)
                    | TimelineEvent::Delete(..)
                    | TimelineEvent::Sender(..)
                    | TimelineEvent::SenderMenu(..)
                    | TimelineEvent::Failed(..) => {}
                })
            })
            .detach();
            cx.subscribe(&timeline, |this: &mut Self, _, event: &TimelineEvent, cx| match event {
                TimelineEvent::Opened(handle) => this.timeline_handle = Some(handle.clone()),
                TimelineEvent::Menu(message, at) => this.open_message_menu(message, *at, cx),
                TimelineEvent::Picker(message, at) => this.open_picker(message.id.clone(), *at, cx),
                TimelineEvent::Delete(message, skip) => {
                    this.delete_message((**message).clone(), *skip, cx)
                }
                TimelineEvent::Failed(what, err) => this.fail(what, err, cx),
                TimelineEvent::SenderMenu(message, at) => this.open_sender_menu(message, *at, cx),
                TimelineEvent::Sender(message, at) => {
                    let seen = Some((message.sender_name.clone(), message.sender_avatar.clone()));
                    this.open_profile(message.sender.clone(), Some(*at), seen, cx)
                }
                _ => {}
            })
            .detach();
            cx.subscribe(&timeline, |this: &mut Self, _, event: &TimelineEvent, cx| {
                if let TimelineEvent::OpenSettings(focus) = event {
                    this.open_settings(*focus, cx);
                }
            })
            .detach();
            let keys = self.keys;
            timeline.update(cx, |timeline, cx| timeline.set_keys(keys, cx));
            // The old one is dropped here: leaving a room reads it.
            self.timeline = Some((id.clone(), timeline));
            self.composer = Some((id.clone(), composer));
            self.timeline_handle = None;
            self.pinned.clear();
        }
        if self.session.is_some() && self.members.as_ref().is_none_or(|(room, _)| room != &id) {
            let session = self.session.clone();
            let room = id.clone();
            let members = cx.new(|cx| MembersView::new(session, room, cx));
            cx.subscribe(&members, |this: &mut Self, _, event: &MemberEvent, cx| match event {
                MemberEvent::Context(member, permissions, at) => {
                    this.open_member_menu(member, *permissions, *at, cx)
                }
                MemberEvent::Open(member, at) => {
                    let seen = Some((member.name.clone(), member.avatar.clone()));
                    this.open_profile(member.user_id.clone(), Some(*at), seen, cx)
                }
            })
            .detach();
            self.members_count = self
                .rooms
                .iter()
                .find(|r| r.id == id)
                .map_or(0, |r| r.member_count);
            self.members = Some((id.clone(), members));
        }
        self.selected = Some(id);
        self.push_nicknames(cx);
        self.refresh_pinned(cx);
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

/// The Secure Backup offer: a card in the corner while the account has no
/// key backup (a new account's first sign-in), until set up or "Later".
fn backup_prompt(cx: &mut Context<Shell>) -> impl IntoElement {
    let action = |id: &'static str, label: &'static str, primary: bool| {
        div()
            .id(id)
            .px(px(12.0))
            .py(px(8.0))
            .rounded(px(theme::RADIUS))
            .cursor_pointer()
            .bg(if primary {
                rgb(theme::ACCENT)
            } else {
                gpui::rgba(theme::BG_HOVER).into()
            })
            .text_color(if primary { rgb(0xffffff) } else { rgb(theme::TEXT) })
            .child(label)
    };
    div()
        .absolute()
        .right(px(16.0))
        .top(px(48.0))
        .w(px(360.0))
        .flex()
        .gap(px(12.0))
        .px(px(16.0))
        .py(px(14.0))
        .rounded(px(10.0))
        .bg(rgb(theme::BG_ELEVATED))
        .shadow_md()
        .occlude()
        .child(
            div()
                .mt(px(2.0))
                .text_color(rgb(theme::ACCENT))
                .child(crate::icons::icon("key", 18.0).text_color(rgb(theme::ACCENT))),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(4.0))
                .text_size(px(0.9 * theme::REM))
                .child(
                    div()
                        .font_weight(gpui::FontWeight::BOLD)
                        .child("Set up Secure Backup")
                        .e2e("strong", "")
                        .text("Set up Secure Backup"),
                )
                .child(
                    div()
                        .text_color(rgb(theme::TEXT_MUTED))
                        .child("Keep your encrypted messages if you lose this device: you'll get a recovery key to save.")
                        .e2e("span", "")
                        .text("Keep your encrypted messages if you lose this device: you'll get a recovery key to save."),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .justify_end()
                        .mt(px(6.0))
                        .child(
                            action("backup-later", "Later", false)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.backup_later = true;
                                    cx.notify();
                                }))
                                .e2e("button", "secondary")
                                .text("Later"),
                        )
                        .child(
                            action("backup-set-up", "Set up", true)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.open_settings(SettingsFocus::SetUpBackup, cx)
                                }))
                                .e2e("button", "primary")
                                .text("Set up"),
                        )
                        .e2e("div", "backup-prompt-actions"),
                )
                .e2e("div", "backup-prompt-text"),
        )
        .e2e("div", "backup-prompt")
        .attr("role", "status")
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.fetch_avatars(cx);
        let menu = menu::render(self, window, cx);
        let dialog = dialogs::render_dialog(self, window, cx);
        let toasts = dialogs::render_toasts(self, cx);
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
            .relative()
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
            .when(show_backup_prompt(self.keys, self.backup_later), |d| {
                d.child(backup_prompt(cx))
            })
            .children(self.settings.clone())
            .children(dialog)
            .children(self.card.clone())
            .children(self.picker.as_ref().map(|(picker, _)| picker.clone()))
            .children(menu)
            .children(toasts)
            .e2e("div", if self.members_open { "shell" } else { "shell no-side" })
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
