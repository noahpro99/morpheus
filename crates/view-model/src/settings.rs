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
    StreamerMode,
    Startup,
    Help,
}

impl SettingsPage {
    pub const ALL: [SettingsPage; 8] = [
        SettingsPage::Account,
        SettingsPage::Security,
        SettingsPage::Notifications,
        SettingsPage::Appearance,
        SettingsPage::VoiceVideo,
        SettingsPage::StreamerMode,
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
            SettingsPage::StreamerMode => "Streamer Mode",
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
            SettingsPage::StreamerMode => "eye-off",
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
        "Message sounds",
        "notifications sound chime message ping",
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
        "Vim mode",
        "keyboard vim modal editing normal insert keys hotkeys shortcuts",
        SettingsPage::Appearance,
        "keyboard",
    ),
    entry(
        "Use the dedicated graphics card",
        "graphics gpu card nvidia intel amd discrete integrated dedicated vulkan",
        SettingsPage::VoiceVideo,
        "graphics",
    ),
    entry(
        "Hardware video acceleration",
        "video gpu graphics card encoder decoder h264 vp8 nvenc vaapi videotoolbox media foundation quick sync camera screen share",
        SettingsPage::VoiceVideo,
        "graphics",
    ),
    entry(
        "Microphone and speaker",
        "voice video audio input output devices calls",
        SettingsPage::VoiceVideo,
        "audio-devices",
    ),
    entry(
        LEAVE_OUT_DISCORD_LABEL,
        "screen share sharing sound audio discord vesktop mute echo go live stream computer",
        SettingsPage::VoiceVideo,
        "screen-share-sound",
    ),
    entry(
        "Streamer mode",
        "streamer hide names privacy sharing screen share notifications ids obscure",
        SettingsPage::StreamerMode,
        "streamer",
    ),
    entry(
        "Don't ask again for channels you've already opened this stream",
        "streamer ask prompt open channel server again remembered stream sharing",
        SettingsPage::StreamerMode,
        "streamer",
    ),
    entry(
        "Keep running in the background",
        "startup background tray close window quit exit notifications calls hidden",
        SettingsPage::Startup,
        "background",
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
    entry(
        "Diagnostics",
        "help diagnostics gpu graphics fps memory call debug stats copy f3",
        SettingsPage::Help,
        "diagnostics",
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
/// Present when new messages make no sound on this device.
const MESSAGE_SOUNDS_OFF: &str = "message_sounds_off";
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

/// Whether a new message that notifies you makes a sound (on unless turned off).
pub fn message_sounds_enabled(data_dir: &Path) -> bool {
    !data_dir.join(MESSAGE_SOUNDS_OFF).exists()
}

pub fn set_message_sounds_enabled(data_dir: &Path, enabled: bool) -> std::io::Result<()> {
    let marker = data_dir.join(MESSAGE_SOUNDS_OFF);
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

// ---- screen share sound -----------------------------------------------------------

/// Settings → Voice & Video → Screen Share: Discord left out of the sound a
/// shared screen carries (on unless turned off). Morpheus's own sound is
/// always left out, with no setting.
pub const LEAVE_OUT_DISCORD_LABEL: &str = "Leave Discord out of shared sound";
pub const LEAVE_OUT_DISCORD_HINT: &str = "When you share your screen with sound, the others don't hear Discord (or Vesktop, WebCord, ...) playing. Morpheus's own sound, the call's voices included, is never shared.";

/// Marks this device as sharing Discord's sound (absent: left out).
const DISCORD_SHARED: &str = "share-discord-sound";

/// Whether this device leaves Discord out of a shared screen's sound.
pub fn leaves_out_discord(data_dir: &Path) -> bool {
    !data_dir.join(DISCORD_SHARED).exists()
}

pub fn set_leaves_out_discord(data_dir: &Path, leave_out: bool) -> std::io::Result<()> {
    let marker = data_dir.join(DISCORD_SHARED);
    if leave_out {
        remove_if_there(&marker)
    } else {
        std::fs::write(marker, "")
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
    /// The "Start hidden" box only works with autostart on, and with the app
    /// keeping running in the background (a hidden start has no window to close).
    pub fn hidden_selectable_with(self, keep_running: bool) -> bool {
        self.enabled && keep_running
    }
}

/// The flag the desktop app reads to start hidden in the tray.
pub const HIDDEN_FLAG: &str = "--hidden";

/// Whether closing the window keeps the app running in the tray (a device
/// setting, kept in the data directory; on unless turned off).
const BACKGROUND_FILE: &str = "keep-running.json";

pub const KEEP_RUNNING_HINT: &str =
    "Closing the window keeps Morpheus in the tray for notifications and calls.";
pub const START_HIDDEN_NEEDS_BACKGROUND: &str =
    "Start hidden needs Keep running in the background.";

pub fn load_keep_running(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join(BACKGROUND_FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|v| v.get("enabled").and_then(serde_json::Value::as_bool))
        .unwrap_or(true)
}

pub fn save_keep_running(dir: &Path, on: bool) -> std::io::Result<()> {
    std::fs::write(
        dir.join(BACKGROUND_FILE),
        serde_json::json!({ "enabled": on }).to_string(),
    )
}

pub const STARTUP_UNAVAILABLE: &str = "Starting with your computer isn't available on this system yet.";

/// Whether this system has an autostart mechanism wired up: an XDG autostart
/// entry (Linux), the Run registry key (Windows), a LaunchAgent (macOS).
const AUTOSTART_SUPPORTED: bool = cfg!(any(
    target_os = "linux",
    target_os = "windows",
    target_os = "macos"
));

/// The command line that starts the app at `exe` at login: the quoted path
/// (it may hold spaces) and the hidden flag if asked. This is the Windows Run
/// value and the XDG entry's `Exec` line.
pub fn run_command(exe: &Path, hidden: bool) -> String {
    let mut command = format!("\"{}\"", exe.display().to_string().replace('"', "\\\""));
    if hidden {
        command.push(' ');
        command.push_str(HIDDEN_FLAG);
    }
    command
}

/// The program a command line from [`run_command`] starts: the quoted path,
/// or the first word of an unquoted one.
pub fn command_exe(command: &str) -> Option<PathBuf> {
    let command = command.trim();
    let exe = match command.strip_prefix('"') {
        Some(rest) => rest.split('"').next()?.replace("\\\"", "\""),
        None => command.split_whitespace().next()?.to_owned(),
    };
    (!exe.is_empty()).then(|| PathBuf::from(exe))
}

/// Whether a command line asks for the app to start hidden.
pub fn command_hidden(command: &str) -> bool {
    command.split_whitespace().any(|word| word == HIDDEN_FLAG)
}

/// The command line to store instead of `command` when the app now lives at
/// `exe` (an in-place update moved it), keeping the hidden choice; `None`
/// when it already points there.
pub fn moved_command(command: &str, exe: &Path) -> Option<String> {
    let old = command_exe(command)?;
    (old != exe).then(|| run_command(exe, command_hidden(command)))
}

/// A macOS LaunchAgent that runs the app at `exe` when the user logs in.
pub fn launch_agent_plist(exe: &Path, hidden: bool) -> String {
    let escape = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let mut args = format!("    <string>{}</string>\n", escape(&exe.display().to_string()));
    if hidden {
        args.push_str(&format!("    <string>{HIDDEN_FLAG}</string>\n"));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key>\n  <string>{LAUNCH_AGENT_LABEL}</string>\n  <key>ProgramArguments</key>\n  <array>\n{args}  </array>\n  <key>RunAtLoad</key>\n  <true/>\n</dict>\n</plist>\n"
    )
}

/// The LaunchAgent's label, and its file's name without `.plist`.
pub const LAUNCH_AGENT_LABEL: &str = "dev.morpheus.Morpheus";

#[cfg(target_os = "linux")]
fn autostart_path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("autostart").join("morpheus.desktop"))
}

#[cfg(target_os = "macos")]
fn autostart_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{LAUNCH_AGENT_LABEL}.plist")),
    )
}

/// The entry's text for `wanted` (the app at `exe`).
pub fn autostart_entry(exe: &Path, wanted: Autostart) -> String {
    let exec = run_command(exe, wanted.hidden);
    format!(
        "[Desktop Entry]\nType=Application\nName=Morpheus\nComment=Matrix chat\nExec={exec}\nIcon=morpheus\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    )
}

/// The `Exec` command of an autostart entry's text.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn entry_exec(text: &str) -> Option<&str> {
    text.lines().find_map(|line| line.strip_prefix("Exec="))
}

#[cfg(target_os = "windows")]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(target_os = "windows")]
const RUN_VALUE: &str = "Morpheus";

/// What the entry says; `None` where autostart is not supported yet.
pub fn read_autostart() -> Option<Autostart> {
    if !AUTOSTART_SUPPORTED {
        return None;
    }
    #[cfg(target_os = "windows")]
    {
        let value = windows_registry::CURRENT_USER
            .open(RUN_KEY)
            .and_then(|key| key.get_string(RUN_VALUE))
            .ok();
        Some(match value {
            Some(command) => Autostart {
                enabled: true,
                hidden: command_hidden(&command),
            },
            None => Autostart::default(),
        })
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let path = autostart_path()?;
        Some(match std::fs::read_to_string(path) {
            Ok(text) => Autostart {
                enabled: true,
                hidden: text.contains(HIDDEN_FLAG),
            },
            Err(_) => Autostart::default(),
        })
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

/// Writes (or removes) the entry to match `wanted`.
pub fn write_autostart(wanted: Autostart) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        let key = windows_registry::CURRENT_USER
            .create(RUN_KEY)
            .map_err(std::io::Error::other)?;
        if wanted.enabled {
            let command = run_command(&morpheus_core::install_layout::front_exe()?, wanted.hidden);
            return key
                .set_string(RUN_VALUE, command)
                .map_err(std::io::Error::other);
        }
        // Removing a value that is not there is fine.
        let _ = key.remove_value(RUN_VALUE);
        Ok(())
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let Some(path) = autostart_path() else {
            return Err(std::io::Error::other("autostart is not supported here yet"));
        };
        if !wanted.enabled {
            return remove_if_there(&path);
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let exe = morpheus_core::install_layout::front_exe()?;
        #[cfg(target_os = "macos")]
        let text = launch_agent_plist(&exe, wanted.hidden);
        #[cfg(target_os = "linux")]
        let text = autostart_entry(&exe, wanted);
        std::fs::write(&path, text)
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let _ = wanted;
        Err(std::io::Error::other("autostart is not supported here yet"))
    }
}

/// The program a LaunchAgent's text runs (its first program argument).
pub fn launch_agent_exe(plist: &str) -> Option<PathBuf> {
    let rest = &plist[plist.find("<key>ProgramArguments</key>")?..];
    let rest = &rest[rest.find("<string>")? + "<string>".len()..];
    let text = &rest[..rest.find("</string>")?];
    Some(PathBuf::from(
        text.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&"),
    ))
}

/// Whether an autostart entry that starts `old` should be pointed at `exe`:
/// the app moved, so `old` is gone. (A dev build or a second copy running
/// beside a live install leaves the entry alone.)
fn entry_is_stale(old: Option<PathBuf>, exe: &Path) -> bool {
    old.is_some_and(|old| old != exe && !old.exists())
}

/// At startup: if start-on-login is on but points at a place the app has left
/// (an in-place update moved it), point it at this executable again.
pub fn refresh_autostart() {
    // Test runs must not touch the user's real entry.
    if !AUTOSTART_SUPPORTED || std::env::var_os("MORPHEUS_E2E_PORT").is_some() {
        return;
    }
    let Ok(exe) = morpheus_core::install_layout::front_exe() else {
        return;
    };
    #[cfg(target_os = "windows")]
    {
        let Ok(key) = windows_registry::CURRENT_USER.open(RUN_KEY) else {
            return;
        };
        if let Ok(command) = key.get_string(RUN_VALUE)
            && entry_is_stale(command_exe(&command), &exe)
            && let Some(new) = moved_command(&command, &exe)
        {
            let _ = key.set_string(RUN_VALUE, new);
        }
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let Some(path) = autostart_path() else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        let hidden = text.contains(HIDDEN_FLAG);
        #[cfg(target_os = "linux")]
        let (old, new) = (
            entry_exec(&text).and_then(command_exe),
            autostart_entry(&exe, Autostart { enabled: true, hidden }),
        );
        #[cfg(target_os = "macos")]
        let (old, new) = (launch_agent_exe(&text), launch_agent_plist(&exe, hidden));
        if entry_is_stale(old, &exe) {
            let _ = std::fs::write(&path, new);
        }
    }
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
                "Streamer Mode",
                "Startup",
                "Help & Feedback"
            ]
        );
        assert_eq!(SettingsPage::listed(true).len(), 8);
        assert!(!SettingsPage::listed(false).contains(&SettingsPage::Startup));
    }

    #[test]
    fn discord_is_left_out_of_shared_sound_unless_turned_off_on_this_device() {
        let dir = std::env::temp_dir().join(format!("morpheus-discord-sound-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        assert!(leaves_out_discord(&dir), "on by default");
        set_leaves_out_discord(&dir, false).expect("save");
        assert!(!leaves_out_discord(&dir), "kept off");
        set_leaves_out_discord(&dir, false).expect("again");
        set_leaves_out_discord(&dir, true).expect("save");
        assert!(leaves_out_discord(&dir), "back on");
        set_leaves_out_discord(&dir, true).expect("on twice is fine");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_search_finds_the_discord_sound_setting() {
        for query in ["discord", "screen share sound", "echo", "go live audio"] {
            let found = find_settings(query, true);
            assert!(
                found
                    .iter()
                    .any(|e| e.label == LEAVE_OUT_DISCORD_LABEL
                        && e.page == SettingsPage::VoiceVideo
                        && e.anchor == "screen-share-sound"),
                "{query:?} finds it"
            );
        }
        assert!(LEAVE_OUT_DISCORD_HINT.contains("Morpheus"));
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
    fn the_run_command_quotes_the_path_and_adds_the_hidden_flag() {
        let exe = Path::new(r"C:\Users\Jo Doe\AppData\Local\Morpheus\morpheus-desktop.exe");
        assert_eq!(
            run_command(exe, false),
            r#""C:\Users\Jo Doe\AppData\Local\Morpheus\morpheus-desktop.exe""#
        );
        let hidden = run_command(exe, true);
        assert!(hidden.ends_with(r#".exe" --hidden"#));
        assert_eq!(command_exe(&hidden), Some(PathBuf::from(exe)));
        assert!(command_hidden(&hidden));
        assert!(!command_hidden(&run_command(exe, false)));
        assert_eq!(command_exe("C:\\x\\a.exe --hidden"), Some(PathBuf::from("C:\\x\\a.exe")));
        assert_eq!(command_exe("  "), None);
    }

    #[test]
    fn a_moved_app_updates_its_run_command_and_keeps_the_hidden_choice() {
        let old = run_command(Path::new("/old/place/morpheus"), true);
        let new_exe = Path::new("/new place/morpheus");
        assert_eq!(
            moved_command(&old, new_exe),
            Some(run_command(new_exe, true))
        );
        let plain = run_command(Path::new("/old/place/morpheus"), false);
        assert_eq!(moved_command(&plain, new_exe), Some(run_command(new_exe, false)));
        // Already pointing here: nothing to change.
        assert_eq!(moved_command(&run_command(new_exe, true), new_exe), None);
        assert_eq!(moved_command("", new_exe), None);
    }

    #[test]
    fn the_autostart_exec_line_is_found_and_the_launch_agent_names_the_app() {
        let exe = Path::new("/Applications/Morpheus.app/Contents/MacOS/morpheus-desktop");
        let entry = autostart_entry(exe, Autostart { enabled: true, hidden: true });
        assert_eq!(entry_exec(&entry), Some(run_command(exe, true).as_str()));
        let plist = launch_agent_plist(exe, true);
        assert!(plist.contains("<key>RunAtLoad</key>\n  <true/>"));
        assert!(plist.contains("<string>--hidden</string>"));
        assert_eq!(launch_agent_exe(&plist).as_deref(), Some(exe));
        assert!(!launch_agent_plist(exe, false).contains("--hidden"));
    }

    #[test]
    fn only_an_entry_for_a_place_the_app_left_is_stale() {
        let here = Path::new("/new/morpheus");
        assert!(entry_is_stale(Some(PathBuf::from("/no/such/dir/morpheus")), here));
        // Same place, or an old copy that still exists (a dev build beside an install): left alone.
        assert!(!entry_is_stale(Some(here.to_owned()), here));
        let live = std::env::current_exe().unwrap();
        assert!(!entry_is_stale(Some(live), here));
        assert!(!entry_is_stale(None, here));
    }

    #[test]
    fn default_choices_and_wording() {
        assert_eq!(notify_default_kind(true), ("dm", "Direct messages"));
        assert_eq!(notify_default_kind(false), ("group", "Group rooms"));
        assert_eq!(NOTIFY_DEFAULT_CHOICES[1].1, "mentions");
        assert_eq!(NotificationContent::Sender.label(), "Name only");
        assert!(version_line().starts_with("Morpheus "));
    }

    #[test]
    fn keeping_running_in_the_background_is_on_unless_turned_off_and_is_kept() {
        let dir = std::env::temp_dir().join(format!("morpheus-bg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let _ = std::fs::remove_file(dir.join(BACKGROUND_FILE));
        assert!(load_keep_running(&dir), "on by default");
        save_keep_running(&dir, false).expect("save");
        assert!(!load_keep_running(&dir));
        save_keep_running(&dir, true).expect("save");
        assert!(load_keep_running(&dir));
        std::fs::write(dir.join(BACKGROUND_FILE), "not json").expect("write");
        assert!(load_keep_running(&dir), "unreadable means the default");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn start_hidden_needs_autostart_and_running_in_the_background() {
        let on = Autostart { enabled: true, hidden: false };
        assert!(on.hidden_selectable_with(true));
        assert!(!on.hidden_selectable_with(false));
        assert!(!Autostart::default().hidden_selectable_with(true));
    }

    #[test]
    fn the_search_finds_running_in_the_background() {
        for word in ["background", "tray", "close"] {
            let found = find_settings(word, true);
            assert!(
                found.iter().any(|e| e.anchor == "background"),
                "{word} finds it"
            );
        }
        assert!(find_settings("background", false).iter().all(|e| e.anchor != "background"));
    }
}
