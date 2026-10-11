use std::path::{Path, PathBuf};

use directories::UserDirs;
use matrix_sdk::{
    media::{MediaFormat, MediaRequestParameters, MediaThumbnailSettings},
    ruma::{OwnedMxcUri, UInt, events::room::MediaSource},
};
use tokio::fs;
use tracing::debug;

use crate::{CoreFuture, Session};

/// Overrides the Downloads directory; used by e2e tests so downloads land in
/// a disposable, inspectable location instead of the real user directory.
const DOWNLOAD_DIR_ENV: &str = "MORPHEUS_DOWNLOAD_DIR";

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

    /// Download the full file behind `media` into the user's Downloads
    /// directory (or `MORPHEUS_DOWNLOAD_DIR`, if set), suffixing the file
    /// name with `(1)`, `(2)`, ... when it already exists. Returns the path
    /// actually written.
    pub fn download(&self, media: MediaRef, suggested_name: String) -> CoreFuture<PathBuf> {
        let client = self.client.clone();
        self.core.run(async move {
            let media_client = client.media();
            let request = MediaRequestParameters {
                source: media.source,
                format: MediaFormat::File,
            };
            let bytes = media_client.get_media_content(&request, true).await?;

            let dir = downloads_dir();
            fs::create_dir_all(&dir).await?;
            let path = unique_path(&dir, &suggested_name).await;
            fs::write(&path, &bytes).await?;
            Ok(path)
        })
    }


    /// Fetch the full-resolution file for a piece of media, e.g. for a
    /// lightbox view. The SDK caches the result on disk.
    pub fn media_file(&self, media: MediaRef) -> CoreFuture<Option<Media>> {
        let client = self.client.clone();
        self.core.run(async move {
            let media_client = client.media();
            let key = media.key.clone();
            let request = MediaRequestParameters {
                source: media.source,
                format: MediaFormat::File,
            };
            match media_client.get_media_content(&request, true).await {
                Ok(bytes) => Ok(Some(Media { bytes })),
                Err(err) => {
                    debug!(%err, key, "full media fetch failed");
                    Ok(None)
                }
            }
        })
    }
}

fn downloads_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(DOWNLOAD_DIR_ENV) {
        return PathBuf::from(dir);
    }
    UserDirs::new()
        .and_then(|dirs| dirs.download_dir().map(Path::to_path_buf))
        .unwrap_or_else(std::env::temp_dir)
}

/// `dir/suggested_name`, or `dir/<stem> (1).<ext>`, `(2)`, ... if that name
/// is already taken. Strips any path components from `suggested_name` so a
/// crafted file name cannot escape `dir`.
async fn unique_path(dir: &Path, suggested_name: &str) -> PathBuf {
    let safe_name = Path::new(suggested_name)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "attachment".to_owned());

    let named = Path::new(&safe_name);
    let stem = named
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "attachment".to_owned());
    let extension = named.extension().map(|s| s.to_string_lossy().into_owned());

    let mut candidate = dir.join(&safe_name);
    let mut attempt = 1u32;
    while fs::try_exists(&candidate).await.unwrap_or(false) {
        let name = match &extension {
            Some(extension) => format!("{stem} ({attempt}).{extension}"),
            None => format!("{stem} ({attempt})"),
        };
        candidate = dir.join(name);
        attempt += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn first_download_keeps_the_suggested_name() {
        let dir = std::env::temp_dir().join(format!("morpheus-media-test-{}", uuid()));
        tokio::fs::create_dir_all(&dir).await.unwrap_or_default();

        let path = unique_path(&dir, "notes.txt").await;
        assert_eq!(path, dir.join("notes.txt"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn repeated_downloads_suffix_instead_of_colliding() {
        let dir = std::env::temp_dir().join(format!("morpheus-media-test-{}", uuid()));
        tokio::fs::create_dir_all(&dir).await.unwrap_or_default();
        tokio::fs::write(dir.join("notes.txt"), b"first")
            .await
            .unwrap_or_default();
        tokio::fs::write(dir.join("notes (1).txt"), b"second")
            .await
            .unwrap_or_default();

        let path = unique_path(&dir, "notes.txt").await;
        assert_eq!(path, dir.join("notes (2).txt"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn path_components_in_the_suggested_name_are_stripped() {
        let dir = std::env::temp_dir().join(format!("morpheus-media-test-{}", uuid()));
        tokio::fs::create_dir_all(&dir).await.unwrap_or_default();

        let path = unique_path(&dir, "../../etc/passwd").await;
        assert_eq!(path, dir.join("passwd"));

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    fn uuid() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    }
}
