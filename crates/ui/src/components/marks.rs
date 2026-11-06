//! Private marks on rooms and people, kept in account data (see
//! `morpheus_core::marks`): pinned and closed DMs, ignored people, and mutes
//! that end. Loaded once, changed from the DM menu.

use std::collections::HashMap;

use dioxus::prelude::*;
use morpheus_core::{NotifyMode, OwnedRoomId, Session, marks};

use super::toast::report;

pub type Map = HashMap<String, i64>;

#[derive(Clone, Copy)]
pub struct Marks {
    pub pinned: Signal<Map>,
    pub closed: Signal<Map>,
    pub ignored: Signal<Map>,
    pub muted_until: Signal<Map>,
    /// Mutes changed here, which the room list only learns of with the
    /// room's next change.
    pub muted_now: Signal<HashMap<String, bool>>,
}

impl Marks {
    /// Whether a room is muted, counting changes made here.
    pub fn is_muted(&self, room: &morpheus_core::RoomSummary) -> bool {
        let key = room.id.to_string();
        self.muted_now
            .read()
            .get(&key)
            .copied()
            .unwrap_or(room.muted)
            || self.muted_until.read().contains_key(&key)
    }
}

impl Marks {
    fn signal(self, kind: &'static str) -> Signal<Map> {
        match kind {
            marks::PINNED => self.pinned,
            marks::CLOSED => self.closed,
            marks::IGNORED => self.ignored,
            _ => self.muted_until,
        }
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Install in the shell: loads every map, and ends mutes whose time is up.
pub fn provide_marks(session: Session) -> Marks {
    let marks = use_context_provider(|| Marks {
        pinned: Signal::new(Map::new()),
        closed: Signal::new(Map::new()),
        ignored: Signal::new(Map::new()),
        muted_until: Signal::new(Map::new()),
        muted_now: Signal::new(HashMap::new()),
    });
    use_future({
        let session = session.clone();
        move || {
            let session = session.clone();
            async move {
                for kind in [
                    marks::PINNED,
                    marks::CLOSED,
                    marks::IGNORED,
                    marks::MUTED_UNTIL,
                ] {
                    match session.marks(kind).await {
                        Ok(map) => marks.signal(kind).set(map),
                        Err(err) => report("loading your marks", &err),
                    }
                }
            }
        }
    });
    use_future(move || {
        let session = session.clone();
        async move {
            loop {
                let now = now_ms();
                let ended: Vec<String> = marks
                    .muted_until
                    .peek()
                    .iter()
                    .filter(|(_, until)| **until > 0 && **until <= now)
                    .map(|(room, _)| room.clone())
                    .collect();
                for room in ended {
                    if let Ok(room_id) = OwnedRoomId::try_from(room.as_str()) {
                        unmute(&session, room_id);
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    });
    marks
}

/// Set or clear one mark, here at once and in the account.
pub fn set_mark(session: &Session, kind: &'static str, key: String, at: Option<i64>) {
    let marks = consume_context::<Marks>();
    let mut signal = marks.signal(kind);
    signal.with_mut(|map| match at {
        Some(at) => {
            map.insert(key.clone(), at);
        }
        None => {
            map.remove(&key);
        }
    });
    let future = session.set_mark(kind, key, at);
    spawn(async move {
        if let Err(err) = future.await {
            report("saving", &err);
        }
    });
}

/// Mute a room until `until` (milliseconds since the epoch; 0 for good).
pub fn mute_until(session: &Session, room_id: OwnedRoomId, until: i64) {
    consume_context::<Marks>()
        .muted_now
        .write()
        .insert(room_id.to_string(), true);
    let future = session.set_notify_mode(room_id.clone(), NotifyMode::Mute);
    spawn(async move {
        if let Err(err) = future.await {
            report("mute", &err);
        }
    });
    set_mark(
        session,
        marks::MUTED_UNTIL,
        room_id.to_string(),
        Some(until),
    );
}

/// Hear a room again.
pub fn unmute(session: &Session, room_id: OwnedRoomId) {
    consume_context::<Marks>()
        .muted_now
        .write()
        .insert(room_id.to_string(), false);
    let future = session.set_notify_mode(room_id.clone(), NotifyMode::AllMessages);
    spawn(async move {
        if let Err(err) = future.await {
            report("unmute", &err);
        }
    });
    set_mark(session, marks::MUTED_UNTIL, room_id.to_string(), None);
}
