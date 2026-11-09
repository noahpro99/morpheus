use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::rt::AbortHandle;
use tokio::sync::mpsc;
use tracing::debug;

use crate::{CoreFuture, Error, OwnedRoomId, OwnedUserId, Session, TimelineHandle};

/// How long a typing notice stays alive without being renewed.
const TYPING_TIMEOUT: Duration = Duration::from_secs(4);

/// Display names of the people currently typing in a room (never includes
/// the current user).
pub struct TypingUpdates {
    rx: mpsc::UnboundedReceiver<Vec<String>>,
}

impl TypingUpdates {
    pub async fn next(&mut self) -> Option<Vec<String>> {
        self.rx.recv().await
    }
}

impl Session {
    /// Subscribe to typing notifications for a room.
    pub fn typing(&self, room_id: OwnedRoomId) -> CoreFuture<TypingUpdates> {
        let core = self.core.clone();
        let client = self.client.clone();
        self.core.run(async move {
            let room = client
                .get_room(&room_id)
                .ok_or_else(|| Error::UnknownRoom(room_id.clone()))?;
            let (guard, mut typing) = room.subscribe_to_typing_notifications();
            let (tx, rx) = mpsc::unbounded_channel();

            core.spawn(async move {
                let _guard = guard;
                let mut names: HashMap<OwnedUserId, String> = HashMap::new();
                loop {
                    let users = match typing.recv().await {
                        Ok(users) => users,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    };
                    let mut resolved = Vec::with_capacity(users.len());
                    for user in users {
                        if !names.contains_key(&user) {
                            let name = match room.get_member_no_sync(&user).await {
                                Ok(Some(member)) => member.name().to_owned(),
                                _ => user.localpart().to_owned(),
                            };
                            names.insert(user.clone(), name);
                        }
                        resolved.push(names[&user].clone());
                    }
                    if tx.send(resolved).is_err() {
                        debug!(%room_id, "typing consumer dropped, stopping");
                        break;
                    }
                }
            });

            Ok(TypingUpdates { rx })
        })
    }
}

/// Debounced "I am typing" state for one timeline. Call [`Self::renew`] on
/// every keystroke and [`Self::stop`] on send; the notice expires by itself.
#[derive(Clone, Default)]
pub struct TypingNotice {
    stop_timer: Arc<Mutex<Option<AbortHandle>>>,
}

impl TimelineHandle {
    /// Tell the room the user is typing. The SDK rate-limits the request;
    /// a stop is scheduled automatically unless renewed.
    pub fn renew_typing(&self) -> CoreFuture<()> {
        let timeline = self.timeline.clone();
        let core = self.core.clone();
        let stop_timer = self.typing.stop_timer.clone();
        self.core.run(async move {
            timeline.room().typing_notice(true).await?;

            let stop = core.spawn({
                let timeline = timeline.clone();
                async move {
                    crate::rt::sleep(TYPING_TIMEOUT).await;
                    let _ = timeline.room().typing_notice(false).await;
                }
            });
            if let Some(previous) = stop_timer
                .lock()
                .map(|mut slot| slot.replace(stop.abort_handle()))
                .ok()
                .flatten()
            {
                previous.abort();
            }
            Ok(())
        })
    }

    /// Clear the typing notice immediately (e.g. after sending).
    pub fn stop_typing(&self) -> CoreFuture<()> {
        let timeline = self.timeline.clone();
        if let Ok(mut slot) = self.typing.stop_timer.lock()
            && let Some(timer) = slot.take()
        {
            timer.abort();
        }
        self.core.run(async move {
            timeline.room().typing_notice(false).await?;
            Ok(())
        })
    }
}
