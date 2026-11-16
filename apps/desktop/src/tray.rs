//! The system tray icon (`apps/desktop/src/tray.rs`): left click opens
//! Morpheus, right click offers Discord's menu (open, check for updates,
//! acknowledgements, quit). StatusNotifierItem through `ksni`.
//!
//! Closing the window hides it: the app (sync, notifications, rings) keeps
//! running, and the tray brings it back.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// What the user asked for from the tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Open,
    CheckForUpdates,
    Acknowledgements,
    Quit,
}

impl TrayAction {
    /// Menu order, Discord's.
    pub const MENU: [TrayAction; 4] = [
        TrayAction::Open,
        TrayAction::CheckForUpdates,
        TrayAction::Acknowledgements,
        TrayAction::Quit,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TrayAction::Open => "Open Morpheus",
            TrayAction::CheckForUpdates => "Check for Updates…",
            TrayAction::Acknowledgements => "Acknowledgements",
            TrayAction::Quit => "Quit Morpheus",
        }
    }
}

/// Runs on whatever thread the tray reports on.
pub type Handler = Arc<dyn Fn(TrayAction) + Send + Sync>;

/// Whether closing the window may hide it: there is a tray to bring it back
/// (or this is a test run, which closes windows to see what keeps running).
static HIDES_ON_CLOSE: AtomicBool = AtomicBool::new(false);

pub fn hides_on_close() -> bool {
    HIDES_ON_CLOSE.load(Ordering::Relaxed)
}

/// Whether this run puts up a tray icon: always, except that test runs (which
/// open many windows) ask for it with `MORPHEUS_E2E_TRAY`.
pub fn wanted() -> bool {
    std::env::var_os("MORPHEUS_E2E_PORT").is_none()
        || std::env::var_os("MORPHEUS_E2E_TRAY").is_some()
}

/// The windows taskbar theme (`SystemUsesLightTheme`) to the icon it needs:
/// a light taskbar wants the dark logo; a dark one, or no answer, the white.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn dark_icon_wanted(system_uses_light_theme: Option<u32>) -> bool {
    system_uses_light_theme == Some(1)
}

/// The logo as 64x64 RGBA: white (`apps/desktop/assets/tray.png`) or, for a
/// light taskbar, dark (`tray-dark.png`, the same shape in near black).
fn logo_rgba(dark: bool) -> Result<(Vec<u8>, u32, u32), String> {
    let bytes: &[u8] = if dark {
        include_bytes!("../assets/tray-dark.png")
    } else {
        include_bytes!("../assets/tray.png")
    };
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    match info.color_type {
        png::ColorType::Rgba => Ok((buf[..info.buffer_size()].to_vec(), info.width, info.height)),
        png::ColorType::Rgb => Ok((
            buf[..info.buffer_size()]
                .chunks(3)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            info.width,
            info.height,
        )),
        other => Err(format!("unsupported tray png colour type {other:?}")),
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use ksni::blocking::TrayMethods;

    use super::{Handler, TrayAction, logo_rgba};

    struct MorpheusTray {
        handler: Handler,
        icon: Vec<ksni::Icon>,
    }

    impl ksni::Tray for MorpheusTray {
        fn id(&self) -> String {
            "morpheus".into()
        }

        fn title(&self) -> String {
            "Morpheus".into()
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            self.icon.clone()
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                title: "Morpheus".into(),
                ..Default::default()
            }
        }

        /// Left click.
        fn activate(&mut self, _x: i32, _y: i32) {
            (self.handler)(TrayAction::Open);
        }

        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            let mut items = Vec::new();
            for action in TrayAction::MENU {
                if action == TrayAction::Quit {
                    items.push(ksni::MenuItem::Separator);
                }
                items.push(
                    ksni::menu::StandardItem {
                        label: action.label().into(),
                        activate: Box::new(move |tray: &mut Self| (tray.handler)(action)),
                        ..Default::default()
                    }
                    .into(),
                );
            }
            items
        }
    }

    /// Register with the desktop's tray host; `false` without one (no
    /// StatusNotifierWatcher on the session bus).
    pub fn install(handler: Handler) -> bool {
        let icon = match logo_rgba(false) {
            Ok((rgba, width, height)) => vec![ksni::Icon {
                width: width as i32,
                height: height as i32,
                // StatusNotifierItem wants ARGB in network byte order.
                data: rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|p| [p[3], p[0], p[1], p[2]])
                    .collect(),
            }],
            Err(err) => {
                eprintln!("morpheus-desktop: tray icon image failed, using none: {err}");
                Vec::new()
            }
        };
        match (MorpheusTray { handler, icon }).spawn() {
            // The service stops when its last handle goes; keep it for good.
            Ok(handle) => {
                std::mem::forget(handle);
                true
            }
            Err(err) => {
                eprintln!("morpheus-desktop: no tray icon: {err}");
                false
            }
        }
    }
}

/// Windows (the notification area) and macOS (the menu bar), through the
/// `tray-icon` crate. It needs an event loop on the thread that creates the
/// icon: GPUI's main thread has one (the Win32 message pump, or the
/// NSApplication run loop), so it is created from inside `Application::run`,
/// where the loop is already going (macOS wants that), and its events come
/// back on that thread through handlers, not a polled channel.
#[cfg(any(target_os = "windows", target_os = "macos"))]
mod native {
    use tray_icon::{
        Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
        menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    };

    use std::cell::RefCell;

    use super::{Handler, TrayAction, dark_icon_wanted, logo_rgba};

    thread_local! {
        /// The icon and whether it is the dark one: kept (dropping removes the
        /// icon) on the main thread, where it was made and where it is changed.
        static ICON: RefCell<Option<(TrayIcon, bool)>> = const { RefCell::new(None) };
    }

    /// Whether the taskbar is light (Windows; the menu bar on macOS adapts to
    /// the template on its own).
    pub(super) fn taskbar_is_light() -> bool {
        #[cfg(target_os = "windows")]
        {
            let value = windows_registry::CURRENT_USER
                .open(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
                .and_then(|key| key.get_u32("SystemUsesLightTheme"))
                .ok();
            dark_icon_wanted(value)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = dark_icon_wanted;
            false
        }
    }

    /// Show the icon that fits the taskbar now; called every few seconds on
    /// Windows (the theme can change while running: WM_SETTINGCHANGE reaches
    /// the window GPUI owns, not this crate, so it is polled instead).
    pub fn refresh_theme() {
        let dark = taskbar_is_light();
        ICON.with_borrow_mut(|slot| {
            if let Some((icon, shown_dark)) = slot
                && *shown_dark != dark
                && let Ok((rgba, w, h)) = logo_rgba(dark)
                && let Ok(new) = Icon::from_rgba(rgba, w, h)
                && icon.set_icon(Some(new)).is_ok()
            {
                *shown_dark = dark;
            }
        });
    }

    /// The menu item id of an action: its place in the menu order.
    fn id_of(action: TrayAction) -> String {
        TrayAction::MENU
            .iter()
            .position(|a| *a == action)
            .map_or_else(String::new, |i| i.to_string())
    }

    fn action_of(id: &str) -> Option<TrayAction> {
        TrayAction::MENU.get(id.parse::<usize>().ok()?).copied()
    }

    /// Whether this click on the icon opens the window: the left button,
    /// released (the right one opens the menu on its own).
    pub(super) fn is_open_click(event: &TrayIconEvent) -> bool {
        matches!(
            event,
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
        )
    }

    /// Must run on the main thread with the event loop going. `false` if the
    /// system would not give an icon.
    pub fn install(handler: Handler) -> bool {
        let menu = Menu::new();
        for action in TrayAction::MENU {
            if action == TrayAction::Quit {
                let _ = menu.append(&PredefinedMenuItem::separator());
            }
            let item = MenuItem::with_id(id_of(action), action.label(), true, None);
            if let Err(err) = menu.append(&item) {
                eprintln!("morpheus-desktop: tray menu failed: {err}");
                return false;
            }
        }
        let mut builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            // Left click opens Morpheus (as Discord's does), right click the menu.
            .with_menu_on_left_click(false)
            .with_tooltip("Morpheus");
        let dark = taskbar_is_light();
        match logo_rgba(dark)
            .and_then(|(rgba, w, h)| Icon::from_rgba(rgba, w, h).map_err(|e| e.to_string()))
        {
            // macOS tints a template (the white logo's shape) for a light or dark menu bar.
            Ok(icon) => builder = builder.with_icon(icon).with_icon_as_template(true),
            Err(err) => eprintln!("morpheus-desktop: tray icon image failed, using none: {err}"),
        }
        let open = handler.clone();
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if is_open_click(&event) {
                open(TrayAction::Open);
            }
        }));
        MenuEvent::set_event_handler(Some(move |event: tray_icon::menu::MenuEvent| {
            if let Some(action) = action_of(&event.id().0) {
                handler(action);
            }
        }));
        match builder.build() {
            // Dropping the icon removes it: it stays for the life of the app.
            Ok(icon) => {
                ICON.with_borrow_mut(|slot| *slot = Some((icon, dark)));
                true
            }
            Err(err) => {
                eprintln!("morpheus-desktop: no tray icon: {err}");
                false
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn menu_ids_map_back_to_their_actions() {
            for action in TrayAction::MENU {
                assert_eq!(action_of(&id_of(action)), Some(action));
            }
            assert_eq!(action_of("9"), None);
            assert_eq!(action_of("x"), None);
        }
    }
}

/// Put up the tray icon, if this run wants one. Without a tray host the
/// window's close button ends the app instead of hiding it for good.
pub fn install(handler: Handler) {
    if !wanted() {
        // A test run closes the window to see what keeps running.
        HIDES_ON_CLOSE.store(
            std::env::var_os("MORPHEUS_E2E_PORT").is_some(),
            Ordering::Relaxed,
        );
        return;
    }
    #[cfg(target_os = "linux")]
    {
        let up = linux::install(handler);
        HIDES_ON_CLOSE.store(
            up || std::env::var_os("MORPHEUS_E2E_PORT").is_some(),
            Ordering::Relaxed,
        );
    }
    // Windows and macOS: the icon is made by `install_native` once the event
    // loop runs; until then it is expected to work.
    #[cfg(not(target_os = "linux"))]
    {
        let _ = handler;
        HIDES_ON_CLOSE.store(true, Ordering::Relaxed);
    }
}

/// Windows and macOS: create the icon, from inside `Application::run` (on the
/// main thread, with the event loop going). Without one the close button ends
/// the app again. Elsewhere does nothing.
pub fn install_native(handler: Handler, cx: &mut gpui::App) {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    if wanted() {
        if native::install(handler) {
            // Windows: follow the taskbar between light and dark.
            #[cfg(target_os = "windows")]
            cx.spawn(async move |cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(3))
                        .await;
                    native::refresh_theme();
                }
            })
            .detach();
        } else {
            HIDES_ON_CLOSE.store(
                std::env::var_os("MORPHEUS_E2E_PORT").is_some(),
                Ordering::Relaxed,
            );
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let _ = handler;
    let _ = cx;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_light_taskbar_gets_the_dark_icon() {
        assert!(dark_icon_wanted(Some(1)));
        assert!(!dark_icon_wanted(Some(0)));
        // Older Windows has no such value: the taskbar is dark.
        assert!(!dark_icon_wanted(None));
    }

    #[test]
    fn both_icons_decode_and_the_dark_one_is_the_same_shape() {
        let (white, w, h) = logo_rgba(false).unwrap();
        let (dark, dw, dh) = logo_rgba(true).unwrap();
        assert_eq!((w, h), (dw, dh));
        for (a, b) in white.chunks(4).zip(dark.chunks(4)) {
            assert_eq!(a[3], b[3], "alpha (the shape) is shared");
            if a[3] > 0 {
                assert!(a[0] > 200, "the white logo is light");
                assert!(b[0] < 60, "the dark logo is dark");
            }
        }
    }
}
