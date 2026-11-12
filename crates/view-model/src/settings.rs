//! What the settings window knows besides encryption
//! (`crates/ui/src/components/settings.rs`, `device_prefs.rs`,
//! `autostart.rs`): the pages and their order, the settings search and the
//! anchors it opens, the themes, what a notification may show, the choices
//! for notification defaults, and the autostart entry. No toolkit in here.

use std::path::{Path, PathBuf};

use morpheus_core::NotifyMode;

use super::keys::SettingsFocus;

/// The settings pages, in the side list's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
    Account,
    Security,
    Notifications,
    Appearance,
    VoiceVideo,
    Startup,
    Help,
}

impl SettingsPage {
    pub const ALL: [SettingsPage; 7] = [
        SettingsPage::Account,
        SettingsPage::Security,
        SettingsPage::Notifications,
        SettingsPage::Appearance,
        SettingsPage::VoiceVideo,
        SettingsPage::Startup,
        SettingsPage::Help,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsPage::Account => "My Account",
            SettingsPage::Security => "Security & Privacy",
            SettingsPage::Notifications => "Notifications",
            SettingsPage::Appearance => "Appearance",
            SettingsPage::VoiceVideo => "Voice & Video",
            SettingsPage::Startup => "Startup",
            SettingsPage::Help => "Help & Feedback",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            SettingsPage::Account => "user",
            SettingsPage::Security => "shield",
            SettingsPage::Notifications => "bell",
            SettingsPage::Appearance => "palette",
            SettingsPage::VoiceVideo => "mic",
            SettingsPage::Startup => "square",
            SettingsPage::Help => "circle-help",
        }
    }

    /// The page a notice's target wants, if any.
    pub fn for_focus(focus: SettingsFocus) -> Option<SettingsPage> {
        match focus {
            SettingsFocus::None => None,
            SettingsFocus::VoiceVideo => Some(SettingsPage::VoiceVideo),
            _ => Some(SettingsPage::Security),
        }
    }

    /// The pages the side list shows: Startup is for a desktop window only.
    pub fn listed(has_window: bool) -> Vec<SettingsPage> {
        Self::ALL
            .into_iter()
            .filter(|page| has_window || *page != SettingsPage::Startup)
            .collect()
    }
}

/// One setting the search finds: its name, other words it answers to, its
/// page, and the `data-setting` anchor on that page.
#[derive(Debug, PartialEq, Eq)]
pub struct SettingEntry {
    pub label: &'static str,
    pub keywords: &'static str,
    pub page: SettingsPage,
    pub anchor: &'static str,
}

const fn entry(
    label: &'static str,
    keywords: &'static str,
    page: SettingsPage,
    anchor: &'static str,
) -> SettingEntry {
    SettingEntry {
        label,
        keywords,
        page,
        anchor,
    }
}

/// Every setting the search can open.
pub const SETTINGS_INDEX: &[SettingEntry] = &[
    entry(
        "Account",
        "profile user name sign out device id",
        SettingsPage::Account,
        "account",
    ),
    entry(
        "Encryption and recovery key",
        "security backup recovery key verify verification restore e2e",
        SettingsPage::Security,
        "encryption",
    ),
    entry(
        "Devices and sessions",
        "security sessions sign out other devices",
        SettingsPage::Security,
        "sessions",
    ),
    entry(
        "Notification defaults for rooms",
        "notifications dms groups all messages mentions",
        SettingsPage::Notifications,
        "room-defaults",
    ),
    entry(
        "Notifications on this device",
        "notifications push turn off on",
        SettingsPage::Notifications,
        "device-notifications",
    ),
    entry(
        "What notifications show",
        "notifications privacy hide message name someone content",
        SettingsPage::Notifications,
        "notification-content",
    ),
    entry(
        "Theme",
        "appearance dark light midnight colours colors",
        SettingsPage::Appearance,
        "theme",
    ),
    entry(
        "Microphone and speaker",
        "voice video audio input output devices calls",
        SettingsPage::VoiceVideo,
        "audio-devices",
    ),
    entry(
        "Open at sign-in",
        "startup autostart login tray hidden",
        SettingsPage::Startup,
        "startup",
    ),
    entry(
        "Report a bug",
        "help feedback issue github",
        SettingsPage::Help,
        "report-bug",
    ),
    entry(
        "Version",
        "help about update",
        SettingsPage::Help,
        "version",
    ),
];

/// The settings matching every word of `query`, in their name or the other
/// words they answer to; the Startup page only with a window.
pub fn find_settings(query: &str, has_window: bool) -> Vec<&'static SettingEntry> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    SETTINGS_INDEX
        .iter()
        .filter(|setting| has_window || setting.page != SettingsPage::Startup)
        .filter(|setting| {
            let words = format!("{} {}", setting.label, setting.keywords).to_lowercase();
            needle.split_whitespace().all(|word| words.contains(word))
        })
        .collect()
}

// ---- themes -----------------------------------------------------------------------

/// The colour themes (kept in the account's UI prefs under `theme`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    #[default]
    Dark,
    /// Black surfaces: easier on OLED phones at night.
    Midnight,
    Light,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::Dark, Theme::Midnight, Theme::Light];

    pub fn key(self) -> &'static str {
        match self {
            Theme::Dark => "dark",
            Theme::Midnight => "midnight",
            Theme::Light => "light",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Theme::Dark => "Dark",
            Theme::Midnight => "Midnight",
            Theme::Light => "Light",
        }
    }

    /// The theme a saved key names (dark when unknown).
    pub fn from_key(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|theme| theme.key() == key)
            .unwrap_or_default()
    }
}

// ---- notifications ----------------------------------------------------------------

/// What a notification on this device may give away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NotificationContent {
    /// Who sent what, and where.
    #[default]
    Full,
    /// Only who sent it.
    Sender,
    /// Nothing: "Someone sent a message".
    Hidden,
}

impl NotificationContent {
    pub const ALL: [Self; 3] = [Self::Full, Self::Sender, Self::Hidden];

    /// As kept in the file (and as the radio's value).
    pub fn key(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Sender => "sender",
            Self::Hidden => "hidden",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "Name and message",
            Self::Sender => "Name only",
            Self::Hidden => "Nothing (\u{201c}Someone sent a message\u{201d})",
        }
    }
}

/// The default notification choices: mode, radio value, words.
pub const NOTIFY_DEFAULT_CHOICES: [(NotifyMode, &str, &str); 2] = [
    (NotifyMode::AllMessages, "all", "All messages"),
    (NotifyMode::MentionsOnly, "mentions", "Only mentions"),
];

/// The two defaults: `dm` or `group`, and the legend over each.
pub fn notify_default_kind(dm: bool) -> (&'static str, &'static str) {
    if dm {
        ("dm", "Direct messages")
    } else {
        ("group", "Group rooms")
    }
}

pub const ROOM_DEFAULTS_BLURB: &str =
    "Rooms without their own setting follow these. Set a room's own from its menu.";

/// What the "This device" section says under the checkbox, on a desktop.
pub const DESKTOP_NOTIFICATION_NOTE: &str = "Morpheus shows a system notification when a message arrives while its window is in the background or hidden in the tray.";

/// Present when notifications are off on this device.
const NOTIFICATIONS_OFF: &str = "notifications_off";
/// Holds `sender` or `hidden`; absent means everything.
const NOTIFICATION_CONTENT: &str = "notification_content";

/// Whether this device shows notifications (on unless turned off).
pub fn notifications_enabled(data_dir: &Path) -> bool {
    !data_dir.join(NOTIFICATIONS_OFF).exists()
}

pub fn set_notifications_enabled(data_dir: &Path, enabled: bool) -> std::io::Result<()> {
    let marker = data_dir.join(NOTIFICATIONS_OFF);
    if enabled {
        remove_if_there(&marker)
    } else {
        std::fs::write(marker, "")
    }
}

pub fn notification_content(data_dir: &Path) -> NotificationContent {
    let kept = std::fs::read_to_string(data_dir.join(NOTIFICATION_CONTENT)).unwrap_or_default();
    NotificationContent::ALL
        .into_iter()
        .find(|content| content.key() == kept.trim())
        .unwrap_or_default()
}

pub fn set_notification_content(
    data_dir: &Path,
    content: NotificationContent,
) -> std::io::Result<()> {
    let path = data_dir.join(NOTIFICATION_CONTENT);
    if content == NotificationContent::Full {
        remove_if_there(&path)
    } else {
        std::fs::write(path, content.key())
    }
}

fn remove_if_there(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

// ---- startup ----------------------------------------------------------------------

/// What the autostart entry says, if this platform has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Autostart {
    pub enabled: bool,
    pub hidden: bool,
}

impl Autostart {
    /// The "Start hidden" box only works with autostart on.
    pub fn hidden_selectable(self) -> bool {
        self.enabled
    }
}

/// The flag the desktop app reads to start hidden in the tray.
pub const HIDDEN_FLAG: &str = "--hidden";

pub const STARTUP_UNAVAILABLE: &str = "Starting with your computer isn't available on this system yet.";

fn autostart_path() -> Option<PathBuf> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("autostart").join("morpheus.desktop"))
}

/// The entry's text for `wanted` (the app at `exe`).
pub fn autostart_entry(exe: &Path, wanted: Autostart) -> String {
    let mut exec = format!("\"{}\"", exe.display().to_string().replace('"', "\\\""));
    if wanted.hidden {
        exec.push(' ');
        exec.push_str(HIDDEN_FLAG);
    }
    format!(
        "[Desktop Entry]\nType=Application\nName=Morpheus\nComment=Matrix chat\nExec={exec}\nIcon=morpheus\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    )
}

/// What the entry says; `None` where autostart is not supported yet.
pub fn read_autostart() -> Option<Autostart> {
    let path = autostart_path()?;
    Some(match std::fs::read_to_string(path) {
        Ok(text) => Autostart {
            enabled: true,
            hidden: text.contains(HIDDEN_FLAG),
        },
        Err(_) => Autostart::default(),
    })
}

/// Writes (or removes) the entry to match `wanted`.
pub fn write_autostart(wanted: Autostart) -> std::io::Result<()> {
    let Some(path) = autostart_path() else {
        return Err(std::io::Error::other("autostart is not supported here yet"));
    };
    if !wanted.enabled {
        return remove_if_there(&path);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, autostart_entry(&std::env::current_exe()?, wanted))
}

// ---- help -------------------------------------------------------------------------

pub const REPORT_BUG_BLURB: &str = "Opens a new issue on GitHub with your version, device and the end of this run's log filled in. Issues are public: read the log over and remove anything you'd rather not share before posting.";

/// The app's version, as the Version section shows it.
pub fn version_line() -> String {
    format!("Morpheus {}", env!("CARGO_PKG_VERSION"))
}

pub const VOICE_NO_DEVICES: &str = "No microphones or speakers found.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pages_are_listed_in_order_with_startup_only_for_a_window() {
        let labels: Vec<_> = SettingsPage::ALL.iter().map(|p| p.label()).collect();
        assert_eq!(
            labels,
            [
                "My Account",
                "Security & Privacy",
                "Notifications",
                "Appearance",
                "Voice & Video",
                "Startup",
                "Help & Feedback"
            ]
        );
        assert_eq!(SettingsPage::listed(true).len(), 7);
        assert!(!SettingsPage::listed(false).contains(&SettingsPage::Startup));
    }

    #[test]
    fn notices_open_the_security_page() {
        assert_eq!(SettingsPage::for_focus(SettingsFocus::None), None);
        assert_eq!(
            SettingsPage::for_focus(SettingsFocus::RecoveryKey),
            Some(SettingsPage::Security)
        );
    }

    #[test]
    fn the_search_wants_every_word_in_the_name_or_keywords() {
        let found = find_settings("microphone", true);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].page, SettingsPage::VoiceVideo);
        assert_eq!(found[0].anchor, "audio-devices");
        assert!(find_settings("  ", true).is_empty());
        assert!(find_settings("nonsense", true).is_empty());
        let both = find_settings("notifications push", true);
        assert_eq!(both[0].anchor, "device-notifications");
        assert!(find_settings("autostart", false).is_empty());
        assert_eq!(find_settings("AUTOSTART", true)[0].page, SettingsPage::Startup);
    }

    #[test]
    fn every_entry_is_on_a_listed_page() {
        for setting in SETTINGS_INDEX {
            assert!(SettingsPage::ALL.contains(&setting.page));
        }
    }

    #[test]
    fn themes_round_trip_through_their_keys() {
        for theme in Theme::ALL {
            assert_eq!(Theme::from_key(theme.key()), theme);
        }
        assert_eq!(Theme::from_key("nonsense"), Theme::Dark);
        assert_eq!(Theme::Midnight.label(), "Midnight");
    }

    #[test]
    fn notification_prefs_are_files_in_the_data_directory() {
        let dir = std::env::temp_dir().join(format!("morpheus-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(notifications_enabled(&dir));
        set_notifications_enabled(&dir, false).unwrap();
        assert!(!notifications_enabled(&dir));
        set_notifications_enabled(&dir, true).unwrap();
        assert!(notifications_enabled(&dir));
        // Turning on twice is fine.
        set_notifications_enabled(&dir, true).unwrap();

        assert_eq!(notification_content(&dir), NotificationContent::Full);
        set_notification_content(&dir, NotificationContent::Hidden).unwrap();
        assert_eq!(notification_content(&dir), NotificationContent::Hidden);
        set_notification_content(&dir, NotificationContent::Sender).unwrap();
        assert_eq!(notification_content(&dir), NotificationContent::Sender);
        set_notification_content(&dir, NotificationContent::Full).unwrap();
        assert_eq!(notification_content(&dir), NotificationContent::Full);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_autostart_entry_runs_this_app_hidden_if_asked() {
        let exe = Path::new("/opt/morpheus gpui");
        let plain = autostart_entry(exe, Autostart { enabled: true, hidden: false });
        assert!(plain.contains("Exec=\"/opt/morpheus gpui\"\n"));
        let hidden = autostart_entry(exe, Autostart { enabled: true, hidden: true });
        assert!(hidden.contains("Exec=\"/opt/morpheus gpui\" --hidden\n"));
        assert!(Autostart { enabled: true, hidden: false }.hidden_selectable());
        assert!(!Autostart::default().hidden_selectable());
    }

    #[test]
    fn default_choices_and_wording() {
        assert_eq!(notify_default_kind(true), ("dm", "Direct messages"));
        assert_eq!(notify_default_kind(false), ("group", "Group rooms"));
        assert_eq!(NOTIFY_DEFAULT_CHOICES[1].1, "mentions");
        assert_eq!(NotificationContent::Sender.label(), "Name only");
        assert!(version_line().starts_with("Morpheus "));
    }
}
