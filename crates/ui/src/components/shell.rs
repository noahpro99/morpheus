use dioxus::prelude::*;
use morpheus_core::{OwnedRoomId, RoomSummary, Session, Vector};
use tracing::error;

use super::{
    composer::{ActiveTimeline, Composer},
    members::Members,
    rail::Rail,
    room_list::RoomList,
    timeline::TimelineView,
};

/// Rooms as the core reports them, kept in sync for the whole shell.
pub type Rooms = Signal<Vector<RoomSummary>>;
/// Currently open room.
pub type Selected = Signal<Option<OwnedRoomId>>;

#[component]
pub fn Shell(session: Session) -> Element {
    use_context_provider(|| session.clone());
    let mut rooms: Rooms = use_context_provider(|| Signal::new(Vector::new()));
    let mut selected: Selected = use_context_provider(|| Signal::new(None));
    let mut load_error = use_signal(|| None::<String>);

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

    // Open the most recent room on first load, like Discord does.
    use_effect(move || {
        if selected.peek().is_none() {
            let first = rooms
                .read()
                .iter()
                .find(|room| !room.is_space)
                .map(|room| room.id.clone());
            if first.is_some() {
                selected.set(first);
            }
        }
    });

    let current = use_memo(move || {
        let id = selected()?;
        rooms.read().iter().find(|room| room.id == id).cloned()
    });

    rsx! {
        div { class: "shell",
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
            Members {}
        }
    }
}

#[component]
fn ChatRoom(room: RoomSummary) -> Element {
    // Shared by the timeline (which fills it) and the composer (which sends through it).
    use_context_provider(|| ActiveTimeline::new(None));

    rsx! {
        header { class: "chat-header",
            span { class: "hash", "#" }
            h2 { "{room.name}" }
            if let Some(topic) = room.topic.as_deref() {
                span { class: "topic", "{topic}" }
            }
        }
        TimelineView { room_id: room.id.clone() }
        Composer { room_name: room.name.clone() }
    }
}
