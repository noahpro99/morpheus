use dioxus::prelude::*;

use super::shell::{Rooms, Scope, SelectedSpace};
use crate::media::Avatar;

/// Left-most column: direct messages, ungrouped rooms, then one badge per
/// top-level space. Clicking a badge scopes the room list; see
/// [`super::shell::rooms_in_scope`] for the exact rule.
#[component]
pub fn Rail() -> Element {
    let rooms = use_context::<Rooms>();
    let mut selected_space = use_context::<SelectedSpace>();

    // Only top-level spaces get their own badge; a nested space's rooms show
    // up when its top-level ancestor is selected instead.
    let spaces: Vec<_> = rooms
        .read()
        .iter()
        .filter(|room| room.is_space && room.parent_spaces.is_empty())
        .cloned()
        .collect();
    let current = selected_space();
    let unread_dms = rooms
        .read()
        .iter()
        .filter(|room| room.is_dm && room.highlight_count > 0)
        .count();

    rsx! {
        nav { class: "rail",
            button {
                class: "rail-item rail-dms",
                class: if current == Scope::Dms { "selected" },
                title: "Direct messages",
                onclick: move |_| selected_space.set(Scope::Dms),
                svg { view_box: "0 0 24 24", width: "24", height: "24", fill: "currentColor",
                    path { d: "M12 3C6.5 3 2 6.9 2 11.7c0 2.6 1.3 4.9 3.4 6.5L4.5 21l3.9-1.9c1.1.3 2.3.5 3.6.5 5.5 0 10-3.9 10-8.9S17.5 3 12 3z" }
                }
                if unread_dms > 0 {
                    span { class: "rail-badge", "{unread_dms}" }
                }
            }
            button {
                class: "rail-item rail-rooms",
                class: if current == Scope::Rooms { "selected" },
                title: "Rooms",
                onclick: move |_| selected_space.set(Scope::Rooms),
                "#"
            }
            div { class: "rail-divider" }
            for space in spaces {
                button {
                    key: "{space.id}",
                    class: "rail-item",
                    class: if current.space_id() == Some(&space.id) { "selected" },
                    title: "{space.name}",
                    onclick: {
                        let id = space.id.clone();
                        move |_| selected_space.set(Scope::Space(id.clone()))
                    },
                    Avatar { media: space.avatar.clone(), fallback: space.name.clone(), size: 48 }
                }
            }
        }
    }
}
