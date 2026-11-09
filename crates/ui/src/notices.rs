//! What a notification says and shows, shared by the desktop notifier and
//! the Android push service (which gets it from `apps/mobile/src/push.rs`).

use std::path::{Path, PathBuf};

use morpheus_core::{MediaRef, Session};

use crate::device_prefs::NotificationContent;

/// A message's notification as it is shown: worded for the room (a room of
/// two reads like a DM, by name alone) and for what this device lets it
/// show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeText {
    pub title: String,
    pub body: String,
    /// Whether the sender's picture may show.
    pub sender_picture: bool,
    /// Whether a photo sent may show.
    pub photo: bool,
}

pub fn notice_text(
    content: NotificationContent,
    sender: &str,
    room: &str,
    body: &str,
    is_dm: bool,
    is_mention: bool,
) -> NoticeText {
    match content {
        NotificationContent::Full => NoticeText {
            title: match (is_dm, is_mention) {
                (true, _) => sender.to_owned(),
                (false, true) => format!("{sender} mentioned you in {room}"),
                (false, false) => format!("{sender} in {room}"),
            },
            body: body.to_owned(),
            sender_picture: true,
            photo: true,
        },
        NotificationContent::Sender => NoticeText {
            title: sender.to_owned(),
            body: if is_mention {
                "Mentioned you".to_owned()
            } else {
                "Sent a message".to_owned()
            },
            sender_picture: true,
            photo: false,
        },
        NotificationContent::Hidden => NoticeText {
            title: "Morpheus".to_owned(),
            body: "Someone sent a message".to_owned(),
            sender_picture: false,
            photo: false,
        },
    }
}

/// Notification pictures: the sender's (small) and a photo sent (larger).
pub const ICON_SIZE: u32 = 96;
pub const IMAGE_SIZE: u32 = 512;

/// A picture for a notification, as a file (notification services read
/// pictures from disk). Kept by the media's key, so each is fetched once.
pub async fn picture_file(
    session: &Session,
    media: Option<MediaRef>,
    size: u32,
    data_dir: &Path,
) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let media = media?;
    let mut hasher = std::hash::DefaultHasher::new();
    (media.key(), size).hash(&mut hasher);
    let dir = data_dir.join("notification-pictures");
    let stem = format!("{:016x}", hasher.finish());
    for ext in ["png", "jpg", "gif", "webp"] {
        let kept = dir.join(format!("{stem}.{ext}"));
        if kept.exists() {
            return Some(kept);
        }
    }
    let fetched = session.thumbnail(media, size, size).await.ok().flatten()?;
    write_picture(&dir, &stem, &fetched)
}

/// A picture already fetched (a push's preview fetches its own), kept by
/// its contents.
pub fn keep_picture(data_dir: &Path, media: &morpheus_core::Media) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    media.bytes.hash(&mut hasher);
    let stem = format!("{:016x}", hasher.finish());
    write_picture(&data_dir.join("notification-pictures"), &stem, media)
}

fn write_picture(dir: &Path, stem: &str, fetched: &morpheus_core::Media) -> Option<PathBuf> {
    let ext = match fetched.mime() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => return None,
    };
    let path = dir.join(format!("{stem}.{ext}"));
    let written = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&path, &fetched.bytes));
    if let Err(err) = written {
        tracing::warn!(%err, "notification picture not kept");
        return None;
    }
    Some(path)
}

/// A message's notification, ready to show: its text, and its pictures as
/// files (only those the device's setting lets show).
pub struct ReadyNotice {
    pub text: NoticeText,
    pub icon: Option<PathBuf>,
    pub image: Option<PathBuf>,
}

pub async fn ready(
    session: &Session,
    notice: morpheus_core::Notification,
    data_dir: &Path,
) -> ReadyNotice {
    let text = notice_text(
        crate::device_prefs::notification_content(data_dir),
        &notice.sender_name,
        &notice.room_name,
        &notice.body_summary,
        notice.is_dm,
        notice.is_mention,
    );
    let icon = match text.sender_picture {
        true => picture_file(session, notice.sender_avatar, ICON_SIZE, data_dir).await,
        false => None,
    };
    let image = match text.photo {
        true => picture_file(session, notice.image, IMAGE_SIZE, data_dir).await,
        false => None,
    };
    ReadyNotice { text, icon, image }
}
