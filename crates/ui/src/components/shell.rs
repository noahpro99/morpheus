use super::toast::report;
use dioxus::prelude::*;
use morpheus_core::{Notification, OwnedRoomId, RoomSummary, Session, Vector};

use super::about::{AboutOpen, AboutShown, Acknowledgements, check_for_updates};
use super::{
    composer::{ActiveTimeline, Composer, Draft, Mode},
    context_menu::{ContextMenu, ContextMenuLayer, MenuState},
    hotkeys::{
        Drops, FILE_LISTENER_JS, FileDrops, Hotkey, HotkeyRequest, LISTENER_JS, parse_dropped,
    },
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
    rail::Rail,
    room_dialogs::{
        CreateRoomModal, CreateSpaceModal, EditRoomModal, InviteModal, LeaveSpaceModal,
        OpenRoomDialog, PrivacyModal, RenameFolderModal, RoomDialog, RoomProfileModal,
    },
    room_list::RoomList,
    settings::{ActiveVerification, OpenSettingsPage, Settings, SettingsOpen, track_verification},
    switcher::QuickSwitcher,
    thread::{ActiveThread, ThreadPanel},
    timeline::{LastOwn, LastOwnMessage, TimelineView},
    titlebar::{HelpModal, InboxPanel, Nav, OpenTopPanel, TitleBar, TopPanel, nav_jump},
    toast::{ToastLayer, ToastList, Toasts},
    user_bar::UserBar,
};
use crate::icons::Icon;
use crate::media::{Avatar, provide_media_cache};

/// Messages announced since launch, oldest first (capped), for the inbox's
/// mentions list and the test bridge.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Notices(pub Vec<Notification>);
pub type NoticeLog = Signal<Notices>;
const NOTICE_CAP: usize = 50;
/// Wait after the phone layout applies before pane slides switch on.
const PANES_SETTLE_MS: u64 = 300;
/// Quarter-second looks for a room asked for before the list has it (~10 s).
const OPEN_ROOM_TRIES: u32 = 40;
/// How long a start waits for the room it was last in to reach the list.
const LAST_PLACE_WAIT_SECS: u64 = 20;

/// Phone-width window (reported by the page): one pane at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Narrow(pub bool);
pub type NarrowWindow = Signal<Narrow>;
/// Where the app was last, until it has been reopened. Going somewhere
/// yourself first ([`go_to`]) drops it.
pub type PendingPlace = Signal<Option<crate::device_prefs::LastPlace>>;

/// Narrow layout: whether the chat pane is in front of the room list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChatInFront(pub bool);
pub type ChatOpen = Signal<ChatInFront>;

/// Rooms as the core reports them, kept in sync for the whole shell.
pub type Rooms = Signal<Vector<RoomSummary>>;
/// Currently open room.
pub type Selected = Signal<Option<OwnedRoomId>>;
/// A server's channel you aren't in, opened from the list: the chat area
/// shows a card to join it while it is the selected room.
pub type Preview = Signal<Option<morpheus_core::SpaceRoom>>;

/// Rooms a joined space lists (its `m.space.child`) belong to it even when
/// they don't name it back (no `m.space.parent`), as other clients have it:
/// the space goes into their parents.
fn adopt_children(list: &mut Vector<RoomSummary>) {
    let links: Vec<(OwnedRoomId, OwnedRoomId)> = list
        .iter()
        .filter(|room| room.is_space && !room.invited)
        .flat_map(|space| {
            space
                .space_children
                .iter()
                .map(|child| (child.clone(), space.id.clone()))
        })
        .collect();
    for (child, space) in links {
        let Some(index) = list
            .iter()
            .position(|room| room.id == child && !room.parent_spaces.contains(&space))
        else {
            continue;
        };
        if let Some(room) = list.get_mut(index) {
            room.parent_spaces.push(space);
        }
    }
}
/// Whether the members column is shown (the header toggle flips it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MembersShown(pub bool);
pub type MembersOpen = Signal<MembersShown>;
/// Whether the room search panel is open (it takes the side column).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SearchShown(pub bool);
pub type SearchOpen = Signal<SearchShown>;
/// A message to scroll to and highlight in the open chat (its event id).
/// (Wrapped: contexts are found by type, and a bare `Signal<bool>` or
/// `Signal<Option<String>>` would collide with other contexts.)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JumpTarget(pub Option<String>);
pub type JumpTo = Signal<JumpTarget>;
/// A display name to insert into the composer (member menu "Mention").
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Mention(pub Option<String>);
pub type MentionRequest = Signal<Mention>;
/// Visited rooms for the title bar's back and forward buttons.
pub type NavHistory = Signal<Nav>;

/// The colour themes (CSS variables in theme.css, under `:root[data-theme]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    #[default]
    Dark,
    /// Black surfaces: easier on OLED phones at night.
    Midnight,
    Light,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::Dark, Theme::Midnight, Theme::Light];

    pub fn key(self) -> &'static str {
        match self {
            Theme::Dark => "dark",
            Theme::Midnight => "midnight",
            Theme::Light => "light",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Theme::Dark => "Dark",
            Theme::Midnight => "Midnight",
            Theme::Light => "Light",
        }
    }

    fn from_key(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|theme| theme.key() == key)
            .unwrap_or_default()
    }
}

/// Layout preferences synced through account data (`dev.morpheus.ui`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Prefs {
    pub hide_muted: bool,
    /// Colour theme, the same on every device signed in to the account.
    pub theme: Theme,
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
                        let name = item
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Folder")
                            .to_owned();
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
            hide_muted: value
                .get("hide_muted")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            theme: value
                .get("theme")
                .and_then(|v| v.as_str())
                .map(Theme::from_key)
                .unwrap_or_default(),
            rail,
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        let rail: Vec<serde_json::Value> = self
            .rail
            .iter()
            .map(|entry| match entry {
                RailEntry::Space(id) => serde_json::json!({ "space": id }),
                RailEntry::Folder { id, name, spaces } => {
                    serde_json::json!({ "folder": id, "name": name, "spaces": spaces })
                }
            })
            .collect();
        serde_json::json!({ "hide_muted": self.hide_muted, "rail": rail, "theme": self.theme.key() })
    }

    /// Remove a space from wherever it sits (dropping folders left empty).
    pub fn detach(&mut self, space: &OwnedRoomId) {
        self.rail
            .retain(|e| !matches!(e, RailEntry::Space(id) if id == space));
        for entry in &mut self.rail {
            if let RailEntry::Folder { spaces, .. } = entry {
                spaces.retain(|id| id != space);
            }
        }
        self.rail
            .retain(|e| !matches!(e, RailEntry::Folder { spaces, .. } if spaces.is_empty()));
    }

    pub fn folder_mut(&mut self, folder_id: &str) -> Option<&mut Vec<OwnedRoomId>> {
        self.rail.iter_mut().find_map(|e| match e {
            RailEntry::Folder { id, spaces, .. } if id == folder_id => Some(spaces),
            _ => None,
        })
    }

    /// A fresh folder id (not a room id, just unique within the layout).
    pub fn new_folder_id(&self) -> String {
        let n = self
            .rail
            .iter()
            .filter(|e| matches!(e, RailEntry::Folder { .. }))
            .count();
        format!(
            "folder-{}-{n}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        )
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
    /// Rooms and DMs in your favorites (`m.favourite`).
    Favorites,
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
        Scope::Favorites => {
            return rooms
                .iter()
                .filter(|room| room.favorite && !room.is_space && !room.invited)
                .cloned()
                .collect();
        }
        Scope::Rooms => {
            return rooms
                .iter()
                .filter(|room| {
                    !room.is_space && !room.is_dm && !room.invited && room.parent_spaces.is_empty()
                })
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
        .filter(|room| {
            !room.is_space && !room.invited && room.parent_spaces.iter().any(|p| scope.contains(p))
        })
        .cloned()
        .collect()
}

#[component]
pub fn Shell(session: Session) -> Element {
    use_context_provider(|| session.clone());
    // The account, for remembering where it was (see `pending_place`).
    let place_user = session
        .user_id()
        .map(|id| id.to_string())
        .unwrap_or_default();
    provide_media_cache();
    let mut rooms: Rooms = use_context_provider(|| Signal::new(Vector::new()));
    let mut selected: Selected = use_context_provider(|| Signal::new(None));
    let preview: Preview = use_context_provider(|| Signal::new(None));
    let mut selected_space: SelectedSpace = use_context_provider(|| Signal::new(Scope::default()));
    // The last scope other than the DMs, for Shift+Alt+Right to go back to.
    let mut last_server = use_signal(|| Scope::Rooms);
    use_effect(move || {
        let scope = selected_space();
        if scope != Scope::Dms {
            last_server.set(scope);
        }
    });
    let mut active_thread: ActiveThread = use_context_provider(|| Signal::new(None));
    let mut load_error = use_signal(|| None::<String>);
    let mut switcher_open = use_signal(|| false);
    let mut settings_open: SettingsOpen = use_context_provider(|| Signal::new(false));
    let mut settings_page: OpenSettingsPage = use_context_provider(|| Signal::new(None));
    let _setting_highlight: super::settings::SettingHighlight =
        use_context_provider(|| Signal::new(None));
    let mut verification: ActiveVerification = use_context_provider(|| Signal::new(None));
    let mut members_open: MembersOpen = use_context_provider(|| Signal::new(MembersShown(true)));
    let mut search_open: SearchOpen = use_context_provider(|| Signal::new(SearchShown(false)));
    use_context_provider(|| Signal::new(JumpTarget(None)) as JumpTo);
    // Who was last seen where (name and picture), for cards to open with.
    use_context_provider(|| {
        Signal::new(std::collections::HashMap::new()) as super::profile::SeenPeople
    });
    let mut open_profile: OpenProfile =
        use_context_provider(|| Signal::new(ProfileTarget::default()));
    let mut context_menu: ContextMenu = use_context_provider(|| Signal::new(MenuState::default()));
    let mut editor: ProfileEditor = use_context_provider(|| Signal::new(Editor::None));
    let _mention: MentionRequest = use_context_provider(|| Signal::new(Mention::default()));
    let mut message_dialog: OpenMessageDialog =
        use_context_provider(|| Signal::new(MessageDialog::None));
    let mut room_dialog: OpenRoomDialog = use_context_provider(|| Signal::new(RoomDialog::None));
    let mut top_panel: OpenTopPanel = use_context_provider(|| Signal::new(TopPanel::None));
    let nav: NavHistory = use_context_provider(|| Signal::new(Nav::default()));
    let lightbox_open: LightboxOpen = use_context_provider(|| Signal::new(LightboxShown(false)));
    let mut escapes: EscapeTick = use_context_provider(|| Signal::new(Escapes(0)));
    let mut prefs: UiPrefs = use_context_provider(|| Signal::new(Prefs::default()));
    let has_window = use_hook(|| try_consume_context::<crate::WindowControls>().is_some());
    super::keys::provide_key_tracking(session.clone());
    let mut nickname_target = super::nicknames::provide_nicknames(session.clone());
    let (_, mut friends_open) = super::friends::provide_friends(session.clone());
    super::marks::provide_marks(session.clone());
    super::call::provide_calls(session.clone());
    // Owned by the app root: background tasks (calls, sign-out) run there and
    // toast through it.
    let toasts: Toasts =
        use_context_provider(|| Signal::new_in_scope(ToastList::default(), ScopeId::ROOT));
    // Background tasks (calls, sign-out) run outside any component.
    super::toast::register_shell_toasts(toasts);
    // Messages and rooms an undoable action has hidden for now.
    use_context_provider(|| {
        Signal::new_in_scope(super::toast::HiddenSet::default(), ScopeId::ROOT)
    });
    // Narrow (phone) layout: the page reports its width; one pane shows at a time.
    let mut narrow: NarrowWindow = use_context_provider(|| Signal::new(Narrow::default()));
    // Pane slides are switched on a moment after the phone layout applies,
    // so its first arrangement does not animate (the chat sliding away).
    let mut panes_ready = use_signal(|| false);
    use_effect(move || {
        let is_narrow = narrow().0;
        if is_narrow == *panes_ready.peek() {
            return;
        }
        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(PANES_SETTLE_MS)).await;
            panes_ready.set(narrow.peek().0);
        });
    });
    let mut chat_open: ChatOpen = use_context_provider(|| Signal::new(ChatInFront::default()));
    let _members_version: MembersVersion =
        use_context_provider(|| Signal::new(MembersStamp::default()));
    // Acknowledgements dialog; the tray menu and the palette open it.
    let mut about_open: AboutOpen = use_context_provider(|| Signal::new(AboutShown::default()));
    let commands = use_context::<crate::AppConfig>().commands;
    use_future(move || {
        let commands = commands.clone();
        async move {
            loop {
                match commands.next().await {
                    crate::AppCommand::Acknowledgements => about_open.set(AboutShown(true)),
                    crate::AppCommand::CheckForUpdates => check_for_updates(),
                    crate::AppCommand::OpenRoom(id) => {
                        // A notification tap can start the app: give the room
                        // list a moment to arrive.
                        let mut target = None;
                        for _ in 0..OPEN_ROOM_TRIES {
                            target = rooms
                                .peek()
                                .iter()
                                .find(|room| room.id.as_str() == id)
                                .cloned();
                            if target.is_some() {
                                break;
                            }
                            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                        }
                        match target {
                            Some(room) => {
                                let scope = scope_for(&rooms.peek(), &room);
                                if scope != *selected_space.peek() {
                                    selected_space.set(scope);
                                }
                                go_to(room.id);
                            }
                            None => {
                                tracing::warn!(%id, "asked to open a room that is not in the list")
                            }
                        }
                    }
                }
            }
        }
    });
    // The platform's notifier skips the room on screen; tell it which that is.
    let open_room = use_context::<crate::AppConfig>().open_room;
    use_effect(move || {
        if let Ok(mut slot) = open_room.lock() {
            *slot = selected();
        }
    });
    // New messages from other people, logged for the inbox and the tests.
    let mut notices: NoticeLog = use_context_provider(|| Signal::new(Notices::default()));
    // Links handed to the browser; test runs read this instead of a browser opening.
    let mut opened_links = use_signal(Vec::<String>::new);
    let open_url = use_context::<crate::AppConfig>().open_url;
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
                // OS pop-ups are the platform's job (see `AppConfig::on_session`):
                // this runtime stalls while the window is hidden, a thread does not.
                while let Some(notice) = updates.next().await {
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
    // The theme lives on the document root so overlays and the page
    // background follow it too.
    use_effect(move || {
        let key = prefs().theme.key();
        document::eval(&format!(
            "document.documentElement.dataset.theme = '{key}';"
        ));
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
                        rooms.with_mut(|list| {
                            diffs.into_iter().for_each(|diff| diff.apply(list));
                            adopt_children(list);
                        });
                    }
                }
                Err(err) => {
                    report("room list", &err);
                    load_error.set(Some(err.to_string()));
                }
            }
        }
    });

    // Where this account was when the app last ran (even if it was quit):
    // reopened once that room is in the list. Until then nothing is saved,
    // so the room picked meanwhile does not overwrite it.
    let place_dir = use_context::<crate::AppConfig>().data_dir;
    let mut pending_place: PendingPlace = use_context_provider({
        let (dir, user) = (place_dir.clone(), place_user.clone());
        move || Signal::new(crate::device_prefs::last_place(&dir, &user))
    });
    // A room that is gone (left elsewhere) is not waited for forever.
    use_future(move || async move {
        tokio::time::sleep(std::time::Duration::from_secs(LAST_PLACE_WAIT_SECS)).await;
        pending_place.set(None);
    });
    use_effect(move || {
        let list = rooms.read();
        let Some(place) = pending_place.peek().clone() else {
            return;
        };
        let Some(room) = list
            .iter()
            .find(|room| room.id.as_str() == place.room && !room.invited)
            .cloned()
        else {
            return;
        };
        // A channel's server can reach the list after the channel: until it
        // has, the channel would be filed under Rooms and moved off again.
        let server_listed = room
            .parent_spaces
            .iter()
            .all(|space| list.iter().any(|r| &r.id == space));
        if !server_listed {
            return;
        }
        let scope = scope_for(&list, &room);
        drop(list);
        tracing::info!(room = %room.id, ?scope, "reopening the last room");
        pending_place.set(None);
        if scope != *selected_space.peek() {
            selected_space.set(scope);
        }
        select(room.id);
        if place.chat {
            chat_open.set(ChatInFront(true));
        }
    });
    use_effect(move || {
        let room = selected();
        let chat = chat_open().0;
        if pending_place().is_some() {
            return;
        }
        let Some(room) = room else {
            return;
        };
        let place = crate::device_prefs::LastPlace {
            room: room.to_string(),
            chat,
        };
        if let Err(err) = crate::device_prefs::set_last_place(&place_dir, &place_user, &place) {
            tracing::warn!(%err, "last place not kept");
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
        let unknown = selected.peek().as_ref().is_some_and(|id| {
            !rooms
                .read()
                .iter()
                .any(|room| &room.id == id && !room.invited)
        });
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
                Some(room) => select(room.id.clone()),
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
        } else if about_open().0 {
            about_open.set(AboutShown(false));
        } else if settings_open() {
            // On a phone a settings page steps back to the list first.
            if narrow.peek().0 && settings_page.peek().is_some() {
                settings_page.set(None);
            } else {
                settings_open.set(false);
                settings_page.set(None);
            }
        } else if nickname_target().0.is_some() {
            nickname_target.set(super::nicknames::NicknameTarget(None));
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
        tracing::debug!(%name, "hotkey");
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
        let step =
            move |list: &[RoomSummary], step: isize, unread_only: bool| -> Option<RoomSummary> {
                let candidates: Vec<&RoomSummary> = list
                    .iter()
                    .filter(|r| {
                        !unread_only
                            || r.unread_messages > 0
                            || r.highlight_count > 0
                            || r.marked_unread
                    })
                    .collect();
                if candidates.is_empty() {
                    return None;
                }
                let current = selected.peek().clone();
                let pos = list.iter().position(|r| Some(&r.id) == current.as_ref());
                let next = match (pos, step.is_positive()) {
                    (Some(i), true) => candidates
                        .iter()
                        .position(|r| list.iter().position(|x| x.id == r.id) > Some(i))
                        .unwrap_or(0),
                    (Some(i), false) => candidates
                        .iter()
                        .rposition(|r| list.iter().position(|x| x.id == r.id) < Some(i))
                        .unwrap_or(candidates.len() - 1),
                    (None, _) => 0,
                };
                Some(candidates[next].clone())
            };
        let scopes = move || super::rail::rail_scopes(&prefs.peek(), &rooms.read());
        match name.as_str() {
            "escape" => {
                let mut call_screen = consume_context::<super::call::CallScreenOpen>();
                // On a phone the full-screen call comes down to its bar first.
                if narrow.peek().0 && call_screen.peek().0 {
                    call_screen.set(super::call::CallScreenShown(false));
                } else if close_top_modal() {
                } else if search_open.peek().0 && active_thread.peek().is_none() {
                    // The search panel closes like a dialog would.
                    search_open.set(SearchShown(false));
                } else if narrow.peek().0 && chat_open.peek().0 {
                    // On a phone, Escape (the back key) steps back one pane:
                    // a thread or the members to the chat, the chat to the list.
                    if active_thread.peek().is_some() {
                        active_thread.set(None);
                    } else if members_open.peek().0 {
                        members_open.set(MembersShown(false));
                    } else {
                        chat_open.set(ChatInFront(false));
                    }
                } else {
                    hotkey.set(Some(Hotkey::JumpToBottom));
                }
            }
            // A phone's leftward swipe on the room list: the last chat again.
            "chat_front" => {
                if narrow.peek().0 && current.peek().is_some() {
                    chat_open.set(ChatInFront(true));
                }
            }
            // Shift+Alt+Right: to the DMs, and from them back to the server.
            "dms_toggle" => {
                if *selected_space.peek() == Scope::Dms {
                    let back = last_server.peek().clone();
                    // A server since left: Rooms instead.
                    let gone = matches!(&back, Scope::Space(id) if !rooms.peek().iter().any(|r| &r.id == id));
                    selected_space.set(if gone { Scope::Rooms } else { back });
                } else {
                    last_server.set(selected_space.peek().clone());
                    selected_space.set(Scope::Dms);
                }
            }
            "switcher" => switcher_open.toggle(),
            "reload" => {
                if let Some(mut reloads) = try_consume_context::<crate::app::Reloads>() {
                    reloads.with_mut(|r| r.0 += 1);
                }
            }
            "shortcuts" => top_panel.set(if top_panel() == TopPanel::Help {
                TopPanel::None
            } else {
                TopPanel::Help
            }),
            "inbox" => top_panel.set(if top_panel() == TopPanel::Inbox {
                TopPanel::None
            } else {
                TopPanel::Inbox
            }),
            "members" => members_open.with_mut(|m| m.0 = !m.0),
            "pins" => message_dialog.set(MessageDialog::Pinned),
            "settings" => settings_open.set(true),
            "attach" => hotkey.set(Some(Hotkey::Attach)),
            "toggle_mute" => super::call::toggle_mute(),
            "toggle_deafen" => super::call::toggle_deafen(),
            "edit_last" => hotkey.set(Some(Hotkey::EditLast)),
            "back" => nav_jump(-1, nav, rooms, selected_space, selected),
            "forward" => nav_jump(1, nav, rooms, selected_space, selected),
            "next_room" | "prev_room" | "next_unread" | "prev_unread" => {
                let list = in_scope();
                let forward = name.starts_with("next");
                // In the DMs, Friends sits above the first DM: Alt+Up from
                // the top lands on it, Alt+Down from the bottom wraps to it,
                // and from it either way goes to the first or last DM.
                if !name.ends_with("unread") && *selected_space.peek() == Scope::Dms {
                    let on_friends = friends_open.peek().0;
                    let at = selected
                        .peek()
                        .as_ref()
                        .and_then(|id| list.iter().position(|r| &r.id == id));
                    let to_friends = !on_friends
                        && match (forward, at) {
                            (false, Some(0)) => true,
                            (true, Some(i)) => i + 1 == list.len(),
                            _ => list.is_empty(),
                        };
                    if to_friends {
                        friends_open.set(super::friends::FriendsShown(true));
                        chat_open.set(ChatInFront(true));
                        return;
                    }
                    if on_friends {
                        let target = if forward { list.first() } else { list.last() };
                        if let Some(room) = target.cloned() {
                            open(&room);
                        }
                        return;
                    }
                }
                if let Some(room) = step(
                    &list,
                    if forward { 1 } else { -1 },
                    name.ends_with("unread"),
                ) {
                    open(&room);
                }
            }
            "next_scope" | "prev_scope" => {
                let all = scopes();
                let current = all
                    .iter()
                    .position(|s| *s == *selected_space.peek())
                    .unwrap_or(0);
                let next = if name == "next_scope" {
                    (current + 1) % all.len()
                } else {
                    (current + all.len() - 1) % all.len()
                };
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
            // Phones pick files through the page (see FILE_LISTENER_JS).
            let page_picker = cfg!(any(target_os = "android", target_os = "ios"));
            let mut listener = document::eval(&format!(
                "window.__morpheusPagePicker = {page_picker};\n{LISTENER_JS}\n{FILE_LISTENER_JS}"
            ));
            while let Ok(name) = listener.recv::<String>().await {
                if let Some(payload) = name.strip_prefix("file:") {
                    match parse_dropped(payload) {
                        Some(file) => drops.with_mut(|d| d.0.push(file)),
                        None => tracing::warn!("ignoring an unreadable dropped file"),
                    }
                } else if let Some(url) = name.strip_prefix("open:") {
                    open_link(url, open_url.as_ref());
                    opened_links.with_mut(|links| links.push(url.to_owned()));
                } else if let Some(message) = name.strip_prefix("jserr:") {
                    tracing::error!(%message, "javascript error in the page");
                } else if let Some(flag) = name.strip_prefix("narrow:") {
                    let is_narrow = flag == "1";
                    if is_narrow != narrow.peek().0 {
                        narrow.set(Narrow(is_narrow));
                        // A phone starts on the room list with the member
                        // list tucked away; widening again restores it.
                        members_open.set(MembersShown(!is_narrow));
                    }
                } else {
                    dispatch(name);
                }
            }
        })
    });
    let side_column = match (
        current().filter(|_| !friends_open().0),
        active_thread(),
        search_open().0,
        members_open().0,
    ) {
        (Some(_), Some(_), _, _) => SideColumn::Thread,
        (Some(_), None, true, _) => SideColumn::Search,
        (Some(_), None, false, true) => SideColumn::Members,
        _ => SideColumn::None,
    };
    // On a phone with a chat in front, the member list waits parked off the
    // right edge so a leftward swipe can pull it in under the finger.
    let parked_members = narrow().0
        && chat_open().0
        && side_column == SideColumn::None
        && current().is_some_and(|room| !room.invited);
    let rendered_side = if parked_members {
        SideColumn::Members
    } else {
        side_column
    };
    // Which pane a phone shows: the list, the chat, or the side panel.
    let pane = if !narrow().0 {
        "all"
    } else if (current().is_none() && !friends_open().0) || !chat_open().0 {
        "rooms"
    } else if side_column != SideColumn::None {
        "side"
    } else {
        "chat"
    };

    rsx! {
        div {
            class: "shell",
            class: if side_column == SideColumn::None { "no-side" },
            class: if narrow().0 { "narrow" },
            class: if panes_ready() { "panes-ready" },
            "data-pane": pane,
            tabindex: "-1",
            // Targets open their own menu and stop the event; everything else
            // gets no native menu at all.
            oncontextmenu: move |e| {
                e.prevent_default();
                context_menu.set(MenuState::default());
            },
            // A window to drag and close needs the desktop bar; a phone has neither.
            if has_window {
                TitleBar {}
            }
            div { class: "drop-hint", "Drop to upload" }
            ToastLayer {}
            super::keys::BackupPrompt {}
            super::about::UpdateCard {}
            super::call::IncomingCalls {}
            // Mirror of what was announced, for the test bridge.
            ul { class: "link-log", hidden: true,
                for (index, url) in opened_links().iter().enumerate() {
                    li { key: "{index}", "{url}" }
                }
            }
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
                    // The Friends page (top of the DMs) takes the chat area.
                    _ if friends_open().0 => rsx! { super::friends::FriendsPage {} },
                    // Keyed on the room id so switching rooms remounts the timeline.
                    Some(room) if room.invited => rsx! { InviteCard { key: "invite-{room.id}", room } },
                    Some(room) => rsx! { ChatRoom { key: "{room.id}", room } },
                    None => match preview().filter(|p| Some(&p.id) == selected().as_ref()) {
                        Some(room) => rsx! { super::invites::JoinCard { key: "join-{room.id}", room } },
                        None => rsx! {
                            div { class: "empty-state",
                                h2 { "Pick a room" }
                                p { class: "muted", "Your conversations will show up here." }
                            }
                        },
                    },
                }
            }
            if rendered_side != SideColumn::None {
                div { class: "side-pane",
                    class: if parked_members { "parked" },
                    if narrow().0 {
                        div { class: "pane-bar",
                            button {
                                class: "ghost pane-back",
                                title: "Back to the chat",
                                onclick: move |_| {
                                    if active_thread.peek().is_some() {
                                        active_thread.set(None);
                                    } else if search_open.peek().0 {
                                        search_open.set(SearchShown(false));
                                    } else {
                                        members_open.set(MembersShown(false));
                                    }
                                },
                                Icon { glyph: crate::icons::CHEVRON_LEFT, size: 18 }
                                "Back"
                            }
                        }
                    }
                    match (rendered_side, current(), active_thread()) {
                        (SideColumn::Thread, Some(room), Some(root)) => rsx! { ThreadPanel { room_id: room.id.clone(), root } },
                        (SideColumn::Members, Some(room), _) => rsx! { Members { room_id: room.id.clone(), member_count: room.member_count } },
                        (SideColumn::Search, Some(room), _) => rsx! { super::search::SearchPanel { key: "{room.id}", room_id: room.id.clone() } },
                        _ => rsx! {},
                    }
                }
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
            if let Some((user_id, current)) = nickname_target().0 {
                super::nicknames::NicknameEditor { user_id, current }
            }
            if settings_open() {
                Settings {}
            }
            if about_open().0 {
                Acknowledgements {}
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
                (MessageDialog::SelectText(text), _) => rsx! { super::message_dialogs::SelectTextModal { text } },
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
                RoomDialog::ModView(user_id) => rsx! { super::mod_view::ModViewModal { user_id } },
                RoomDialog::None => rsx! {},
            }
            if narrow().0 {
                super::call::CallScreen {}
            }
            ContextMenuLayer {}
        }
    }
}

#[component]
fn ChatRoom(room: RoomSummary) -> Element {
    let session = use_context::<Session>();
    // Kept for the header buttons; `session` goes into the typing watcher.
    let header_session = session.clone();
    let call_room = use_context::<super::call::ActiveCall>()()
        .0
        .map(|c| c.room_id);
    let title = try_use_context::<super::nicknames::Nicknames>()
        .map(|map| super::nicknames::room_title(&map(), &room))
        .unwrap_or_else(|| room.name.clone());
    let narrow = use_context::<NarrowWindow>();
    let mut chat_open = use_context::<ChatOpen>();
    // Menus built from this room's signals must not outlive it.
    use_drop(move || consume_context::<ContextMenu>().set(MenuState::default()));
    let mut members_open = use_context::<MembersOpen>();
    let mut search_open = use_context::<SearchOpen>();
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
            if narrow().0 {
                button {
                    class: "ghost pane-back",
                    title: "Back to rooms",
                    onclick: move |_| chat_open.set(ChatInFront(false)),
                    Icon { glyph: crate::icons::CHEVRON_LEFT, size: 18 }
                }
            }
            if room.is_dm {
                Avatar { media: room.avatar.clone(), fallback: room.name.clone(), size: 24 }
            } else {
                span { class: "hash", "#" }
            }
            h2 { "{title}" }
            // Whether the room is end-to-end encrypted; opens its privacy
            // settings, where encryption is turned on.
            button {
                class: "ghost e2e-badge",
                class: if room.is_encrypted { "on" } else { "off" },
                title: if room.is_encrypted { "End-to-end encrypted" } else { "Not encrypted" },
                onclick: {
                    let room_id = room.id.clone();
                    move |_| consume_context::<OpenRoomDialog>().set(RoomDialog::Privacy(room_id.clone()))
                },
                Icon { glyph: if room.is_encrypted { crate::icons::LOCK } else { crate::icons::LOCK_OPEN }, size: 14 }
            }
            if let Some(topic) = room.topic.as_deref() {
                span { class: "topic", "{topic}" }
            }
            button {
                class: "ghost room-menu-button",
                title: "Room options",
                onclick: {
                    let (session, room) = (header_session.clone(), room.clone());
                    move |e: Event<MouseData>| {
                        e.stop_propagation();
                        let at = e.client_coordinates();
                        super::room_list::open_room_menu_at(&session, room.clone(), at.x, at.y);
                    }
                },
                Icon { glyph: crate::icons::ELLIPSIS, size: 16 }
            }
            button {
                class: "ghost member-toggle",
                class: if members_open().0 { "active" },
                title: "Member list (Ctrl+U)",
                onclick: move |_| members_open.with_mut(|shown| shown.0 = !shown.0),
                Icon { glyph: crate::icons::USERS, size: 16 }
                "{room.member_count}"
            }
            // Calls belong to DMs and voice channels, as in Discord: a text
            // channel has none.
            if room.is_dm || room.is_voice {
                {
                    let in_this_call = call_room.as_ref() == Some(&room.id);
                    let (session, room_id, room_name) = (header_session.clone(), room.id.clone(), room.name.clone());
                    rsx! {
                        button {
                            class: "ghost call-button",
                            class: if in_this_call { "active" },
                            title: if in_this_call && narrow().0 { "Open the call" } else if in_this_call { "Leave the call" } else if room.call_participants.is_empty() { "Start a call" } else { "Join the call" },
                            onclick: move |_| {
                                if in_this_call && narrow.peek().0 {
                                    // A phone opens the call; hanging up is in there.
                                    super::call::show_call_screen(true);
                                } else if in_this_call {
                                    super::call::hang_up(session.clone());
                                } else {
                                    super::call::join_call(session.clone(), room_id.clone(), room_name.clone());
                                }
                            },
                            Icon { glyph: if in_this_call && !narrow().0 { crate::icons::PHONE_OFF } else { crate::icons::PHONE }, size: 16 }
                        }
                    }
                }
            }
            button {
                class: "ghost pin-toggle",
                title: "Pinned messages (Ctrl+P)",
                onclick: move |_| message_dialog.set(MessageDialog::Pinned),
                Icon { glyph: crate::icons::PIN, size: 16 }
                "{pinned().0.len()}"
            }
            // A search bar at the top right (an icon on a phone); opens the
            // search panel.
            button {
                class: "ghost search-toggle",
                class: if search_open().0 { "active" },
                title: "Search this room",
                onclick: move |_| search_open.with_mut(|s| s.0 = !s.0),
                span { class: "search-toggle-label", "Search" }
                Icon { glyph: crate::icons::SEARCH, size: 16 }
            }
        }
        // A phone shows the call on its own screen.
        if !narrow().0 {
            super::call::CallStage { room_id: room.id.clone() }
        }
        TimelineView { room_id: room.id.clone() }
        TypingLine { names: typing() }
        if narrow().0 {
            super::call::CallBar {}
        }
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
    Search,
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
    // Somewhere chosen before the last place was reopened: stay there.
    if let Some(mut pending) = try_consume_context::<PendingPlace>()
        && pending.peek().is_some()
    {
        pending.set(None);
    }
    // Going to a room leaves the Friends page.
    if let Some(mut friends) = try_consume_context::<super::friends::FriendsOpen>() {
        friends.set(super::friends::FriendsShown(false));
    }
    select(room_id);
    // On a phone the chat pane comes forward; on a desktop this is unused.
    consume_context::<ChatOpen>().set(ChatInFront(true));
}

/// Open a room without bringing the chat pane forward (the shell's own
/// re-pick on load and scope changes).
pub fn select(room_id: OwnedRoomId) {
    let mut nav = consume_context::<NavHistory>();
    nav.with_mut(|n| n.record(room_id.clone()));
    consume_context::<Selected>().set(Some(room_id));
}

/// Show a web link in the browser: the platform's opener (Android's intent),
/// else the desktop default. Test runs never open a browser on the
/// developer's desktop; the shell's link log records the hand-off instead.
fn open_link(url: &str, open_url: Option<&morpheus_core::UrlOpener>) {
    if std::env::var_os("MORPHEUS_E2E_PORT").is_some() {
        tracing::info!(%url, "link click (test run, not opened)");
        return;
    }
    let result = match open_url {
        Some(open) => open(url),
        None => open::that_detached(url),
    };
    if let Err(err) = result {
        report("opening the link", &err);
    }
}
