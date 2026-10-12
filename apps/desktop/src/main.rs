//! Desktop entry point. Picks the renderer, locates the data directory and
//! hands everything else to `morpheus_ui`. Also owns what must keep working
//! while the window is hidden in the tray: OS notifications and reopening.

use std::{
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use dioxus::desktop::{
    Config, LogicalSize, WindowBuilder, WindowCloseBehaviour,
    tao::event::{Event, WindowEvent},
    trayicon::{
        Icon as TrayImage, TrayIconBuilder,
        menu::{Menu, MenuItem, PredefinedMenuItem},
    },
    use_tray_menu_event_handler, use_wry_event_handler,
};
use dioxus::prelude::*;
use morpheus_core::Session;
use morpheus_ui::{AppConfig, OpenRoom, WindowControls};
use tracing_subscriber::EnvFilter;

const DEFAULT_HOMESERVER: &str = "http://localhost:8008";

/// Window focus as the event loop reports it, readable from any thread.
#[derive(Clone)]
struct Focus(Arc<AtomicBool>);

/// Set by the control-port thread; the event loop shows the window on the main
/// thread (GTK insists) at the next event, which the thread asks for.
static SHOW_REQUESTED: AtomicBool = AtomicBool::new(false);

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "morpheus=debug,warn".into()),
        )
        .init();

    let focus = Focus(Arc::new(AtomicBool::new(true)));
    let mut config = AppConfig::new(
        data_dir(),
        std::env::var("MORPHEUS_HOMESERVER").unwrap_or_else(|_| DEFAULT_HOMESERVER.into()),
    );
    // OS notifications run on their own thread: the webview's runtime stops
    // while the window is hidden, and that is exactly when they matter.
    config.on_session = Some(Arc::new({
        let focus = focus.clone();
        let open_room = config.open_room.clone();
        move |session| notifier(session, focus.clone(), open_room.clone())
    }));
    tracing::info!(?config.data_dir, "starting morpheus desktop");

    // Test runs open many windows; they must not steal focus from whatever
    // the developer is typing into. A distinct title lets the e2e driver
    // (and a compositor rule) tell them apart from a real session.
    let under_test = std::env::var_os("MORPHEUS_E2E_PORT").is_some();
    // Tests can ask for a phone-sized window to exercise the narrow layout.
    let (width, height) = std::env::var("MORPHEUS_E2E_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse::<f64>().ok()?, h.parse::<f64>().ok()?))
        })
        .unwrap_or((1280.0, 800.0));
    let window = WindowBuilder::new()
        .with_title(if under_test { "Morpheus (test)" } else { "Morpheus" })
        .with_focused(!under_test)
        // The shell draws its own Discord-style bar; see `DesktopRoot`.
        .with_decorations(false)
        .with_inner_size(LogicalSize::new(width, height));

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
                .with_exits_when_last_window_closes(false),
        )
        .launch(DesktopRoot);
}

/// Wraps the shared app with the window controls its title bar needs, the
/// tray icon, and the ways back from the tray.
#[component]
fn DesktopRoot() -> Element {
    let window = dioxus::desktop::use_window();
    let focus = use_context::<Focus>();
    // Tray icon: left click shows the window (Dioxus does that), the menu
    // reopens or quits. Test runs skip it: they open many windows.
    use_hook(|| {
        if std::env::var_os("MORPHEUS_E2E_PORT").is_none() {
            install_tray();
        }
    });
    use_tray_menu_event_handler({
        let window = window.clone();
        move |event| match event.id().0.as_str() {
            "open" => {
                window.window.set_visible(true);
                window.window.set_focus();
            }
            "quit" => std::process::exit(0),
            _ => {}
        }
    });
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
                style: "width: {size.split('x').next().unwrap_or(\"1280\")}px; height: {size.split('x').nth(1).unwrap_or(\"800\")}px; overflow: hidden;",
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
fn notifier(session: Session, focus: Focus, open_room: OpenRoom) {
    std::thread::Builder::new()
        .name("morpheus-notifier".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(runtime) => runtime,
                Err(err) => {
                    tracing::warn!(%err, "notifier runtime failed");
                    return;
                }
            };
            runtime.block_on(async move {
                let mut updates = match session.notifications().await {
                    Ok(updates) => updates,
                    Err(err) => {
                        tracing::warn!(%err, "notification subscription failed");
                        return;
                    }
                };
                while let Some(notice) = updates.next().await {
                    let looking = focus.0.load(Ordering::Relaxed)
                        && open_room.lock().ok().is_some_and(|room| room.as_ref() == Some(&notice.room_id));
                    if looking {
                        continue;
                    }
                    let title = if notice.is_mention {
                        format!("{} mentioned you in {}", notice.sender_name, notice.room_name)
                    } else {
                        format!("{} in {}", notice.sender_name, notice.room_name)
                    };
                    notify(title, notice.body_summary);
                }
            });
        })
        .map(drop)
        .unwrap_or_else(|err| tracing::warn!(%err, "notifier thread failed"));
}

/// Raise an OS notification. Off the UI thread, since the D-Bus round trip
/// can stall when no notification daemon answers. Test runs never notify:
/// they would pile real pop-ups on the developer's desktop.
fn notify(title: String, body: String) {
    if std::env::var_os("MORPHEUS_E2E_PORT").is_some() {
        return;
    }
    std::thread::spawn(move || {
        if let Err(err) = notify_rust::Notification::new()
            .appname("Morpheus")
            .summary(&title)
            .body(&body)
            .show()
        {
            tracing::warn!(%err, "desktop notification failed");
        }
    });
}

/// Under test (`MORPHEUS_E2E_PORT` set), a connection to the port after the
/// bridge's brings a hidden window back, the way a tray click does. The
/// hidden webview cannot take that request through the bridge itself.
fn reopen_on_control_port(window: Arc<dioxus::desktop::tao::window::Window>) {
    let Some(port) = std::env::var("MORPHEUS_E2E_PORT").ok().and_then(|p| p.parse::<u16>().ok()) else {
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

/// Put the logo in the system tray with an Open / Quit menu. The tray
/// handle must stay alive, so it is parked in the component's context. On
/// Linux this needs libayatana-appindicator; without it the app runs
/// without a tray and says so in the log.
fn install_tray() {
    let menu = Menu::new();
    if let Err(err) = menu.append_items(&[
        &MenuItem::with_id("open", "Open Morpheus", true, None),
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id("quit", "Quit", true, None),
    ]) {
        tracing::warn!(%err, "tray menu failed");
        return;
    }
    let mut builder = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("Morpheus");
    match tray_image() {
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

/// The logo rendered to 64x64 RGBA (`assets/tray.png`, made with
/// `rsvg-convert` from `crates/ui/assets/logo.svg`).
fn tray_image() -> Result<TrayImage, String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(include_bytes!("../assets/tray.png").as_slice()));
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => buf[..info.buffer_size()].chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        other => return Err(format!("unsupported tray png colour type {other:?}")),
    };
    TrayImage::from_rgba(rgba, info.width, info.height).map_err(|e| e.to_string())
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
