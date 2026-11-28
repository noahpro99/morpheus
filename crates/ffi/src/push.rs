//! Notifications through UnifiedPush: the pusher the homeserver sends to,
//! and the words of one pushed message decrypted without the app running.

use morpheus_core::{Core, SessionStoreKind};
use morpheus_view_model::{
    notices::device_notice,
    settings::{self, NotificationContent},
};

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
    let core = Core::with_session_store(data_dir.clone(), SessionStoreKind::File).ok()?;
    let preview = core.preview_notification(room_id, event_id).await.ok()??;
    // What this device's settings allow: nothing at all when notifications are off here.
    let dir = std::path::Path::new(&data_dir);
    // A call rings whatever the content setting says; a message follows it, and nothing shows when notifications are off here.
    let text = match device_notice(dir, &preview.sender, &preview.room, &preview.body, preview.is_dm, preview.is_mention) {
        Some(text) => text,
        None if preview.is_call => morpheus_view_model::notices::notice_text(NotificationContent::Full, &preview.sender, &preview.room, &preview.body, preview.is_dm, preview.is_mention),
        None => return None,
    };
    Some(PushNotice {
        title: text.title,
        body: text.body,
        sender: preview.sender,
        call: preview.is_call,
        icon: preview.sender_avatar.filter(|_| text.sender_picture).map(|m| m.bytes),
        image: preview.image.filter(|_| text.photo).map(|m| m.bytes),
    })
}

/// Whether this device shows notifications for messages (the Notifications setting; on unless turned off).
#[uniffi::export]
pub fn push_enabled(data_dir: String) -> bool {
    settings::notifications_enabled(std::path::Path::new(&data_dir))
}

/// Whether a message that notifies makes a sound on this device.
#[uniffi::export]
pub fn push_sounds(data_dir: String) -> bool {
    settings::message_sounds_enabled(std::path::Path::new(&data_dir))
}

/// A notification's Reply, with no app running: sends `text` to the room from the saved session.
#[uniffi::export]
pub async fn reply_from_push(data_dir: String, room_id: String, text: String) -> Result<(), FfiError> {
    let core = Core::with_session_store(data_dir, SessionStoreKind::File)?;
    core.reply_from_push(room_id, text).await?;
    Ok(())
}

/// A notification's Mark as read, with no app running: a read receipt for the room's newest event.
#[uniffi::export]
pub async fn read_from_push(data_dir: String, room_id: String, event_id: String) -> Result<(), FfiError> {
    let core = Core::with_session_store(data_dir, SessionStoreKind::File)?;
    core.read_from_push(room_id, event_id).await?;
    Ok(())
}

/// One message in a room's notification stack.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct PushLine {
    pub sender: String,
    pub text: String,
    pub at_ms: i64,
    pub event_id: String,
}

/// The most messages a room's notification stacks.
const STACK_LINES: usize = 8;

/// The file of a room's stack: a push often wakes a fresh process, so the stack lives on disk (in the app's files dir).
fn stack_file(data_dir: &str, room_id: &str) -> std::path::PathBuf {
    let name: String = room_id.bytes().map(|b| format!("{b:02x}")).collect();
    std::path::Path::new(data_dir).join("push-stack").join(format!("{name}.json"))
}

fn read_stack(data_dir: &str, room_id: &str) -> Vec<PushLine> {
    std::fs::read(stack_file(data_dir, room_id)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// Add `line` to the room's stack (a message already in it, by event id, is not added twice), keep the newest 8, and
/// return the stack, oldest first.
#[uniffi::export]
pub fn push_stack_add(data_dir: String, room_id: String, line: PushLine) -> Vec<PushLine> {
    let mut lines = read_stack(&data_dir, &room_id);
    if line.event_id.is_empty() || !lines.iter().any(|l| l.event_id == line.event_id) {
        lines.push(line);
    }
    let extra = lines.len().saturating_sub(STACK_LINES);
    lines.drain(..extra);
    let file = stack_file(&data_dir, &room_id);
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec(&lines) {
        let _ = std::fs::write(file, bytes);
    }
    lines
}

/// The room is read, answered or open: its stack goes.
#[uniffi::export]
pub fn push_stack_clear(data_dir: String, room_id: String) {
    let _ = std::fs::remove_file(stack_file(&data_dir, &room_id));
}

/// The event id of the newest message in the room's stack, or empty (what Mark as read sends a receipt for).
#[uniffi::export]
pub fn push_stack_newest(data_dir: String, room_id: String) -> String {
    read_stack(&data_dir, &room_id).iter().rev().find(|l| !l.event_id.is_empty()).map(|l| l.event_id.clone()).unwrap_or_default()
}

#[cfg(test)]
mod stack_tests {
    use super::*;

    fn line(n: usize) -> PushLine {
        PushLine { sender: "bob".into(), text: format!("m{n}"), at_ms: n as i64, event_id: format!("$e{n}") }
    }

    #[test]
    fn the_stack_survives_a_new_process_keeps_eight_and_goes_when_cleared() {
        let dir = tempfile::tempdir().expect("dir").keep().display().to_string();
        let room = "!room:server".to_string();
        for n in 1..=10 {
            // (each call reads the file: what a fresh process does)
            push_stack_add(dir.clone(), room.clone(), line(n));
        }
        let stack = push_stack_add(dir.clone(), room.clone(), line(10));
        assert_eq!(stack.len(), 8, "capped at 8, a repeat not added twice");
        assert_eq!(stack.first().map(|l| l.text.as_str()), Some("m3"));
        assert_eq!(stack.last().map(|l| l.text.as_str()), Some("m10"));
        assert_eq!(push_stack_newest(dir.clone(), room.clone()), "$e10");
        assert!(push_stack_add(dir.clone(), "!other:server".into(), line(1)).len() == 1, "rooms have their own stacks");
        push_stack_clear(dir.clone(), room.clone());
        assert_eq!(push_stack_newest(dir.clone(), room.clone()), "");
        assert_eq!(push_stack_add(dir, room, line(11)).len(), 1);
    }
}
