//! Messages announced while the app runs: what a system notification says
//! (`crates/ui/src/notices.rs`), the log behind the inbox's Mentions and the
//! test bridge, and the pictures a notification shows, kept as files.

use std::path::{Path, PathBuf};

use morpheus_core::{MediaRef, Notification, Session};

use crate::settings::NotificationContent;

/// How many announcements the log keeps.
pub const NOTICE_CAP: usize = 50;

/// A message's notification as it is shown: worded for the room (a room of
/// two reads like a DM, by name alone) and for what this device lets it show.
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

/// Add an announcement to the log, dropping the oldest past the cap.
pub fn log_notice(log: &mut Vec<Notification>, notice: Notification) {
    if log.len() >= NOTICE_CAP {
        log.remove(0);
    }
    log.push(notice);
}

/// The inbox's Mentions: the most recent first, at most ten.
pub fn recent_mentions(log: &[Notification]) -> Vec<&Notification> {
    log.iter().rev().filter(|n| n.is_mention).take(10).collect()
}

/// Whether a message is announced by the system: not when you are looking at
/// that very room, and only while this device has notifications on.
pub fn should_show(looking_at_room: bool) -> bool {
    !looking_at_room
}

/// What the notification-log line of a message reads: `sender: summary`.
pub fn log_line(notice: &Notification) -> String {
    format!("{}: {}", notice.sender_name, notice.body_summary)
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
    let ext = match fetched.mime() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => return None,
    };
    let path = dir.join(format!("{stem}.{ext}"));
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::write(&path, &fetched.bytes).ok()?;
    Some(path)
}

/// A message's notification, ready to show: its text, and its pictures as
/// files (only those the device's setting lets show).
pub struct ReadyNotice {
    pub text: NoticeText,
    pub icon: Option<PathBuf>,
    pub image: Option<PathBuf>,
}

pub async fn ready(session: &Session, notice: Notification, data_dir: &Path) -> ReadyNotice {
    let text = notice_text(
        crate::settings::notification_content(data_dir),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(n: usize, mention: bool) -> Notification {
        Notification {
            event_id: format!("$e{n}"),
            room_id: "!r:x".try_into().expect("room id"),
            room_name: "general".into(),
            sender_name: "alice".into(),
            body_summary: format!("message {n}"),
            is_mention: mention,
            sender_avatar: None,
            image: None,
            is_dm: false,
        }
    }

    #[test]
    fn titles_follow_the_room_and_the_setting() {
        let full = |dm, mention| {
            notice_text(NotificationContent::Full, "alice", "general", "hi", dm, mention).title
        };
        assert_eq!(full(true, false), "alice");
        assert_eq!(full(false, false), "alice in general");
        assert_eq!(full(false, true), "alice mentioned you in general");
        let sender = notice_text(NotificationContent::Sender, "alice", "general", "hi", false, true);
        assert_eq!((sender.title.as_str(), sender.body.as_str()), ("alice", "Mentioned you"));
        assert!(sender.sender_picture && !sender.photo);
        let hidden = notice_text(NotificationContent::Hidden, "alice", "general", "hi", false, false);
        assert_eq!(hidden.body, "Someone sent a message");
        assert!(!hidden.sender_picture && !hidden.photo);
    }

    #[test]
    fn the_log_keeps_the_newest_fifty() {
        let mut log = Vec::new();
        for n in 0..NOTICE_CAP + 5 {
            log_notice(&mut log, notice(n, false));
        }
        assert_eq!(log.len(), NOTICE_CAP);
        assert_eq!(log[0].event_id, "$e5");
    }

    #[test]
    fn mentions_come_newest_first_and_capped_at_ten() {
        let log: Vec<_> = (0..30).map(|n| notice(n, n % 2 == 0)).collect();
        let found = recent_mentions(&log);
        assert_eq!(found.len(), 10);
        assert_eq!(found[0].event_id, "$e28");
        assert!(found.iter().all(|n| n.is_mention));
    }

    #[test]
    fn a_line_names_the_sender() {
        assert_eq!(log_line(&notice(1, false)), "alice: message 1");
        assert!(should_show(false) && !should_show(true));
    }
}
