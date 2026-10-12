//! Visual building blocks. Each file is one region of the Discord-style shell.

mod composer;
mod context_menu;
mod lightbox;
mod login;
mod members;
mod message_dialogs;
mod profile;
mod rail;
mod room_dialogs;
mod room_list;
mod settings;
mod shell;
mod switcher;
mod thread;
mod timeline;
mod titlebar;

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
