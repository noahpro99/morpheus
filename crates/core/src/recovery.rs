//! Key backup recovery and the encryption status shown in settings.

use matrix_sdk::encryption::{VerificationState, recovery::RecoveryState};

use crate::{CoreFuture, OwnedDeviceId, Session};

/// Where this device stands with respect to end-to-end encryption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptionStatus {
    pub device_id: Option<OwnedDeviceId>,
    /// This device is cross-signed (verified by another device or recovery).
    pub device_verified: bool,
    /// Whether encrypted history can be restored from the server-side backup.
    pub recovery: RecoveryAvailability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAvailability {
    /// Not determined yet (sync still catching up).
    Unknown,
    /// Backup and secret storage are set up and this device has the keys.
    Enabled,
    /// A backup exists on the server but this device cannot read it yet;
    /// entering the recovery key fixes that.
    NeedsKey,
    /// The account has no recovery set up at all.
    Disabled,
}

impl Session {
    pub fn encryption_status(&self) -> CoreFuture<EncryptionStatus> {
        let client = self.client.clone();
        self.core.run(async move {
            let encryption = client.encryption();
            let device_verified = matches!(
                encryption.verification_state().get(),
                VerificationState::Verified
            );
            let recovery = match encryption.recovery().state() {
                RecoveryState::Unknown => RecoveryAvailability::Unknown,
                RecoveryState::Enabled => RecoveryAvailability::Enabled,
                RecoveryState::Incomplete => RecoveryAvailability::NeedsKey,
                RecoveryState::Disabled => {
                    // The local state says "disabled" until secrets are seen;
                    // a backup on the server means a key would still help.
                    if encryption
                        .backups()
                        .fetch_exists_on_server()
                        .await
                        .unwrap_or(false)
                    {
                        RecoveryAvailability::NeedsKey
                    } else {
                        RecoveryAvailability::Disabled
                    }
                }
            };
            Ok(EncryptionStatus {
                device_id: client.device_id().map(ToOwned::to_owned),
                device_verified,
                recovery,
            })
        })
    }

    /// Turn on key backup and recovery for an account that has none, and
    /// return the new recovery key for the user to keep. Sets up
    /// cross-signing first if the account has none, so the key also restores
    /// this identity on a new device.
    pub fn enable_recovery(&self) -> CoreFuture<String> {
        let client = self.client.clone();
        self.core.run(async move {
            let encryption = client.encryption();
            bootstrap_cross_signing(&client).await;
            let key = encryption
                .recovery()
                .enable()
                .wait_for_backups_to_upload()
                .await?;
            Ok(key)
        })
    }

    /// Replace the recovery key (lost or never saved), as Element's "Change
    /// recovery key": needs this device to hold the account's secrets
    /// (recovery enabled here). Returns the new key; the old one stops working.
    pub fn change_recovery_key(&self) -> CoreFuture<String> {
        let client = self.client.clone();
        self.core.run(async move {
            let recovery = client.encryption().recovery();
            let key = recovery.reset_key().await?;
            Ok(key)
        })
    }

    /// Restore cross-signing and room keys with a recovery key (or passphrase).
    /// Encrypted history becomes readable as keys are downloaded.
    pub fn recover(&self, recovery_key: String) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            client
                .encryption()
                .recovery()
                .recover(recovery_key.trim())
                .await?;
            Ok(())
        })
    }
}

impl Session {
    /// Give the account a cross-signing identity if it has none (this device
    /// becomes its first verified one). A no-op otherwise.
    pub fn ensure_cross_signing(&self) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            bootstrap_cross_signing(&client).await;
            Ok(())
        })
    }
}

/// Accounts that already have cross-signing (or need a password to change
/// it) carry on without.
async fn bootstrap_cross_signing(client: &matrix_sdk::Client) {
    if let Err(err) = client
        .encryption()
        .bootstrap_cross_signing_if_needed(None)
        .await
    {
        tracing::debug!(%err, "cross-signing not bootstrapped");
    }
}

/// A brand-new account's first sign-in (as Element does it): give the
/// account its cross-signing identity from this device, which so becomes
/// its first verified device; nothing else could verify it. Only when the
/// server has no identity for the account (an existing one is never
/// replaced). If the server wants the password for it, `password` (the one
/// just signed in with) is given.
pub(crate) async fn set_up_first_device(client: &matrix_sdk::Client, password: Option<String>) {
    use matrix_sdk::ruma::api::client::uiaa;
    let encryption = client.encryption();
    let Err(err) = encryption.bootstrap_cross_signing_if_needed(None).await else {
        return;
    };
    let (Some(info), Some(password), Some(user_id)) =
        (err.as_uiaa_response(), password, client.user_id())
    else {
        tracing::warn!(%err, "first device: cross-signing not set up");
        return;
    };
    let mut auth = uiaa::Password::new(
        uiaa::UserIdentifier::Matrix(uiaa::MatrixUserIdentifier::new(user_id.to_string())),
        password,
    );
    auth.session = info.session.clone();
    if let Err(err) = encryption
        .bootstrap_cross_signing_if_needed(Some(uiaa::AuthData::Password(auth)))
        .await
    {
        tracing::warn!(%err, "first device: cross-signing not set up (with the password)");
    }
}
