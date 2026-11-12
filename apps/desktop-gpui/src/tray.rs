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
    std::env::var_os("MORPHEUS_E2E_PORT").is_none() || std::env::var_os("MORPHEUS_E2E_TRAY").is_some()
}

/// The white logo as 64x64 RGBA (`apps/desktop/assets/tray.png`).
fn logo_rgba() -> Result<(Vec<u8>, u32, u32), String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(
        include_bytes!("../../desktop/assets/tray.png").as_slice(),
    ));
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
        let icon = match logo_rgba() {
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
                eprintln!("morpheus-gpui: tray icon image failed, using none: {err}");
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
                eprintln!("morpheus-gpui: no tray icon: {err}");
                false
            }
        }
    }
}

/// Put up the tray icon, if this run wants one. Without a tray host the
/// window's close button ends the app instead of hiding it for good.
pub fn install(handler: Handler) {
    if !wanted() {
        // A test run closes the window to see what keeps running.
        HIDES_ON_CLOSE.store(std::env::var_os("MORPHEUS_E2E_PORT").is_some(), Ordering::Relaxed);
        return;
    }
    #[cfg(target_os = "linux")]
    {
        let up = linux::install(handler);
        HIDES_ON_CLOSE.store(up || std::env::var_os("MORPHEUS_E2E_PORT").is_some(), Ordering::Relaxed);
    }
    #[cfg(not(target_os = "linux"))]
    let _ = handler;
}
