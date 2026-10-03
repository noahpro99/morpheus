use matrix_sdk::{
    media::{MediaFormat, MediaRequestParameters, MediaThumbnailSettings},
    ruma::{OwnedMxcUri, UInt, events::room::MediaSource},
};
use tracing::debug;

use crate::{CoreFuture, Session};

/// A reference to a piece of media (avatar, image, file) that the core can fetch.
///
/// Wraps the SDK's `MediaSource` so encrypted attachments work transparently.
/// Equality and hashing use the media's URI, which makes it a good cache key.
#[derive(Clone, Debug)]
pub struct MediaRef {
    key: String,
    source: MediaSource,
}

impl MediaRef {
    pub fn from_mxc(uri: OwnedMxcUri) -> Self {
        Self {
            key: uri.to_string(),
            source: MediaSource::Plain(uri),
        }
    }

    pub(crate) fn from_source(source: MediaSource) -> Self {
        let key = match &source {
            MediaSource::Plain(uri) => uri.to_string(),
            MediaSource::Encrypted(file) => file.url.to_string(),
        };
        Self { key, source }
    }

    /// Stable identity, suitable as a cache key.
    pub fn key(&self) -> &str {
        &self.key
    }
}

impl PartialEq for MediaRef {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl Eq for MediaRef {}

impl std::hash::Hash for MediaRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}

/// Fetched media bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Media {
    pub bytes: Vec<u8>,
}

impl Media {
    /// MIME type sniffed from the magic bytes; servers do not always say.
    pub fn mime(&self) -> &'static str {
        match self.bytes.as_slice() {
            [0x89, b'P', b'N', b'G', ..] => "image/png",
            [0xFF, 0xD8, 0xFF, ..] => "image/jpeg",
            [b'G', b'I', b'F', b'8', ..] => "image/gif",
            [
                b'R',
                b'I',
                b'F',
                b'F',
                _,
                _,
                _,
                _,
                b'W',
                b'E',
                b'B',
                b'P',
                ..,
            ] => "image/webp",
            _ => "application/octet-stream",
        }
    }
}

impl Session {
    /// Fetch a thumbnail at most `width`×`height`, falling back to the full
    /// file when the server cannot thumbnail (tiny images, SVGs, ...).
    /// The SDK caches results on disk.
    pub fn thumbnail(&self, media: MediaRef, width: u32, height: u32) -> CoreFuture<Option<Media>> {
        let client = self.client.clone();
        self.core.run(async move {
            let media_client = client.media();
            let settings = MediaThumbnailSettings::new(UInt::from(width), UInt::from(height));
            let thumbnail = MediaRequestParameters {
                source: media.source.clone(),
                format: MediaFormat::Thumbnail(settings),
            };

            let bytes = match media_client.get_media_content(&thumbnail, true).await {
                Ok(bytes) => bytes,
                Err(err) => {
                    debug!(%err, key = media.key, "thumbnail unavailable, fetching full file");
                    let full = MediaRequestParameters {
                        source: media.source,
                        format: MediaFormat::File,
                    };
                    match media_client.get_media_content(&full, true).await {
                        Ok(bytes) => bytes,
                        Err(err) => {
                            debug!(%err, "media fetch failed");
                            return Ok(None);
                        }
                    }
                }
            };
            Ok(Some(Media { bytes }))
        })
    }
}
