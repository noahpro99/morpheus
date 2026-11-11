//! What the menus of rooms, direct messages and servers do (`room_menu` in
//! `view_model` decides what they offer): marks on rooms and people kept in
//! the account, mutes that end, favorites, leaving a room (undoable),
//! friends, and the layout preferences.

use std::time::Duration;

use gpui::{AppContext as _, ClipboardItem, Context, Pixels, Point};
use morpheus_core::{NotifyMode, OwnedRoomId, RoomSummary, marks};

use crate::{
    dialogs::{Closed, Dialog, PendingUndo},
    room_dialogs::{Done, RoomDialog},
    menu::OpenMenu,
    model::{Scope, rooms_in_scope},
    shell::Shell,
    view_model::{
        menu::{MenuAction, MenuTarget},
        room_menu::{DmState, dm_menu, invite_targets, room_menu, space_menu},
        nicknames::with_nickname,
        room_dialogs::RoomDialogKind,
    },
};

/// Milliseconds since the epoch.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// How often timed mutes are looked at.
const MUTE_CHECK: Duration = Duration::from_secs(5);

impl Shell {
    /// Reads the marks and friends from the account, and ends mutes whose
    /// time is up as they come.
    pub fn load_marks(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        for kind in [
            marks::PINNED,
            marks::CLOSED,
            marks::IGNORED,
            marks::MUTED_UNTIL,
        ] {
            let reading = session.marks(kind);
            cx.spawn(async move |this, cx| match reading.await {
                Ok(map) => {
                    this.update(cx, |this, cx| {
                        *this.marks.map_mut(kind) = map;
                        this.reopen_closed(cx);
                        cx.notify();
                    })
                    .ok();
                }
                Err(err) => {
                    this.update(cx, |this, cx| this.fail("loading your marks", &err.to_string(), cx))
                        .ok();
                }
            })
            .detach();
        }
        self.refresh_friends(cx);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(MUTE_CHECK).await;
                let alive = this
                    .update(cx, |this, cx| {
                        for room in this.marks.ended_mutes(now_ms()) {
                            if let Ok(room) = OwnedRoomId::try_from(room.as_str()) {
                                this.unmute(room, cx);
                            }
                        }
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    fn refresh_friends(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let reading = session.friends();
        cx.spawn(async move |this, cx| {
            if let Ok(friends) = reading.await {
                this.update(cx, |this, cx| {
                    this.friends = friends;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// A closed DM that has a newer message is open again: its mark goes.
    pub fn reopen_closed(&mut self, cx: &mut Context<Self>) {
        let dms: Vec<RoomSummary> = self
            .rooms
            .iter()
            .filter(|room| room.is_dm)
            .cloned()
            .collect();
        let back = self
            .marks
            .reopened(&self.marks.visible_dms(dms, self.selected.as_ref(), false));
        for id in back {
            self.set_mark(marks::CLOSED, id, None, cx);
        }
    }

    /// Sets or clears one mark: here at once, and in the account.
    pub fn set_mark(&mut self, kind: &'static str, key: String, at: Option<i64>, cx: &mut Context<Self>) {
        self.marks.set(kind, &key, at);
        cx.notify();
        if let Some(session) = self.session.clone() {
            let saving = session.set_mark(kind, key, at);
            self.run_in_background("saving", saving, cx);
        }
    }

    fn mute(&mut self, room: OwnedRoomId, until: i64, cx: &mut Context<Self>) {
        self.marks.muted_now.insert(room.to_string(), true);
        if let Some(session) = self.session.clone() {
            let muting = session.set_notify_mode(room.clone(), NotifyMode::Mute);
            self.run_in_background("mute", muting, cx);
        }
        self.set_mark(marks::MUTED_UNTIL, room.to_string(), Some(until), cx);
    }

    fn unmute(&mut self, room: OwnedRoomId, cx: &mut Context<Self>) {
        self.marks.muted_now.insert(room.to_string(), false);
        if let Some(session) = self.session.clone() {
            let unmuting = session.set_notify_mode(room.clone(), NotifyMode::AllMessages);
            self.run_in_background("unmute", unmuting, cx);
        }
        self.set_mark(marks::MUTED_UNTIL, room.to_string(), None, cx);
    }

    /// The menu of a room or DM right-clicked in the list (built after a
    /// quick lookup: the room's notification mode, or whether you blocked
    /// the person in a DM).
    pub fn open_room_menu(&mut self, room: RoomSummary, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        match room.dm_user.clone().filter(|_| room.is_dm) {
            Some(user) => {
                let blocked = session.is_blocked(user.clone());
                cx.spawn(async move |this, cx| {
                    let blocked = blocked.await.unwrap_or(false);
                    this.update(cx, |this, cx| {
                        let state = DmState {
                            pinned: this.marks.is_pinned(&room),
                            ignored: this.marks.ignored.contains_key(user.as_str()),
                            blocked,
                            muted: this.marks.is_muted(&room),
                            friend: this.friends.contains(&user),
                            invite_to: invite_targets(&this.rooms),
                            name: room.name.clone(),
                            user,
                        };
                        let items = dm_menu(&room, &state);
                        this.open_menu(OpenMenu::new(at, items, false, MenuTarget::Other), cx);
                    })
                    .ok();
                })
                .detach();
            }
            None => {
                let mode = session.notify_mode(room.id.clone());
                cx.spawn(async move |this, cx| {
                    let mode = mode.await.ok();
                    this.update(cx, |this, cx| {
                        let items = room_menu(&room, mode);
                        this.open_menu(OpenMenu::new(at, items, false, MenuTarget::Other), cx);
                    })
                    .ok();
                })
                .detach();
            }
        }
    }

    /// The menu of a server: its name clicked, or its badge right-clicked.
    pub fn open_space_menu(&mut self, space: &OwnedRoomId, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(space) = self.rooms.iter().find(|r| &r.id == space).cloned() else {
            return;
        };
        let children = rooms_in_scope(&self.rooms, &Scope::Space(space.id.clone()));
        let folders = self.prefs.folder_choices(&space.id);
        let items = space_menu(&space, &children, self.prefs.hide_muted, &folders);
        self.open_menu(OpenMenu::new(at, items, false, MenuTarget::Other), cx);
    }

    /// Runs what a room, server or person menu item asked for.
    pub fn run_room_action(&mut self, action: MenuAction, cx: &mut Context<Self>) {
        let session = self.session.clone();
        match action {
            MenuAction::MarkRead(rooms) => {
                if let Some(session) = session {
                    for room in rooms {
                        let reading = session.mark_room_read(room);
                        self.run_in_background("mark read", reading, cx);
                    }
                }
            }
            MenuAction::MarkRoomUnread(room) => {
                if let Some(session) = session {
                    let marking = session.mark_room_unread(room);
                    self.run_in_background("mark unread", marking, cx);
                }
            }
            MenuAction::Notify(rooms, mode) => {
                if let Some(session) = session {
                    for room in rooms {
                        let setting = session.set_notify_mode(room, mode);
                        self.run_in_background("notification mode", setting, cx);
                    }
                }
            }
            MenuAction::Leave(room, name) => {
                // No "are you sure": it goes from the list now, undoable a moment.
                self.hidden_rooms.insert(room.to_string());
                self.undoable(&format!("Left #{name}"), PendingUndo::Leave(room), cx);
            }
            MenuAction::Favorite(room, favorite) => {
                if let Some(session) = session {
                    let setting = session.set_favorite(room, favorite);
                    self.run_in_background("favorites", setting, cx);
                }
            }
            MenuAction::PinDm(room, pin) => {
                self.set_mark(marks::PINNED, room.to_string(), pin.then(now_ms), cx);
            }
            MenuAction::CloseDm(room) => {
                // Read, or its unread messages would bring it straight back.
                if let Some(session) = session {
                    let reading = session.mark_room_read(room.clone());
                    self.run_in_background("mark read", reading, cx);
                }
                self.set_mark(marks::CLOSED, room.to_string(), Some(now_ms()), cx);
                if self.selected.as_ref() == Some(&room) {
                    self.close_room(cx);
                }
            }
            MenuAction::Ignore(user, ignore) => {
                self.set_mark(marks::IGNORED, user.to_string(), ignore.then(now_ms), cx);
            }
            MenuAction::Block(user, block) => {
                if let Some(session) = session {
                    let blocking = session.block(user, block);
                    self.moderate("block", blocking, cx);
                    self.refresh_profile_block(cx);
                }
            }
            MenuAction::SetRole(room, user, role) => {
                if let Some(session) = session {
                    let setting = session.set_role(room, user, role);
                    self.moderate("role change", setting, cx);
                }
            }
            MenuAction::Kick(room, user) => {
                if let Some(session) = session {
                    let kicking = session.kick(room, user, None);
                    self.moderate("kick", kicking, cx);
                }
            }
            MenuAction::Ban(room, user) => {
                if let Some(session) = session {
                    let banning = session.ban(room, user, None);
                    self.moderate("ban", banning, cx);
                }
            }
            MenuAction::Message(user) => self.message_user(user, cx),
            MenuAction::Mention(name) => {
                if let Some((_, composer)) = &self.composer {
                    composer.update(cx, |composer, cx| composer.mention(&name, cx));
                }
            }
            MenuAction::Mute(room, length) => {
                let until = if length == 0 { 0 } else { now_ms() + length };
                self.mute(room, until, cx);
            }
            MenuAction::Unmute(room) => self.unmute(room, cx),
            MenuAction::Friend(user, friend) => {
                if let Some(session) = session {
                    let setting = session.set_friend(user, friend);
                    cx.spawn(async move |this, cx| {
                        let result = setting.await;
                        this.update(cx, |this, cx| {
                            if let Err(err) = result {
                                this.fail("friend", &err.to_string(), cx);
                            }
                            this.refresh_friends(cx);
                        })
                        .ok();
                    })
                    .detach();
                }
            }
            MenuAction::Invite(room, user) => {
                if let Some(session) = session {
                    let inviting = session.invite(room, user);
                    self.run_in_background("invite", inviting, cx);
                }
            }
            MenuAction::ToggleHideMuted => {
                self.prefs.hide_muted = !self.prefs.hide_muted;
                self.save_prefs(cx);
                cx.notify();
            }
            MenuAction::MoveToFolder(space, folder) => {
                self.prefs.move_to_folder(&space, &folder);
                self.save_prefs(cx);
                cx.notify();
            }
            MenuAction::MoveToNewFolder(space) => {
                self.prefs.move_to_new_folder(&space, now_ms());
                self.save_prefs(cx);
                cx.notify();
            }
            MenuAction::MoveOutOfFolders(space) => {
                self.prefs.move_out_of_folders(&space);
                self.save_prefs(cx);
                cx.notify();
            }
            MenuAction::Profile(user) => {
                let at = Some(self.menu_at);
                self.open_profile(user, at, None, cx);
            }
            MenuAction::FullProfile(_) => self.show_full_profile(cx),
            MenuAction::ShareProfile(link) => {
                cx.write_to_clipboard(ClipboardItem::new_string(link));
                self.toast("Link copied", cx);
            }
            MenuAction::RoomDialog(kind) => self.open_room_dialog(kind, cx),
            MenuAction::CreateChannel(space) => {
                self.set_scope(Scope::Space(space), cx);
                self.open_room_dialog(RoomDialogKind::Create, cx);
            }
            MenuAction::ReportUser(_) => {
                if let Some(card) = &self.card {
                    card.update(cx, |card, cx| card.start_report(cx));
                }
            }
            MenuAction::CopyText(text) | MenuAction::CopyLink(text) | MenuAction::CopyId(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
            // The rest is for messages.
            _ => {}
        }
    }

    /// Leaves the open room's page (a DM was closed).
    fn close_room(&mut self, cx: &mut Context<Self>) {
        self.selected = None;
        self.timeline = None;
        self.composer = None;
        self.timeline_handle = None;
        self.members = None;
        self.pinned.clear();
        cx.notify();
    }

    /// The layout preferences, kept in the account.
    pub fn save_prefs(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = self.session.clone() {
            let saving = session.set_ui_prefs(self.prefs.to_json());
            self.run_in_background("save prefs", saving, cx);
        }
    }
}

impl Shell {
    /// Opens one of the room dialogs over the chat.
    pub fn open_room_dialog(&mut self, kind: RoomDialogKind, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let rooms = self.rooms.clone();
        let scope = self.scope.clone();
        let nicknames = self.nicknames.clone();
        let dialog = cx.new(|cx| RoomDialog::new(kind, session, rooms, scope, &nicknames, cx));
        cx.subscribe(&dialog, |this, _, _: &Closed, cx| this.dialog_close(cx))
            .detach();
        cx.subscribe(&dialog, |this, _, done: &Done, cx| this.room_dialog_done(done, cx))
            .detach();
        self.dialog_open(Dialog::Room(dialog), cx);
    }

    /// A room dialog did its job: land where the new thing is.
    fn room_dialog_done(&mut self, done: &Done, cx: &mut Context<Self>) {
        match done {
            Done::RoomCreated { room, space } => {
                self.dialog_close(cx);
                self.set_scope(space.clone().map_or(Scope::Rooms, Scope::Space), cx);
                self.open_room(room.clone(), cx);
            }
            Done::SpaceCreated { space, parent } => {
                self.dialog_close(cx);
                self.set_scope(Scope::Space(parent.clone().unwrap_or_else(|| space.clone())), cx);
            }
            Done::LeftServer => {
                self.dialog_close(cx);
                self.set_scope(Scope::Rooms, cx);
            }
            Done::ProfileSaved => self.refresh_members(cx),
            Done::NicknameSaved { user, nickname } => {
                self.dialog_close(cx);
                let names = std::mem::take(&mut self.nicknames);
                self.nicknames = with_nickname(names, user.as_str(), nickname);
                self.push_nicknames(cx);
            }
        }
    }
}

impl Shell {
    /// Tells what shows names (the open timeline, member list, message box
    /// and profile card) about your nicknames.
    pub fn push_nicknames(&mut self, cx: &mut Context<Self>) {
        let names = self.nicknames.clone();
        if let Some((_, timeline)) = &self.timeline {
            let shared = std::sync::Arc::new(names.clone());
            timeline.update(cx, |timeline, cx| timeline.set_nicknames(shared, cx));
        }
        if let Some((_, members)) = &self.members {
            let names = names.clone();
            members.update(cx, |members, cx| members.set_nicknames(names, cx));
        }
        if let Some((_, composer)) = &self.composer {
            let names = names.clone();
            composer.update(cx, |composer, cx| composer.set_nicknames(names, cx));
        }
        if let Some(card) = &self.card {
            let nickname = names.get(card.read(cx).user.as_str()).cloned();
            card.update(cx, |card, cx| {
                card.nickname = nickname;
                cx.notify();
            });
        }
        cx.notify();
    }
}

impl Shell {
    /// The mod view of a person, from their card.
    pub fn open_mod_view(
        &mut self,
        user: morpheus_core::OwnedUserId,
        shown: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let shown = shown.unwrap_or_else(|| user.localpart().to_owned());
        let view = cx.new(|cx| crate::mod_view::ModView::new(session, user, shown, cx));
        cx.subscribe(&view, |this, _, _: &Closed, cx| this.dialog_close(cx))
            .detach();
        cx.subscribe(&view, |this, _, _: &crate::mod_view::Moderated, cx| {
            this.refresh_members(cx)
        })
        .detach();
        self.dialog_open(Dialog::ModView(view), cx);
    }
}
