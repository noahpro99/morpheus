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
