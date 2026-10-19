//! Opening Morpheus when you sign in to the computer: an XDG autostart entry
//! on Linux (`$XDG_CONFIG_HOME/autostart/morpheus.desktop`), optionally
//! starting hidden in the tray (`--hidden`). Other platforms: not yet.

use std::path::PathBuf;

/// What the autostart entry says, if this platform has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Autostart {
    pub enabled: bool,
    pub hidden: bool,
}

/// The flag the desktop app reads to start hidden in the tray.
pub const HIDDEN_FLAG: &str = "--hidden";

#[cfg(target_os = "linux")]
fn entry_path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("autostart").join("morpheus.desktop"))
}

#[cfg(not(target_os = "linux"))]
fn entry_path() -> Option<PathBuf> {
    None
}

/// `None` where autostart is not supported yet.
pub fn read() -> Option<Autostart> {
    let path = entry_path()?;
    Some(match std::fs::read_to_string(path) {
        Ok(text) => Autostart {
            enabled: true,
            hidden: text.contains(HIDDEN_FLAG),
        },
        Err(_) => Autostart::default(),
    })
}

/// Write (or remove) the entry to match `wanted`.
pub fn write(wanted: Autostart) -> std::io::Result<()> {
    let Some(path) = entry_path() else {
        return Err(std::io::Error::other("autostart is not supported here yet"));
    };
    if !wanted.enabled {
        return match std::fs::remove_file(&path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
            _ => Ok(()),
        };
    }
    let exe = std::env::current_exe()?;
    let mut exec = format!("\"{}\"", exe.display().to_string().replace('"', "\\\""));
    if wanted.hidden {
        exec.push(' ');
        exec.push_str(HIDDEN_FLAG);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(
        &path,
        format!(
            "[Desktop Entry]\nType=Application\nName=Morpheus\nComment=Matrix chat\nExec={exec}\nIcon=morpheus\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
        ),
    )
}
