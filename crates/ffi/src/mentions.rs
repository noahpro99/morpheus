//! The Inbox's recent mentions (the desktop's `notify.rs` log behind `invites.rs`): the core's notification stream while the app runs,
//! kept as `view_model::notices` keeps it, the newest mentions first. Nothing is kept across runs, as on the desktop.

use std::sync::Arc;

use morpheus_view_model::notices;

use crate::{FfiError, Morpheus, TaskHandle, runtime};

/// A message that mentioned you.
#[derive(Clone, uniffi::Record)]
pub struct MentionItem {
    pub event_id: String,
    pub room_id: String,
    pub room_name: String,
    pub sender: String,
    /// What the message said, shortened.
    pub body: String,
    /// "alice in general" (a direct message reads as the sender's name alone).
    pub headline: String,
}

#[uniffi::export(with_foreign)]
pub trait MentionListener: Send + Sync {
    /// The recent mentions, newest first, whenever there is a new one.
    fn on_mentions(&self, items: Vec<MentionItem>);
}

#[uniffi::export]
impl Morpheus {
    /// Be told of the recent mentions now and as new ones arrive.
    pub fn watch_mentions(&self, listener: Arc<dyn MentionListener>) -> Result<Arc<TaskHandle>, FfiError> {
        let session = self.session()?;
        let task = runtime().spawn(async move {
            let Ok(mut updates) = session.notifications().await else { return };
            let mut log = Vec::new();
            while let Some(notice) = updates.next().await {
                notices::log_notice(&mut log, notice);
                let items = notices::recent_mentions(&log)
                    .into_iter()
                    .map(|n| MentionItem {
                        event_id: n.event_id.clone(),
                        room_id: n.room_id.to_string(),
                        room_name: n.room_name.clone(),
                        sender: n.sender_name.clone(),
                        body: n.body_summary.clone(),
                        headline: if n.is_dm { n.sender_name.clone() } else { morpheus_view_model::i18n::tf("%{who} in %{room}", &[("who", &n.sender_name), ("room", &n.room_name)]) },
                    })
                    .collect();
                listener.on_mentions(items);
            }
        });
        Ok(TaskHandle::new(task))
    }
}
