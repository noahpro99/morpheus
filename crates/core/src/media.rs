#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::path::PathBuf;

#[cfg(not(target_arch = "wasm32"))]
use crate::fs::nonblocking as fs;
#[cfg(not(target_arch = "wasm32"))]
use directories::UserDirs;
use matrix_sdk::{
    media::{MediaFormat, MediaRequestParameters, MediaThumbnailSettings},
    ruma::{OwnedMxcUri, OwnedRoomId, OwnedUserId, UInt, events::room::MediaSource, profile::AvatarUrl},
};
use tracing::{debug, info};

use crate::{CoreFuture, Session};

/// Overrides the Downloads directory; used by e2e tests so downloads land in
/// a disposable, inspectable location instead of the real user directory.
#[cfg(not(target_arch = "wasm32"))]
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
            // ISO media (MP4, QuickTime): a size, then "ftyp" and its brand.
            [_, _, _, _, b'f', b't', b'y', b'p', b'q', b't', ..] => "video/quicktime",
            [_, _, _, _, b'f', b't', b'y', b'p', ..] => "video/mp4",
            // Matroska and WebM.
            [0x1A, 0x45, 0xDF, 0xA3, ..] => "video/webm",
            _ => "application/octet-stream",
        }
    }
}

/// [`Session::thumbnail`], for a client without a session (a push's preview).
pub(crate) async fn fetch_thumbnail(
    client: &matrix_sdk::Client,
    media: MediaRef,
    width: u32,
    height: u32,
) -> Option<Media> {
    let media_client = client.media();
    let settings = MediaThumbnailSettings::new(UInt::from(width), UInt::from(height));
    let thumbnail = MediaRequestParameters {
        source: media.source.clone(),
        format: MediaFormat::Thumbnail(settings),
    };

    let bytes = match media_client.get_media_content(&thumbnail, true).await {
        Ok(bytes) => bytes,
        // Not on its server: the full file is not there either.
        Err(err)
            if err
                .as_client_api_error()
                .is_some_and(|e| e.status_code.as_u16() == 404) =>
        {
            debug!(%err, key = media.key, "media not found");
            return None;
        }
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
                    return None;
                }
            }
        }
    };
    Some(Media { bytes })
}


/// How long a person's picture that could not be had is not asked for again
/// (it is retried after that: a server may have been down).
const BROKEN_FOR: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// Pictures that could not be had lately (gone from their server, or not a
/// picture), by media key, and since when.
fn broken() -> &'static std::sync::Mutex<std::collections::HashMap<String, crate::rt::Instant>> {
    static BROKEN: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, crate::rt::Instant>>> =
        std::sync::OnceLock::new();
    BROKEN.get_or_init(Default::default)
}

/// People's pictures fetched at once, at most. A big room's member list and
/// timeline ask for dozens together; past this many they wait their turn
/// instead of all crowding the connection (#112).
const AVATAR_FETCHES: usize = 6;

fn avatar_turns() -> &'static tokio::sync::Semaphore {
    static TURNS: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    TURNS.get_or_init(|| tokio::sync::Semaphore::new(AVATAR_FETCHES))
}

/// `media` as a picture, unless it failed lately or fails now (fetching it,
/// or it is no picture at all), which is remembered for [`BROKEN_FOR`].
async fn avatar_picture(client: &matrix_sdk::Client, media: &MediaRef, width: u32, height: u32) -> Option<Media> {
    let known_broken = || {
        let mut known = broken().lock().ok()?;
        let at = *known.get(&media.key)?;
        if at.elapsed() < BROKEN_FOR {
            return Some(());
        }
        known.remove(&media.key);
        None
    };
    if known_broken().is_some() {
        return None;
    }
    // A turn first; one that failed while this one waited is not asked again.
    let _turn = avatar_turns().acquire().await.ok()?;
    if known_broken().is_some() {
        return None;
    }
    let found = fetch_thumbnail(client, media.clone(), width, height)
        .await
        .filter(|found| found.mime().starts_with("image/"));
    if found.is_none()
        && let Ok(mut known) = broken().lock()
    {
        known.insert(media.key.clone(), crate::rt::Instant::now());
    }
    found
}

impl Session {
    /// A person's picture, at most `width`×`height`: `media`, the one the
    /// caller shows them with (their member event's in a room, or their
    /// global profile's), else, when that one cannot be had (gone from its
    /// server, not a picture), the other one known for them: their member
    /// event in `room`, then their global profile. Every place a person's
    /// picture shows asks this, so a stale copy in one of the two does not
    /// leave them faceless there. What failed and what was used instead is
    /// logged.
    pub fn user_avatar(
        &self,
        user_id: OwnedUserId,
        room_id: Option<OwnedRoomId>,
        media: MediaRef,
        width: u32,
        height: u32,
    ) -> CoreFuture<Option<Media>> {
        let client = self.client.clone();
        self.core.run(async move {
            // Nothing of theirs loaded lately: not looked up again (their
            // profile is a request each time) until it is retried.
            let faceless = format!("{user_id} {}", media.key);
            if let Ok(known) = broken().lock()
                && known.get(&faceless).is_some_and(|at| at.elapsed() < BROKEN_FOR)
            {
                return Ok(None);
            }
            if let Some(found) = avatar_picture(&client, &media, width, height).await {
                return Ok(Some(found));
            }
            // The other pictures this person has: where the failed one came from
            // is whichever of them it is.
            let mut known: Vec<(&'static str, MediaRef)> = Vec::new();
            if let Some(room) = room_id.and_then(|id| client.get_room(&id))
                && let Ok(Some(member)) = room.get_member_no_sync(&user_id).await
                && let Some(url) = member.avatar_url()
            {
                known.push(("room member", MediaRef::from_mxc(url.to_owned())));
            }
            if let Ok(profile) = client.account().fetch_user_profile_of(&user_id).await
                && let Some(url) = profile.data.get_static::<AvatarUrl>().ok().flatten()
            {
                known.push(("global profile", MediaRef::from_mxc(url)));
            }
            let failed_source = known
                .iter()
                .find(|(_, other)| *other == media)
                .map_or("as shown", |(source, _)| *source);
            for (source, other) in known.iter().filter(|(_, other)| *other != media) {
                if let Some(found) = avatar_picture(&client, other, width, height).await {
                    info!(user = %user_id, failed = media.key, failed_source, used = other.key, used_source = source, "avatar unavailable; using the person's other picture");
                    return Ok(Some(found));
                }
            }
            info!(user = %user_id, failed = media.key, failed_source, "avatar unavailable, and no other picture of the person loads");
            if let Ok(mut known) = broken().lock() {
                known.insert(faceless, crate::rt::Instant::now());
            }
            Ok(None)
        })
    }
}
impl Session {
    /// Fetch a thumbnail at most `width`×`height`, falling back to the full
    /// file when the server cannot thumbnail (tiny images, SVGs, ...).
    /// The SDK caches results on disk.
    pub fn thumbnail(&self, media: MediaRef, width: u32, height: u32) -> CoreFuture<Option<Media>> {
        let client = self.client.clone();
        self.core
            .run(async move { Ok(fetch_thumbnail(&client, media, width, height).await) })
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

            #[cfg(target_arch = "wasm32")]
            return browser_download(&bytes, &suggested_name);
            #[cfg(not(target_arch = "wasm32"))]
            {
                let dir = downloads_dir();
                fs::create_dir_all(&dir).await?;
                let path = unique_path(&dir, &suggested_name).await;
                fs::write(&path, &bytes).await?;
                Ok(path)
            }
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

#[cfg(not(target_arch = "wasm32"))]
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
#[cfg(not(target_arch = "wasm32"))]
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

/// A browser saves a file the way it saves any download.
#[cfg(target_arch = "wasm32")]
fn browser_download(bytes: &[u8], name: &str) -> crate::Result<PathBuf> {
    use wasm_bindgen::JsCast;
    let fail = |what: &str| crate::Error::Io(std::io::Error::other(what.to_owned()));
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let blob = web_sys::Blob::new_with_u8_array_sequence(&parts).map_err(|_| fail("blob"))?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(|_| fail("blob url"))?;
    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or_else(|| fail("no document"))?;
    let link: web_sys::HtmlAnchorElement = document
        .create_element("a")
        .map_err(|_| fail("link"))?
        .dyn_into()
        .map_err(|_| fail("link"))?;
    link.set_href(&url);
    link.set_download(name);
    link.click();
    let _ = web_sys::Url::revoke_object_url(&url);
    Ok(PathBuf::from(name))
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
        crate::rt::SystemTime::now()
            .duration_since(crate::rt::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    }
}

/// How long a person's global picture (or its absence) is remembered by [`global_avatar`].
const GLOBAL_FOR: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// The picture in `user`'s global profile, for a DM whose room has none of its own (their
/// member event there carries no picture: they set theirs after being invited, or their
/// server does not copy it into the rooms). Remembered for [`GLOBAL_FOR`], absence too,
/// as the room list asks for every room on each update.
pub(crate) async fn global_avatar(client: &matrix_sdk::Client, user: &OwnedUserId) -> Option<OwnedMxcUri> {
    type Known = std::collections::HashMap<OwnedUserId, (crate::rt::Instant, Option<OwnedMxcUri>)>;
    static KNOWN: std::sync::OnceLock<std::sync::Mutex<Known>> = std::sync::OnceLock::new();
    let known = KNOWN.get_or_init(Default::default);
    if let Ok(known) = known.lock()
        && let Some((at, found)) = known.get(user)
        && at.elapsed() < GLOBAL_FOR
    {
        return found.clone();
    }
    let found = client
        .account()
        .fetch_user_profile_of(user)
        .await
        .ok()
        .and_then(|profile| profile.data.get_static::<AvatarUrl>().ok().flatten());
    if let Ok(mut known) = known.lock() {
        known.insert(user.clone(), (crate::rt::Instant::now(), found.clone()));
    }
    found
}
