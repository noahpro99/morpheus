//! Notifications through UnifiedPush: the pusher the homeserver sends to,
//! and the words of one pushed message decrypted without the app running.

use morpheus_core::{Core, SessionStoreKind};
use morpheus_view_model::{notices::notice_text, settings::NotificationContent};

use crate::{FfiError, Morpheus};

/// What a notification says.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PushNotice {
    pub title: String,
    pub body: String,
    pub sender: String,
    /// Someone is calling.
    pub call: bool,
    /// The sender's picture (PNG/JPEG bytes), when allowed and known.
    pub icon: Option<Vec<u8>>,
    /// A photo sent, when allowed.
    pub image: Option<Vec<u8>>,
}

#[uniffi::export]
impl Morpheus {
    /// Have the homeserver send notifications to the UnifiedPush `endpoint`;
    /// the Matrix gateway it goes through.
    pub async fn register_push(&self, endpoint: String, device_name: String) -> Result<String, FfiError> {
        Ok(self.session()?.register_push(endpoint, device_name).await?)
    }

    pub async fn unregister_push(&self, endpoint: String) -> Result<(), FfiError> {
        self.session()?.unregister_push(endpoint).await?;
        Ok(())
    }
}

/// Word one pushed message for a notification: decrypts it with the saved
/// session in `data_dir`, without starting sync. `None` when there is
/// nothing to show (filtered out, redacted, not found, nobody signed in).
#[uniffi::export]
pub async fn preview_push(data_dir: String, room_id: String, event_id: String) -> Option<PushNotice> {
    let core = Core::with_session_store(data_dir, SessionStoreKind::File).ok()?;
    let preview = core.preview_notification(room_id, event_id).await.ok()??;
    let text = notice_text(
        NotificationContent::Full,
        &preview.sender,
        &preview.room,
        &preview.body,
        preview.is_dm,
        preview.is_mention,
    );
    Some(PushNotice {
        title: text.title,
        body: text.body,
        sender: preview.sender,
        call: preview.is_call,
        icon: preview.sender_avatar.filter(|_| text.sender_picture).map(|m| m.bytes),
        image: preview.image.filter(|_| text.photo).map(|m| m.bytes),
    })
}
