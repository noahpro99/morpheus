//! Windows toasts: the app's identity (AppUserModelID), what a toast carries
//! to say what a click opens, and the toast's XML. Pure, so it is tested on
//! every platform; `apps/desktop/src/win_notify.rs` is the Windows shell
//! round it.
//!
//! A click on a toast is a protocol activation: the toast's `launch` is a
//! `morpheus-notify:` link that Windows starts through the key
//! `HKCU\Software\Classes\morpheus-notify` (the launcher, with the link as its
//! argument). That reaches the running app through the single-instance
//! handover (which shows its window, from the tray or minimised, and opens
//! the Matrix link), or starts the app with the link when it is not running.

use std::path::Path;

/// What names the app to the shell: the process, the Start menu shortcut and
/// the registry entry that gives toasts their name and icon.
pub const APP_ID: &str = "dev.morpheus.Morpheus";

/// The name toasts show.
pub const DISPLAY_NAME: &str = "Morpheus";

/// The registry key (under HKCU) that names `APP_ID` when no shortcut does.
pub fn app_id_key() -> String {
    format!(r"Software\Classes\AppUserModelId\{APP_ID}")
}

/// The URL scheme of a toast's click.
pub const PROTOCOL: &str = "morpheus-notify";

/// The registry key (under HKCU) of that scheme.
pub fn protocol_key() -> String {
    format!(r"Software\Classes\{PROTOCOL}")
}

/// The command of that key for the app at `exe`.
pub fn protocol_command(exe: &Path) -> String {
    format!("\"{}\" \"%1\"", exe.display())
}

/// The launch argument of a toast about `room` (and its message `event`, when
/// there is one): the Matrix link of the target behind the toast's scheme.
pub fn launch_arg(room: &str, event: Option<&str>) -> String {
    let room = room.strip_prefix('!').unwrap_or(room);
    match event {
        Some(event) => format!(
            "{PROTOCOL}:matrix:roomid/{room}/e/{}",
            event.strip_prefix('$').unwrap_or(event)
        ),
        None => format!("{PROTOCOL}:matrix:roomid/{room}"),
    }
}

/// The Matrix link a launch argument carries (what the app opens), or `None`
/// when `arg` is not one of ours.
pub fn target_of(arg: &str) -> Option<String> {
    let link = arg.trim().strip_prefix(PROTOCOL)?.strip_prefix(':')?;
    link.starts_with("matrix:").then(|| link.to_owned())
}

/// What a toast shows.
#[derive(Debug, Default, Clone)]
pub struct Toast<'a> {
    pub title: &'a str,
    pub body: &'a str,
    /// The sender's picture (a file), shown round at the left.
    pub icon: Option<&'a Path>,
    /// A photo sent, shown inline.
    pub image: Option<&'a Path>,
    /// What a click opens (a [`launch_arg`]).
    pub launch: Option<&'a str>,
    /// A ringing call: the system's call sound.
    pub ringing: bool,
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // XML 1.0 cannot carry other control characters.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

fn file_uri(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

impl Toast<'_> {
    /// The toast's XML (the ToastGeneric template).
    pub fn xml(&self) -> String {
        let mut xml = String::from("<toast");
        if let Some(launch) = self.launch {
            xml.push_str(&format!(
                " activationType=\"protocol\" launch=\"{}\"",
                escape(launch)
            ));
        }
        if self.ringing {
            xml.push_str(" scenario=\"incomingCall\"");
        }
        xml.push_str("><visual><binding template=\"ToastGeneric\">");
        if let Some(icon) = self.icon {
            xml.push_str(&format!(
                "<image placement=\"appLogoOverride\" hint-crop=\"circle\" src=\"{}\"/>",
                escape(&file_uri(icon))
            ));
        }
        xml.push_str(&format!("<text>{}</text>", escape(self.title)));
        if !self.body.is_empty() {
            xml.push_str(&format!("<text>{}</text>", escape(self.body)));
        }
        if let Some(image) = self.image {
            xml.push_str(&format!("<image src=\"{}\"/>", escape(&file_uri(image))));
        }
        xml.push_str("</binding></visual>");
        if self.ringing {
            xml.push_str("<audio src=\"ms-winsoundevent:Notification.Looping.Call\"/>");
        }
        xml.push_str("</toast>");
        xml
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_has_its_own_id_and_registry_key() {
        assert_eq!(APP_ID, "dev.morpheus.Morpheus");
        assert!(!APP_ID.to_lowercase().contains("powershell"));
        assert_eq!(
            app_id_key(),
            r"Software\Classes\AppUserModelId\dev.morpheus.Morpheus"
        );
    }

    #[test]
    fn a_launch_argument_names_the_room_and_the_message() {
        assert_eq!(
            launch_arg("!abc:example.org", Some("$ev1")),
            "morpheus-notify:matrix:roomid/abc:example.org/e/ev1"
        );
        assert_eq!(
            launch_arg("!abc:example.org", None),
            "morpheus-notify:matrix:roomid/abc:example.org"
        );
    }

    #[test]
    fn a_launch_argument_parses_back_into_the_link_the_app_opens() {
        let arg = launch_arg("!abc:example.org", Some("$ev1"));
        let link = target_of(&arg).expect("ours");
        // The same Matrix link as every other way in.
        assert_eq!(link, "matrix:roomid/abc:example.org/e/ev1");
        assert_eq!(target_of(&launch_arg("!abc:example.org", None)).as_deref(), Some("matrix:roomid/abc:example.org"));
        assert_eq!(target_of("--hidden"), None);
        assert_eq!(target_of("matrix:roomid/abc:example.org"), None);
        assert_eq!(target_of("morpheus-notify:https://evil"), None);
    }

    #[test]
    fn the_protocol_command_passes_the_argument() {
        assert_eq!(
            protocol_command(Path::new(r"C:\Users\a b\Morpheus.exe")),
            "\"C:\\Users\\a b\\Morpheus.exe\" \"%1\""
        );
    }

    #[test]
    fn the_toast_xml_carries_text_launch_and_pictures_escaped() {
        let launch = launch_arg("!abc:example.org", Some("$ev1"));
        let xml = Toast {
            title: "Bob <3 & \"co\"",
            body: "hi",
            icon: Some(Path::new(r"C:\a b\avatar.png")),
            image: None,
            launch: Some(&launch),
            ringing: false,
        }
        .xml();
        assert!(xml.starts_with("<toast activationType=\"protocol\" launch=\"morpheus-notify:matrix:roomid/abc:example.org/e/ev1\">"));
        assert!(xml.contains("<text>Bob &lt;3 &amp; &quot;co&quot;</text>"));
        assert!(xml.contains("src=\"file:///C:/a b/avatar.png\""));
        assert!(!xml.contains("<audio"));
        let ring = Toast { title: "Bob is calling", ringing: true, ..Toast::default() }.xml();
        assert!(ring.contains("scenario=\"incomingCall\""));
        assert!(ring.contains("<audio"));
        assert!(!ring.contains("launch="));
    }

    #[test]
    fn the_launch_target_is_a_link_the_core_understands() {
        let link = target_of(&launch_arg("!abc:example.org", Some("$ev1"))).unwrap();
        assert!(morpheus_core::parse_matrix_link(&link).is_some());
    }
}
