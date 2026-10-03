use dioxus::prelude::*;

use super::shell::Rooms;
use crate::media::Avatar;

/// Left-most column: Home plus one badge per space. Space filtering lands in
/// phase 2; today the badges are informational.
#[component]
pub fn Rail() -> Element {
    let rooms = use_context::<Rooms>();
    let spaces: Vec<_> = rooms
        .read()
        .iter()
        .filter(|room| room.is_space)
        .cloned()
        .collect();

    rsx! {
        nav { class: "rail",
            button { class: "rail-item selected", title: "Home", "M" }
            div { class: "rail-divider" }
            for space in spaces {
                button { key: "{space.id}", class: "rail-item", title: "{space.name}",
                    Avatar { media: space.avatar.clone(), fallback: space.name.clone(), size: 48 }
                }
            }
        }
    }
}
