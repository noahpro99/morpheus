//! Encryption on this device: where it stands, the recovery key, and
//! verification against another device. The wording and the buttons of each
//! step are `morpheus_view_model::verification::panel`, the desktop's.

use std::sync::{Arc, Mutex};

use morpheus_core::{RecoveryAvailability, VerificationActions, VerificationHandle};
use morpheus_view_model::verification::{self, Action, Kind, Tone};

use crate::{FfiError, Morpheus, TaskHandle, runtime};

/// Whether encrypted history can be read here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RecoveryKind {
    /// Not known yet: the first sync is still settling.
    Unknown,
    /// Backup and secret storage are set up and this device has the keys.
    Enabled,
    /// A backup exists; entering the recovery key unlocks it.
    NeedsKey,
    /// The account has no recovery set up.
    Disabled,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SecurityStatus {
    pub device_id: Option<String>,
    /// Verified by another device or by the recovery key.
    pub device_verified: bool,
    pub recovery: RecoveryKind,
    /// How many of the account's other devices there are (to verify with).
    pub other_devices: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum VerificationAction {
    Accept,
    Cancel,
    UseEmoji,
    ConfirmScanned,
    ConfirmMatch,
    Mismatch,
    /// Close a finished or cancelled panel (nothing to tell the core).
    Dismiss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Danger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PanelTone {
    Plain,
    Ok,
    Error,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PanelButton {
    pub label: String,
    pub kind: ButtonKind,
    pub action: VerificationAction,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PanelEmoji {
    pub symbol: String,
    pub name: String,
}

/// A code to scan: `size` x `size` modules, row by row, `true` is dark, with
/// the quiet border already in.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PanelCode {
    pub size: u32,
    pub modules: Vec<bool>,
}

/// What the verification screen shows at one step of the flow.
#[derive(Debug, Clone, uniffi::Record)]
pub struct VerificationPanel {
    pub text: String,
    pub tone: PanelTone,
    pub emojis: Vec<PanelEmoji>,
    pub qr: Option<PanelCode>,
    pub buttons: Vec<PanelButton>,
    /// The flow succeeded.
    pub finished: bool,
}

#[uniffi::export(with_foreign)]
pub trait VerificationListener: Send + Sync {
    fn on_panel(&self, panel: VerificationPanel);
}

/// Another device of this account asks to verify this one.
#[uniffi::export(with_foreign)]
pub trait IncomingVerificationListener: Send + Sync {
    fn on_incoming(&self, flow: Arc<VerificationFlow>);
}

fn panel_of(step: &morpheus_core::VerificationStep, incoming: bool) -> VerificationPanel {
    let panel = verification::panel(step, incoming);
    VerificationPanel {
        text: panel.text,
        tone: match panel.tone {
            Tone::Plain => PanelTone::Plain,
            Tone::Ok => PanelTone::Ok,
            Tone::Error => PanelTone::Error,
        },
        emojis: panel
            .emojis
            .into_iter()
            .map(|(symbol, name)| PanelEmoji { symbol, name })
            .collect(),
        qr: panel.qr.as_ref().map(|code| {
            let (size, modules) = verification::qr_modules(code);
            PanelCode {
                size: size as u32,
                modules,
            }
        }),
        buttons: panel
            .buttons
            .into_iter()
            .map(|button| PanelButton {
                label: button.label.to_owned(),
                kind: match button.kind {
                    Kind::Primary => ButtonKind::Primary,
                    Kind::Secondary => ButtonKind::Secondary,
                    Kind::Danger => ButtonKind::Danger,
                },
                action: match button.action {
                    Action::Accept => VerificationAction::Accept,
                    Action::Cancel => VerificationAction::Cancel,
                    Action::UseEmoji => VerificationAction::UseEmoji,
                    Action::ConfirmScanned => VerificationAction::ConfirmScanned,
                    Action::ConfirmMatch => VerificationAction::ConfirmMatch,
                    Action::Mismatch => VerificationAction::Mismatch,
                    Action::Dismiss => VerificationAction::Dismiss,
                },
            })
            .collect(),
        finished: verification::is_done(step),
    }
}

#[derive(Default)]
struct Shown {
    latest: Option<VerificationPanel>,
    listener: Option<Arc<dyn VerificationListener>>,
}

/// One verification, ours or the other device's. Watch it for the panel to
/// show; the buttons call [`VerificationFlow::act`].
#[derive(uniffi::Object)]
pub struct VerificationFlow {
    actions: VerificationActions,
    shown: Arc<Mutex<Shown>>,
}

impl VerificationFlow {
    fn start(mut handle: VerificationHandle) -> Arc<Self> {
        let incoming = !handle.we_started();
        let shown = Arc::new(Mutex::new(Shown::default()));
        let flow = Arc::new(Self {
            actions: handle.actions(),
            shown: shown.clone(),
        });
        runtime().spawn(async move {
            while let Some(step) = handle.next().await {
                let panel = panel_of(&step, incoming);
                let listener = match shown.lock() {
                    Ok(mut shown) => {
                        shown.latest = Some(panel.clone());
                        shown.listener.clone()
                    }
                    Err(_) => return,
                };
                if let Some(listener) = listener {
                    listener.on_panel(panel);
                }
            }
        });
        flow
    }
}

#[uniffi::export]
impl VerificationFlow {
    /// Get the current panel now and every change after it.
    pub fn watch(&self, listener: Arc<dyn VerificationListener>) -> Arc<TaskHandle> {
        let latest = match self.shown.lock() {
            Ok(mut shown) => {
                shown.listener = Some(listener.clone());
                shown.latest.clone()
            }
            Err(_) => None,
        };
        let task = runtime().spawn(async move {
            if let Some(panel) = latest {
                listener.on_panel(panel);
            }
        });
        TaskHandle::new(task)
    }

    pub async fn act(&self, action: VerificationAction) -> Result<(), FfiError> {
        match action {
            VerificationAction::Accept => self.actions.accept().await?,
            VerificationAction::Cancel => self.actions.cancel().await?,
            VerificationAction::UseEmoji => self.actions.use_emoji().await?,
            VerificationAction::ConfirmScanned => self.actions.confirm_scanned().await?,
            VerificationAction::ConfirmMatch => self.actions.confirm_match().await?,
            VerificationAction::Mismatch => self.actions.mismatch().await?,
            VerificationAction::Dismiss => {}
        }
        Ok(())
    }
}

#[uniffi::export]
impl Morpheus {
    pub async fn security_status(&self) -> Result<SecurityStatus, FfiError> {
        let session = self.session()?;
        let status = session.encryption_status().await?;
        let devices = session.own_devices().await.unwrap_or_default();
        Ok(SecurityStatus {
            device_id: status.device_id.map(|id| id.to_string()),
            device_verified: status.device_verified,
            recovery: match status.recovery {
                RecoveryAvailability::Unknown => RecoveryKind::Unknown,
                RecoveryAvailability::Enabled => RecoveryKind::Enabled,
                RecoveryAvailability::NeedsKey => RecoveryKind::NeedsKey,
                RecoveryAvailability::Disabled => RecoveryKind::Disabled,
            },
            other_devices: devices.iter().filter(|d| !d.is_current).count() as u32,
        })
    }

    /// Unlock the key backup with the recovery key (or passphrase).
    pub async fn recover(&self, recovery_key: String) -> Result<(), FfiError> {
        self.session()?.recover(recovery_key).await?;
        Ok(())
    }

    /// Set recovery up; the new key, to be shown once.
    pub async fn enable_recovery(&self) -> Result<String, FfiError> {
        Ok(self.session()?.enable_recovery().await?)
    }

    /// Ask one of this account's other devices to verify this one.
    pub async fn request_verification(&self) -> Result<Arc<VerificationFlow>, FfiError> {
        let handle = self.session()?.request_self_verification().await?;
        Ok(VerificationFlow::start(handle))
    }

    /// Be told when another device of this account asks to verify this one.
    pub async fn watch_incoming_verifications(
        &self,
        listener: Arc<dyn IncomingVerificationListener>,
    ) -> Result<Arc<TaskHandle>, FfiError> {
        let mut updates = self.session()?.verification_requests().await?;
        let task = runtime().spawn(async move {
            while let Some(handle) = updates.next().await {
                listener.on_incoming(VerificationFlow::start(handle));
            }
        });
        Ok(TaskHandle::new(task))
    }
}
