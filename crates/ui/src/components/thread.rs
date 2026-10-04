use dioxus::prelude::*;
use morpheus_core::{Message, MessageId, OwnedRoomId};

use super::{
    composer::{ActiveTimeline, Composer, Draft, Mode},
    timeline::{MessageRow, TimelineView},
};

/// The thread panel's open root message, if any. Lives in the shell so the
/// message list (which sets it) and the right column (which reads it) agree
/// on what is open.
pub type ActiveThread = Signal<Option<Message>>;

/// Right column, in place of members: the thread's root message, its
/// replies, and a composer that posts back into the thread.
#[component]
pub fn ThreadPanel(room_id: OwnedRoomId, root: Message) -> Element {
    let mut active_thread = use_context::<ActiveThread>();
    // Own composer state, isolated from the main chat's.
    use_context_provider(|| ActiveTimeline::new(None));
    let mut mode = use_context_provider(|| Mode::new(None));
    let mut draft = use_context_provider(|| Draft::new(String::new()));

    let MessageId::Event(root_event_id) = root.id.clone() else {
        // Local echoes cannot be thread roots; nothing sensible to open.
        return rsx! {};
    };

    // Jumping straight from one thread to another should not carry over a
    // reply/edit draft meant for the previous root.
    use_effect(use_reactive!(|root_event_id| {
        let _ = root_event_id;
        mode.set(None);
        draft.set(String::new());
    }));

    rsx! {
        aside { class: "thread-panel",
            header { class: "thread-panel-header",
                h3 { "Thread" }
                button {
                    class: "ghost",
                    title: "Close thread",
                    onclick: move |_| active_thread.set(None),
                    "✕"
                }
            }
            div { class: "thread-panel-root",
                MessageRow { key: "root-{root.unique_id}", message: root.clone(), show_header: true }
            }
            div { class: "thread-panel-divider",
                span { "{reply_count_label(&root)}" }
            }
            TimelineView {
                key: "{root_event_id}",
                room_id: room_id.clone(),
                root_event_id: Some(root_event_id),
            }
            Composer { room_name: String::new(), placeholder: Some("Reply in thread".to_owned()) }
        }
    }
}

fn reply_count_label(root: &Message) -> String {
    match root.thread.as_ref().map(|t| t.reply_count) {
        None | Some(0) => "No replies yet".to_owned(),
        Some(1) => "1 reply".to_owned(),
        Some(n) => format!("{n} replies"),
    }
}
