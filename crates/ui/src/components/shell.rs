use dioxus::prelude::*;
use super::toast::report;
use morpheus_core::{Notification, OwnedRoomId, RoomSummary, Session, Vector};

use super::{
    composer::{ActiveTimeline, Composer, Draft, Mode},
    context_menu::{ContextMenu, ContextMenuLayer, MenuState},
    hotkeys::{Drops, FILE_LISTENER_JS, FileDrops, Hotkey, HotkeyRequest, LISTENER_JS, parse_dropped},
    invites::InviteCard,
    lightbox::{EscapeTick, Escapes, Lightbox, LightboxOpen, LightboxShown, provide_lightbox},
    members::Members,
    message_dialogs::{
        MessageDialog, OpenMessageDialog, Pinned, PinnedIds, PinnedPanel, PinnedRefresh,
        ReactionsModal, ReportModal, use_pinned_fetcher,
    },
    profile::{
        EditProfileModal, Editor, OpenProfile, ProfileCard, ProfileEditor, ProfileModal,
        ProfileRefresh, ProfileTarget, ProfileVersion, StatusModal,
    },
    rail::{Dragging, Rail, RailDrag},
    room_dialogs::{
        CreateRoomModal, CreateSpaceModal, EditRoomModal, InviteModal, LeaveSpaceModal,
        OpenRoomDialog, PrivacyModal, RenameFolderModal, RoomDialog, RoomProfileModal,
    },
    room_list::RoomList,
    settings::{ActiveVerification, Settings, SettingsOpen, track_verification},
    switcher::QuickSwitcher,
    thread::{ActiveThread, ThreadPanel},
    timeline::{LastOwn, LastOwnMessage, TimelineView},
    titlebar::{HelpModal, InboxPanel, Nav, OpenTopPanel, TitleBar, TopPanel, nav_jump},
    toast::{ToastLayer, ToastList, Toasts},
    user_bar::UserBar,
};
use crate::WindowControls;
use crate::icons::Icon;
use crate::media::{Avatar, provide_media_cache};

/// Messages announced since launch, oldest first (capped), for the inbox's
/// mentions list and the test bridge.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Notices(pub Vec<Notification>);
pub type NoticeLog = Signal<Notices>;
const NOTICE_CAP: usize = 50;

/// Rooms as the core reports them, kept in sync for the whole shell.
pub type Rooms = Signal<Vector<RoomSummary>>;
/// Currently open room.
pub type Selected = Signal<Option<OwnedRoomId>>;
/// Whether the members column is shown (the header toggle flips it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MembersShown(pub bool);
pub type MembersOpen = Signal<MembersShown>;
/// A display name to insert into the composer (member menu "Mention").
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Mention(pub Option<String>);
pub type MentionRequest = Signal<Mention>;
/// Visited rooms for the title bar's back and forward buttons.
pub type NavHistory = Signal<Nav>;

/// Layout preferences synced through account data (`dev.morpheus.ui`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Prefs {
    pub hide_muted: bool,
    /// Rail order: spaces and folders of spaces. Spaces missing here are
    /// appended in the order the server lists them.
    pub rail: Vec<RailEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RailEntry {
    Space(OwnedRoomId),
    Folder {
        id: String,
        name: String,
        spaces: Vec<OwnedRoomId>,
    },
}

impl Prefs {
    pub fn from_json(value: &serde_json::Value) -> Self {
        let rail = value
            .get("rail")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        if let Some(space) = item.get("space").and_then(|v| v.as_str()) {
                            return space.parse().ok().map(RailEntry::Space);
                        }
                        let id = item.get("folder")?.as_str()?.to_owned();
                        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("Folder").to_owned();
                        let spaces = item
                            .get("spaces")
                            .and_then(|v| v.as_array())
                            .map(|s| s.iter().filter_map(|v| v.as_str()?.parse().ok()).collect())
                            .unwrap_or_default();
                        Some(RailEntry::Folder { id, name, spaces })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            hide_muted: value.get("hide_muted").and_then(|v| v.as_bool()).unwrap_or(false),
            rail,
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        let rail: Vec<serde_json::Value> = self
            .rail
            .iter()
            .map(|entry| match entry {
                RailEntry::Space(id) => serde_json::json!({ "space": id }),
                RailEntry::Folder { id, name, spaces } => serde_json::json!({ "folder": id, "name": name, "spaces": spaces }),
            })
            .collect();
        serde_json::json!({ "hide_muted": self.hide_muted, "rail": rail })
    }

    /// Remove a space from wherever it sits (dropping folders left empty).
    pub fn detach(&mut self, space: &OwnedRoomId) {
        self.rail.retain(|e| !matches!(e, RailEntry::Space(id) if id == space));
        for entry in &mut self.rail {
            if let RailEntry::Folder { spaces, .. } = entry {
                spaces.retain(|id| id != space);
            }
        }
        self.rail.retain(|e| !matches!(e, RailEntry::Folder { spaces, .. } if spaces.is_empty()));
    }

    pub fn folder_mut(&mut self, folder_id: &str) -> Option<&mut Vec<OwnedRoomId>> {
        self.rail.iter_mut().find_map(|e| match e {
            RailEntry::Folder { id, spaces, .. } if id == folder_id => Some(spaces),
            _ => None,
        })
    }

    /// A fresh folder id (not a room id, just unique within the layout).
    pub fn new_folder_id(&self) -> String {
        let n = self.rail.iter().filter(|e| matches!(e, RailEntry::Folder { .. })).count();
        format!("folder-{}-{n}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0))
    }
}
pub type UiPrefs = Signal<Prefs>;
/// Bumped when member state changed outside the members panel (nicknames),
/// so it refetches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MembersStamp(pub u32);
pub type MembersVersion = Signal<MembersStamp>;
/// What the sidebar lists: direct messages, ungrouped rooms, or one space.
/// Its own type, not a bare `Option<OwnedRoomId>`: Dioxus context is keyed by
/// type, and sharing a type with [`Selected`] made room clicks land on the
/// scope signal.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Scope {
    /// Direct messages (rooms with a DM target).
    Dms,
    /// Rooms that are not DMs and not inside any space.
    #[default]
    Rooms,
    /// A top-level space and everything nested under it.
    Space(OwnedRoomId),
}

impl Scope {
    pub fn space_id(&self) -> Option<&OwnedRoomId> {
        match self {
            Self::Space(id) => Some(id),
            _ => None,
        }
    }
}
pub type SelectedSpace = Signal<Scope>;

/// Rooms visible for the current scope.
///
/// Discord-like rule: `Dms` lists direct messages, `Rooms` lists rooms that
/// are neither DMs nor inside any space, and a space shows the rooms that
/// list it (or one of its nested spaces) as a parent, so a space's whole
/// subtree shows up under one badge.
pub fn rooms_in_scope(rooms: &Vector<RoomSummary>, scope: &Scope) -> Vec<RoomSummary> {
    // Invites are listed separately (see `invites.rs`), never as rooms.
    let space_id = match scope {
        Scope::Dms => {
            return rooms
                .iter()
                .filter(|room| room.is_dm && !room.is_space && !room.invited)
                .cloned()
                .collect();
        }
        Scope::Rooms => {
            return rooms
                .iter()
                .filter(|room| !room.is_space && !room.is_dm && !room.invited && room.parent_spaces.is_empty())
                .cloned()
                .collect();
        }
        Scope::Space(id) => id,
    };

    // Grow the scope to every space reachable from the selected one through
    // nested `m.space.parent` relationships.
    let mut scope = std::collections::HashSet::new();
    scope.insert(space_id.clone());
    loop {
        let mut grew = false;
        for space in rooms.iter().filter(|room| room.is_space) {
            if !scope.contains(&space.id) && space.parent_spaces.iter().any(|p| scope.contains(p)) {
                scope.insert(space.id.clone());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }

    rooms
        .iter()
        .filter(|room| !room.is_space && !room.invited && room.parent_spaces.iter().any(|p| scope.contains(p)))
        .cloned()
        .collect()
}

#[component]
pub fn Shell(session: Session) -> Element {
    use_context_provider(|| session.clone());
    provide_media_cache();
    let mut rooms: Rooms = use_context_provider(|| Signal::new(Vector::new()));
    let mut selected: Selected = use_context_provider(|| Signal::new(None));
    let mut selected_space: SelectedSpace = use_context_provider(|| Signal::new(Scope::default()));
    let mut active_thread: ActiveThread = use_context_provider(|| Signal::new(None));
    let mut load_error = use_signal(|| None::<String>);
    let mut switcher_open = use_signal(|| false);
    let mut settings_open: SettingsOpen = use_context_provider(|| Signal::new(false));
    let mut verification: ActiveVerification = use_context_provider(|| Signal::new(None));
    let mut members_open: MembersOpen = use_context_provider(|| Signal::new(MembersShown(true)));
    let mut open_profile: OpenProfile = use_context_provider(|| Signal::new(ProfileTarget::default()));
    let mut context_menu: ContextMenu = use_context_provider(|| Signal::new(MenuState::default()));
    let mut editor: ProfileEditor = use_context_provider(|| Signal::new(Editor::None));
    let _mention: MentionRequest = use_context_provider(|| Signal::new(Mention::default()));
    let mut message_dialog: OpenMessageDialog = use_context_provider(|| Signal::new(MessageDialog::None));
    let mut room_dialog: OpenRoomDialog = use_context_provider(|| Signal::new(RoomDialog::None));
    let mut top_panel: OpenTopPanel = use_context_provider(|| Signal::new(TopPanel::None));
    let nav: NavHistory = use_context_provider(|| Signal::new(Nav::default()));
    let lightbox_open: LightboxOpen = use_context_provider(|| Signal::new(LightboxShown(false)));
    let mut escapes: EscapeTick = use_context_provider(|| Signal::new(Escapes(0)));
    let mut prefs: UiPrefs = use_context_provider(|| Signal::new(Prefs::default()));
    let _drag: RailDrag = use_context_provider(|| Signal::new(Dragging::default()));
    let _toasts: Toasts = use_context_provider(|| Signal::new(ToastList::default()));
    let _members_version: MembersVersion = use_context_provider(|| Signal::new(MembersStamp::default()));
    // New messages from other people: logged for the inbox, and raised as an
    // OS notification unless they land in the room being looked at.
    let mut notices: NoticeLog = use_context_provider(|| Signal::new(Notices::default()));
    let controls = use_hook(try_consume_context::<WindowControls>);
    use_hook({
        let session = session.clone();
        move || {
            spawn(async move {
                let mut updates = match session.notifications().await {
                    Ok(updates) => updates,
                    Err(err) => {
                        report("notification subscription", &err);
                        return;
                    }
                };
                while let Some(notice) = updates.next().await {
                    let looking = selected.peek().as_ref() == Some(&notice.room_id)
                        && controls.as_ref().is_some_and(|c| (c.is_focused)());
                    if !looking && let Some(controls) = &controls {
                        let title = if notice.is_mention {
                            format!("{} mentioned you in {}", notice.sender_name, notice.room_name)
                        } else {
                            format!("{} in {}", notice.sender_name, notice.room_name)
                        };
                        (controls.notify)(title, notice.body_summary.clone());
                    }
                    notices.with_mut(|log| {
                        if log.0.len() >= NOTICE_CAP {
                            log.0.remove(0);
                        }
                        log.0.push(notice);
                    });
                }
            })
        }
    });
    use_future({
        let session = session.clone();
        move || {
            let session = session.clone();
            async move {
                if let Ok(value) = session.ui_prefs().await {
                    prefs.set(Prefs::from_json(&value));
                }
            }
        }
    });
    // History is recorded by `go_to`, not by an effect on `selected`: an
    // effect only runs after the next render and would miss a visit when two
    // changes land before one.
    let _ = nav;
    let pinned: Pinned = use_context_provider(|| Signal::new(PinnedIds::default()));
    let pinned_refresh: PinnedRefresh = use_context_provider(|| Signal::new(0));
    use_pinned_fetcher(session.clone(), selected, pinned_refresh, pinned);
    let _refresh: ProfileRefresh = use_context_provider(|| Signal::new(ProfileVersion::default()));

    // Incoming verification requests from the user's other devices pop the settings dialog.
    use_future({
        let session = session.clone();
        move || {
            let session = session.clone();
            async move {
                let Ok(mut requests) = session.verification_requests().await else {
                    return;
                };
                while let Some(handle) = requests.next().await {
                    verification.set(Some(track_verification(handle, true)));
                    settings_open.set(true);
                }
            }
        }
    });

    let session_for_keys = session.clone();
    use_future(move || {
        let session = session.clone();
        async move {
            match session.room_list().await {
                Ok(mut updates) => {
                    while let Some(diffs) = updates.next().await {
                        rooms.with_mut(|list| diffs.into_iter().for_each(|diff| diff.apply(list)));
                    }
                }
                Err(err) => {
                    report("room list", &err);
                    load_error.set(Some(err.to_string()));
                }
            }
        }
    });

    // Keep the open room inside the current space scope: pick the first
    // room in scope on load, and again whenever the rail selection changes
    // the scope out from under the open room (like Discord does).
    use_effect(move || {
        let in_scope = rooms_in_scope(&rooms.read(), &selected_space.read());
        // A room we just created (or a DM just opened) is selected before the
        // list has it; leave that alone rather than bouncing elsewhere.
        // Invites show in every scope, so an open invite stays put too.
        let unknown = selected
            .peek()
            .as_ref()
            .is_some_and(|id| !rooms.read().iter().any(|room| &room.id == id && !room.invited));
        if unknown {
            return;
        }
        let still_open = selected
            .peek()
            .as_ref()
            .is_some_and(|id| in_scope.iter().any(|room| &room.id == id));
        if !still_open {
            tracing::debug!(
                previous = ?selected.peek(),
                in_scope = in_scope.len(),
                rooms = rooms.peek().len(),
                scope = ?selected_space.peek(),
                "re-picking the open room"
            );
            match in_scope.first() {
                Some(room) => go_to(room.id.clone()),
                None => selected.set(None),
            }
        }
    });

    let current = use_memo(move || {
        let id = selected()?;
        rooms.read().iter().find(|room| room.id == id).cloned()
    });

    // A thread belongs to the room it was opened from; close it when switching
    // rooms. (Menus built from a room's signals close when that room's
    // component unmounts, see `ChatRoom`.)
    use_effect(move || {
        selected();
        active_thread.set(None);
    });

    // Escape closes whichever modal is open, top-most first. A running
    // verification keeps going in the background. Driven by a window-level
    // listener (below) so it works whatever has focus.
    // Returns whether anything was open to close.
    let mut close_top_modal = move || -> bool {
        if lightbox_open().0 {
            escapes.with_mut(|e| e.0 += 1);
        } else if context_menu().0.is_some() {
            context_menu.set(MenuState::default());
        } else if top_panel() != TopPanel::None {
            top_panel.set(TopPanel::None);
        } else if room_dialog() != RoomDialog::None {
            room_dialog.set(RoomDialog::None);
        } else if message_dialog() != MessageDialog::None {
            message_dialog.set(MessageDialog::None);
        } else if editor() != Editor::None {
            editor.set(Editor::None);
        } else if open_profile().0.is_some() {
            open_profile.set(ProfileTarget::default());
        } else if settings_open() {
            settings_open.set(false);
        } else if switcher_open() {
            switcher_open.set(false);
        } else {
            return false;
        }
        true
    };
    // Keyboard shortcuts (Discord's set; see hotkeys.rs). The JS listener
    // names the combo; everything the shell owns is handled here, the rest
    // is handed to the owning component through `HotkeyRequest`.
    let mut hotkey: HotkeyRequest = use_context_provider(|| Signal::new(None));
    let mut dispatch = move |name: String| {
        let session = session_for_keys.clone();
        // Rooms as the sidebar lists them, for next/previous.
        let in_scope = move || rooms_in_scope(&rooms.read(), &selected_space.read());
        let mut open = move |room: &RoomSummary| {
            let room_scope = scope_for(&rooms.read(), room);
            if room_scope != *selected_space.peek() {
                selected_space.set(room_scope);
            }
            go_to(room.id.clone());
        };
        let step = move |list: &[RoomSummary], step: isize, unread_only: bool| -> Option<RoomSummary> {
            let candidates: Vec<&RoomSummary> = list
                .iter()
                .filter(|r| !unread_only || r.unread_messages > 0 || r.highlight_count > 0 || r.marked_unread)
                .collect();
            if candidates.is_empty() {
                return None;
            }
            let current = selected.peek().clone();
            let pos = list.iter().position(|r| Some(&r.id) == current.as_ref());
            let next = match (pos, step.is_positive()) {
                (Some(i), true) => candidates.iter().position(|r| list.iter().position(|x| x.id == r.id) > Some(i)).unwrap_or(0),
                (Some(i), false) => candidates.iter().rposition(|r| list.iter().position(|x| x.id == r.id) < Some(i)).unwrap_or(candidates.len() - 1),
                (None, _) => 0,
            };
            Some(candidates[next].clone())
        };
        let scopes = move || {
            let mut all = vec![Scope::Dms, Scope::Rooms];
            all.extend(
                rooms
                    .read()
                    .iter()
                    .filter(|r| r.is_space && r.parent_spaces.is_empty())
                    .map(|r| Scope::Space(r.id.clone())),
            );
            all
        };
        match name.as_str() {
            "escape" => {
                if !close_top_modal() {
                    hotkey.set(Some(Hotkey::JumpToBottom));
                }
            }
            "switcher" => switcher_open.toggle(),
            "shortcuts" => top_panel.set(if top_panel() == TopPanel::Help { TopPanel::None } else { TopPanel::Help }),
            "inbox" => top_panel.set(if top_panel() == TopPanel::Inbox { TopPanel::None } else { TopPanel::Inbox }),
            "members" => members_open.with_mut(|m| m.0 = !m.0),
            "pins" => message_dialog.set(MessageDialog::Pinned),
            "settings" => settings_open.set(true),
            "attach" => hotkey.set(Some(Hotkey::Attach)),
            "edit_last" => hotkey.set(Some(Hotkey::EditLast)),
            "back" => nav_jump(-1, nav, rooms, selected_space, selected),
            "forward" => nav_jump(1, nav, rooms, selected_space, selected),
            "next_room" | "prev_room" | "next_unread" | "prev_unread" => {
                let list = in_scope();
                let forward = name.starts_with("next");
                if let Some(room) = step(&list, if forward { 1 } else { -1 }, name.ends_with("unread")) {
                    open(&room);
                }
            }
            "next_scope" | "prev_scope" => {
                let all = scopes();
                let current = all.iter().position(|s| *s == *selected_space.peek()).unwrap_or(0);
                let next = if name == "next_scope" { (current + 1) % all.len() } else { (current + all.len() - 1) % all.len() };
                selected_space.set(all[next].clone());
            }
            "mark_scope_read" => {
                for room in in_scope() {
                    let future = session.mark_room_read(room.id);
                    spawn(async move {
                        if let Err(err) = future.await {
                            report("mark read", &err);
                        }
                    });
                }
            }
            _ => {}
        }
    };
    // Dropped and pasted files arrive on the same channel, for the composer.
    let mut drops: FileDrops = use_context_provider(|| Signal::new(Drops::default()));
    use_hook(move || {
        spawn(async move {
            let mut listener = document::eval(&format!("{LISTENER_JS}\n{FILE_LISTENER_JS}"));
            while let Ok(name) = listener.recv::<String>().await {
                if let Some(payload) = name.strip_prefix("file:") {
                    match parse_dropped(payload) {
                        Some(file) => drops.with_mut(|d| d.0.push(file)),
                        None => tracing::warn!("ignoring an unreadable dropped file"),
                    }
                } else {
                    dispatch(name);
                }
            }
        })
    });
    let side_column = match (current(), active_thread(), members_open().0) {
        (Some(_), Some(_), _) => SideColumn::Thread,
        (Some(_), None, true) => SideColumn::Members,
        _ => SideColumn::None,
    };

    rsx! {
        div {
            class: "shell",
            class: if side_column == SideColumn::None { "no-side" },
            tabindex: "-1",
            // Targets open their own menu and stop the event; everything else
            // gets no native menu at all.
            oncontextmenu: move |e| {
                e.prevent_default();
                context_menu.set(MenuState::default());
            },
            TitleBar {}
            div { class: "drop-hint", "Drop to upload" }
            ToastLayer {}
            // Mirror of what was announced, for the test bridge.
            ul { class: "notification-log", hidden: true,
                for (index, notice) in notices().0.iter().enumerate() {
                    li {
                        key: "{index}",
                        "data-room": "{notice.room_id}",
                        "data-mention": if notice.is_mention { "true" } else { "false" },
                        "{notice.sender_name}: {notice.body_summary}"
                    }
                }
            }
            div { class: "shell-body",
            Rail {}
            RoomList { error: load_error() }
            UserBar {}
            main { class: "chat",
                match current() {
                    // Keyed on the room id so switching rooms remounts the timeline.
                    Some(room) if room.invited => rsx! { InviteCard { key: "invite-{room.id}", room } },
                    Some(room) => rsx! { ChatRoom { key: "{room.id}", room } },
                    None => rsx! {
                        div { class: "empty-state",
                            h2 { "Pick a room" }
                            p { class: "muted", "Your conversations will show up here." }
                        }
                    },
                }
            }
            match (side_column, current(), active_thread()) {
                (SideColumn::Thread, Some(room), Some(root)) => rsx! { ThreadPanel { room_id: room.id.clone(), root } },
                (SideColumn::Members, Some(room), _) => rsx! { Members { room_id: room.id.clone(), member_count: room.member_count } },
                _ => rsx! {},
            }
            }
            match top_panel() {
                TopPanel::Inbox => rsx! { InboxPanel {} },
                TopPanel::Help => rsx! { HelpModal {} },
                TopPanel::None => rsx! {},
            }
            if switcher_open() {
                QuickSwitcher { open: switcher_open }
            }
            if settings_open() {
                Settings {}
            }
            match open_profile().0 {
                Some(open) if open.full => rsx! { ProfileModal { user_id: open.user_id } },
                Some(open) => rsx! { ProfileCard { open } },
                None => rsx! {},
            }
            match editor() {
                Editor::Profile => rsx! { EditProfileModal {} },
                Editor::Status => rsx! { StatusModal {} },
                Editor::None => rsx! {},
            }
            match (message_dialog(), current()) {
                (MessageDialog::Reactions(message), _) => rsx! { ReactionsModal { message: *message } },
                (MessageDialog::Report(event_id), _) => rsx! { ReportModal { event_id } },
                (MessageDialog::Pinned, Some(room)) => rsx! { PinnedPanel { room_id: room.id.clone() } },
                _ => rsx! {},
            }
            match room_dialog() {
                RoomDialog::Invite(room_id) => rsx! { InviteModal { room_id } },
                RoomDialog::Edit(room_id) => rsx! { EditRoomModal { room_id } },
                RoomDialog::Create => rsx! { CreateRoomModal {} },
                RoomDialog::Privacy(room_id) => rsx! { PrivacyModal { room_id } },
                RoomDialog::RoomProfile(room_id) => rsx! { RoomProfileModal { room_id } },
                RoomDialog::CreateSpace(parent) => rsx! { CreateSpaceModal { parent } },
                RoomDialog::LeaveSpace(space_id) => rsx! { LeaveSpaceModal { space_id } },
                RoomDialog::RenameFolder(folder_id) => rsx! { RenameFolderModal { folder_id } },
                RoomDialog::None => rsx! {},
            }
            ContextMenuLayer {}
        }
    }
}

#[component]
fn ChatRoom(room: RoomSummary) -> Element {
    let session = use_context::<Session>();
    // Menus built from this room's signals must not outlive it.
    use_drop(move || consume_context::<ContextMenu>().set(MenuState::default()));
    let mut members_open = use_context::<MembersOpen>();
    let mut message_dialog = use_context::<OpenMessageDialog>();
    let pinned = use_context::<Pinned>();
    // Opening a room clears a manual unread flag, like reading does. Only on
    // open (mount): flagging the room you are in keeps it flagged until you
    // come back, as in Discord. The hook call itself is unconditional.
    use_hook({
        let session = session.clone();
        let room_id = room.id.clone();
        let flagged = room.marked_unread;
        move || {
            if flagged {
                spawn(async move {
                    if let Err(err) = session.mark_unread(room_id, false).await {
                        report("clearing the unread flag", &err);
                    }
                });
            }
        }
    });
    // Shared by the timeline (which fills it) and the composer (which sends through it).
    use_context_provider(|| ActiveTimeline::new(None));
    use_context_provider(|| Mode::new(None));
    use_context_provider(|| Draft::new(String::new()));
    use_context_provider(|| LastOwnMessage::new(LastOwn::default()));
    provide_lightbox();
    let mut typing = use_signal(Vec::<String>::new);

    use_future({
        let room_id = room.id.clone();
        move || {
            let session = session.clone();
            let room_id = room_id.clone();
            async move {
                match session.typing(room_id).await {
                    Ok(mut updates) => {
                        while let Some(names) = updates.next().await {
                            typing.set(names);
                        }
                    }
                    Err(err) => report("typing subscription", &err),
                }
            }
        }
    });

    rsx! {
        header { class: "chat-header",
            if room.is_dm {
                Avatar { media: room.avatar.clone(), fallback: room.name.clone(), size: 24 }
            } else {
                span { class: "hash", "#" }
            }
            h2 { "{room.name}" }
            if let Some(topic) = room.topic.as_deref() {
                span { class: "topic", "{topic}" }
            }
            button {
                class: "ghost pin-toggle",
                title: "Pinned messages",
                onclick: move |_| message_dialog.set(MessageDialog::Pinned),
                Icon { glyph: crate::icons::PIN, size: 16 }
                "{pinned().0.len()}"
            }
            button {
                class: "ghost member-toggle",
                class: if members_open().0 { "active" },
                title: "Toggle member list",
                onclick: move |_| members_open.with_mut(|shown| shown.0 = !shown.0),
                Icon { glyph: crate::icons::USERS, size: 16 }
                "{room.member_count}"
            }
        }
        TimelineView { room_id: room.id.clone() }
        TypingLine { names: typing() }
        Composer {
            room_name: room.name.clone(),
            placeholder: room.is_dm.then(|| format!("Message @{}", room.name)),
            accepts_drops: true,
            room_id: Some(room.id.clone()),
        }
        Lightbox {}
    }
}

#[component]
fn TypingLine(names: Vec<String>) -> Element {
    let text = match names.as_slice() {
        [] => return rsx! { div { class: "typing empty" } },
        [one] => format!("{one} is typing…"),
        [a, b] => format!("{a} and {b} are typing…"),
        [a, b, ..] => format!("{a}, {b} and others are typing…"),
    };
    rsx! { div { class: "typing", "{text}" } }
}

/// What the right-hand column holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SideColumn {
    Thread,
    Members,
    None,
}

/// The rail scope a room belongs to: DMs, Rooms, or its top-level space.
/// Used when something other than the rail opens a room (the quick switcher),
/// so the sidebar follows instead of bouncing the selection back.
pub fn scope_for(rooms: &Vector<RoomSummary>, room: &RoomSummary) -> Scope {
    if room.is_dm {
        return Scope::Dms;
    }
    let mut current = room.parent_spaces.first().cloned();
    let mut hops = 0;
    while let Some(space_id) = current {
        let Some(space) = rooms.iter().find(|r| r.id == space_id) else {
            return Scope::Rooms;
        };
        match space.parent_spaces.first() {
            None => return Scope::Space(space_id),
            Some(parent) if hops < 16 => {
                hops += 1;
                current = Some(parent.clone());
            }
            Some(_) => return Scope::Rooms,
        }
    }
    Scope::Rooms
}

/// Open a room and remember the visit for back/forward. Every navigation
/// that is not itself back/forward goes through here.
pub fn go_to(room_id: OwnedRoomId) {
    let mut nav = consume_context::<NavHistory>();
    nav.with_mut(|n| n.record(room_id.clone()));
    consume_context::<Selected>().set(Some(room_id));
}
