use dioxus::prelude::*;
use morpheus_core::{OwnedRoomId, RoomSummary, Session, Vector};
use tracing::error;

use super::{
    composer::{ActiveTimeline, Composer, Draft, Mode},
    context_menu::{ContextMenu, ContextMenuLayer, MenuState},
    lightbox::{Lightbox, provide_lightbox},
    members::Members,
    message_dialogs::{
        MessageDialog, OpenMessageDialog, Pinned, PinnedIds, PinnedPanel, PinnedRefresh,
        ReactionsModal, ReportModal, use_pinned_fetcher,
    },
    profile::{
        EditProfileModal, Editor, OpenProfile, ProfileEditor, ProfileModal, ProfileRefresh,
        ProfileTarget, ProfileVersion, StatusModal,
    },
    rail::Rail,
    room_dialogs::{CreateRoomModal, EditRoomModal, InviteModal, OpenRoomDialog, RoomDialog},
    room_list::RoomList,
    settings::{ActiveVerification, Settings, SettingsOpen, track_verification},
    switcher::QuickSwitcher,
    thread::{ActiveThread, ThreadPanel},
    timeline::TimelineView,
    titlebar::{HelpModal, InboxPanel, Nav, OpenTopPanel, TitleBar, TopPanel},
};
use crate::media::{Avatar, provide_media_cache};

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
    let space_id = match scope {
        Scope::Dms => {
            return rooms
                .iter()
                .filter(|room| room.is_dm && !room.is_space)
                .cloned()
                .collect();
        }
        Scope::Rooms => {
            return rooms
                .iter()
                .filter(|room| !room.is_space && !room.is_dm && room.parent_spaces.is_empty())
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
        .filter(|room| !room.is_space && room.parent_spaces.iter().any(|p| scope.contains(p)))
        .cloned()
        .collect()
}

#[component]
pub fn Shell(session: Session) -> Element {
    use_context_provider(|| session.clone());
    provide_media_cache();
    let mut rooms: Rooms = use_context_provider(|| Signal::new(Vector::new()));
    let mut selected: Selected = use_context_provider(|| Signal::new(None));
    let selected_space: SelectedSpace = use_context_provider(|| Signal::new(Scope::default()));
    let mut active_thread: ActiveThread = use_context_provider(|| Signal::new(None));
    let mut load_error = use_signal(|| None::<String>);
    let mut switcher_open = use_signal(|| false);
    let mut settings_open: SettingsOpen = use_context_provider(|| Signal::new(false));
    let mut verification: ActiveVerification = use_context_provider(|| Signal::new(None));
    let members_open: MembersOpen = use_context_provider(|| Signal::new(MembersShown(true)));
    let mut open_profile: OpenProfile = use_context_provider(|| Signal::new(ProfileTarget::default()));
    let mut context_menu: ContextMenu = use_context_provider(|| Signal::new(MenuState::default()));
    let mut editor: ProfileEditor = use_context_provider(|| Signal::new(Editor::None));
    let _mention: MentionRequest = use_context_provider(|| Signal::new(Mention::default()));
    let mut message_dialog: OpenMessageDialog = use_context_provider(|| Signal::new(MessageDialog::None));
    let mut room_dialog: OpenRoomDialog = use_context_provider(|| Signal::new(RoomDialog::None));
    let mut top_panel: OpenTopPanel = use_context_provider(|| Signal::new(TopPanel::None));
    let mut nav: NavHistory = use_context_provider(|| Signal::new(Nav::default()));
    // Every room the user lands in becomes a history entry (back/forward).
    use_effect(move || {
        if let Some(room_id) = selected() {
            nav.with_mut(|n| n.record(room_id));
        }
    });
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
                    error!(%err, "room list failed");
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
            selected.set(in_scope.first().map(|room| room.id.clone()));
        }
    });

    let current = use_memo(move || {
        let id = selected()?;
        rooms.read().iter().find(|room| room.id == id).cloned()
    });

    // A thread belongs to the room it was opened from; close it when switching
    // rooms, along with any menu built from that room's signals.
    use_effect(move || {
        selected();
        active_thread.set(None);
        context_menu.set(MenuState::default());
    });

    // Global shortcuts. The shell is focusable so keys reach it even when no input has focus.
    let on_key = move |e: Event<KeyboardData>| {
        let ctrl = e.modifiers().ctrl() || e.modifiers().meta();
        if ctrl && matches!(e.key(), Key::Character(ref c) if c.eq_ignore_ascii_case("k")) {
            e.prevent_default();
            switcher_open.toggle();
        } else if e.key() == Key::Escape {
            // Escape closes whichever modal is open, top-most first. A running
            // verification keeps going in the background.
            if context_menu().0.is_some() {
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
            }
        }
    };
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
            onkeydown: on_key,
            // Targets open their own menu and stop the event; everything else
            // gets no native menu at all.
            oncontextmenu: move |e| {
                e.prevent_default();
                context_menu.set(MenuState::default());
            },
            TitleBar {}
            div { class: "shell-body",
            Rail {}
            RoomList { error: load_error() }
            main { class: "chat",
                match current() {
                    // Keyed on the room id so switching rooms remounts the timeline.
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
            if let Some(user_id) = open_profile().0 {
                ProfileModal { user_id }
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
                RoomDialog::None => rsx! {},
            }
            ContextMenuLayer {}
        }
    }
}

#[component]
fn ChatRoom(room: RoomSummary) -> Element {
    let session = use_context::<Session>();
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
                        error!(%err, "clearing the unread flag failed");
                    }
                });
            }
        }
    });
    // Shared by the timeline (which fills it) and the composer (which sends through it).
    use_context_provider(|| ActiveTimeline::new(None));
    use_context_provider(|| Mode::new(None));
    use_context_provider(|| Draft::new(String::new()));
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
                    Err(err) => error!(%err, "typing subscription failed"),
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
                "📌 {pinned().0.len()}"
            }
            button {
                class: "ghost member-toggle",
                class: if members_open().0 { "active" },
                title: "Toggle member list",
                onclick: move |_| members_open.with_mut(|shown| shown.0 = !shown.0),
                "👥 {room.member_count}"
            }
        }
        TimelineView { room_id: room.id.clone() }
        TypingLine { names: typing() }
        Composer {
            room_name: room.name.clone(),
            placeholder: room.is_dm.then(|| format!("Message @{}", room.name)),
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
