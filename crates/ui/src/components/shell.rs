use dioxus::prelude::*;
use morpheus_core::{OwnedRoomId, RoomSummary, Session, Vector};
use tracing::error;

use super::{
    composer::{ActiveTimeline, Composer, Draft, Mode},
    members::Members,
    rail::Rail,
    room_list::RoomList,
    switcher::QuickSwitcher,
    timeline::TimelineView,
};
use crate::media::provide_media_cache;

/// Rooms as the core reports them, kept in sync for the whole shell.
pub type Rooms = Signal<Vector<RoomSummary>>;
/// Currently open room.
pub type Selected = Signal<Option<OwnedRoomId>>;
/// The space currently selected in the rail, or `None` for Home.
pub type SelectedSpace = Signal<Option<OwnedRoomId>>;

/// Rooms visible for the current space selection.
///
/// Discord-like rule: Home (`selected_space` is `None`) shows rooms that are
/// not in any space (this covers DMs, which are never in a space, and any
/// ungrouped rooms). Selecting a space shows the rooms that list it — or one
/// of its nested spaces — as a parent, so a space's whole subtree shows up
/// under one badge.
pub fn rooms_in_scope(
    rooms: &Vector<RoomSummary>,
    selected_space: Option<&OwnedRoomId>,
) -> Vec<RoomSummary> {
    let Some(space_id) = selected_space else {
        return rooms
            .iter()
            .filter(|room| !room.is_space && room.parent_spaces.is_empty())
            .cloned()
            .collect();
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
    let selected_space: SelectedSpace = use_context_provider(|| Signal::new(None));
    let mut load_error = use_signal(|| None::<String>);
    let mut switcher_open = use_signal(|| false);

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
        let in_scope = rooms_in_scope(&rooms.read(), selected_space.read().as_ref());
        let still_open = selected
            .peek()
            .as_ref()
            .is_some_and(|id| in_scope.iter().any(|room| &room.id == id));
        if !still_open {
            selected.set(in_scope.first().map(|room| room.id.clone()));
        }
    });

    let current = use_memo(move || {
        let id = selected()?;
        rooms.read().iter().find(|room| room.id == id).cloned()
    });

    // Global shortcuts. The shell is focusable so keys reach it even when no input has focus.
    let on_key = move |e: Event<KeyboardData>| {
        let ctrl = e.modifiers().ctrl() || e.modifiers().meta();
        if ctrl && matches!(e.key(), Key::Character(ref c) if c.eq_ignore_ascii_case("k")) {
            e.prevent_default();
            switcher_open.toggle();
        }
    };

    rsx! {
        div { class: "shell", tabindex: "-1", onkeydown: on_key,
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
            match current() {
                Some(room) => rsx! { Members { room_id: room.id.clone() } },
                None => rsx! { aside { class: "members" } },
            }
            if switcher_open() {
                QuickSwitcher { open: switcher_open }
            }
        }
    }
}

#[component]
fn ChatRoom(room: RoomSummary) -> Element {
    let session = use_context::<Session>();
    // Shared by the timeline (which fills it) and the composer (which sends through it).
    use_context_provider(|| ActiveTimeline::new(None));
    use_context_provider(|| Mode::new(None));
    use_context_provider(|| Draft::new(String::new()));
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
            span { class: "hash", "#" }
            h2 { "{room.name}" }
            if let Some(topic) = room.topic.as_deref() {
                span { class: "topic", "{topic}" }
            }
            span { class: "member-count", title: "Members", "👥 {room.member_count}" }
        }
        TimelineView { room_id: room.id.clone() }
        TypingLine { names: typing() }
        Composer { room_name: room.name.clone() }
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
