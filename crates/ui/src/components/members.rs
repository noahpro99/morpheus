use dioxus::prelude::*;

/// Right column. Real member lists arrive with phase 2.
#[component]
pub fn Members() -> Element {
    rsx! {
        aside { class: "members",
            header { class: "members-header", h3 { "Members" } }
            p { class: "muted pad", "Coming soon." }
        }
    }
}
