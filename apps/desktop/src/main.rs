//! Morpheus desktop, drawn with GPUI. The window, the bundled fonts and the
//! root view: a splash while the saved session restores, then the sign-in
//! screen or the room list.

// A window, not a console: release builds on Windows open no terminal.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::{borrow::Cow, path::PathBuf};

mod about;
mod badge;
mod call;
mod call_invite;
mod call_ui;
mod chat;
mod chrome;
mod composer;
mod diagnostics;
mod dialogs;
mod e2e;
mod emoji_picker;
mod friend_picker;
mod friends;
mod graphics;
mod hotkeys;
mod hover_underline;
mod icons;
mod input;
mod instance;
mod invites;
mod last_place;
mod lightbox;
mod links;
mod locks;
mod log_file;
mod login;
mod mac_menu;
mod media;
mod memory;
mod member_actions;
mod members;
mod menu;
mod mini_call;
mod message;
mod mod_view;
mod model;
mod noise;
mod notify;
mod pictures;
mod prefs;
mod press;
mod profile;
mod profile_actions;
mod profile_sync;
mod rail;
mod room_dialogs;
mod room_list;
mod room_menus;
mod room_preview;
mod screen_picker;
mod screen_quality;
mod search;
mod settings;
mod shell;
mod sounds;
mod streamer;
mod switcher;
mod tab;
mod theme;
mod thread;
mod timeline;
mod titlebar;
mod tray;
mod typing;
mod update;
mod update_self_test;
mod user_bar;
mod video;
mod welcome;
mod vim;
#[cfg(target_os = "windows")]
mod win_badge;
#[cfg(target_os = "windows")]
mod win_sys;
mod zoom;
// The view-models live in their own crate; the old module path stays valid.
use morpheus_view_model as view_model;

use e2e::E2eExt as _;

/// Rust's allocations through mimalloc, which hands freed memory back to
/// the system; glibc's malloc held on to most of what a big room freed.
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

use gpui::{
    App, Application, AssetSource, Bounds, Context, Entity, IntoElement, Render, SharedString,
    Window, WindowBounds, WindowOptions, div, prelude::*, px, rgb, size, svg,
};
use morpheus_core::Core;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};

use login::{Login, SignedIn};
use shell::Shell;

/// Static instances of Inter and Unbounded (GPUI cannot read the woff2
/// variable fonts the Dioxus app uses), made from `crates/ui/assets/fonts`.
const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/Inter-400.ttf"),
    include_bytes!("../assets/fonts/Inter-600.ttf"),
    include_bytes!("../assets/fonts/Inter-700.ttf"),
    include_bytes!("../assets/fonts/Unbounded-400.ttf"),
    include_bytes!("../assets/fonts/Unbounded-600.ttf"),
    include_bytes!("../assets/fonts/Unbounded-700.ttf"),
    // Noto Color Emoji 2.051 (OFL, bitmap strikes, 10.7 MB): gpui shapes emoji sequences with it
    // (vendor/gpui text_system.rs), whatever the system has installed.
    include_bytes!("../assets/fonts/NotoColorEmoji.ttf"),
];

/// Files `svg().path(..)` loads: the logo.
struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        Ok(match path {
            "logo.svg" => Some(Cow::Borrowed(
                include_bytes!("../../../crates/ui/assets/logo.svg").as_slice(),
            )),
            // The glasses only, as the site's wordmark crops them.
            "logo-tight.svg" => Some(Cow::Owned(
                include_str!("../../../crates/ui/assets/logo.svg")
                    .replacen("viewBox=\"0 0 512 512\"", "viewBox=\"48 182 416 154\"", 1)
                    .into_bytes(),
            )),
            other => icons::svg_for(other).map(|svg| Cow::Owned(svg.into_bytes())),
        })
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

enum Screen {
    /// The saved session is being restored.
    Splash,
    Login(Entity<Login>),
    Rooms(Entity<Shell>),
}

struct Root {
    core: Core,
    homeserver: String,
    screen: Screen,
    /// Matrix links handed over before anyone is signed in.
    pending_links: Vec<String>,
    /// Tray items that need the shell, asked for before it was there.
    pending_tray: Vec<tray::TrayAction>,
}

impl Root {
    fn new(core: Core, homeserver: String, cx: &mut Context<Self>) -> Self {
        let kept = kept_session();
        let restoring = kept.is_none().then(|| core.restore_session());
        cx.spawn(async move |this, cx| {
            let result = match (kept, restoring) {
                (Some(session), _) => Ok(Some(session)),
                (None, Some(restoring)) => restoring.await,
                (None, None) => Ok(None),
            };
            cx.update(|cx| {
                this.update(cx, |this, cx| match result {
                    Ok(Some(session)) => this.signed_in(session, cx),
                    Ok(None) => this.show_login(None, cx),
                    Err(err) => this.show_login(Some(err.to_string()), cx),
                })
            })
            .ok();
        })
        .detach();
        Self {
            core,
            homeserver,
            screen: Screen::Splash,
            pending_links: Vec::new(),
            pending_tray: Vec::new(),
        }
    }

    fn show_login(&mut self, restore_error: Option<String>, cx: &mut Context<Self>) {
        let (core, homeserver) = (self.core.clone(), self.homeserver.clone());
        // Started hidden in the tray, but nobody is signed in: the window comes up.
        if cx.windows().is_empty() {
            show_window(cx);
        }
        let Some(window) = cx.windows().first().copied() else {
            return;
        };
        let login = window
            .update(cx, |_, window, cx| {
                let login = cx.new(|cx| Login::new(core, &homeserver, restore_error, window, cx));
                login
            })
            .ok();
        if let Some(login) = login {
            cx.subscribe(&login, |this: &mut Root, _, event: &SignedIn, cx| {
                this.signed_in(event.0.clone(), cx);
            })
            .detach();
            self.screen = Screen::Login(login);
        }
        cx.notify();
    }

    fn signed_in(&mut self, session: morpheus_core::Session, cx: &mut Context<Self>) {
        if let Ok(mut kept) = SESSION.lock() {
            *kept = Some(session.clone());
        }
        // Each sign-in (a switch of account too) gets its own announcers; those
        // of the account left behind go quiet with its stopped sync.
        notify::spawn_notifier(session.clone(), || send_command(Command::Redraw));
        notify::spawn_ring_watcher(session.clone());
        self.show_shell(session, cx);
        for link in std::mem::take(&mut self.pending_links) {
            self.open_link(link, cx);
        }
        for action in std::mem::take(&mut self.pending_tray) {
            self.tray_action(action, cx);
        }
        cx.notify();
    }

    /// Builds the interface (the shell) for a signed-in account; also what Ctrl+R
    /// does, for the same account.
    fn show_shell(&mut self, session: morpheus_core::Session, cx: &mut Context<Self>) {
        let shell = cx.new(|cx| Shell::new(session, cx));
        cx.subscribe(
            &shell,
            |this: &mut Root, _, event: &shell::AccountEvent, cx| {
                this.account_event(event.clone(), cx)
            },
        )
        .detach();
        // The accounts on this device, for the account menu.
        let listing = self.core.accounts();
        let target = shell.clone();
        cx.spawn(async move |_, cx| {
            if let Ok(accounts) = listing.await {
                target
                    .update(cx, |shell, cx| {
                        shell.accounts = accounts;
                        cx.notify();
                    })
                    .ok();
            }
        })
        .detach();
        self.screen = Screen::Rooms(shell);
        cx.notify();
    }

    /// The account menu's doings: switch to a stored account, sign another
    /// in (this one stays signed in; the sign-in screen offers the way
    /// back), or sign this one out (the next stored account, if any, opens).
    fn account_event(&mut self, event: shell::AccountEvent, cx: &mut Context<Self>) {
        let Screen::Rooms(shell) = &self.screen else {
            return;
        };
        let Some(session) = shell.read(cx).session.clone() else {
            return;
        };
        if matches!(event, shell::AccountEvent::Reload) {
            self.show_shell(session, cx);
            return;
        }
        let core = self.core.clone();
        cx.spawn(async move |this, cx| {
            let next = match event {
                shell::AccountEvent::Reload => None,
                shell::AccountEvent::Add => {
                    let _ = session.suspend().await;
                    None
                }
                shell::AccountEvent::Switch(key) => {
                    let _ = session.suspend().await;
                    core.switch_account(key).await.ok().flatten()
                }
                shell::AccountEvent::SignOut => {
                    locks::clear_all();
                    let _ = session.logout().await;
                    core.restore_session().await.ok().flatten()
                }
            };
            this.update(cx, |this, cx| match next {
                Some(session) => this.signed_in(session, cx),
                None => this.show_login(None, cx),
            })
            .ok();
        })
        .detach();
    }
}
impl Root {
    /// Run `f` on the shell, when signed in.
    fn with_shell(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Shell, &mut Context<Shell>),
    ) {
        if let Screen::Rooms(shell) = &self.screen {
            shell.update(cx, f);
        }
    }

    /// A tray item that needs the shell: done once signed in.
    fn tray_action(&mut self, action: tray::TrayAction, cx: &mut Context<Self>) {
        if !matches!(self.screen, Screen::Rooms(_)) {
            self.pending_tray.push(action);
            return;
        }
        self.with_shell(cx, |shell, cx| match action {
            tray::TrayAction::Acknowledgements => shell.open_about(cx),
            tray::TrayAction::CheckForUpdates => shell.check_for_updates(true, cx),
            _ => {}
        });
    }

    /// A Matrix link from outside: opened once signed in.
    fn open_link(&mut self, link: String, cx: &mut Context<Self>) {
        if matches!(self.screen, Screen::Rooms(_)) {
            self.with_shell(cx, |shell, cx| shell.open_link(&link, cx));
        } else {
            self.pending_links.push(link);
        }
    }
}

impl Render for Root {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let base = div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme::bg_chat()))
            .font_family("Inter")
            .text_size(px(theme::REM))
            .text_color(rgb(theme::text()));
        e2e::frame(
            match &self.screen {
                // The logo, no name.
                Screen::Splash => base.child(chrome::window_bar(window)).child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            svg()
                                .path("logo.svg")
                                .size(px(160.0))
                                .text_color(rgb(theme::accent()))
                                .e2e("svg", "splash-logo"),
                        )
                        .e2e("div", "splash"),
                ),
                Screen::Login(login) => base
                    .child(chrome::window_bar(window))
                    .child(div().flex_1().min_h_0().child(login.clone())),
                Screen::Rooms(rooms) => base.child(div().flex_1().min_h_0().child(rooms.clone())),
            }
            .child(chrome::resize_edges(window))
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, window, cx| {
                tab::on_key(event, None, window, cx);
            }))
            .e2e("body", "app")
            .attr("data-font", "Inter")
            .attr("data-bg", format!("{:06x}", theme::bg_chat()))
            .attr("data-color", format!("{:06x}", theme::text()))
            .attr(
                "data-cursor",
                if cx.has_active_drag() {
                    "grabbing"
                } else {
                    "auto"
                },
            ),
        )
    }
}

/// Test builds are titled as the Dioxus app's are, so the test setup's window
/// rules (unfocused, on a monitor of their own) apply to them.
const TITLE: &str = if cfg!(feature = "e2e") {
    "Morpheus (test)"
} else {
    "Morpheus"
};

/// 1100x720, or `MORPHEUS_E2E_SIZE=WxH` in a test build.
fn window_size() -> gpui::Size<gpui::Pixels> {
    let asked = cfg!(feature = "e2e")
        .then(|| std::env::var("MORPHEUS_E2E_SIZE").ok())
        .flatten()
        .and_then(|size| {
            let (w, h) = size.split_once('x')?;
            Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))
        });
    let (w, h) = asked.unwrap_or((1100.0, 720.0));
    size(px(w), px(h))
}

/// `MORPHEUS_DATA_DIR`, else the platform data directory the Dioxus app used.
pub(crate) fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MORPHEUS_DATA_DIR") {
        return PathBuf::from(dir);
    }
    default_data_dir()
}

/// Where a release keeps its data: the Dioxus app's directory (qualifier
/// "dev", organisation "morpheus", application "Morpheus": on Linux
/// `~/.local/share/morpheus`), never renamed, so the login, keys and settings
/// survive the switch to this app.
fn default_data_dir() -> PathBuf {
    directories::ProjectDirs::from("dev", "morpheus", "Morpheus")
        .map(|dirs| dirs.data_dir().to_owned())
        .unwrap_or_else(|| std::env::temp_dir().join("morpheus"))
}

/// What the sign-in form suggests: `MORPHEUS_HOMESERVER`, else the dev server
/// in test builds and matrix.org otherwise.
fn default_homeserver() -> String {
    std::env::var("MORPHEUS_HOMESERVER").unwrap_or_else(|_| {
        if cfg!(feature = "e2e") {
            "http://localhost:8008".into()
        } else {
            "matrix.org".into()
        }
    })
}

/// What the tray, a later launch or the test control port asks the running
/// app for.
/// What the tray, a later launch or the test control port asks the running
/// app for.
#[derive(Debug, Clone)]
pub(crate) enum Command {
    /// Bring the window back (opening it if it was closed to the tray).
    Show,
    /// Open a Matrix link in the app.
    Open(String),
    Tray(tray::TrayAction),
    /// The notifier logged something: an open window shows it.
    Redraw,
    /// Quit (the tray, Cmd+Q or Ctrl+Q, SIGTERM): hang up first, then end.
    Quit,
}

static COMMANDS: OnceLock<UnboundedSender<Command>> = OnceLock::new();

/// Ask the running app for something, from any thread.
pub(crate) fn send_command(command: Command) {
    if let Some(commands) = COMMANDS.get() {
        let _ = commands.unbounded_send(command);
    }
}

/// The signed-in session, kept while the window is closed to the tray and
/// handed to the next window, so no one signs in again.
static SESSION: Mutex<Option<morpheus_core::Session>> = Mutex::new(None);

fn kept_session() -> Option<morpheus_core::Session> {
    SESSION.lock().ok()?.clone()
}

/// Whether no window is open (closed to the tray): what a window does is done
/// by threads meanwhile.
static HEADLESS: AtomicBool = AtomicBool::new(false);

pub(crate) fn headless() -> bool {
    HEADLESS.load(Ordering::Relaxed)
}

/// The one root view of a window's run.
struct MainRoot(Entity<Root>);
impl gpui::Global for MainRoot {}

fn window_options(cx: &App) -> WindowOptions {
    let bounds = Bounds::centered(None, window_size(), cx);
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        // macOS: the system bar transparent, its traffic lights on ours;
        // Windows: no system caption, ours is the only title bar.
        titlebar: Some(
            chrome::Layout::for_os(chrome::Os::current(), false)
                .titlebar_options(SharedString::from(TITLE)),
        ),
        app_id: Some("morpheus-desktop".into()),
        // The app draws its own bar, buttons and resize edges (Linux;
        // macOS and Windows ignore this).
        window_decorations: Some(gpui::WindowDecorations::Client),
        ..Default::default()
    }
}

/// Open the window on the root view.
fn open_main_window(cx: &mut App) {
    let Some(root) = cx.try_global::<MainRoot>().map(|root| root.0.clone()) else {
        return;
    };
    let options = window_options(cx);
    let opened = cx.open_window(options, move |window, _| {
        window.set_window_title(TITLE);
        root
    });
    match &opened {
        Err(e) => eprintln!("morpheus-desktop: opening the window failed: {e}"),
        // For scripts/mac-check.sh: whose buttons are on the bar, where the
        // bar's content starts, and whether the content reaches the window's
        // top (a transparent system bar) or sits under a system bar.
        Ok(handle) => {
            let _ = handle.update(cx, |_, window, _| {
                let layout = chrome::Layout::current(window);
                let (frame, content) = (window.bounds().size, window.viewport_size());
                eprintln!(
                    "morpheus-desktop: window chrome: buttons {}, inset {}, frame {}x{}, content {}x{}",
                    layout.buttons(),
                    layout.left_padding,
                    f32::from(frame.width),
                    f32::from(frame.height),
                    f32::from(content.width),
                    f32::from(content.height),
                );
            });
        }
    }
    // macOS: a program started from a shell (not Finder) is not made the
    // active app by the system; a window of an inactive app takes no keys.
    #[cfg(target_os = "macos")]
    cx.activate(true);
    HEADLESS.store(false, Ordering::Relaxed);
    gpui::KEEP_RUNNING_WITHOUT_WINDOWS.store(tray::hides_on_close(), Ordering::Relaxed);
    #[cfg(feature = "e2e")]
    if opened.is_ok() {
        e2e::start(cx);
    }
}

/// The window forward: raised if it is open.
pub(crate) fn show_window(cx: &mut App) {
    match cx.windows().first().copied() {
        Some(window) => {
            window
                .update(cx, |_, window, _| window.activate_window())
                .ok();
            #[cfg(target_os = "macos")]
            cx.activate(true);
        }
        None => open_main_window(cx),
    }
}

fn run_command(command: Command, cx: &mut App) {
    let Some(root) = cx.try_global::<MainRoot>().map(|root| root.0.clone()) else {
        return;
    };
    match command {
        Command::Show | Command::Tray(tray::TrayAction::Open) => show_window(cx),
        Command::Open(link) => {
            show_window(cx);
            root.update(cx, |root, cx| root.open_link(link, cx));
        }
        Command::Tray(
            action @ (tray::TrayAction::Acknowledgements | tray::TrayAction::CheckForUpdates),
        ) => {
            show_window(cx);
            root.update(cx, |root, cx| root.tray_action(action, cx));
        }
        Command::Tray(tray::TrayAction::Quit) | Command::Quit => quit(cx),
        Command::Redraw => {
            root.update(cx, |root, cx| {
                root.with_shell(cx, |shell, cx| shell.sync_notices(cx))
            });
        }
    }
}

/// Set once quitting has begun: a second ask (a signal during the tray's
/// Quit) does not start it over.
static QUITTING: AtomicBool = AtomicBool::new(false);

/// End the app, hanging up any call first (at most a couple of seconds, see
/// `Shell::leave_before_quit`), so the others do not see a ghost in it. Every
/// way out goes through here: the tray's and the menu's Quit, Cmd+Q and
/// Ctrl+Q, the window's close button when there is no tray to close to,
/// SIGTERM/SIGINT/SIGHUP (logout, `kill`).
pub(crate) fn quit(cx: &mut App) {
    if QUITTING.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Some(root) = cx.try_global::<MainRoot>().map(|root| root.0.clone()) {
        root.update(cx, |root, cx| {
            root.with_shell(cx, |shell, _| shell.leave_before_quit())
        });
    }
    std::process::exit(0);
}

/// The system asking the app to end (logout, shutdown, `kill`, Ctrl+C in a
/// terminal) quits as the Quit item does. Should the app not get to it
/// (its event loop stuck), it ends a little later anyway.
#[cfg(unix)]
fn quit_on_signals() {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    let mut signals = match signal_hook::iterator::Signals::new([SIGTERM, SIGINT, SIGHUP]) {
        Ok(signals) => signals,
        Err(err) => {
            eprintln!("morpheus-desktop: not listening for signals: {err}");
            return;
        }
    };
    std::thread::spawn(move || {
        if let Some(signal) = signals.forever().next() {
            eprintln!("morpheus-desktop: signal {signal}: quitting");
            send_command(Command::Quit);
            std::thread::sleep(std::time::Duration::from_secs(4));
            eprintln!("morpheus-desktop: quitting took too long; ending now");
            std::process::exit(0);
        }
    });
}

/// Under test, a connection to the port after the bridge's brings a closed
/// window back, the way a tray click does.
#[cfg(feature = "e2e")]
fn control_port() {
    let Some(port) = std::env::var("MORPHEUS_E2E_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
    else {
        return;
    };
    let listener = match std::net::TcpListener::bind(("127.0.0.1", port + 1)) {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("morpheus-desktop: show-window control port not bound: {err}");
            return;
        }
    };
    std::thread::spawn(move || {
        for _ in listener.incoming().flatten() {
            send_command(Command::Show);
        }
    });
}

type Commands = Arc<Mutex<UnboundedReceiver<Command>>>;

/// The next command waiting, if any.
fn next_command(commands: &Commands) -> Option<Command> {
    commands.lock().ok()?.try_recv().ok()
}

/// The window system's run: one for the whole life of the app. Closing the
/// last window does not end it (vendor/gpui's `KEEP_RUNNING_WITHOUT_WINDOWS`,
/// set while there is a tray to bring the window back): the shell, a call's
/// media and the sync all live in this run's entities, so they go on, and
/// what needs a window opens a new one on the same root.
fn run_window(core: Core, commands: Commands, first: Option<Command>, start_hidden: bool) {
    let application = Application::new().with_assets(Assets);
    // Clicking the app in the Dock (or opening it again) on a Mac brings the
    // closed window back, as Discord's does.
    #[cfg(target_os = "macos")]
    application.on_reopen(show_window);
    // matrix: links (the bundle registers the scheme) arrive as events, not arguments.
    #[cfg(target_os = "macos")]
    application.on_open_urls(|urls| {
        for url in urls {
            send_command(Command::Open(url));
        }
    });
    application.run(move |cx: &mut App| {
        if let Err(e) = cx
            .text_system()
            .add_fonts(FONTS.iter().map(|f| Cow::Borrowed(*f)).collect())
        {
            eprintln!("morpheus-desktop: loading fonts failed: {e}");
        }
        // macOS: the menu bar and its shortcuts.
        mac_menu::install(cx);
        // Windows and macOS: the tray icon, now that the event loop runs.
        tray::install_native(Arc::new(|action| send_command(Command::Tray(action))), cx);
        let root = cx.new(|cx| Root::new(core, default_homeserver(), cx));
        cx.set_global(MainRoot(root));
        cx.on_window_closed(|cx| {
            // No tray to close to: closing the window is quitting.
            if cx.windows().is_empty()
                && !gpui::KEEP_RUNNING_WITHOUT_WINDOWS.load(Ordering::Relaxed)
            {
                eprintln!("morpheus-desktop: window closed, quitting");
                quit(cx);
            }
            // Closed to the tray, by the app's close or the system's (the
            // caption's close on Windows, Alt+F4, the taskbar's Close
            // window): nobody is looking, so messages notify.
            if cx.windows().is_empty() {
                eprintln!("morpheus-desktop: window closed, app runs on");
                notify::set_looking(false, None);
                HEADLESS.store(true, Ordering::Relaxed);
                #[cfg(feature = "e2e")]
                e2e::run_ended();
            }
        })
        .detach();
        cx.spawn(async move |cx| {
            loop {
                match next_command(&commands) {
                    Some(command) => {
                        cx.update(|cx| run_command(command, cx)).ok();
                    }
                    None => {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(30))
                            .await
                    }
                }
            }
        })
        .detach();
        if start_hidden {
            // Started hidden in the tray (autostart): no window until asked.
            gpui::KEEP_RUNNING_WITHOUT_WINDOWS.store(true, Ordering::Relaxed);
            HEADLESS.store(true, Ordering::Relaxed);
        } else {
            open_main_window(cx);
        }
        if let Some(command) = first {
            run_command(command, cx);
        }
    });
}

/// The terminal, and a file in the data dir (`morpheus.log`, fresh each
/// start, the last three runs' kept beside it; see `log_file`) to read
/// after something went wrong; Settings' "Report a bug" sends its end.
/// Panics are written to it too.
fn init_logging() {
    use tracing_subscriber::{
        EnvFilter, Layer as _, filter::FilterExt as _, layer::SubscriberExt as _,
        util::SubscriberInitExt as _,
    };
    let filter = || {
        EnvFilter::try_from_default_env()
            // PulseAudio's client (cpal's, for the microphone) reports its
            // own closing as an error each time a stream stops.
            .unwrap_or_else(|_| {
                "morpheus=debug,livekit=info,pulseaudio::client::reactor=off,warn".into()
            })
    };
    let log_file = log_file::start(&data_dir()).ok();
    if log_file.is_some() {
        log_file::log_panics(log_file::path(&data_dir(), 0));
    }
    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(filter().and(log_file::QuietExpected)))
        .with(log_file.map(|file| {
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(std::sync::Mutex::new(file))
                .with_filter(filter().and(log_file::QuietExpected))
        }))
        .try_init();
}

fn main() {
    // Sounds at the volume the app plays them: WirePlumber otherwise saves a
    // volume per app name and puts it back on every stream (across restarts),
    // and one saved at 0 silenced every chime and ring for good. PulseAudio's
    // client properties come from this variable (what the system mixer shows
    // too); setting it means starting over with it.
    //
    // The same restart tells mimalloc (read when the process starts) to hand
    // freed memory back at once: with its default delay, memory freed by a
    // thread that then goes idle (GPUI's workers decoding pictures) stayed
    // resident; opening 30 rooms with pictures left 636 MB resident against
    // 459 MB with this (issue #10).
    #[cfg(target_os = "linux")]
    if (std::env::var_os("PULSE_PROP").is_none()
        || std::env::var_os("MIMALLOC_PURGE_DELAY").is_none())
        && let Ok(exe) = std::env::current_exe()
    {
        use std::os::unix::process::CommandExt as _;
        let mut command = std::process::Command::new(exe);
        command.args(std::env::args_os().skip(1));
        if std::env::var_os("PULSE_PROP").is_none() {
            command.env("PULSE_PROP", "state.restore-props=false");
        }
        if std::env::var_os("MIMALLOC_PURGE_DELAY").is_none() {
            command.env("MIMALLOC_PURGE_DELAY", "0");
        }
        let err = command.exec();
        eprintln!("morpheus-desktop: could not restart with PULSE_PROP: {err}");
    }
    // CI's check that an update installs and the new version starts: no window.
    if update_self_test::requested() {
        update_self_test::run(data_dir());
    }
    // CI's check (scripts/mac-check.sh) that the shared screen's sound starts
    // with what this computer plays, or says why not: no window.
    if let Some(seconds) = std::env::var_os("MORPHEUS_SCREEN_AUDIO_SELF_TEST") {
        init_logging();
        let seconds = seconds.to_string_lossy().parse().unwrap_or(3.0);
        match morpheus_call::screen_audio_self_test(seconds) {
            Ok(report) => eprintln!("screen audio self-test: ok: {report}"),
            Err(why) => eprintln!("screen audio self-test: failed: {why}"),
        }
        std::process::exit(0);
    }
    // The camera path without a window (#74; windows.yml, or by hand on a
    // laptop): list the cameras and, with one, open it as the preview does,
    // take pictures for the seconds given, and let it go. The result also
    // goes to MORPHEUS_CAMERA_SELF_TEST_OUT if set (a release build on
    // Windows has no console).
    if let Some(seconds) = std::env::var_os("MORPHEUS_CAMERA_SELF_TEST") {
        init_logging();
        let seconds = seconds.to_string_lossy().parse().unwrap_or(3.0);
        let (line, code) = match morpheus_call::camera_self_test(seconds) {
            Ok(report) => (format!("camera self-test: ok: {report}"), 0),
            Err(why) => (format!("camera self-test: failed: {why}"), 1),
        };
        eprintln!("{line}");
        if let Some(out) = std::env::var_os("MORPHEUS_CAMERA_SELF_TEST_OUT") {
            let _ = std::fs::write(out, format!("{line}\n"));
        }
        std::process::exit(code);
    }
    // The video encoders without a call (#102; windows.yml, or by hand on a
    // laptop): which hardware encoder was found and, on Windows, pictures
    // through Media Foundation's. To a file (no console on Windows).
    if let Some(out) = std::env::var_os("MORPHEUS_VIDEO_ENCODER_SELF_TEST") {
        init_logging();
        let text = morpheus_call::video_encoder_self_test();
        eprint!("{text}");
        let _ = std::fs::write(out, &text);
        std::process::exit(if text.contains(": error:") { 1 } else { 0 });
    }
    // The report's parts that need no window, to a file (windows.yml: a
    // release build on Windows has no console).
    if let Some(out) = std::env::var_os("MORPHEUS_DIAGNOSTICS_SELF_TEST") {
        let text = diagnostics::self_test_text();
        eprint!("{text}");
        let _ = std::fs::write(out, text);
        std::process::exit(0);
    }
    // A later launch hands over to the one running (its window shows, or the
    // Matrix link it was given opens) and ends.
    let link = view_model::instance::link_in_args(std::env::args().skip(1));
    if std::env::var_os("MORPHEUS_UPDATED_FROM").is_some() {
        // Started by an update: the old version is quitting; wait for it to let
        // go of this data rather than hand over to it.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !instance::claim(&data_dir(), link.as_deref()) {
            if std::time::Instant::now() > deadline {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        morpheus_core::clean_up_after_update();
    } else if !instance::claim(&data_dir(), link.as_deref()) {
        eprintln!("morpheus-desktop: already running on this data; showing its window instead");
        return;
    }
    init_logging();
    streamer::load(&data_dir());
    // Start on login must follow the app when an update moved it.
    view_model::settings::refresh_autostart();
    vim::load(&data_dir());
    // The GPU is chosen when the window system starts (MORPHEUS_GPU overrides it).
    graphics::load(&data_dir());
    let start_hidden = std::env::args().any(|arg| arg == view_model::settings::HIDDEN_FLAG);
    let core = match Core::new(data_dir()) {
        Ok(core) => core,
        Err(err) => {
            eprintln!("morpheus-desktop: starting the core failed: {err}");
            std::process::exit(1);
        }
    };
    let (tx, rx) = futures::channel::mpsc::unbounded::<Command>();
    let _ = COMMANDS.set(tx);
    let commands: Commands = Arc::new(Mutex::new(rx));
    instance::listen(|request| {
        send_command(match request {
            view_model::instance::Request::Show => Command::Show,
            view_model::instance::Request::Open(link) => Command::Open(link),
        });
    });
    tray::install(Arc::new(|action| send_command(Command::Tray(action))));
    #[cfg(unix)]
    quit_on_signals();
    #[cfg(feature = "e2e")]
    control_port();
    if let Some(link) = link {
        send_command(Command::Open(link));
    }
    // Started hidden in the tray (autostart's "start hidden"): no window until asked.
    let hidden = start_hidden && tray::hides_on_close();
    eprintln!("morpheus-desktop: window system starting");
    run_window(core, commands, None, hidden);
    eprintln!("morpheus-desktop: window system ended");
}

/// What an update from the Dioxus app must find: its data directory, and the
/// files it left there read as they were written (the formats are shared with
/// `crates/ui`; the fixtures below are what that app writes).
#[cfg(test)]
#[allow(clippy::expect_used)]
mod dioxus_data {
    use super::*;
    use view_model::{calls, emoji_picker, last_place, settings};

    #[test]
    fn the_data_dir_is_the_one_the_dioxus_app_used() {
        let dir = default_data_dir();
        let dioxus = directories::ProjectDirs::from("dev", "morpheus", "Morpheus")
            .expect("a home directory")
            .data_dir()
            .to_owned();
        assert_eq!(dir, dioxus);
        eprintln!("data dir: {}", dir.display());
        #[cfg(target_os = "linux")]
        assert!(dir.ends_with("morpheus"));
    }

    fn fixture_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("morpheus-dioxus-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    #[test]
    fn files_written_by_the_dioxus_app_read_the_same() {
        let dir = fixture_dir("fixtures");
        let w = |name: &str, text: &str| std::fs::write(dir.join(name), text).expect("write");
        w(
            "last_place.json",
            r#"{"@a:x":{"room":"!r:x","chat":true},"@b:x":{"room":"!q:x","chat":false}}"#,
        );
        w("emoji.json", r#"{"recent":["🚀","👍"],"tone":3}"#);
        w(
            "audio_devices.json",
            r#"{"microphone":"Mic","speaker":null,"camera":"cam0"}"#,
        );
        w("notification_content", "sender");
        w("notifications_off", "");
        assert_eq!(
            last_place::read(&dir, "@a:x"),
            Some(last_place::LastPlace {
                room: "!r:x".into(),
                chat: true
            })
        );
        let emoji = emoji_picker::Remembered::load(&dir);
        assert_eq!(emoji.recent, ["🚀", "👍"]);
        assert_eq!(emoji.tone, Some(3));
        let devices = calls::read_device_prefs(&dir);
        assert_eq!(devices.microphone.as_deref(), Some("Mic"));
        assert_eq!(devices.speaker, None);
        assert_eq!(devices.camera.as_deref(), Some("cam0"));
        assert_eq!(
            settings::notification_content(&dir),
            settings::NotificationContent::Sender
        );
        assert!(!settings::notifications_enabled(&dir));
        // No last_rooms.json or screen-quality.json before: empty, not an error.
        assert!(last_place::read_rooms(&dir, "@a:x").is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_file_that_does_not_parse_is_ignored_not_fatal() {
        let dir = fixture_dir("garbage");
        for name in [
            "last_place.json",
            "last_rooms.json",
            "emoji.json",
            "audio_devices.json",
            "screen-quality.json",
        ] {
            std::fs::write(dir.join(name), "\u{0}{ not json [").expect("write");
        }
        std::fs::write(dir.join("notification_content"), "nonsense").expect("write");
        assert_eq!(last_place::read(&dir, "@a:x"), None);
        assert!(last_place::read_rooms(&dir, "@a:x").is_empty());
        assert!(emoji_picker::Remembered::load(&dir).recent.is_empty());
        assert_eq!(
            calls::read_device_prefs(&dir),
            calls::DevicePrefs::default()
        );
        assert_eq!(
            morpheus_call::ScreenQuality::from_json("\u{0}{ not json ["),
            morpheus_call::ScreenQuality::default()
        );
        assert_eq!(
            settings::notification_content(&dir),
            settings::NotificationContent::Full
        );
        last_place::write(
            &dir,
            "@a:x",
            &last_place::LastPlace {
                room: "!r:x".into(),
                chat: false,
            },
        )
        .expect("write");
        assert!(last_place::read(&dir, "@a:x").is_some());
        std::fs::remove_dir_all(&dir).ok();
    }
}
