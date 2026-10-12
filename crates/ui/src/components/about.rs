//! Acknowledgements (the open-source work Morpheus is built on) and the
//! update check. Both are reachable from the tray menu and the Ctrl+K
//! palette; the tray reaches them through [`AppCommands`].

use dioxus::prelude::*;
use morpheus_core::{Session, UpdateStatus};

use super::toast::{report, toast};
use crate::icons::{Icon, X};

/// Whether the acknowledgements dialog is open. Provided by the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AboutShown(pub bool);
pub type AboutOpen = Signal<AboutShown>;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The projects a user would recognise, ahead of the full crate list.
const HEADLINERS: [(&str, &str); 5] = [
    (
        "Matrix and matrix-rust-sdk",
        "The open protocol and the SDK doing sync, encryption and timelines.",
    ),
    (
        "Dioxus",
        "The Rust UI framework behind every screen, desktop and mobile.",
    ),
    (
        "Lucide",
        "The icon set used in menus and buttons (ISC licence).",
    ),
    ("Tokio", "The async runtime underneath the core."),
    (
        "WebKitGTK, wry and tao",
        "The webview and windowing the desktop app runs in.",
    ),
];

/// Every third-party crate in the build, from the lockfile: (name, version).
fn crates() -> Vec<(&'static str, &'static str)> {
    const LOCK: &str = include_str!("../../../../Cargo.lock");
    let field = |line: &'static str, key: &str| {
        line.strip_prefix(key)?
            .trim()
            .strip_prefix("= \"")?
            .strip_suffix('"')
    };
    let mut list = Vec::new();
    let mut name = None;
    for line in LOCK.lines() {
        if let Some(found) = field(line, "name") {
            name = Some(found);
        } else if let (Some(version), Some(found)) = (field(line, "version"), name.take())
            && !found.starts_with("morpheus")
        {
            list.push((found, version));
        }
    }
    list
}

#[component]
pub fn Acknowledgements() -> Element {
    let mut open = use_context::<AboutOpen>();
    let list = use_hook(crates);
    rsx! {
        div { class: "overlay", onclick: move |_| open.set(AboutShown(false)),
            div { class: "settings about", tabindex: "-1", onclick: move |e| e.stop_propagation(),
                header { class: "settings-header",
                    h2 { "Acknowledgements" }
                    button { class: "ghost", title: "Close", onclick: move |_| open.set(AboutShown(false)), Icon { glyph: X, size: 18 } }
                }
                section { class: "settings-section",
                    p { class: "about-version", "Morpheus {VERSION}" }
                    p { class: "muted", "Morpheus stands on a great deal of open-source work. Thank you to everyone behind it." }
                    ul { class: "about-headliners",
                        for (name, what) in HEADLINERS {
                            li { key: "{name}",
                                strong { "{name}" }
                                span { class: "muted", "{what}" }
                            }
                        }
                    }
                }
                section { class: "settings-section",
                    h3 { "Open-source libraries ({list.len()})" }
                    ul { class: "about-crates",
                        for (name, version) in list.iter() {
                            li { key: "{name}-{version}",
                                span { class: "crate-name", "{name}" }
                                span { class: "muted", "{version}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Ask the release feed and say what it found in a toast.
pub fn check_for_updates() {
    let Some(session) = try_consume_context::<Session>() else {
        return;
    };
    let future = session.check_for_update(VERSION);
    spawn(async move {
        match future.await {
            Ok(UpdateStatus::UpToDate) => toast(format!("Morpheus {VERSION} is up to date")),
            Ok(UpdateStatus::NoFeed) => toast(format!(
                "Morpheus {VERSION}: this build has no release feed to check"
            )),
            Ok(UpdateStatus::Available { version, url }) => {
                toast(match &url {
                    Some(url) => format!("Morpheus {version} is available: {url}"),
                    None => format!("Morpheus {version} is available"),
                });
            }
            Err(err) => report("update check", &err),
        }
    });
}
