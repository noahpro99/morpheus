//! Visual building blocks. Each file is one region of the Discord-style shell.

mod composer;
mod context_menu;
mod lightbox;
mod login;
mod members;
mod profile;
mod rail;
mod room_list;
mod settings;
mod shell;
mod switcher;
mod thread;
mod timeline;

pub use login::Login;
pub use shell::Shell;

use dioxus::prelude::*;

#[component]
pub fn Splash() -> Element {
    rsx! {
        main { class: "centered",
            div { class: "splash", "Morpheus" }
        }
    }
}
