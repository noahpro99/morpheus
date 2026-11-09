//! The room search panel (header magnifier): finds messages this device has
//! loaded, decrypted ones included, and jumps to one when picked.

use std::time::Duration;

use dioxus::prelude::*;
use morpheus_core::{OwnedRoomId, SearchHit, Session};

use super::shell::{JumpTarget, JumpTo, NarrowWindow, SearchOpen, SearchShown};
use crate::format::{calendar_date, clock_time};

/// Quiet time after typing before searching.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(250);
/// Most results shown.
const SEARCH_LIMIT: usize = 50;

#[component]
pub fn SearchPanel(room_id: OwnedRoomId) -> Element {
    let session = use_context::<Session>();
    let mut jump = use_context::<JumpTo>();
    let mut search_open = use_context::<SearchOpen>();
    let narrow = use_context::<NarrowWindow>();
    let mut query = use_signal(String::new);
    let mut typed = use_signal(|| 0u64);
    let mut settled = use_signal(String::new);

    let results = use_resource(move || {
        let session = session.clone();
        let room_id = room_id.clone();
        let text = settled();
        async move {
            if text.trim().is_empty() {
                return Ok(Vec::<SearchHit>::new());
            }
            session
                .search_room(room_id, text, SEARCH_LIMIT)
                .await
                .map_err(|err| err.to_string())
        }
    });

    rsx! {
        div { class: "search-panel",
            input {
                name: "search",
                r#type: "search",
                placeholder: "Search messages",
                autofocus: true,
                value: "{query}",
                oninput: move |e| {
                    let value = e.value();
                    query.set(value.clone());
                    let stamp = *typed.peek() + 1;
                    typed.set(stamp);
                    spawn(async move {
                        morpheus_core::rt::sleep(SEARCH_DEBOUNCE).await;
                        if *typed.peek() == stamp {
                            settled.set(value);
                        }
                    });
                },
            }
            p { class: "muted search-note", "Searches the messages this device has loaded." }
            match &*results.read() {
                None => rsx! {},
                Some(Err(err)) => rsx! { p { class: "error", "{err}" } },
                Some(Ok(hits)) if hits.is_empty() && !settled().trim().is_empty() => rsx! {
                    p { class: "muted", "No messages found." }
                },
                Some(Ok(hits)) => rsx! {
                    ul { class: "search-results",
                        for hit in hits.clone() {
                            li {
                                key: "{hit.event_id}",
                                class: "search-hit",
                                onclick: {
                                    let id = hit.event_id.clone();
                                    move |_| {
                                        jump.set(JumpTarget(Some(id.clone())));
                                        // A phone shows one pane: go to the chat.
                                        if narrow.peek().0 {
                                            search_open.set(SearchShown(false));
                                        }
                                    }
                                },
                                div { class: "search-meta",
                                    span { class: "search-sender", "{hit.sender_name}" }
                                    span { class: "search-time", "{calendar_date(hit.timestamp_ms)} {clock_time(hit.timestamp_ms)}" }
                                }
                                p { class: "search-body", "{hit.body}" }
                            }
                        }
                    }
                },
            }
        }
    }
}
