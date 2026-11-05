//! Desktop entry point. Picks the renderer, locates the data directory and
//! hands everything else to `morpheus_ui`. Also owns what must keep working
//! while the window is hidden in the tray: OS notifications and reopening.

use std::{
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use dioxus::desktop::{
    Config, LogicalSize, WindowBuilder, WindowCloseBehaviour,
    tao::event::{Event, WindowEvent},
    use_wry_event_handler,
};
use dioxus::prelude::*;
use morpheus_core::{MediaRef, Session};
use morpheus_ui::{AppCommand, AppConfig, OpenRoom, WindowControls};
use tracing_subscriber::EnvFilter;

mod tray;
use tray::TrayAction;

/// The homeserver the sign-in form suggests: the dev server in test builds
/// (`just e2e-build`), matrix.org otherwise.
fn default_homeserver() -> &'static str {
    if cfg!(feature = "e2e") {
        "http://localhost:8008"
    } else {
        "matrix.org"
    }
}

/// Rust's allocations through mimalloc, which hands freed memory back to
/// the system; glibc's malloc held on to most of what a big room freed.
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Window focus as the event loop reports it, readable from any thread.
#[derive(Clone)]
struct Focus(Arc<AtomicBool>);

/// Set by the control-port thread; the event loop shows the window on the main
/// thread (GTK insists) at the next event, which the thread asks for.
static SHOW_REQUESTED: AtomicBool = AtomicBool::new(false);

fn main() {
    // The terminal, and a file in the data dir (`morpheus.log`, fresh each
    // start) to read after something went wrong. Dioxus's signal-ownership
    // warnings are pages long; they are left out.
    use tracing_subscriber::Layer as _;
    use tracing_subscriber::layer::SubscriberExt as _;
    use tracing_subscriber::util::SubscriberInitExt as _;
    let filter = || {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            "morpheus=debug,livekit=info,warn,dioxus_signals::warnings=off".into()
        })
    };
    let log_file = std::fs::create_dir_all(data_dir())
        .and_then(|()| std::fs::File::create(data_dir().join("morpheus.log")))
        .ok();
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(filter()))
        .with(log_file.map(|file| {
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(std::sync::Mutex::new(file))
                .with_filter(filter())
        }))
        .init();

    // Unfocused until the window says otherwise: one started hidden in the
    // tray never gets a focus event, and must still notify.
    let focus = Focus(Arc::new(AtomicBool::new(false)));
    let mut config = AppConfig::new(
        data_dir(),
        std::env::var("MORPHEUS_HOMESERVER").unwrap_or_else(|_| default_homeserver().into()),
    );
    // OS notifications run on their own thread: the webview's runtime stops
    // while the window is hidden, and that is exactly when they matter.
    config.on_session = Some(Arc::new({
        let focus = focus.clone();
        let open_room = config.open_room.clone();
        let data_dir = config.data_dir.clone();
        move |session| notifier(session, focus.clone(), open_room.clone(), data_dir.clone())
    }));
    if !single_instance::claim(&config.data_dir) {
        tracing::info!("Morpheus already runs on this data; showing its window instead");
        return;
    }
    tracing::info!(?config.data_dir, "starting morpheus desktop");

    // Test runs open many windows; they must not steal focus from whatever
    // the developer is typing into. A distinct title lets the e2e driver
    // (and a compositor rule) tell them apart from a real session.
    let under_test = std::env::var_os("MORPHEUS_E2E_PORT").is_some();
    let start_hidden = std::env::args().any(|arg| arg == morpheus_ui::autostart::HIDDEN_FLAG);
    // Tests can ask for a phone-sized window to exercise the narrow layout.
    let (width, height) = std::env::var("MORPHEUS_E2E_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse::<f64>().ok()?, h.parse::<f64>().ok()?))
        })
        .unwrap_or((1280.0, 800.0));
    let window = WindowBuilder::new()
        .with_title(if under_test {
            "Morpheus (test)"
        } else {
            "Morpheus"
        })
        .with_focused(!under_test)
        // Started at sign-in with "Start hidden in the tray": straight to the tray.
        .with_visible(!start_hidden)
        // The shell draws its own Discord-style bar; see `DesktopRoot`.
        .with_decorations(false)
        .with_inner_size(LogicalSize::new(width, height));

    // Dioxus runs on the runtime it finds; unasked it builds one with a
    // thread per CPU core, which the UI does not need.
    #[allow(clippy::expect_used)]
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("the UI's runtime");
    let _runtime = runtime.enter();
    LaunchBuilder::new()
        .with_context(config)
        .with_context(focus)
        .with_cfg(
            Config::new()
                .with_window(window)
                .with_menu(None)
                // Closing the window keeps Morpheus syncing in the tray, so
                // notifications keep coming; the tray menu quits for real.
                .with_close_behaviour(WindowCloseBehaviour::WindowHides)
                .with_exits_when_last_window_closes(false)
                .with_background_color((13, 15, 20, 255)),
        )
        .launch(DesktopRoot);
}

/// Wraps the shared app with the window controls its title bar needs, the
/// tray icon, and the ways back from the tray.
#[component]
fn DesktopRoot() -> Element {
    let window = dioxus::desktop::use_window();
    let focus = use_context::<Focus>();
    // Tray icon (see `tray.rs`): a click opens the window, the menu offers
    // the rest. Test runs skip it unless they ask; they open many windows.
    let commands = use_context::<AppConfig>().commands;
    let on_tray: tray::Handler = use_hook(|| {
        let window = window.window.clone();
        Arc::new(move |action| {
            let show = || {
                // tao proxies these to the GTK thread; the flag covers
                // platforms where it does not.
                SHOW_REQUESTED.store(true, Ordering::Relaxed);
                window.set_visible(true);
                window.set_focus();
                window.request_redraw();
            };
            match action {
                TrayAction::Open => show(),
                TrayAction::CheckForUpdates => {
                    commands.send(AppCommand::CheckForUpdates);
                    show();
                }
                TrayAction::Acknowledgements => {
                    commands.send(AppCommand::Acknowledgements);
                    show();
                }
                TrayAction::Quit => std::process::exit(0),
            }
        })
    });
    let wanted = std::env::var_os("MORPHEUS_E2E_PORT").is_none()
        || std::env::var_os("MORPHEUS_E2E_TRAY").is_some();
    #[cfg(target_os = "linux")]
    use_hook({
        let on_tray = on_tray.clone();
        move || {
            if wanted {
                tray::install(on_tray);
            }
        }
    });
    #[cfg(not(target_os = "linux"))]
    {
        use_hook(|| {
            if wanted {
                install_builtin_tray();
            }
        });
        // Left click shows the window (Dioxus does that itself here).
        dioxus::desktop::use_tray_menu_event_handler(move |event| {
            let picked = event.id().0.parse::<usize>().ok();
            if let Some(action) = picked.and_then(|index| TrayAction::MENU.get(index)) {
                on_tray(*action);
            }
        });
    }
    // Track focus for the notifier, and show the window when asked from
    // another thread (the tests' stand-in for a tray click).
    use_wry_event_handler({
        let window = window.window.clone();
        move |event, _| {
            if let Event::WindowEvent {
                event: WindowEvent::Focused(focused),
                ..
            } = event
            {
                focus.0.store(*focused, Ordering::Relaxed);
            }
            if SHOW_REQUESTED.swap(false, Ordering::Relaxed) {
                window.set_visible(true);
                window.set_focus();
            }
        }
    });
    use_hook({
        let window = window.window.clone();
        move || reopen_on_control_port(window)
    });
    // A second launch on the same data shows this window.
    use_hook({
        let window = window.window.clone();
        move || {
            single_instance::on_show(move || {
                SHOW_REQUESTED.store(true, Ordering::Relaxed);
                window.set_visible(true);
                window.set_focus();
                window.request_redraw();
            })
        }
    });

    use_context_provider(|| WindowControls {
        drag: Rc::new({
            let window = window.clone();
            move || window.drag()
        }),
        minimize: Rc::new({
            let window = window.clone();
            move || window.window.set_minimized(true)
        }),
        toggle_maximize: Rc::new({
            let window = window.clone();
            move || window.window.set_maximized(!window.window.is_maximized())
        }),
        // Hide rather than close: a programmatic close tears the webview down
        // regardless of the close behaviour, which is the tray's job to avoid.
        close: Rc::new(move || window.window.set_visible(false)),
    });
    // A test can box the app to a phone-sized viewport, since a tiling
    // compositor ignores the requested window size.
    let boxed = std::env::var("MORPHEUS_E2E_SIZE").ok();
    rsx! {
        if let Some(size) = boxed {
            div {
                class: "viewport-box",
                // The transform makes the box hold fixed overlays (menus, dialogs)
                // the way a phone screen would.
                style: "width: {size.split('x').next().unwrap_or(\"1280\")}px; height: {size.split('x').nth(1).unwrap_or(\"800\")}px; overflow: hidden; transform: translateZ(0);",
                morpheus_ui::App {}
            }
        } else {
            morpheus_ui::App {}
        }
    }
}

/// Consume the session's notification stream on a plain thread with its own
/// small runtime, and raise OS notifications unless the user is looking at
/// that room right now.
fn notifier(session: Session, focus: Focus, open_room: OpenRoom, data_dir: PathBuf) {
    std::thread::Builder::new()
        .name("morpheus-notifier".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(err) => {
                    tracing::warn!(%err, "notifier runtime failed");
                    return;
                }
            };
            runtime.block_on(async move {
                tokio::spawn(ring_notifier(
                    session.clone(),
                    focus.clone(),
                    data_dir.clone(),
                ));
                let mut updates = match session.notifications().await {
                    Ok(updates) => updates,
                    Err(err) => {
                        tracing::warn!(%err, "notification subscription failed");
                        return;
                    }
                };
                while let Some(notice) = updates.next().await {
                    let looking = focus.0.load(Ordering::Relaxed)
                        && open_room
                            .lock()
                            .ok()
                            .is_some_and(|room| room.as_ref() == Some(&notice.room_id));
                    if looking {
                        continue;
                    }
                    let title = if notice.is_mention {
                        format!(
                            "{} mentioned you in {}",
                            notice.sender_name, notice.room_name
                        )
                    } else {
                        format!("{} in {}", notice.sender_name, notice.room_name)
                    };
                    // Checked each time: Settings can turn it off while running.
                    if !morpheus_ui::device_prefs::notifications_enabled(&data_dir) {
                        tracing::info!("notifications are off on this device; not shown");
                        continue;
                    }
                    let icon =
                        picture_file(&session, notice.sender_avatar, ICON_SIZE, &data_dir).await;
                    let image = picture_file(&session, notice.image, IMAGE_SIZE, &data_dir).await;
                    notify(&data_dir, title, notice.body_summary, icon, image);
                }
            });
        })
        .map(drop)
        .unwrap_or_else(|err| tracing::warn!(%err, "notifier thread failed"));
}

/// Someone calling while the window is in the background or hidden: say
/// who (the ringtone plays in the app either way).
async fn ring_notifier(session: Session, focus: Focus, data_dir: PathBuf) {
    let mut rings = match session.ring_updates().await {
        Ok(rings) => rings,
        Err(err) => {
            tracing::warn!(%err, "ring subscription failed");
            return;
        }
    };
    while let Some(ring) = rings.next().await {
        let morpheus_core::Ringing::Incoming(call) = ring else {
            continue;
        };
        if focus.0.load(Ordering::Relaxed)
            || !morpheus_ui::device_prefs::notifications_enabled(&data_dir)
        {
            continue;
        }
        let (name, avatar) = match session.profile(call.caller.clone()).await {
            Ok(profile) => (profile.name, profile.avatar),
            Err(_) => (call.caller.localpart().to_owned(), None),
        };
        let icon = picture_file(&session, avatar, ICON_SIZE, &data_dir).await;
        let title = format!("{name} is calling");
        tracing::info!(%title, "ring notification");
        notify(&data_dir, title, "Open Morpheus to answer".into(), icon, None);
    }
}

/// Notification pictures: the sender's (small) and a photo sent (larger).
const ICON_SIZE: u32 = 96;
const IMAGE_SIZE: u32 = 512;

/// A picture for a notification, as a file (notification daemons read
/// pictures from disk). Kept by the media's key, so each is fetched once.
async fn picture_file(
    session: &Session,
    media: Option<MediaRef>,
    size: u32,
    data_dir: &Path,
) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let media = media?;
    let mut hasher = std::hash::DefaultHasher::new();
    (media.key(), size).hash(&mut hasher);
    let dir = data_dir.join("notification-pictures");
    let stem = format!("{:016x}", hasher.finish());
    for ext in ["png", "jpg", "gif", "webp"] {
        let kept = dir.join(format!("{stem}.{ext}"));
        if kept.exists() {
            return Some(kept);
        }
    }
    let fetched = session.thumbnail(media, size, size).await.ok().flatten()?;
    let ext = match fetched.mime() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => return None,
    };
    let path = dir.join(format!("{stem}.{ext}"));
    let written =
        std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, &fetched.bytes));
    if let Err(err) = written {
        tracing::warn!(%err, "notification picture not kept");
        return None;
    }
    Some(path)
}

/// Raise an OS notification, the sender's picture as its icon and a photo
/// sent shown in it. Off the UI thread, since the D-Bus round trip can
/// stall when no notification daemon answers. Test runs never notify (they
/// would pile real pop-ups on the developer's desktop): they write what
/// would show to `shown-notifications.jsonl` instead.
fn notify(
    data_dir: &Path,
    title: String,
    body: String,
    icon: Option<PathBuf>,
    image: Option<PathBuf>,
) {
    if std::env::var_os("MORPHEUS_E2E_PORT").is_some() {
        let line =
            serde_json::json!({ "title": title, "body": body, "icon": icon, "image": image });
        let written = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(data_dir.join("shown-notifications.jsonl"))
            .and_then(|mut file| {
                std::io::Write::write_all(&mut file, format!("{line}\n").as_bytes())
            });
        if let Err(err) = written {
            tracing::warn!(%err, "test notification log not written");
        }
        return;
    }
    std::thread::spawn(move || {
        let mut notification = notify_rust::Notification::new();
        notification.appname("Morpheus").summary(&title).body(&body);
        if let Some(icon) = &icon {
            notification.icon(&icon.to_string_lossy());
        }
        // Linux daemons show a picture in the notification; elsewhere the icon is all.
        #[cfg(all(unix, not(target_os = "macos")))]
        if let Some(image) = &image {
            notification.image_path(&image.to_string_lossy());
        }
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        drop(image);
        if let Err(err) = notification.show() {
            tracing::warn!(%err, "desktop notification failed");
        }
    });
}

/// Under test (`MORPHEUS_E2E_PORT` set), a connection to the port after the
/// bridge's brings a hidden window back, the way a tray click does. The
/// hidden webview cannot take that request through the bridge itself.
fn reopen_on_control_port(window: Arc<dioxus::desktop::tao::window::Window>) {
    let Some(port) = std::env::var("MORPHEUS_E2E_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
    else {
        return;
    };
    let listener = match std::net::TcpListener::bind(("127.0.0.1", port + 1)) {
        Ok(listener) => listener,
        Err(err) => {
            tracing::warn!(%err, "show-window control port not bound");
            return;
        }
    };
    std::thread::spawn(move || {
        for _ in listener.incoming().flatten() {
            // tao proxies these to the GTK thread, so they are safe here; the
            // flag covers platforms where they are not.
            SHOW_REQUESTED.store(true, Ordering::Relaxed);
            window.set_visible(true);
            window.set_focus();
            window.request_redraw();
        }
    });
}

/// `MORPHEUS_DATA_DIR` wins so several accounts can run side by side during
/// development (see `just desktop-as`). Otherwise the XDG data dir.
fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MORPHEUS_DATA_DIR") {
        return PathBuf::from(dir);
    }
    directories::ProjectDirs::from("dev", "morpheus", "Morpheus")
        .map(|dirs| dirs.data_dir().to_owned())
        .unwrap_or_else(|| std::env::temp_dir().join("morpheus"))
}

/// The tray through Dioxus's built-in support (macOS, Windows), where a
/// left click is reported. The handle must stay alive, so it is parked in
/// the component's context.
#[cfg(not(target_os = "linux"))]
fn install_builtin_tray() {
    use dioxus::desktop::trayicon::{
        Icon, TrayIconBuilder,
        menu::{Menu, MenuItem, PredefinedMenuItem},
    };
    let menu = Menu::new();
    for (index, action) in TrayAction::MENU.into_iter().enumerate() {
        if action == TrayAction::Quit {
            let _ = menu.append(&PredefinedMenuItem::separator());
        }
        let item = MenuItem::with_id(index.to_string(), action.label(), true, None);
        if let Err(err) = menu.append(&item) {
            tracing::warn!(%err, "tray menu failed");
            return;
        }
    }
    let mut builder = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("Morpheus");
    match tray::logo_rgba().and_then(|(rgba, width, height)| {
        Icon::from_rgba(rgba, width, height).map_err(|e| e.to_string())
    }) {
        Ok(icon) => builder = builder.with_icon(icon),
        Err(err) => tracing::warn!(%err, "tray icon image failed, using none"),
    }
    match builder.build() {
        Ok(tray) => {
            provide_context(tray);
        }
        Err(err) => tracing::warn!(%err, "no tray icon"),
    }
}

/// One Morpheus per data directory: a second launch asks the running one to
/// show its window over a socket there, then exits (Unix; elsewhere a second
/// copy still starts).
mod single_instance {
    use std::path::Path;
    use std::sync::OnceLock;

    #[cfg(unix)]
    use std::os::unix::net::{UnixListener, UnixStream};

    const SOCKET: &str = "instance.sock";

    #[cfg(unix)]
    static LISTENER: OnceLock<UnixListener> = OnceLock::new();

    /// `false` when another Morpheus already runs on `data_dir` (it has been
    /// asked to show itself, and this process should exit).
    /// The socket for `data_dir`: in the runtime dir, named after a hash of
    /// the data dir (socket paths are limited to about a hundred bytes, which
    /// a data dir can exceed).
    #[cfg(unix)]
    fn socket_path(data_dir: &Path) -> std::path::PathBuf {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        data_dir.hash(&mut hasher);
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        dir.join(format!("morpheus-{:016x}-{SOCKET}", hasher.finish()))
    }

    pub fn claim(data_dir: &Path) -> bool {
        #[cfg(unix)]
        {
            use std::io::Write;
            let path = socket_path(data_dir);
            if let Ok(mut running) = UnixStream::connect(&path) {
                let _ = running.write_all(b"show\n");
                return false;
            }
            // Nobody answered: a leftover from a crash, or nothing yet.
            let _ = std::fs::remove_file(&path);
            match UnixListener::bind(&path) {
                Ok(listener) => {
                    let _ = LISTENER.set(listener);
                }
                Err(err) => tracing::warn!(%err, "single-instance socket not bound"),
            }
        }
        #[cfg(not(unix))]
        let _ = data_dir;
        true
    }

    /// Call `show` for every later launch that asks.
    pub fn on_show(show: impl Fn() + Send + 'static) {
        #[cfg(unix)]
        if let Some(listener) = LISTENER.get() {
            std::thread::spawn(move || {
                for _ in listener.incoming().flatten() {
                    show();
                }
            });
        }
        #[cfg(not(unix))]
        let _ = show;
    }
}

#[cfg(test)]
mod tests {
    /// A release suggests a real homeserver; the dev one is for test builds.
    #[test]
    fn suggests_matrix_org_outside_test_builds() {
        assert_eq!(super::default_homeserver(), "matrix.org");
    }
}
