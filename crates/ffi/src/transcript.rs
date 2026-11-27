//! Copying several messages at once (#417): the transcript the desktop copies, from the messages the phone selected.

use morpheus_view_model::transcript::{self, Body, Item};

use crate::timeline::{FileKind, MessageBody, MessageItem};

/// The text of `messages` (oldest first): each with its sender and time, then what it says.
#[uniffi::export]
pub fn transcript_of(messages: Vec<MessageItem>) -> String {
    let items: Vec<Item> = messages
        .iter()
        .map(|m| Item {
            sender: m.sender_name.clone(),
            timestamp_ms: m.timestamp_ms,
            reply_to: m.reply_to.as_ref().and_then(|r| r.sender_name.clone()),
            body: match &m.body {
                MessageBody::Text { text, .. } => Body::Text(text.clone()),
                MessageBody::Image { caption, .. } => Body::Image(caption.clone()),
                MessageBody::File { name, kind: FileKind::File, .. } => Body::File(name.clone()),
                MessageBody::File { name, kind: FileKind::Video, .. } => Body::Video(name.clone()),
                MessageBody::File { name, kind: FileKind::Audio, .. } => Body::Audio(name.clone()),
                MessageBody::Other { summary } => Body::Text(summary.clone()),
            },
        })
        .collect();
    transcript::transcript(&items)
}

/// A clip's length as a player shows it (`1:07`).
#[uniffi::export]
pub fn duration_label(ms: u64) -> String {
    morpheus_view_model::attachments::duration_label(ms)
}
