//! The accounts of this device: who is signed in (name and picture), the others stored beside, and switching
//! between them (the core keeps them; this does not touch how it stores sessions).

use std::sync::Arc;

use morpheus_view_model::menu::account_menu;

use crate::{Account, FfiError, MediaHandle, Morpheus, menus::MenuRow};

/// The signed-in person as they show: name and picture.
#[derive(uniffi::Record)]
pub struct OwnProfile {
    pub user_id: String,
    pub name: String,
    pub avatar: Option<Arc<MediaHandle>>,
    /// The picture's key, for the disk cache (it can be drawn before there is a session).
    pub avatar_key: Option<String>,
}

/// An account stored on this device.
#[derive(Clone, uniffi::Record)]
pub struct AccountItem {
    pub key: String,
    pub user_id: String,
    pub homeserver: String,
    /// The one signed in now.
    pub active: bool,
}

#[uniffi::export]
impl Morpheus {
    pub async fn own_profile(&self) -> Result<OwnProfile, FfiError> {
        let session = self.session()?;
        let me = session.user_id().ok_or(FfiError::SignedOut)?;
        let profile = session.profile(me.clone()).await?;
        Ok(OwnProfile {
            user_id: me.to_string(),
            name: profile.name,
            avatar_key: profile.avatar.as_ref().map(|m| m.key().to_owned()),
            avatar: profile.avatar.map(|m| MediaHandle::new(m, &session)),
        })
    }

    /// My status message (empty for none).
    pub async fn own_status(&self) -> Result<String, FfiError> {
        Ok(self.session()?.own_status().await?.unwrap_or_default())
    }

    /// Set my status message; empty clears it.
    pub async fn set_status(&self, text: String) -> Result<(), FfiError> {
        Ok(self.session()?.set_status(morpheus_view_model::room_dialogs::status_to_save(&text)).await?)
    }

    /// Every account with a session on this device.
    pub async fn saved_accounts(&self) -> Result<Vec<AccountItem>, FfiError> {
        Ok(self
            .core
            .accounts()
            .await?
            .into_iter()
            .map(|a| AccountItem { key: a.key, user_id: a.user_id.to_string(), homeserver: a.homeserver, active: a.active })
            .collect())
    }

    /// Make another stored account the signed-in one.
    pub async fn switch_account(&self, key: String) -> Result<Account, FfiError> {
        if let Ok(current) = self.session() {
            let _ = current.suspend().await;
        }
        let session = self
            .core
            .switch_account(key)
            .await?
            .ok_or_else(|| FfiError::Failed { reason: "that account has no session to open".into() })?;
        // What was kept of the last account's lists is not this one's.
        let _ = std::fs::remove_file(self.dir.join("sidebar-cache.json"));
        crate::timeline_cache::forget(&self.dir);
        if let Ok(mut rooms) = self.rooms_now.lock() {
            rooms.clear();
        }
        Ok(self.keep(session))
    }

    /// The menu of the You button's long press: the desktop's account menu (the accounts to switch to, adding one, signing out).
    pub async fn account_menu_rows(&self) -> Result<Vec<MenuRow>, FfiError> {
        let session = self.session()?;
        let me = session.user_id().ok_or(FfiError::SignedOut)?;
        let accounts = self.core.accounts().await?;
        let items = account_menu(&me, &accounts);
        Ok(self.store_menu(&items))
    }
}

/// One of this account's devices, for the Security page.
#[derive(Clone, uniffi::Record)]
pub struct DeviceItem {
    pub id: String,
    pub name: String,
    pub verified: bool,
    pub is_current: bool,
}

#[uniffi::export]
impl Morpheus {
    /// Change my display name (empty clears it).
    pub async fn set_own_name(&self, name: String) -> Result<(), FfiError> {
        Ok(self.session()?.set_display_name(name).await?)
    }

    /// Make a picture (the bytes of an image file, named for its type) my picture.
    pub async fn set_own_picture(&self, file_name: String, bytes: Vec<u8>) -> Result<(), FfiError> {
        if morpheus_view_model::room_dialogs::picture_mime(&file_name).is_none() {
            return Err(FfiError::Failed { reason: morpheus_view_model::i18n::t(morpheus_view_model::room_dialogs::NOT_A_PICTURE) });
        }
        let path = self.dir.join(format!("own-picture-{}", file_name.replace('/', "_")));
        std::fs::write(&path, bytes).map_err(|e| FfiError::Failed { reason: e.to_string() })?;
        let done = self.session()?.set_avatar(path.clone()).await;
        let _ = std::fs::remove_file(&path);
        Ok(done?)
    }

    /// Take my picture off.
    pub async fn remove_own_picture(&self) -> Result<(), FfiError> {
        Ok(self.session()?.remove_avatar().await?)
    }

    /// Where the homeserver lets this account be managed (its sign-in provider's account page), when it has one.
    pub async fn own_manage_url(&self) -> Result<Option<String>, FfiError> {
        Ok(self.session()?.account_management_url().await?)
    }

    /// This account's devices, this one first.
    pub async fn own_device_list(&self) -> Result<Vec<DeviceItem>, FfiError> {
        Ok(self
            .session()?
            .own_devices()
            .await?
            .into_iter()
            .map(|d| DeviceItem { id: d.id.to_string(), name: d.name, verified: d.verified, is_current: d.is_current })
            .collect())
    }

    /// Replace the recovery key (this device holds the account's secrets); the new key, shown once.
    pub async fn change_recovery_key(&self) -> Result<String, FfiError> {
        Ok(self.session()?.change_recovery_key().await?)
    }

    /// The recovery key is lost: reset the encryption identity (the account password when the server asks); the new key.
    pub async fn reset_recovery_key(&self, password: String) -> Result<String, FfiError> {
        Ok(self.session()?.reset_lost_recovery((!password.is_empty()).then_some(password)).await?)
    }
}

/// What a homeserver lets you sign in with.
#[derive(Clone, uniffi::Record)]
pub struct SignInFlows {
    pub password: bool,
    /// Single sign-on (the provider's own page, in the browser).
    pub sso: bool,
}

/// Shows a web page (the phone's browser).
#[uniffi::export(with_foreign)]
pub trait UrlLauncher: Send + Sync {
    /// Open `url`; false when nothing could.
    fn open(&self, url: String) -> bool;
}

#[uniffi::export]
impl Morpheus {
    /// How `homeserver` (a name or an address) can be signed in to.
    pub async fn login_flows(&self, homeserver: String) -> Result<SignInFlows, FfiError> {
        let flows = self.core.login_flows(homeserver).await?;
        Ok(SignInFlows { password: flows.password, sso: flows.sso })
    }

    /// Sign in through the homeserver's single sign-on page: `opener` shows it in the browser, which sends the phone's own loopback address
    /// the login token; this returns once that came.
    pub async fn login_sso(&self, homeserver: String, opener: std::sync::Arc<dyn UrlLauncher>) -> Result<Account, FfiError> {
        let open: morpheus_core::UrlOpener = std::sync::Arc::new(move |url: &str| {
            if opener.open(url.to_owned()) { Ok(()) } else { Err(std::io::Error::other("no browser")) }
        });
        let session = self.core.login_sso(homeserver, Some(open)).await?;
        Ok(self.keep(session))
    }
}
