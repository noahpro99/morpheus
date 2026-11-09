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
            Ok(UpdateStatus::Available { version, url, .. }) => {
                toast(match &url {
                    Some(url) => format!("Morpheus {version} is available: {url}"),
                    None => format!("Morpheus {version} is available"),
                });
            }
            Err(err) => report("update check", &err),
        }
    });
}

/// How often the app looks for a new release while it runs.
const UPDATE_CHECK_EVERY: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

/// A newer release, found by the app itself: a card offering it. Update
/// installs it in place and restarts (desktops); phones get the download.
#[component]
pub fn UpdateCard() -> Element {
    let session = use_context::<Session>();
    let mut found = use_signal(|| None::<(String, Option<String>, Option<String>)>);
    let mut later = use_signal(|| false);
    let mut busy = use_signal(|| false);
    use_future({
        let session = session.clone();
        move || {
            let session = session.clone();
            async move {
                loop {
                    match session.check_for_update(VERSION).await {
                        Ok(UpdateStatus::Available {
                            version,
                            url,
                            download,
                        }) => found.set(Some((version, url, download))),
                        Ok(_) => {}
                        Err(err) => tracing::info!(%err, "update check failed"),
                    }
                    morpheus_core::rt::sleep(UPDATE_CHECK_EVERY).await;
                }
            }
        }
    });
    let Some((version, url, download)) = found() else {
        return rsx! {};
    };
    if later() {
        return rsx! {};
    }
    let installs = morpheus_core::installs_updates();
    let install = installs && download.is_some();
    // Not installed here: the release page, as an install that isn't the
    // app's to change wants its own package rather than this zip.
    let link = if installs {
        download.clone().or(url.clone())
    } else {
        url.clone().or(download.clone())
    }
    .unwrap_or_default();
    rsx! {
        div { class: "backup-prompt update-card", role: "status",
            Icon { glyph: crate::icons::REFRESH_CW, size: 18, class: "backup-prompt-icon" }
            div { class: "backup-prompt-text",
                strong { "Morpheus {version} is available" }
                span {
                    if install {
                        "Update now: Morpheus restarts on the new version."
                    } else {
                        "Download it to update."
                    }
                }
            }
            div { class: "backup-prompt-actions",
                button { class: "secondary", onclick: move |_| later.set(true), "Later" }
                if install {
                    button {
                        class: "primary update-button",
                        disabled: busy(),
                        onclick: move |_| {
                            let Some(download) = download.clone() else { return };
                            busy.set(true);
                            let future = session.install_update(download);
                            spawn(async move {
                                match future.await {
                                    // The new version is starting; it waits for this one.
                                    Ok(()) => std::process::exit(0),
                                    Err(err) => {
                                        report("updating", &err);
                                        busy.set(false);
                                    }
                                }
                            });
                        },
                        if busy() { "Updating…" } else { "Update" }
                    }
                } else if !link.is_empty() {
                    a { class: "button primary update-button", href: "{link}", "Download" }
                }
            }
        }
    }
}
