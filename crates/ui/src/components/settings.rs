//! Settings modal (gear button in the sidebar footer): account, encryption
//! status, device verification, recovery key, and the device list.

use std::rc::Rc;

use dioxus::prelude::*;
use super::toast::report;
use morpheus_core::{
    Device, EncryptionStatus, RecoveryAvailability, Session, VerificationActions,
    VerificationHandle, VerificationStep,
};

/// Whether the settings modal is open. Provided by the shell.
pub type SettingsOpen = Signal<bool>;

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
        }
    });

    let user = session
        .user_id()
        .map(|id| id.to_string())
        .unwrap_or_default();
    let mut close = move || open.set(false);

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
                    }
                    Err(err) => recovery_message.set(Some(Err(err.to_string()))),
                }
                busy.set(false);
            });
        }
    };

    rsx! {
        div { class: "overlay", onclick: move |_| close(),
            div { class: "settings", tabindex: "-1", autofocus: true, onclick: move |e| e.stop_propagation(),
                header { class: "settings-header",
                    h2 { "Settings" }
                    button { class: "ghost", title: "Close", onclick: move |_| close(), "✕" }
                }

                section { class: "settings-section",
                    h3 { "Account" }
                    p { class: "settings-user", "{user}" }
                    if let Some(Ok(status)) = &*status.read() {
                        p { class: "muted", "This device: {status.device_id.as_ref().map(|d| d.to_string()).unwrap_or_default()}" }
                    }
                }

                section { class: "settings-section",
                    h3 { "Encryption" }
                    match &*status.read() {
                        None => rsx! { p { class: "muted", "Checking…" } },
                        Some(Err(err)) => rsx! { p { class: "error", "{err}" } },
                        Some(Ok(status)) => rsx! { EncryptionSummary { status: status.clone() } },
                    }

                    if let Some(active) = verification() {
                        VerificationPanel { verification: active }
                    } else {
                        div { class: "settings-actions",
                            button { class: "primary", disabled: busy(), onclick: start_verification,
                                "Verify with another signed-in device"
                            }
                        }
                        form { class: "recovery-form", onsubmit: submit_recovery,
                            label { "Recovery key or passphrase"
                                input {
                                    r#type: "password",
                                    placeholder: "EsTx xxxx xxxx …",
                                    value: "{recovery_key}",
                                    oninput: move |e| recovery_key.set(e.value()),
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
                        for (symbol, name) in emojis {
                            li { key: "{name}", span { class: "emoji", "{symbol}" } span { class: "emoji-name", "{name}" } }
                        }
                    }
                    div { class: "settings-actions",
                        button { class: "primary", onclick: { let h = handle.clone(); move |_| run(h.confirm_match()) }, "They match" }
                        button { class: "danger", onclick: { let h = handle.clone(); move |_| run(h.mismatch()) }, "They don't match" }
                    }
                },
                VerificationStep::Confirmed => rsx! { p { "Waiting for your other device to confirm…" } },
                VerificationStep::Done => rsx! {
                    p { class: "ok", "Verified. Encrypted messages will now sync between your devices." }
                    button { class: "secondary", onclick: move |_| active.set(None), "Close" }
                },
                VerificationStep::Cancelled(reason) => rsx! {
                    p { class: "error", "Verification cancelled: {reason}" }
                    button { class: "secondary", onclick: move |_| active.set(None), "Close" }
                },
            }
        }
    }
}
