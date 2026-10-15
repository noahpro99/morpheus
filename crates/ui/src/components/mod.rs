//! Visual building blocks. Each file is one region of the Discord-style shell.

mod about;
mod composer;
mod context_menu;
mod emoji;
mod hotkeys;
mod invites;
mod keys;
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
mod space_menu;
mod switcher;
mod thread;
mod timeline;
mod titlebar;
mod toast;
mod user_bar;

pub use login::Login;
pub use shell::Shell;

use dioxus::prelude::*;

#[component]
pub fn Splash() -> Element {
    rsx! {
        main { class: "centered",
            div { class: "splash", role: "img", "aria-label": "Morpheus",
                crate::icons::Logo { size: 160, class: "splash-logo" }
            }
        }
    }
}
