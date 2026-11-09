use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex, atomic::Ordering},
};

use crate::rt::JoinHandle;
use futures_util::{StreamExt, pin_mut};
use matrix_sdk::{
    Client,
    encryption::verification::{
        QrVerification, QrVerificationData, QrVerificationState, SasState, SasVerification,
        VerificationRequest, VerificationRequestState,
    },
    ruma::{
        MilliSecondsSinceUnixEpoch,
        events::{
            ToDeviceEvent,
            key::verification::{
                VerificationMethod, request::ToDeviceKeyVerificationRequestEventContent,
            },
        },
    },
};
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::{Core, CoreFuture, Error, OwnedDeviceId, Session};

/// One of the current user's devices (including this one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: OwnedDeviceId,
    pub name: String,
    /// Locally trusted or signed by our cross-signing identity.
    pub verified: bool,
    /// Whether this is the device the running app is using.
    pub is_current: bool,
}

/// A step in a verification flow: emoji (short authentication string) or
/// QR code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationStep {
    /// Waiting on the other device: either we asked and it hasn't answered
    /// yet, or it asked and the UI hasn't accepted or declined yet.
    Requested,
    /// Both sides agreed to verify; the emoji exchange is about to start.
    Ready,
    /// The other device can scan: show it this code (or compare emoji
    /// instead, [`VerificationActions::use_emoji`]).
    ShowQr(QrImage),
    /// The other device scanned this one's code: confirm once it says so
    /// ([`VerificationActions::confirm_scanned`]).
    QrScanned,
    /// This device scanned the other's code; waiting for it to confirm.
    Reciprocated,
    /// Compare these seven emoji, in order, with the other device.
    Emojis(Vec<(String, String)>),
    /// The user said the emoji matched; waiting for the other side to do the same.
    Confirmed,
    /// The flow finished successfully; the other device is now verified.
    Done,
    /// The flow was cancelled, locally or by the other side, for `reason`.
    Cancelled(String),
}

/// A verification QR code: `width`×`width` modules, row by row, `true` for
/// dark; and the bytes it encodes (what a scanner reads).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrImage {
    pub width: usize,
    pub dark: Vec<bool>,
    pub data: Vec<u8>,
}

/// The methods this device offers: emoji, showing a QR code and answering
/// one scanned; scanning only with a camera to scan with.
fn our_methods(can_scan: bool) -> Vec<VerificationMethod> {
    let mut methods = vec![
        VerificationMethod::SasV1,
        VerificationMethod::QrCodeShowV1,
        VerificationMethod::ReciprocateV1,
    ];
    if can_scan {
        methods.push(VerificationMethod::QrCodeScanV1);
    }
    methods
}

/// A new incoming verification request from one of this user's own devices.
pub struct VerificationUpdates {
    rx: mpsc::UnboundedReceiver<VerificationHandle>,
}

impl VerificationUpdates {
    /// Next incoming request, or `None` once the session is gone.
    pub async fn next(&mut self) -> Option<VerificationHandle> {
        self.rx.recv().await
    }
}

/// Actions and live state for one verification flow, incoming or outgoing.
pub struct VerificationHandle {
    actions: VerificationActions,
    rx: mpsc::UnboundedReceiver<VerificationStep>,
    _forwarder: Arc<AbortOnDrop>,
}

/// The actions of a flow, separable from its step stream so a UI can keep
/// the receiver in one task and the buttons elsewhere. Clone freely.
#[derive(Clone)]
pub struct VerificationActions {
    core: Core,
    request: VerificationRequest,
    sas: Arc<StdMutex<Option<SasVerification>>>,
    qr: Arc<StdMutex<Option<QrVerification>>>,
    we_started: bool,
    can_scan: bool,
}

struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl VerificationHandle {
    /// Did this device start the flow (as opposed to receiving a request).
    pub fn we_started(&self) -> bool {
        self.actions.we_started
    }

    /// Next step of the flow, or `None` once it can no longer change.
    pub async fn next(&mut self) -> Option<VerificationStep> {
        self.rx.recv().await
    }

    /// The flow's actions, usable independently of this receiver.
    pub fn actions(&self) -> VerificationActions {
        self.actions.clone()
    }

    pub fn accept(&self) -> CoreFuture<()> {
        self.actions.accept()
    }

    pub fn confirm_match(&self) -> CoreFuture<()> {
        self.actions.confirm_match()
    }

    pub fn mismatch(&self) -> CoreFuture<()> {
        self.actions.mismatch()
    }

    pub fn cancel(&self) -> CoreFuture<()> {
        self.actions.cancel()
    }
}

impl VerificationActions {
    pub fn we_started(&self) -> bool {
        self.we_started
    }

    /// Accept an incoming request. Only meaningful when [`Self::we_started`] is `false`.
    pub fn accept(&self) -> CoreFuture<()> {
        let request = self.request.clone();
        let methods = our_methods(self.can_scan);
        self.core.run(async move {
            request.accept_with_methods(methods).await?;
            Ok(())
        })
    }

    /// Compare emoji instead of using the QR code shown.
    pub fn use_emoji(&self) -> CoreFuture<()> {
        let request = self.request.clone();
        self.core.run(async move {
            request.start_sas().await?;
            Ok(())
        })
    }

    /// The other device says it scanned this one's code: finish.
    pub fn confirm_scanned(&self) -> CoreFuture<()> {
        let qr = self.current_qr();
        self.core.run(async move {
            let qr = qr.ok_or(Error::VerificationNotReady)?;
            qr.confirm().await?;
            Ok(())
        })
    }

    /// The other device's code, as scanned by this one.
    pub fn scan_qr(&self, data: Vec<u8>) -> CoreFuture<()> {
        let request = self.request.clone();
        self.core.run(async move {
            let data = QrVerificationData::from_bytes(data)
                .map_err(|err| Error::Verification(err.to_string()))?;
            // The other device can show its code before its acceptance has
            // reached this one: give that a moment to arrive.
            let changes = request.changes();
            pin_mut!(changes);
            let _ = crate::rt::timeout(std::time::Duration::from_secs(10), async {
                while !request.is_ready() && changes.next().await.is_some() {}
            })
            .await;
            request
                .scan_qr_code(data)
                .await?
                .ok_or(Error::VerificationNotReady)?;
            Ok(())
        })
    }

    /// Confirm that the emoji matched on both sides.
    pub fn confirm_match(&self) -> CoreFuture<()> {
        let sas = self.current_sas();
        self.core.run(async move {
            let sas = sas.ok_or(Error::VerificationNotReady)?;
            sas.confirm().await?;
            Ok(())
        })
    }

    /// The emoji did not match: cancel the flow and flag it as a possible attack.
    pub fn mismatch(&self) -> CoreFuture<()> {
        let sas = self.current_sas();
        self.core.run(async move {
            let sas = sas.ok_or(Error::VerificationNotReady)?;
            sas.mismatch().await?;
            Ok(())
        })
    }

    /// Cancel the flow, at whatever stage it is in.
    pub fn cancel(&self) -> CoreFuture<()> {
        let sas = self.current_sas();
        let qr = self.current_qr();
        let request = self.request.clone();
        self.core.run(async move {
            if let Some(sas) = sas {
                sas.cancel().await?;
            } else if let Some(qr) = qr {
                qr.cancel().await?;
            } else {
                request.cancel().await?;
            }
            Ok(())
        })
    }

    fn current_sas(&self) -> Option<SasVerification> {
        self.sas.lock().ok().and_then(|guard| guard.clone())
    }

    fn current_qr(&self) -> Option<QrVerification> {
        self.qr.lock().ok().and_then(|guard| guard.clone())
    }
}

impl VerificationHandle {
    /// Wrap a freshly created or received `VerificationRequest`, spawning the
    /// task that drives it and forwards [`VerificationStep`]s to the UI.
    fn spawn(core: Core, request: VerificationRequest, we_started: bool, can_scan: bool) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let sas: Arc<StdMutex<Option<SasVerification>>> = Arc::new(StdMutex::new(None));
        let qr: Arc<StdMutex<Option<QrVerification>>> = Arc::new(StdMutex::new(None));

        let forwarder = core.spawn({
            let request = request.clone();
            let (sas, qr) = (sas.clone(), qr.clone());
            async move {
                drive_request(request, we_started, &tx, &sas, &qr).await;
            }
        });

        Self {
            actions: VerificationActions {
                core,
                request,
                sas,
                qr,
                we_started,
                can_scan,
            },
            rx,
            _forwarder: Arc::new(AbortOnDrop(forwarder)),
        }
    }
}

/// Follow a verification request from whatever state it is currently in
/// through to completion, forwarding [`VerificationStep`]s as they happen.
async fn drive_request(
    request: VerificationRequest,
    we_started: bool,
    tx: &mpsc::UnboundedSender<VerificationStep>,
    sas_slot: &StdMutex<Option<SasVerification>>,
    qr_slot: &StdMutex<Option<QrVerification>>,
) {
    let changes = request.changes();
    pin_mut!(changes);
    let mut pending = Some(request.state());
    // A QR flow runs beside the request, which can still turn to emoji.
    let mut qr_flow: Option<AbortOnDrop> = None;

    loop {
        let state = match pending.take() {
            Some(state) => state,
            None => match changes.next().await {
                Some(state) => state,
                None => return,
            },
        };

        debug!(we_started, ?state, "verification request state");
        match state {
            VerificationRequestState::Created { .. }
            | VerificationRequestState::Requested { .. } => {
                if tx.send(VerificationStep::Requested).is_err() {
                    return;
                }
            }
            VerificationRequestState::Ready { their_methods, .. } => {
                if tx.send(VerificationStep::Ready).is_err() {
                    return;
                }
                // The other device can scan: show it a code (the request
                // moves on to the QR flow), emoji staying a button away.
                if their_methods.contains(&VerificationMethod::QrCodeScanV1) {
                    match request.generate_qr_code().await {
                        Ok(Some(_)) => continue,
                        Ok(None) => {}
                        Err(err) => warn!(%err, "could not make a verification QR code"),
                    }
                }
                // Only the side that did not initiate the request starts the
                // concrete SAS flow; otherwise both sides would race.
                if !we_started && let Err(err) = request.start_sas().await {
                    warn!(%err, "could not start SAS verification");
                }
            }
            VerificationRequestState::Transitioned { verification } => {
                if let Some(qr) = verification.clone().qr() {
                    if let Ok(mut slot) = qr_slot.lock() {
                        *slot = Some(qr.clone());
                    }
                    let tx = tx.clone();
                    qr_flow = Some(AbortOnDrop(crate::rt::spawn(async move {
                        drive_qr(qr, &tx).await;
                    })));
                    continue;
                }
                if let Some(sas) = verification.sas() {
                    // Emoji after all: the code shown is done with.
                    drop(qr_flow.take());
                    if let Ok(mut slot) = qr_slot.lock() {
                        *slot = None;
                    }
                    if let Ok(mut slot) = sas_slot.lock() {
                        *slot = Some(sas.clone());
                    }
                    drive_sas(sas, tx).await;
                } else {
                    debug!("verification transitioned into an unsupported flow");
                }
                return;
            }
            VerificationRequestState::Done => {
                let _ = tx.send(VerificationStep::Done);
                return;
            }
            VerificationRequestState::Cancelled(info) => {
                let _ = tx.send(VerificationStep::Cancelled(info.reason().to_owned()));
                return;
            }
        }
    }
}

/// Follow a QR flow through to completion: this device's code shown and
/// scanned, or the other's scanned by this one.
async fn drive_qr(qr: QrVerification, tx: &mpsc::UnboundedSender<VerificationStep>) {
    let changes = qr.changes();
    pin_mut!(changes);
    let mut pending = Some(qr.state());

    loop {
        let state = match pending.take() {
            Some(state) => state,
            None => match changes.next().await {
                Some(state) => state,
                None => return,
            },
        };

        debug!(?state, "qr verification state");
        let step = match state {
            QrVerificationState::Started => match qr_image(&qr) {
                Some(image) => VerificationStep::ShowQr(image),
                None => continue,
            },
            QrVerificationState::Scanned => VerificationStep::QrScanned,
            QrVerificationState::Reciprocated => VerificationStep::Reciprocated,
            QrVerificationState::Confirmed => VerificationStep::Confirmed,
            QrVerificationState::Done { .. } => {
                let _ = tx.send(VerificationStep::Done);
                return;
            }
            QrVerificationState::Cancelled(info) => {
                let _ = tx.send(VerificationStep::Cancelled(info.reason().to_owned()));
                return;
            }
        };
        if tx.send(step).is_err() {
            return;
        }
    }
}

fn qr_image(qr: &QrVerification) -> Option<QrImage> {
    let code = qr
        .to_qr_code()
        .inspect_err(|err| warn!(%err, "verification QR code not drawn"))
        .ok()?;
    Some(QrImage {
        width: code.width(),
        dark: code
            .to_colors()
            .into_iter()
            .map(|color| color.select(true, false))
            .collect(),
        data: qr.to_bytes().ok()?,
    })
}

/// Follow a concrete SAS flow through to completion, forwarding
/// [`VerificationStep`]s as they happen.
async fn drive_sas(sas: SasVerification, tx: &mpsc::UnboundedSender<VerificationStep>) {
    let changes = sas.changes();
    pin_mut!(changes);
    let mut pending = Some(sas.state());

    loop {
        let state = match pending.take() {
            Some(state) => state,
            None => match changes.next().await {
                Some(state) => state,
                None => return,
            },
        };

        debug!(we_started = sas.we_started(), ?state, "sas state");
        match state {
            SasState::Started { .. } => {
                // `Started` means the other side sent the start; we must accept
                // before keys are exchanged. (`sas.we_started()` tracks who
                // initiated the *request*, so it cannot be used here.)
                if let Err(err) = sas.accept().await {
                    warn!(%err, "could not accept SAS verification");
                }
            }
            SasState::Created { .. } | SasState::Accepted { .. } => {}
            SasState::KeysExchanged { emojis, .. } => {
                let Some(emojis) = emojis else {
                    // We only advertise support for the emoji method.
                    continue;
                };
                let emojis = emojis
                    .emojis
                    .iter()
                    .map(|emoji| (emoji.symbol.to_owned(), emoji.description.to_owned()))
                    .collect();
                if tx.send(VerificationStep::Emojis(emojis)).is_err() {
                    return;
                }
            }
            SasState::Confirmed => {
                if tx.send(VerificationStep::Confirmed).is_err() {
                    return;
                }
            }
            SasState::Done { .. } => {
                let _ = tx.send(VerificationStep::Done);
                return;
            }
            SasState::Cancelled(info) => {
                let _ = tx.send(VerificationStep::Cancelled(info.reason().to_owned()));
                return;
            }
        }
    }
}

impl Session {
    /// Subscribe to verification requests arriving from this user's other
    /// devices. The handler stays registered for the lifetime of the session.
    pub fn verification_requests(&self) -> CoreFuture<VerificationUpdates> {
        let core = self.core.clone();
        let client = self.client.clone();
        let can_scan = self.can_scan_qr.clone();
        self.core.run(async move {
            let (tx, rx) = mpsc::unbounded_channel();

            client.add_event_handler(
                move |ev: ToDeviceEvent<ToDeviceKeyVerificationRequestEventContent>,
                      client: Client| {
                    let tx = tx.clone();
                    let core = core.clone();
                    let can_scan = can_scan.load(Ordering::Relaxed);
                    async move {
                        // Only our own other devices can self-verify with us.
                        let sender: &matrix_sdk::ruma::UserId = &ev.sender;
                        if client.user_id() != Some(sender) {
                            return;
                        }

                        match client
                            .encryption()
                            .get_verification_request(&ev.sender, &ev.content.transaction_id)
                            .await
                        {
                            Some(request) => {
                                let handle =
                                    VerificationHandle::spawn(core, request, false, can_scan);
                                if tx.send(handle).is_err() {
                                    debug!("verification request consumer dropped, stopping");
                                }
                            }
                            None => debug!(
                                "incoming verification request vanished before it could be loaded"
                            ),
                        }
                    }
                },
            );

            Ok(VerificationUpdates { rx })
        })
    }

    /// Start verifying this device against another one of this user's
    /// devices. Picks whichever other device was active most recently (an
    /// account can accumulate many old, now-offline devices over time, and a
    /// request sent to one of those would never be seen); a future revision
    /// could let the UI choose explicitly when there is more than one.
    pub fn request_self_verification(&self) -> CoreFuture<VerificationHandle> {
        let core = self.core.clone();
        let client = self.client.clone();
        let can_scan = self.can_scan_qr.load(Ordering::Relaxed);
        self.core.run(async move {
            let own_user_id = client.user_id().ok_or(Error::NoSession)?.to_owned();
            let current_device_id = client.device_id().ok_or(Error::NoSession)?.to_owned();

            let recency: HashMap<OwnedDeviceId, MilliSecondsSinceUnixEpoch> = client
                .devices()
                .await
                .map_err(matrix_sdk::Error::from)?
                .devices
                .into_iter()
                .filter_map(|device| Some((device.device_id, device.last_seen_ts?)))
                .collect();

            let devices = client.encryption().get_user_devices(&own_user_id).await?;
            let mut others: Vec<_> = devices
                .devices()
                .filter(|device| device.device_id() != current_device_id)
                .collect();
            others.sort_by(|a, b| {
                let a_seen = recency.get(a.device_id());
                let b_seen = recency.get(b.device_id());
                b_seen
                    .cmp(&a_seen)
                    .then_with(|| a.device_id().cmp(b.device_id()))
            });

            let target = others.into_iter().next().ok_or(Error::NoOtherDevice)?;
            let request = target
                .request_verification_with_methods(our_methods(can_scan))
                .await?;

            Ok(VerificationHandle::spawn(core, request, true, can_scan))
        })
    }

    /// Whether this device can scan QR codes (has a camera the app reads).
    /// Verification then offers the other device's code to scan as well;
    /// off by default, as nothing would read the code the other device shows.
    pub fn set_can_scan_qr(&self, can: bool) {
        self.can_scan_qr.store(can, Ordering::Relaxed);
    }

    /// This user's devices, current one first, then alphabetical by name.
    pub fn own_devices(&self) -> CoreFuture<Vec<Device>> {
        let client = self.client.clone();
        self.core.run(async move {
            let own_user_id = client.user_id().ok_or(Error::NoSession)?.to_owned();
            let current_device_id = client.device_id().map(ToOwned::to_owned);

            let devices = client.encryption().get_user_devices(&own_user_id).await?;
            let mut list: Vec<Device> = devices
                .devices()
                .map(|device| {
                    let is_current = current_device_id.as_deref() == Some(device.device_id());
                    Device {
                        id: device.device_id().to_owned(),
                        name: device
                            .display_name()
                            .filter(|name| !name.is_empty())
                            .map(ToOwned::to_owned)
                            .unwrap_or_else(|| device.device_id().to_string()),
                        verified: device.is_verified(),
                        is_current,
                    }
                })
                .collect();
            list.sort_by(|a, b| {
                b.is_current
                    .cmp(&a.is_current)
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
            Ok(list)
        })
    }
}
