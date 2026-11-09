//! Settings (gear button in the sidebar footer): categories down the side,
//! about 70% of a desktop window; full screen on a phone, list first. My
//! Account, Security & Privacy (encryption, verification, recovery key,
//! devices), Notifications (account-wide defaults, this device's delivery),
//! Appearance (colour theme), Voice & Video.

use std::rc::Rc;

use dioxus::prelude::*;
use morpheus_core::{
    Device, EncryptionStatus, NotifyMode, RecoveryAvailability, Session, VerificationActions,
    VerificationHandle, VerificationStep,
};

use super::keys::{KeysRefresh, SettingsFocus, SettingsTarget};
use super::shell::{NarrowWindow, Theme, UiPrefs};
use super::toast::report;
use crate::device_prefs::NotificationContent;
use crate::icons::{BELL, Icon, MIC, PALETTE, SHIELD, USER};

/// How long a finished verification shows its success before stepping aside.
const VERIFIED_NOTE_SECS: u64 = 3;

/// Whether the settings modal is open. Provided by the shell.
pub type SettingsOpen = Signal<bool>;

/// How long a setting opened from the palette stays highlighted.
const SETTING_HIGHLIGHT_MS: u32 = 2500;

/// The settings categories, in the order the side list shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
    Account,
    Security,
    Notifications,
    Appearance,
    VoiceVideo,
    /// Desktop only: opening at sign-in, starting hidden.
    Startup,
    /// Reporting a bug, and the version.
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
            SettingsPage::Account => USER,
            SettingsPage::Security => SHIELD,
            SettingsPage::Notifications => BELL,
            SettingsPage::Appearance => PALETTE,
            SettingsPage::VoiceVideo => MIC,
            SettingsPage::Startup => crate::icons::SQUARE,
            SettingsPage::Help => crate::icons::CIRCLE_HELP,
        }
    }
}

/// The page shown, if one is chosen: on a phone `None` is the list; on a
/// desktop it means My Account. Provided by the shell, which steps a phone
/// back from a page to the list on Escape / the back key.
pub type OpenSettingsPage = Signal<Option<SettingsPage>>;

/// One setting the command palette finds: its name, other words it answers
/// to, its page, and the `data-setting` anchor on that page.
#[derive(PartialEq, Eq)]
pub struct SettingEntry {
    pub label: &'static str,
    pub keywords: &'static str,
    pub page: SettingsPage,
    pub anchor: &'static str,
}

/// Every setting the palette can open.
pub const SETTINGS_INDEX: &[SettingEntry] = &[
    SettingEntry {
        label: "Account",
        keywords: "profile user name sign out device id",
        page: SettingsPage::Account,
        anchor: "account",
    },
    SettingEntry {
        label: "Encryption and recovery key",
        keywords: "security backup recovery key verify verification restore e2e",
        page: SettingsPage::Security,
        anchor: "encryption",
    },
    SettingEntry {
        label: "Devices and sessions",
        keywords: "security sessions sign out other devices",
        page: SettingsPage::Security,
        anchor: "sessions",
    },
    SettingEntry {
        label: "Notification defaults for rooms",
        keywords: "notifications dms groups all messages mentions",
        page: SettingsPage::Notifications,
        anchor: "room-defaults",
    },
    SettingEntry {
        label: "Notifications on this device",
        keywords: "notifications push turn off on",
        page: SettingsPage::Notifications,
        anchor: "device-notifications",
    },
    SettingEntry {
        label: "What notifications show",
        keywords: "notifications privacy hide message name someone content",
        page: SettingsPage::Notifications,
        anchor: "notification-content",
    },
    SettingEntry {
        label: "Theme",
        keywords: "appearance dark light midnight colours colors",
        page: SettingsPage::Appearance,
        anchor: "theme",
    },
    SettingEntry {
        label: "Microphone and speaker",
        keywords: "voice video audio input output devices calls",
        page: SettingsPage::VoiceVideo,
        anchor: "audio-devices",
    },
    SettingEntry {
        label: "Open at sign-in",
        keywords: "startup autostart login tray hidden",
        page: SettingsPage::Startup,
        anchor: "startup",
    },
    SettingEntry {
        label: "Report a bug",
        keywords: "help feedback issue github",
        page: SettingsPage::Help,
        anchor: "report-bug",
    },
    SettingEntry {
        label: "Version",
        keywords: "help about update",
        page: SettingsPage::Help,
        anchor: "version",
    },
];

/// The setting to scroll to and highlight once its page shows (from the
/// command palette).
pub type SettingHighlight = Signal<Option<&'static str>>;

/// Open Settings at one setting, highlighted.
pub fn open_setting(entry: &SettingEntry) {
    consume_context::<SettingsOpen>().set(true);
    show_setting(entry);
}

/// Show one setting's page with it highlighted (Settings being open).
pub fn show_setting(entry: &SettingEntry) {
    consume_context::<OpenSettingsPage>().set(Some(entry.page));
    consume_context::<SettingHighlight>().set(Some(entry.anchor));
}

/// The settings matching every word of `query`, in their name or the other
/// words they answer to; the Startup page only with a window.
pub fn find_settings(query: &str, has_window: bool) -> Vec<&'static SettingEntry> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    SETTINGS_INDEX
        .iter()
        .filter(|s| has_window || s.page != SettingsPage::Startup)
        .filter(|s| {
            let words = format!("{} {}", s.label, s.keywords).to_lowercase();
            needle.split_whitespace().all(|w| words.contains(w))
        })
        .collect()
}

/// The verification flow currently shown, if any. Incoming requests set this
/// from the shell; the "Verify" button sets it from the modal.
pub type ActiveVerification = Signal<Option<Rc<Verification>>>;

/// A verification flow as the UI sees it. The handle is driven by a task
/// that mirrors its steps into `step`.
pub struct Verification {
    pub handle: VerificationActions,
    pub step: Signal<VerificationStep>,
    pub incoming: bool,
}

impl PartialEq for Verification {
    /// Flows are identified by their step signal; props only need identity.
    fn eq(&self, other: &Self) -> bool {
        self.step == other.step && self.incoming == other.incoming
    }
}

/// Wrap a handle and start mirroring its steps.
///
/// The step signal and the task feeding it belong to the app's root scope,
/// not the caller's. A flow started from the settings modal must keep running
/// and stay readable after the modal closes and reopens: owned by the modal's
/// scope, the signal was dropped on close and the next read panicked.
pub fn track_verification(mut handle: VerificationHandle, incoming: bool) -> Rc<Verification> {
    let step = Signal::new_in_scope(VerificationStep::Requested, ScopeId::APP);
    let mut step_writer = step;
    // Split: the task owns the receiving end; actions go through a clone.
    let actions = handle.actions();
    dioxus::core::spawn_forever(async move {
        while let Some(next) = handle.next().await {
            step_writer.set(next);
        }
    });
    Rc::new(Verification {
        handle: actions,
        step,
        incoming,
    })
}

#[component]
pub fn Settings() -> Element {
    let session = use_context::<Session>();
    let mut open = use_context::<SettingsOpen>();
    let mut verification = use_context::<ActiveVerification>();
    let mut recovery_key = use_signal(String::new);
    let mut recovery_message = use_signal(|| None::<Result<String, String>>);
    let mut busy = use_signal(|| false);
    // Bumped to refresh status and devices after a flow finishes.
    let mut refresh = use_signal(|| 0u32);

    let status = use_resource({
        let session = session.clone();
        move || {
            let session = session.clone();
            let _ = refresh();
            async move { session.encryption_status().await.map_err(|e| e.to_string()) }
        }
    });
    let devices = use_resource({
        let session = session.clone();
        move || {
            let session = session.clone();
            let _ = refresh();
            async move { session.own_devices().await.map_err(|e| e.to_string()) }
        }
    });

    // A setting picked in the palette: once its page has rendered, scroll it
    // into view and flash it.
    let mut highlight = use_context::<SettingHighlight>();
    use_effect(move || {
        let Some(anchor) = highlight() else {
            return;
        };
        highlight.set(None);
        spawn(async move {
            let js = format!(
                "for (let i = 0; i < 40; i++) {{ \
                   const el = document.querySelector('.settings-page [data-setting={anchor}]'); \
                   if (el) {{ el.scrollIntoView({{ block: 'center' }}); el.classList.add('setting-highlight'); \
                     setTimeout(() => el.classList.remove('setting-highlight'), {SETTING_HIGHLIGHT_MS}); return; }} \
                   await new Promise(r => setTimeout(r, 50)); \
                 }}"
            );
            let _ = document::eval(&js).await;
        });
    });

    // Refresh once a verification flow completes.
    use_effect(move || {
        if let Some(v) = verification()
            && matches!(*v.step.read(), VerificationStep::Done)
        {
            refresh += 1;
            bump_keys();
        }
    });

    let user = session
        .user_id()
        .map(|id| id.to_string())
        .unwrap_or_default();
    let mut page = use_context::<OpenSettingsPage>();
    let narrow = try_use_context::<NarrowWindow>().is_some_and(|n| n().0);
    // A desktop window: only it has startup options.
    let has_window = use_hook(|| try_consume_context::<crate::WindowControls>().is_some());
    let mut search = use_signal(String::new);
    let mut close = move || {
        open.set(false);
        page.set(None);
    };
    // Opened to verify or to enter the recovery key: that page.
    use_effect(move || {
        if let Some(mut target) = try_consume_context::<SettingsTarget>() {
            let wanted = *target.peek();
            match wanted {
                SettingsFocus::RecoveryKey | SettingsFocus::SetUpBackup => {
                    page.set(Some(SettingsPage::Security))
                }
                SettingsFocus::Security => {
                    target.set(SettingsFocus::None);
                    page.set(Some(SettingsPage::Security));
                }
                SettingsFocus::None => {}
            }
        }
    });
    // A desktop always shows a page (Security while a verification runs);
    // a phone starts on the list (on Security, when opened mid-verification).
    let landing = if verification().is_some() {
        SettingsPage::Security
    } else {
        SettingsPage::Account
    };
    use_hook(move || {
        if narrow && verification.peek().is_some() && page.peek().is_none() {
            page.set(Some(SettingsPage::Security));
        }
    });
    let shown = page().or((!narrow).then_some(landing));

    // A recovery key just created here, shown until Settings closes.
    let mut new_key = use_signal(|| None::<String>);
    let mut set_up_recovery = {
        let session = session.clone();
        move || {
            if busy() {
                return;
            }
            busy.set(true);
            recovery_message.set(None);
            let session = session.clone();
            spawn(async move {
                match session.enable_recovery().await {
                    Ok(key) => {
                        new_key.set(Some(key));
                        refresh += 1;
                        bump_keys();
                    }
                    Err(err) => recovery_message.set(Some(Err(err.to_string()))),
                }
                busy.set(false);
            });
        }
    };
    let mut change_key = {
        let session = session.clone();
        move || {
            if busy() {
                return;
            }
            busy.set(true);
            recovery_message.set(None);
            let session = session.clone();
            spawn(async move {
                match session.change_recovery_key().await {
                    Ok(key) => {
                        new_key.set(Some(key));
                        refresh += 1;
                    }
                    Err(err) => recovery_message.set(Some(Err(err.to_string()))),
                }
                busy.set(false);
            });
        }
    };
    let recovery_enabled = matches!(
        &*status.read(),
        Some(Ok(status)) if status.recovery == RecoveryAvailability::Enabled
    );
    let backup_missing = matches!(
        &*status.read(),
        Some(Ok(status)) if status.recovery == RecoveryAvailability::Disabled
    );
    // Opened from the Secure Backup offer: set it up straight away.
    use_effect({
        let mut set_up_recovery = set_up_recovery.clone();
        move || {
            let missing = matches!(
                &*status.read(),
                Some(Ok(status)) if status.recovery == RecoveryAvailability::Disabled
            );
            if let Some(mut target) = try_consume_context::<super::keys::SettingsTarget>()
                && missing
                && *target.peek() == super::keys::SettingsFocus::SetUpBackup
            {
                target.set(super::keys::SettingsFocus::None);
                set_up_recovery();
            }
        }
    });

    let start_verification = {
        let session = session.clone();
        move |_| {
            if busy() {
                return;
            }
            busy.set(true);
            let session = session.clone();
            spawn(async move {
                match session.request_self_verification().await {
                    Ok(handle) => verification.set(Some(track_verification(handle, false))),
                    Err(err) => recovery_message.set(Some(Err(err.to_string()))),
                }
                busy.set(false);
            });
        }
    };

    let submit_recovery = {
        let session = session.clone();
        move |evt: FormEvent| {
            evt.prevent_default();
            if busy() || recovery_key().trim().is_empty() {
                return;
            }
            busy.set(true);
            recovery_message.set(None);
            let session = session.clone();
            let key = recovery_key();
            spawn(async move {
                match session.recover(key).await {
                    Ok(()) => {
                        recovery_key.set(String::new());
                        recovery_message.set(Some(Ok(
                            "Recovery key accepted. Encrypted history will decrypt as keys arrive."
                                .into(),
                        )));
                        refresh += 1;
                        bump_keys();
                    }
                    Err(err) => recovery_message.set(Some(Err(err.to_string()))),
                }
                busy.set(false);
            });
        }
    };

    rsx! {
        div { class: "overlay settings-overlay", onclick: move |_| close(),
            div {
                class: "settings",
                class: if shown.is_some() { "has-page" },
                tabindex: "-1",
                autofocus: true,
                onclick: move |e| e.stop_propagation(),
                nav { class: "settings-nav",
                    header { class: "settings-nav-header",
                        h2 { "Settings" }
                        button { class: "ghost settings-nav-close", title: "Close", onclick: move |_| close(), "✕" }
                    }
                    // Find a single setting; on a desktop it has the keyboard
                    // as Settings opens (a phone's would raise its keyboard).
                    input {
                        class: "settings-search",
                        r#type: "search",
                        placeholder: "Search settings",
                        value: "{search}",
                        oninput: move |e| search.set(e.value()),
                        onmounted: move |e| {
                            if has_window {
                                spawn(async move {
                                    let _ = e.data().set_focus(true).await;
                                });
                            }
                        },
                        onkeydown: move |e: KeyboardEvent| {
                            if e.key() == Key::Enter
                                && let Some(entry) = find_settings(&search(), has_window).first()
                            {
                                e.prevent_default();
                                show_setting(entry);
                                search.set(String::new());
                            }
                        },
                    }
                    if !search().trim().is_empty() {
                        for entry in find_settings(&search(), has_window) {
                            button {
                                class: "settings-result",
                                onclick: move |_| {
                                    show_setting(entry);
                                    search.set(String::new());
                                },
                                Icon { glyph: entry.page.glyph(), size: 18 }
                                span { "{entry.label}" }
                                span { class: "settings-result-page muted", "{entry.page.label()}" }
                            }
                        }
                        if find_settings(&search(), has_window).is_empty() {
                            p { class: "muted settings-no-result", "No setting matches." }
                        }
                    }
                    for item in SettingsPage::ALL.into_iter().filter(|p| has_window || *p != SettingsPage::Startup).filter(|_| search().trim().is_empty()) {
                        button {
                            class: "settings-nav-item",
                            class: if shown == Some(item) { "selected" },
                            onclick: move |_| page.set(Some(item)),
                            Icon { glyph: item.glyph(), size: 18 }
                            span { "{item.label()}" }
                        }
                    }
                }
                if let Some(current) = shown {
                    section { class: "settings-page",
                        header { class: "settings-header",
                            button {
                                class: "settings-back",
                                title: "Back",
                                onclick: move |_| page.set(None),
                                Icon { glyph: crate::icons::CHEVRON_LEFT, size: 20 }
                            }
                            h2 { "{current.label()}" }
                            button { class: "ghost", title: "Close", onclick: move |_| close(), "✕" }
                        }
                        match current {
                            SettingsPage::Account => rsx! {
                                section { class: "settings-section", "data-setting": "account",
                                    h3 { "Account" }
                                    p { class: "settings-user", "{user}" }
                                    if let Some(Ok(status)) = &*status.read() {
                                        p { class: "muted", "This device: {status.device_id.as_ref().map(|d| d.to_string()).unwrap_or_default()}" }
                                    }
                                }
                            },
                            SettingsPage::Security => rsx! {
                                section { class: "settings-section", "data-setting": "encryption",
                                    h3 { "Encryption" }
                                    match &*status.read() {
                                        None => rsx! { p { class: "muted", "Checking…" } },
                                        Some(Err(err)) => rsx! { p { class: "error", "{err}" } },
                                        Some(Ok(status)) => rsx! { EncryptionSummary { status: status.clone() } },
                                    }

                                    if let Some(key) = new_key() {
                                        div { class: "recovery-key-new",
                                            p { strong { "Your recovery key" } }
                                            p { class: "muted",
                                                "Save it somewhere safe, like a password manager. With it, any new device can read your encrypted history; without it, history is lost if you lose all your signed-in devices."
                                            }
                                            code { "{key}" }
                                            button {
                                                class: "secondary",
                                                onclick: move |_| super::context_menu::copy_to_clipboard(&key),
                                                Icon { glyph: crate::icons::COPY }
                                                "Copy"
                                            }
                                        }
                                    } else if backup_missing {
                                        div { class: "settings-actions",
                                            button { class: "primary setup-recovery", disabled: busy(), onclick: move |_| set_up_recovery(),
                                                Icon { glyph: SHIELD }
                                                "Set up recovery"
                                            }
                                        }
                                    } else if recovery_enabled {
                                        div { class: "settings-actions",
                                            button { class: "secondary change-recovery-key", disabled: busy(), onclick: move |_| change_key(),
                                                Icon { glyph: crate::icons::REFRESH_CW }
                                                "Change recovery key"
                                            }
                                            p { class: "muted",
                                                "Lost your recovery key? Make a new one here; the old one stops working."
                                            }
                                        }
                                    }
                                    if let Some(active) = verification() {
                                        VerificationPanel { verification: active }
                                    } else {
                                        div { class: "settings-actions",
                                            button { class: "primary", disabled: busy(), onclick: start_verification.clone(),
                                                Icon { glyph: crate::icons::SHIELD_CHECK }
                                                "Verify with another signed-in device"
                                            }
                                        }
                                        form { class: "recovery-form", onsubmit: submit_recovery.clone(),
                                            label { "Recovery key or passphrase"
                                                input {
                                                    r#type: "password",
                                                    placeholder: "EsTx xxxx xxxx …",
                                                    value: "{recovery_key}",
                                                    oninput: move |e| recovery_key.set(e.value()),
                                                    // Opened from "Enter recovery key": straight to typing.
                                                    onmounted: move |e| {
                                                        if let Some(mut target) = try_consume_context::<SettingsTarget>()
                                                            && *target.peek() == SettingsFocus::RecoveryKey
                                                        {
                                                            target.set(SettingsFocus::None);
                                                            spawn(async move {
                                                                let _ = e.data().set_focus(true).await;
                                                            });
                                                        }
                                                    },
                                                }
                                            }
                                            button { r#type: "submit", class: "secondary", disabled: busy(),
                                                Icon { glyph: crate::icons::KEY }
                                                "Restore from backup"
                                            }
                                        }
                                    }
                                    match recovery_message() {
                                        Some(Ok(message)) => rsx! { p { class: "ok", "{message}" } },
                                        Some(Err(message)) => rsx! { p { class: "error", "{message}" } },
                                        None => rsx! {},
                                    }
                                }

                                section { class: "settings-section", "data-setting": "sessions",
                                    h3 { "Devices" }
                                    match &*devices.read() {
                                        None => rsx! { crate::media::Skeleton { rows: 2 } },
                                        Some(Err(err)) => rsx! { p { class: "error", "{err}" } },
                                        Some(Ok(list)) => rsx! { DeviceList { devices: list.clone() } },
                                    }
                                }
                            },
                            SettingsPage::Notifications => rsx! { NotificationsPage {} },
                            SettingsPage::Startup => rsx! { StartupPage {} },
                            SettingsPage::Help => rsx! { HelpPage {} },
                            SettingsPage::Appearance => rsx! { AppearancePage {} },
                            SettingsPage::VoiceVideo => rsx! { VoicePage {} },
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn EncryptionSummary(status: EncryptionStatus) -> Element {
    let (recovery_text, recovery_class) = match status.recovery {
        RecoveryAvailability::Unknown => ("Checking key backup…", "muted"),
        RecoveryAvailability::Enabled => ("Key backup connected", "ok"),
        RecoveryAvailability::NeedsKey => (
            "Key backup found; enter your recovery key to read older messages",
            "warn",
        ),
        RecoveryAvailability::Disabled => ("No key backup on this account", "muted"),
    };
    rsx! {
        ul { class: "status-list",
            li { class: if status.device_verified { "ok" } else { "warn" },
                if status.device_verified { "This device is verified" } else { "This device is not verified" }
            }
            li { class: "{recovery_class}", "{recovery_text}" }
        }
    }
}

#[component]
fn DeviceList(devices: Vec<Device>) -> Element {
    rsx! {
        ul { class: "device-list",
            for device in devices {
                li { key: "{device.id}", class: "device",
                    span { class: "device-name", "{device.name}" }
                    span { class: "device-id muted", "{device.id}" }
                    if device.is_current {
                        span { class: "tag", "this device" }
                    }
                    span { class: if device.verified { "tag ok" } else { "tag warn" },
                        if device.verified { "verified" } else { "unverified" }
                    }
                }
            }
        }
    }
}

/// The emoji SAS dialog, for an incoming request or one we started.
#[component]
pub fn VerificationPanel(verification: Rc<Verification>) -> Element {
    let mut active = use_context::<ActiveVerification>();
    let step = (verification.step)();
    let handle = verification.handle.clone();

    // A finished verification shows its success for a moment, then makes way
    // for the settings (which now list this device as verified).
    let finished = matches!(step, VerificationStep::Done);
    use_effect(use_reactive!(|finished| {
        if finished {
            spawn(async move {
                morpheus_core::rt::sleep(std::time::Duration::from_secs(VERIFIED_NOTE_SECS)).await;
                if active
                    .peek()
                    .as_ref()
                    .is_some_and(|v| matches!(*v.step.peek(), VerificationStep::Done))
                {
                    active.set(None);
                }
            });
        }
    }));

    let run = move |future: morpheus_core::CoreFuture<()>| {
        spawn(async move {
            if let Err(err) = future.await {
                report("verification action", &err);
            }
        });
    };

    rsx! {
        div { class: "verification",
            match step {
                VerificationStep::Requested if verification.incoming => rsx! {
                    p { "Another one of your devices wants to verify this one." }
                    div { class: "settings-actions",
                        button { class: "primary", onclick: { let h = handle.clone(); move |_| run(h.accept()) }, "Accept" }
                        button { class: "secondary", onclick: { let h = handle.clone(); move |_| run(h.cancel()) }, "Decline" }
                    }
                },
                VerificationStep::Requested => rsx! {
                    p { "Waiting for your other device to accept…" }
                    button { class: "secondary", onclick: { let h = handle.clone(); move |_| run(h.cancel()) }, "Cancel" }
                },
                VerificationStep::Ready => rsx! { p { "Connecting…" } },
                VerificationStep::ShowQr(code) => rsx! {
                    p { "Scan this code with your other device." }
                    div {
                        class: "verification-qr",
                        // What the code holds, for test runs to scan it.
                        "data-qr": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &code.data),
                        dangerous_inner_html: qr_svg(&code),
                    }
                    div { class: "settings-actions",
                        button { class: "secondary use-emoji", onclick: { let h = handle.clone(); move |_| run(h.use_emoji()) }, "Compare emoji instead" }
                        button { class: "secondary", onclick: { let h = handle.clone(); move |_| run(h.cancel()) }, "Cancel" }
                    }
                },
                VerificationStep::QrScanned => rsx! {
                    p { "Your other device scanned the code. Does it say it verified this one?" }
                    div { class: "settings-actions",
                        button { class: "primary confirm-scanned", onclick: { let h = handle.clone(); move |_| run(h.confirm_scanned()) }, "Yes" }
                        button { class: "danger", onclick: { let h = handle.clone(); move |_| run(h.cancel()) }, "No" }
                    }
                },
                VerificationStep::Reciprocated => rsx! { p { "Scanned. Confirm on your other device…" } },
                VerificationStep::Emojis(emojis) => rsx! {
                    p { "Compare these with your other device. They must match in the same order." }
                    ul { class: "emoji-grid",
                        // By position: the same emoji can come up twice in one code,
                        // and duplicate keys crash the renderer.
                        for (index, (symbol, name)) in emojis.into_iter().enumerate() {
                            li { key: "{index}", span { class: "emoji", "{symbol}" } span { class: "emoji-name", "{name}" } }
                        }
                    }
                    div { class: "settings-actions",
                        button { class: "primary", onclick: { let h = handle.clone(); move |_| run(h.confirm_match()) }, "They match" }
                        button { class: "danger", onclick: { let h = handle.clone(); move |_| run(h.mismatch()) }, "They don't match" }
                    }
                },
                VerificationStep::Confirmed => rsx! { p { "Waiting for your other device to confirm…" } },
                // Nothing to dismiss: the panel steps aside by itself (see above).
                VerificationStep::Done => rsx! {
                    p { class: "ok", "Verified. Encrypted messages will now sync between your devices." }
                },
                VerificationStep::Cancelled(reason) => rsx! {
                    p { class: "error", "Verification cancelled: {reason}" }
                    button { class: "secondary", onclick: move |_| active.set(None), "Close" }
                },
            }
        }
    }
}

/// A verification code as SVG: dark modules on white, with the four-module
/// quiet border scanners need, scaled to fit.
fn qr_svg(code: &morpheus_core::QrImage) -> String {
    const BORDER: usize = 4;
    let size = code.width + 2 * BORDER;
    let mut path = String::new();
    for (index, _) in code.dark.iter().enumerate().filter(|(_, dark)| **dark) {
        let (x, y) = (index % code.width + BORDER, index / code.width + BORDER);
        path.push_str(&format!("M{x} {y}h1v1h-1z"));
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {size} {size}\" shape-rendering=\"crispEdges\">\
         <rect width=\"{size}\" height=\"{size}\" fill=\"#fff\"/><path d=\"{path}\" fill=\"#000\"/></svg>"
    )
}

/// Re-check whether this device can read encrypted history (the chat's
/// locked-messages notice), after a verification or recovery succeeded.
fn bump_keys() {
    if let Some(mut keys) = try_consume_context::<KeysRefresh>() {
        keys.with_mut(|stamp| stamp.0 += 1);
    }
}

/// Account-wide notification defaults, and how this device gets them.
#[component]
fn NotificationsPage() -> Element {
    let data_dir = use_context::<crate::AppConfig>().data_dir;
    let mut on_here = use_signal({
        let data_dir = data_dir.clone();
        move || crate::device_prefs::notifications_enabled(&data_dir)
    });
    let mut shows = use_signal({
        let data_dir = data_dir.clone();
        move || crate::device_prefs::notification_content(&data_dir)
    });
    let content_dir = data_dir.clone();
    // Written by the Android push bridge (apps/mobile/src/push.rs).
    let push = morpheus_core::fs::read_to_string(data_dir.join("push_status")).ok();
    let device = if cfg!(target_os = "android") {
        match push.as_deref().map(str::trim) {
            Some(status) if status.starts_with("registered via ") => format!(
                "Push notifications are on, through your UnifiedPush app ({}).",
                status.trim_start_matches("registered via ")
            ),
            Some(status) => format!("Push notifications are not working: {status}"),
            None => "Install a UnifiedPush app such as Sunup or ntfy to be notified while Morpheus is closed.".to_owned(),
        }
    } else {
        "Morpheus shows a system notification when a message arrives while its window is in the background or hidden in the tray.".to_owned()
    };
    rsx! {
        section { class: "settings-section", "data-setting": "room-defaults",
            h3 { "Default for rooms" }
            p { class: "muted", "Rooms without their own setting follow these. Set a room's own from its menu." }
            NotifyDefault { dm: true }
            NotifyDefault { dm: false }
        }
        section { class: "settings-section", "data-setting": "device-notifications",
            h3 { "This device" }
            label { class: "notify-choice",
                input {
                    r#type: "checkbox",
                    name: "device-notifications",
                    checked: on_here(),
                    onchange: move |_| {
                        let want = !on_here();
                        match crate::device_prefs::set_notifications_enabled(&data_dir, want) {
                            Ok(()) => on_here.set(want),
                            Err(err) => report("saving the notification setting", &err),
                        }
                    },
                }
                span { "Notifications on this device" }
            }
            p { class: "device-notifications muted", "{device}" }
            fieldset { class: "notify-default notification-content", "data-setting": "notification-content",
                legend { "What notifications show" }
                for content in NotificationContent::ALL {
                    label { class: "notify-choice",
                        input {
                            r#type: "radio",
                            name: "notification-content",
                            value: "{content.key()}",
                            checked: shows() == content,
                            onchange: {
                                let data_dir = content_dir.clone();
                                move |_| match crate::device_prefs::set_notification_content(&data_dir, content) {
                                    Ok(()) => shows.set(content),
                                    Err(err) => report("saving what notifications show", &err),
                                }
                            },
                        }
                        span { "{content.label()}" }
                    }
                }
            }
        }
    }
}

/// One default (direct messages or group rooms): all messages or mentions.
#[component]
fn NotifyDefault(dm: bool) -> Element {
    let session = use_context::<Session>();
    let kind = if dm { "dm" } else { "group" };
    let title = if dm { "Direct messages" } else { "Group rooms" };
    let mut chosen = use_signal(|| None::<NotifyMode>);
    let mut note = use_signal(|| None::<Result<&'static str, String>>);
    let loaded = use_resource({
        let session = session.clone();
        move || {
            let session = session.clone();
            async move { session.default_notify_mode(dm).await.ok() }
        }
    });
    let current = chosen().or_else(|| loaded().flatten());
    let choices = [
        (NotifyMode::AllMessages, "all", "All messages"),
        (NotifyMode::MentionsOnly, "mentions", "Only mentions"),
    ];
    rsx! {
        fieldset { class: "notify-default", "data-kind": "{kind}",
            legend { "{title}" }
            for (mode, value, label) in choices {
                label { class: "notify-choice",
                    input {
                        r#type: "radio",
                        name: "notify-{kind}",
                        value: "{value}",
                        checked: current == Some(mode),
                        onchange: {
                            let session = session.clone();
                            move |_| {
                                chosen.set(Some(mode));
                                note.set(None);
                                let session = session.clone();
                                spawn(async move {
                                    match session.set_default_notify_mode(dm, mode).await {
                                        Ok(()) => note.set(Some(Ok("Saved"))),
                                        Err(err) => note.set(Some(Err(err.to_string()))),
                                    }
                                });
                            }
                        },
                    }
                    span { "{label}" }
                }
            }
            match note() {
                Some(Ok(text)) => rsx! { span { class: "ok", "{text}" } },
                Some(Err(text)) => rsx! { span { class: "error", "{text}" } },
                None => rsx! {},
            }
        }
    }
}

/// The colour theme, saved to account data so every device follows it.
#[component]
fn AppearancePage() -> Element {
    let session = use_context::<Session>();
    let mut prefs = use_context::<UiPrefs>();
    let current = prefs().theme;
    rsx! {
        section { class: "settings-section", "data-setting": "theme",
            h3 { "Theme" }
            div { class: "theme-options",
                for theme in Theme::ALL {
                    button {
                        class: "theme-option",
                        class: if theme == current { "selected" },
                        "data-theme": "{theme.key()}",
                        onclick: {
                            let session = session.clone();
                            move |_| {
                                prefs.with_mut(|p| p.theme = theme);
                                let future = session.set_ui_prefs(prefs.peek().to_json());
                                spawn(async move {
                                    if let Err(err) = future.await {
                                        report("saving the theme", &err);
                                    }
                                });
                            }
                        },
                        span { class: "theme-swatch", "aria-hidden": "true",
                            span { class: "theme-swatch-side" }
                            span { class: "theme-swatch-chat" }
                        }
                        span { class: "theme-name", "{theme.label()}" }
                    }
                }
            }
        }
    }
}

/// The microphone and speaker calls use, kept on this device. Phones choose
/// for themselves, so they get a note instead.
#[component]
fn VoicePage() -> Element {
    let data_dir = use_context::<crate::AppConfig>().data_dir;
    let camera_dir = data_dir.clone();
    let mut camera = use_signal(morpheus_call::chosen_camera);
    // The cameras, as device lists show them.
    let cameras: Vec<morpheus_call::AudioDevice> =
        morpheus_call::cameras(morpheus_call::AudioMode::from_env())
            .into_iter()
            .map(|c| morpheus_call::AudioDevice {
                id: c.id,
                name: c.name,
            })
            .collect();
    let devices =
        use_resource(|| morpheus_call::audio_devices(morpheus_call::AudioMode::from_env()));
    let mut chosen = use_signal(morpheus_call::chosen_audio_devices);
    let choose = use_callback(move |choice: morpheus_call::DeviceChoice| {
        if let Err(err) = crate::device_prefs::set_audio_devices(&data_dir, &choice) {
            report("saving the audio devices", &err);
        }
        super::call::use_devices(choice.clone());
        chosen.set(choice);
    });
    rsx! {
        section { class: "settings-section voice-devices", "data-setting": "audio-devices",
            h3 { "Devices" }
            match devices() {
                None => rsx! { p { class: "muted", "Looking for devices…" } },
                Some(found) if found.microphones.is_empty() && found.speakers.is_empty() => rsx! {
                    p { class: "muted",
                        if cfg!(any(target_os = "android", target_os = "ios")) {
                            "Your phone picks the microphone and speaker (or headset) for calls."
                        } else {
                            "No microphones or speakers found."
                        }
                    }
                },
                Some(found) => rsx! {
                    DevicePicker {
                        kind: "microphone",
                        icon: crate::icons::MIC,
                        label: "Microphone",
                        devices: found.microphones,
                        current: chosen().microphone,
                        on_pick: move |id| {
                            let mut choice = chosen();
                            choice.microphone = id;
                            choose.call(choice);
                        },
                    }
                    DevicePicker {
                        kind: "speaker",
                        icon: crate::icons::VOLUME_2,
                        label: "Speaker",
                        devices: found.speakers,
                        current: chosen().speaker,
                        on_pick: move |id| {
                            let mut choice = chosen();
                            choice.speaker = id;
                            choose.call(choice);
                        },
                    }
                    if !cameras.is_empty() {
                        DevicePicker {
                            kind: "camera",
                            icon: crate::icons::VIDEO,
                            label: "Camera",
                            devices: cameras.clone(),
                            current: camera(),
                            on_pick: move |id: Option<String>| {
                                morpheus_call::choose_camera(id.clone());
                                if let Err(err) = crate::device_prefs::set_camera(&camera_dir, id.as_deref()) {
                                    report("saving the camera", &err);
                                }
                                camera.set(id);
                            },
                        }
                    }
                    p { class: "muted", "Used for calls on this device; a call going now switches over." }
                },
            }
        }
    }
}

/// One device list: the current choice, and the others below it when opened.
/// Drawn by the app rather than the system, whose (GTK) lists ignore the theme.
#[component]
fn DevicePicker(
    kind: &'static str,
    label: &'static str,
    icon: &'static str,
    devices: Vec<morpheus_call::AudioDevice>,
    current: Option<String>,
    on_pick: EventHandler<Option<String>>,
) -> Element {
    let mut open = use_signal(|| false);
    let current_name = current
        .as_ref()
        .and_then(|id| devices.iter().find(|d| &d.id == id))
        .map_or_else(|| "System default".to_owned(), |d| d.name.clone());
    let options: Vec<(Option<String>, String)> =
        std::iter::once((None, "System default".to_owned()))
            .chain(devices.into_iter().map(|d| (Some(d.id), d.name)))
            .collect();
    rsx! {
        div { class: "device-choice", "data-kind": "{kind}",
            span { class: "device-label", Icon { glyph: icon, size: 16 } "{label}" }
            button {
                class: "device-current",
                class: if open() { "open" },
                "data-id": "{current.clone().unwrap_or_default()}",
                onclick: move |_| open.toggle(),
                span { "{current_name}" }
                Icon { glyph: crate::icons::CHEVRON_DOWN, size: 16 }
            }
            if open() {
                div { class: "device-options", role: "listbox",
                    for (id, name) in options {
                        button {
                            class: "device-option",
                            class: if id == current { "selected" },
                            role: "option",
                            "data-id": "{id.clone().unwrap_or_default()}",
                            onclick: move |_| {
                                open.set(false);
                                on_pick.call(id.clone());
                            },
                            "{name}"
                        }
                    }
                }
            }
        }
    }
}

/// Desktop: open Morpheus when you sign in to the computer, maybe hidden.
#[component]
fn StartupPage() -> Element {
    let mut current = use_signal(crate::autostart::read);
    let mut note = use_signal(|| None::<String>);
    let mut apply = move |wanted: crate::autostart::Autostart| match crate::autostart::write(wanted)
    {
        Ok(()) => {
            current.set(crate::autostart::read());
            note.set(None);
        }
        Err(err) => note.set(Some(err.to_string())),
    };
    rsx! {
        section { class: "settings-section", "data-setting": "startup",
            h3 { "When you sign in to your computer" }
            match current() {
                None => rsx! { p { class: "muted", "Starting with your computer isn't available on this system yet." } },
                Some(state) => rsx! {
                    label { class: "notify-choice",
                        input {
                            r#type: "checkbox",
                            name: "autostart",
                            checked: state.enabled,
                            onchange: move |_| apply(crate::autostart::Autostart { enabled: !state.enabled, hidden: state.hidden }),
                        }
                        span { "Open Morpheus" }
                    }
                    label { class: "notify-choice",
                        input {
                            r#type: "checkbox",
                            name: "start-hidden",
                            checked: state.hidden,
                            disabled: !state.enabled,
                            onchange: move |_| apply(crate::autostart::Autostart { enabled: state.enabled, hidden: !state.hidden }),
                        }
                        span { "Start hidden in the tray" }
                    }
                },
            }
            if let Some(text) = note() {
                p { class: "error", "{text}" }
            }
        }
    }
}

/// Help & Feedback: the version, and Report a bug, which opens a new GitHub
/// issue filled in with the version, this device and the end of the log.
#[component]
fn HelpPage() -> Element {
    let version = super::about::VERSION;
    rsx! {
        section { class: "settings-section", "data-setting": "report-bug",
            h3 { "Report a bug" }
            p { class: "muted",
                "Opens a new issue on GitHub with your version, device and the end of this run's log filled in. Issues are public: read the log over and remove anything you'd rather not share before posting."
            }
            ReportBug {}
        }
        section { class: "settings-section", "data-setting": "version",
            h3 { "Version" }
            p { "Morpheus {version}" }
            p { class: "muted", "{morpheus_core::device()}" }
        }
    }
}

/// The Report a bug button: a new GitHub issue filled in with the version,
/// this device and the end of the log (as of when it is shown).
#[component]
pub fn ReportBug() -> Element {
    let config = use_context::<crate::AppConfig>();
    let url = use_hook(|| morpheus_core::bug_report_url(super::about::VERSION, &config.data_dir));
    rsx! {
        a { class: "button primary report-bug", href: "{url}",
            Icon { glyph: crate::icons::BUG }
            "Report a bug"
        }
    }
}
