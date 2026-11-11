//! Whether this device can read encrypted history, and the logic of the
//! screens that fix it (`crates/ui/src/components/keys.rs` and the
//! Security page of settings): which message rows are locked, when the key
//! banner shows, the Security page's status lines, and the recovery key form.
//! No UI toolkit in here.

use morpheus_core::{EncryptionStatus, RecoveryAvailability};

/// What the core reports about this device's keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyAccess {
    /// This device cannot read encrypted history and will not until the
    /// user acts; false while that is still unknown.
    pub locked: bool,
    /// The account has no key backup at all: offer Secure Backup.
    pub backup_missing: bool,
}

impl KeyAccess {
    /// Locked once the device is not verified and has no recovery key.
    pub fn from_status(status: &EncryptionStatus) -> Self {
        Self {
            locked: !status.device_verified && status.recovery != RecoveryAvailability::Enabled,
            backup_missing: status.recovery == RecoveryAvailability::Disabled,
        }
    }
}

/// How a message that cannot be decrypted is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Undecryptable {
    /// The device can read history, so keys usually arrive shortly (backup,
    /// another device): a loading shimmer.
    Waiting,
    /// The device cannot, and never will until the user acts: say so.
    Locked,
}

pub fn undecryptable(access: KeyAccess) -> Undecryptable {
    if access.locked {
        Undecryptable::Locked
    } else {
        Undecryptable::Waiting
    }
}

/// The words of a locked row, its hover text.
pub const LOCKED_TEXT: &str = "Encrypted message — this device has no key for it yet";
pub const LOCKED_HINT: &str = "Verify this device or enter your recovery key to read it";
pub const WAITING_HINT: &str = "Waiting for the keys to decrypt this message";

/// The notice above a chat that has messages this device cannot read (the
/// main timeline only, not a thread).
pub fn show_banner(main_timeline: bool, access: KeyAccess, has_undecryptable: bool) -> bool {
    main_timeline && access.locked && has_undecryptable
}

pub const BANNER_TITLE: &str = "Some messages are locked on this device";
pub const BANNER_TEXT: &str = "Encrypted messages from before you signed in here need keys that this device does not have yet.";
pub const BANNER_VERIFY: &str = "Verify with another device";
pub const BANNER_RECOVERY: &str = "Enter recovery key";

/// The Secure Backup offer: shown while there is no backup, until "Later".
pub fn show_backup_prompt(access: KeyAccess, later: bool) -> bool {
    access.backup_missing && !later
}

/// Where Settings puts the cursor when opened from a notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsFocus {
    #[default]
    None,
    RecoveryKey,
    /// Set up Secure Backup (key backup and a recovery key) at once.
    SetUpBackup,
    /// The Security & Privacy page (verifying this device).
    Security,
}

/// The settings pages this port has so far, in the side list's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
    Account,
    Security,
}

impl SettingsPage {
    pub const ALL: [SettingsPage; 2] = [SettingsPage::Account, SettingsPage::Security];

    pub fn label(self) -> &'static str {
        match self {
            SettingsPage::Account => "My Account",
            SettingsPage::Security => "Security & Privacy",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            SettingsPage::Account => "user",
            SettingsPage::Security => "shield",
        }
    }

    /// The page a notice's target wants, if any.
    pub fn for_focus(focus: SettingsFocus) -> Option<SettingsPage> {
        match focus {
            SettingsFocus::None => None,
            _ => Some(SettingsPage::Security),
        }
    }
}

/// One line of the Security page's status list, with its colour class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    pub text: &'static str,
    pub class: &'static str,
}

pub fn status_lines(status: &EncryptionStatus) -> [StatusLine; 2] {
    let (recovery, class) = match status.recovery {
        RecoveryAvailability::Unknown => ("Checking key backup…", "muted"),
        RecoveryAvailability::Enabled => ("Key backup connected", "ok"),
        RecoveryAvailability::NeedsKey => (
            "Key backup found; enter your recovery key to read older messages",
            "warn",
        ),
        RecoveryAvailability::Disabled => ("No key backup on this account", "muted"),
    };
    [
        if status.device_verified {
            StatusLine {
                text: "This device is verified",
                class: "ok",
            }
        } else {
            StatusLine {
                text: "This device is not verified",
                class: "warn",
            }
        },
        StatusLine {
            text: recovery,
            class,
        },
    ]
}

/// What the Security page offers under the status list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryOffer {
    /// A key was just made: show it until Settings closes.
    NewKey,
    /// No backup on the account: set up recovery.
    SetUp,
    /// This device has the keys: a new key can replace the old one.
    Change,
    None,
}

pub fn recovery_offer(status: Option<&EncryptionStatus>, has_new_key: bool) -> RecoveryOffer {
    if has_new_key {
        return RecoveryOffer::NewKey;
    }
    match status.map(|s| s.recovery) {
        Some(RecoveryAvailability::Disabled) => RecoveryOffer::SetUp,
        Some(RecoveryAvailability::Enabled) => RecoveryOffer::Change,
        _ => RecoveryOffer::None,
    }
}

pub const RECOVERY_ACCEPTED: &str =
    "Recovery key accepted. Encrypted history will decrypt as keys arrive.";
pub const RECOVERY_SAVE_NOTE: &str = "Save it somewhere safe, like a password manager. With it, any new device can read your encrypted history; without it, history is lost if you lose all your signed-in devices.";
pub const RECOVERY_CHANGE_NOTE: &str =
    "Lost your recovery key? Make a new one here; the old one stops working.";
pub const RECOVERY_PLACEHOLDER: &str = "EsTx xxxx xxxx …";

/// A recovery key as the user pasted or typed it: spaces around and inside
/// groups are kept by the core (it ignores them), only the ends are trimmed.
pub fn clean_recovery_key(input: &str) -> &str {
    input.trim()
}

/// Whether the form has anything to send.
pub fn recovery_key_ready(input: &str) -> bool {
    !clean_recovery_key(input).is_empty()
}

/// A recovery key in groups of four, as it is read out and written down
/// (the core already returns it so; this is for one typed without spaces).
#[allow(dead_code)] // for the phone front ends to share
pub fn group_recovery_key(key: &str) -> String {
    let compact: Vec<char> = key.chars().filter(|c| !c.is_whitespace()).collect();
    compact
        .chunks(4)
        .map(|chunk| chunk.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The recovery key form and the buttons beside it: what is shown and
/// whether the next click does anything.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecoveryForm {
    pub busy: bool,
    /// The last outcome: Ok for a note, Err for an error.
    pub message: Option<Result<String, String>>,
    /// A key just made here, shown until Settings closes.
    pub new_key: Option<String>,
}

impl RecoveryForm {
    /// Starting any recovery action: false if one is already running.
    pub fn begin(&mut self) -> bool {
        if self.busy {
            return false;
        }
        self.busy = true;
        self.message = None;
        true
    }

    /// Submitting the typed key: false when busy or empty.
    pub fn begin_submit(&mut self, input: &str) -> bool {
        if self.busy || !recovery_key_ready(input) {
            return false;
        }
        self.begin()
    }

    pub fn restored(&mut self) {
        self.busy = false;
        self.message = Some(Ok(RECOVERY_ACCEPTED.into()));
    }

    pub fn made(&mut self, key: String) {
        self.busy = false;
        self.new_key = Some(key);
    }

    pub fn failed(&mut self, error: String) {
        self.busy = false;
        self.message = Some(Err(error));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(verified: bool, recovery: RecoveryAvailability) -> EncryptionStatus {
        EncryptionStatus {
            device_id: None,
            device_verified: verified,
            recovery,
        }
    }

    #[test]
    fn a_device_is_locked_until_verified_or_recovered() {
        use RecoveryAvailability::*;
        assert!(KeyAccess::from_status(&status(false, NeedsKey)).locked);
        assert!(KeyAccess::from_status(&status(false, Disabled)).locked);
        assert!(KeyAccess::from_status(&status(false, Unknown)).locked);
        assert!(!KeyAccess::from_status(&status(true, NeedsKey)).locked);
        assert!(!KeyAccess::from_status(&status(false, Enabled)).locked);
        assert!(KeyAccess::from_status(&status(false, Disabled)).backup_missing);
        assert!(!KeyAccess::from_status(&status(true, Enabled)).backup_missing);
    }

    #[test]
    fn undecryptable_rows_are_locked_only_on_a_locked_device() {
        let locked = KeyAccess {
            locked: true,
            backup_missing: false,
        };
        assert_eq!(undecryptable(locked), Undecryptable::Locked);
        assert_eq!(undecryptable(KeyAccess::default()), Undecryptable::Waiting);
    }

    #[test]
    fn the_banner_needs_a_locked_device_and_a_locked_message_in_the_main_timeline() {
        let locked = KeyAccess {
            locked: true,
            backup_missing: false,
        };
        assert!(show_banner(true, locked, true));
        assert!(!show_banner(true, locked, false));
        assert!(!show_banner(true, KeyAccess::default(), true));
        assert!(!show_banner(false, locked, true));
    }

    #[test]
    fn the_backup_offer_waits_for_no_backup_and_hides_after_later() {
        let missing = KeyAccess {
            locked: true,
            backup_missing: true,
        };
        assert!(show_backup_prompt(missing, false));
        assert!(!show_backup_prompt(missing, true));
        assert!(!show_backup_prompt(KeyAccess::default(), false));
    }

    #[test]
    fn notices_open_the_security_page() {
        assert_eq!(SettingsPage::for_focus(SettingsFocus::None), None);
        for focus in [
            SettingsFocus::RecoveryKey,
            SettingsFocus::SetUpBackup,
            SettingsFocus::Security,
        ] {
            assert_eq!(SettingsPage::for_focus(focus), Some(SettingsPage::Security));
        }
    }

    #[test]
    fn status_lines_say_what_the_device_has() {
        let lines = status_lines(&status(false, RecoveryAvailability::NeedsKey));
        assert_eq!(lines[0].text, "This device is not verified");
        assert_eq!(lines[0].class, "warn");
        assert!(lines[1].text.contains("enter your recovery key"));
        let lines = status_lines(&status(true, RecoveryAvailability::Enabled));
        assert_eq!(lines[0].text, "This device is verified");
        assert_eq!(lines[1].class, "ok");
    }

    #[test]
    fn the_page_offers_what_fits_the_backup() {
        let none = status(false, RecoveryAvailability::Disabled);
        let enabled = status(true, RecoveryAvailability::Enabled);
        assert_eq!(recovery_offer(Some(&none), false), RecoveryOffer::SetUp);
        assert_eq!(recovery_offer(Some(&enabled), false), RecoveryOffer::Change);
        assert_eq!(recovery_offer(Some(&enabled), true), RecoveryOffer::NewKey);
        assert_eq!(recovery_offer(None, false), RecoveryOffer::None);
    }

    #[test]
    fn recovery_keys_are_trimmed_and_grouped() {
        assert_eq!(clean_recovery_key("  EsTx 1234 \n"), "EsTx 1234");
        assert!(!recovery_key_ready("   "));
        assert!(recovery_key_ready(" a "));
        assert_eq!(group_recovery_key("EsTx12345678ab"), "EsTx 1234 5678 ab");
        assert_eq!(group_recovery_key("EsTx 1234"), "EsTx 1234");
        assert_eq!(group_recovery_key(""), "");
    }

    #[test]
    fn the_form_runs_one_action_at_a_time() {
        let mut form = RecoveryForm::default();
        assert!(!form.begin_submit("  "), "nothing to send");
        assert!(form.begin_submit("key"));
        assert!(!form.begin_submit("key"), "busy");
        assert!(!form.begin());
        form.failed("bad key".into());
        assert!(!form.busy);
        assert_eq!(form.message, Some(Err("bad key".into())));
        assert!(form.begin());
        assert_eq!(form.message, None, "a new try clears the old message");
        form.restored();
        assert_eq!(form.message, Some(Ok(RECOVERY_ACCEPTED.into())));
        assert!(form.begin());
        form.made("EsTx".into());
        assert_eq!(form.new_key.as_deref(), Some("EsTx"));
        assert!(!form.busy);
    }
}
