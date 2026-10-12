//! The system tray icon: left click opens Morpheus, right click offers
//! Discord's menu (open, check for updates, acknowledgements, quit).
//!
//! On Linux this speaks StatusNotifierItem directly (`ksni`): the
//! appindicator route Dioxus's built-in tray takes there can only show a
//! menu and never reports a plain click. Other platforms use the built-in
//! tray, which does (see `main.rs`).

use std::sync::Arc;

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

/// The white logo as 64x64 RGBA (`assets/tray.png`, rendered with
/// `rsvg-convert` from `crates/ui/assets/logo.svg` with a white fill).
pub fn logo_rgba() -> Result<(Vec<u8>, u32, u32), String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(
        include_bytes!("../assets/tray.png").as_slice(),
    ));
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => buf[..info.buffer_size()]
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        other => return Err(format!("unsupported tray png colour type {other:?}")),
    };
    Ok((rgba, info.width, info.height))
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

    /// Register with the desktop's tray host. Without one (no
    /// StatusNotifierWatcher on the session bus) the app runs without a
    /// tray and says so in the log.
    pub fn install(handler: Handler) {
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
                tracing::warn!(%err, "tray icon image failed, using none");
                Vec::new()
            }
        };
        match (MorpheusTray { handler, icon }).spawn() {
            // The service stops when its last handle goes; keep it for good.
            Ok(handle) => std::mem::forget(handle),
            Err(err) => tracing::warn!(%err, "no tray icon"),
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux::install;
