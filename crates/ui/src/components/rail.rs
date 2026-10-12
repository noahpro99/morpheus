use dioxus::prelude::*;

use super::shell::{Rooms, SelectedSpace, SpaceScope};
use crate::media::Avatar;

/// Left-most column: Home plus one badge per top-level space. Clicking a
/// badge filters the room list to that space's subtree; see
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
    let current = selected_space().0;

    rsx! {
        nav { class: "rail",
            button {
                class: "rail-item",
                class: if current.is_none() { "selected" },
                title: "Home",
                onclick: move |_| selected_space.set(SpaceScope(None)),
                "M"
            }
            div { class: "rail-divider" }
            for space in spaces {
                button {
                    key: "{space.id}",
                    class: "rail-item",
                    class: if current.as_ref() == Some(&space.id) { "selected" },
                    title: "{space.name}",
                    onclick: {
                        let id = space.id.clone();
                        move |_| selected_space.set(SpaceScope(Some(id.clone())))
                    },
                    Avatar { media: space.avatar.clone(), fallback: space.name.clone(), size: 48 }
                }
            }
        }
    }
}
