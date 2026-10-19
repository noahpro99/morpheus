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
use crate::icons::{BELL, Icon, MIC, PALETTE, SHIELD, USER};

/// How long a finished verification shows its success before stepping aside.
const VERIFIED_NOTE_SECS: u64 = 3;

/// Whether the settings modal is open. Provided by the shell.
pub type SettingsOpen = Signal<bool>;

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
}

impl SettingsPage {
    pub const ALL: [SettingsPage; 6] = [
        SettingsPage::Account,
        SettingsPage::Security,
        SettingsPage::Notifications,
        SettingsPage::Appearance,
        SettingsPage::VoiceVideo,
        SettingsPage::Startup,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsPage::Account => "My Account",
            SettingsPage::Security => "Security & Privacy",
            SettingsPage::Notifications => "Notifications",
            SettingsPage::Appearance => "Appearance",
            SettingsPage::VoiceVideo => "Voice & Video",
            SettingsPage::Startup => "Startup",
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            SettingsPage::Account => USER,
            SettingsPage::Security => SHIELD,
            SettingsPage::Notifications => BELL,
            SettingsPage::Appearance => PALETTE,
            SettingsPage::VoiceVideo => MIC,
            SettingsPage::Startup => crate::icons::SQUARE,
        }
    }
}

/// The page shown, if one is chosen: on a phone `None` is the list; on a
/// desktop it means My Account. Provided by the shell, which steps a phone
/// back from a page to the list on Escape / the back key.
pub type OpenSettingsPage = Signal<Option<SettingsPage>>;

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
    let mut close = move || {
        open.set(false);
        page.set(None);
    };
    // Opened to verify or to enter the recovery key: that page.
    use_effect(move || {
        if let Some(mut target) = try_consume_context::<SettingsTarget>() {
            let wanted = *target.peek();
            match wanted {
                SettingsFocus::RecoveryKey => page.set(Some(SettingsPage::Security)),
                SettingsFocus::Security => {
                    target.set(SettingsFocus::None);
                    page.set(Some(SettingsPage::Security));
                }
                SettingsFocus::None => {}
            }
        }
    });
    // A desktop always shows a page (Security while a verification runs);
    // a phone starts on the list.
    let landing = if verification().is_some() {
        SettingsPage::Security
    } else {
        SettingsPage::Account
    };
    let shown = page().or((!narrow).then_some(landing));

    // A recovery key just created here, shown until Settings closes.
    let mut new_key = use_signal(|| None::<String>);
    let set_up_recovery = {
        let session = session.clone();
        move |_| {
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
    let backup_missing = matches!(
        &*status.read(),
        Some(Ok(status)) if status.recovery == RecoveryAvailability::Disabled
    );

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
                    for item in SettingsPage::ALL.into_iter().filter(|p| has_window || *p != SettingsPage::Startup) {
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
                                section { class: "settings-section",
                                    h3 { "Account" }
                                    p { class: "settings-user", "{user}" }
                                    if let Some(Ok(status)) = &*status.read() {
                                        p { class: "muted", "This device: {status.device_id.as_ref().map(|d| d.to_string()).unwrap_or_default()}" }
                                    }
                                }
                            },
                            SettingsPage::Security => rsx! {
                                section { class: "settings-section",
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
                                                "Copy"
                                            }
                                        }
                                    } else if backup_missing {
                                        div { class: "settings-actions",
                                            button { class: "primary setup-recovery", disabled: busy(), onclick: set_up_recovery,
                                                "Set up recovery"
                                            }
                                        }
                                    }
                                    if let Some(active) = verification() {
                                        VerificationPanel { verification: active }
                                    } else {
                                        div { class: "settings-actions",
                                            button { class: "primary", disabled: busy(), onclick: start_verification.clone(),
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
                                            button { r#type: "submit", class: "secondary", disabled: busy(), "Restore from backup" }
                                        }
                                    }
                                    match recovery_message() {
                                        Some(Ok(message)) => rsx! { p { class: "ok", "{message}" } },
                                        Some(Err(message)) => rsx! { p { class: "error", "{message}" } },
                                        None => rsx! {},
                                    }
                                }

                                section { class: "settings-section",
                                    h3 { "Devices" }
                                    match &*devices.read() {
                                        None => rsx! { p { class: "muted", "Loading…" } },
                                        Some(Err(err)) => rsx! { p { class: "error", "{err}" } },
                                        Some(Ok(list)) => rsx! { DeviceList { devices: list.clone() } },
                                    }
                                }
                            },
                            SettingsPage::Notifications => rsx! { NotificationsPage {} },
                            SettingsPage::Startup => rsx! { StartupPage {} },
                            SettingsPage::Appearance => rsx! { AppearancePage {} },
                            SettingsPage::VoiceVideo => rsx! {
                                section { class: "settings-section",
                                    h3 { "Calls" }
                                    p { class: "muted",
                                        "Voice and video calls aren't supported yet. Microphone, speaker and camera choices will live here once they are."
                                    }
                                }
                            },
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
                tokio::time::sleep(std::time::Duration::from_secs(VERIFIED_NOTE_SECS)).await;
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
    // Written by the Android push bridge (apps/mobile/src/push.rs).
    let push = std::fs::read_to_string(data_dir.join("push_status")).ok();
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
        section { class: "settings-section",
            h3 { "Default for rooms" }
            p { class: "muted", "Rooms without their own setting follow these. Set a room's own from its menu." }
            NotifyDefault { dm: true }
            NotifyDefault { dm: false }
        }
        section { class: "settings-section",
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
        section { class: "settings-section",
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
        section { class: "settings-section",
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
