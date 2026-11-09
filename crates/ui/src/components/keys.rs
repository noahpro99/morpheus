//! Whether this device can read encrypted history. A fresh sign-in has no
//! keys for older messages until it is verified from another device or
//! unlocked with the recovery key; until then those messages show as locked
//! rows and a notice above the chat says how to unlock them.

use std::time::Duration;

use dioxus::prelude::*;
use morpheus_core::{RecoveryAvailability, Session};

use super::settings::SettingsOpen;
use crate::icons::{Icon, KEY, LOCK, SHIELD_CHECK};

/// `locked` once the core reports this device cannot read encrypted
/// history; false while that is still unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyAccess {
    pub locked: bool,
    /// The account has no key backup at all: offer Secure Backup.
    pub backup_missing: bool,
}
pub type DeviceKeys = Signal<KeyAccess>;

/// Bumped after a verification or recovery succeeds, to re-check at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeysStamp(pub u32);
pub type KeysRefresh = Signal<KeysStamp>;

/// Where Settings should put the cursor when opened from the notice.
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
pub type SettingsTarget = Signal<SettingsFocus>;

/// How often the lock state is re-read (keys can arrive from elsewhere).
const RECHECK: Duration = Duration::from_secs(30);

/// Install in the shell: provides the signals and keeps `DeviceKeys` current.
pub fn provide_key_tracking(session: Session) {
    let mut keys: DeviceKeys = use_context_provider(|| Signal::new(KeyAccess::default()));
    let refresh: KeysRefresh = use_context_provider(|| Signal::new(KeysStamp::default()));
    use_context_provider(|| Signal::new(SettingsFocus::default()) as SettingsTarget);
    let mut tick = use_signal(|| 0u32);
    use_future(move || async move {
        loop {
            morpheus_core::rt::sleep(RECHECK).await;
            tick += 1;
        }
    });
    let status = use_resource(move || {
        let session = session.clone();
        let _ = (refresh(), tick());
        async move { session.encryption_status().await.ok() }
    });
    use_effect(move || {
        if let Some(Some(status)) = status() {
            let locked =
                !status.device_verified && status.recovery != RecoveryAvailability::Enabled;
            let backup_missing = status.recovery == RecoveryAvailability::Disabled;
            let now = KeyAccess {
                locked,
                backup_missing,
            };
            if *keys.peek() != now {
                keys.set(now);
            }
        }
    });
}

/// Above a chat that has messages this device cannot read.
#[component]
pub fn KeyBanner() -> Element {
    let mut settings_open = use_context::<SettingsOpen>();
    let mut target = use_context::<SettingsTarget>();
    rsx! {
        div { class: "key-banner", role: "status",
            Icon { glyph: LOCK, size: 18, class: "key-banner-icon" }
            div { class: "key-banner-text",
                strong { "Some messages are locked on this device" }
                span { "Encrypted messages from before you signed in here need keys that this device does not have yet." }
            }
            div { class: "key-banner-actions",
                button {
                    class: "primary",
                    onclick: move |_| {
                        target.set(SettingsFocus::Security);
                        settings_open.set(true);
                    },
                    Icon { glyph: SHIELD_CHECK, size: 16 }
                    "Verify with another device"
                }
                button {
                    class: "secondary",
                    onclick: move |_| {
                        target.set(SettingsFocus::RecoveryKey);
                        settings_open.set(true);
                    },
                    Icon { glyph: KEY, size: 16 }
                    "Enter recovery key"
                }
            }
        }
    }
}

/// Offered while the account has no key backup (a new account's first
/// sign-in, as Element does): Secure Backup keeps encrypted history
/// readable on new devices, behind a recovery key to keep. "Later" hides
/// it until the app next starts.
#[component]
pub fn BackupPrompt() -> Element {
    let keys = use_context::<DeviceKeys>();
    let mut settings_open = use_context::<SettingsOpen>();
    let mut target = use_context::<SettingsTarget>();
    let mut later = use_signal(|| false);
    if !keys().backup_missing || later() {
        return rsx! {};
    }
    rsx! {
        div { class: "backup-prompt", role: "status",
            Icon { glyph: KEY, size: 18, class: "backup-prompt-icon" }
            div { class: "backup-prompt-text",
                strong { "Set up Secure Backup" }
                span { "Keep your encrypted messages if you lose this device: you'll get a recovery key to save." }
            }
            div { class: "backup-prompt-actions",
                button { class: "secondary", onclick: move |_| later.set(true), "Later" }
                button {
                    class: "primary",
                    onclick: move |_| {
                        target.set(SettingsFocus::SetUpBackup);
                        settings_open.set(true);
                    },
                    "Set up"
                }
            }
        }
    }
}
