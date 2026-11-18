use morpheus_core::{MediaRef, Session};

/// A picture (avatar, image) the app can fetch when it draws it.
#[derive(uniffi::Object)]
pub struct MediaHandle {
    media: MediaRef,
    session: Session,
}

impl MediaHandle {
    pub(crate) fn new(media: MediaRef, session: &Session) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            media,
            session: session.clone(),
        })
    }
}

#[uniffi::export]
impl MediaHandle {
    /// A stable identity, for the app's image cache.
    pub fn key(&self) -> String {
        self.media.key().to_owned()
    }

    /// The image's bytes (PNG, JPEG, ...) at most `width`×`height` px, or the
    /// file whole when the server cannot scale it; `None` when it cannot be
    /// fetched. The core caches on disk.
    pub async fn thumbnail(&self, width: u32, height: u32) -> Option<Vec<u8>> {
        self.session
            .thumbnail(self.media.clone(), width, height)
            .await
            .ok()
            .flatten()
            .map(|media| media.bytes)
    }
}
