//! Incoming calls: someone starts a call in a direct message and the phone
//! rings (the core's `ring_updates`: `m.rtc.notification` and the declines).
//! A ring ends when its time is up, someone declines it (this account on
//! another device, or the person called), the caller leaves the call, or this
//! account joins it.

use std::sync::Arc;
use std::time::Duration;

use morpheus_core::{IncomingCall, OwnedEventId, OwnedRoomId, Ringing, Session};

use crate::{FfiError, Morpheus, TaskHandle, runtime};

/// A call ringing this account.
#[derive(Debug, Clone, uniffi::Record)]
pub struct IncomingRing {
    pub room_id: String,
    /// The ring's event: what a decline answers.
    pub notification: String,
    pub caller_id: String,
    /// The caller's name, or the start of their ID.
    pub caller: String,
    /// When it stops ringing, milliseconds since the Unix epoch.
    pub rings_until_ms: u64,
}

#[uniffi::export(with_foreign)]
pub trait RingListener: Send + Sync {
    /// Someone is calling.
    fn on_ring(&self, ring: IncomingRing);
    /// The ring in `notification` is over (declined, answered, given up, out of time).
    fn on_ring_over(&self, room_id: String, notification: String);
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// Whether the ring should stop: its time is up, the caller left the call, or this account is in it.
pub fn ring_is_over(now: u64, call: &IncomingCall, waited: Duration, in_call: &[String], me: Option<&str>) -> bool {
    now >= call.rings_until
        || me.is_some_and(|me| in_call.iter().any(|u| u == me))
        // A moment for the caller's own membership to arrive, then: gone, they gave up.
        || (waited > Duration::from_secs(5) && !in_call.iter().any(|u| u == call.caller.as_str()))
}

async fn follow(session: Session, call: IncomingCall, declined: Arc<std::sync::Mutex<std::collections::HashSet<String>>>, listener: Arc<dyn RingListener>) {
    let me = session.user_id().map(|u| u.to_string());
    let started = tokio::time::Instant::now();
    loop {
        let in_call: Vec<String> = session
            .call_participants(call.room_id.clone())
            .await
            .unwrap_or_default()
            .iter()
            .map(|u| u.to_string())
            .collect();
        let gone = declined.lock().is_ok_and(|d| d.contains(call.notification.as_str()));
        if gone || ring_is_over(now_ms(), &call, started.elapsed(), &in_call, me.as_deref()) {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    listener.on_ring_over(call.room_id.to_string(), call.notification.to_string());
}

#[uniffi::export]
impl Morpheus {
    /// Start telling `listener` about calls ringing this account.
    pub async fn watch_rings(&self, listener: Arc<dyn RingListener>) -> Result<Arc<TaskHandle>, FfiError> {
        let session = self.session()?;
        let mut rings = session.ring_updates().await?;
        let task = runtime().spawn(async move {
            let declined = Arc::new(std::sync::Mutex::new(std::collections::HashSet::new()));
            let mut following: Vec<tokio::task::JoinHandle<()>> = Vec::new();
            while let Some(ring) = rings.next().await {
                match ring {
                    Ringing::Declined { notification, .. } => {
                        if let Ok(mut declined) = declined.lock() {
                            declined.insert(notification.to_string());
                        }
                    }
                    Ringing::Incoming(call) => {
                        let caller = match session.profile(call.caller.clone()).await {
                            Ok(profile) => profile.name,
                            Err(_) => call.caller.localpart().to_owned(),
                        };
                        listener.on_ring(IncomingRing {
                            room_id: call.room_id.to_string(),
                            notification: call.notification.to_string(),
                            caller_id: call.caller.to_string(),
                            caller,
                            rings_until_ms: call.rings_until,
                        });
                        following.retain(|t| !t.is_finished());
                        following.push(tokio::spawn(follow(session.clone(), call, declined.clone(), listener.clone())));
                    }
                }
            }
        });
        Ok(TaskHandle::new(task))
    }

    /// Decline a ring: the caller hears, and this account's other devices stop ringing.
    pub async fn decline_call(&self, room_id: String, notification: String) -> Result<(), FfiError> {
        let room: OwnedRoomId = room_id.parse().map_err(|_| FfiError::Failed { reason: "not a room id".into() })?;
        let event: OwnedEventId = notification.parse().map_err(|_| FfiError::Failed { reason: "not an event id".into() })?;
        self.session()?.decline_call(room, event).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(until: u64) -> IncomingCall {
        IncomingCall {
            room_id: "!r:localhost".try_into().expect("room"),
            caller: "@bob:localhost".try_into().expect("user"),
            notification: "$n".try_into().expect("event"),
            rings_until: until,
        }
    }

    #[test]
    fn a_ring_ends_when_its_time_is_up() {
        let bob = vec!["@bob:localhost".to_owned()];
        assert!(!ring_is_over(1000, &call(2000), Duration::from_secs(1), &bob, Some("@alice:localhost")));
        assert!(ring_is_over(2000, &call(2000), Duration::from_secs(1), &bob, Some("@alice:localhost")));
    }

    #[test]
    fn a_ring_ends_when_this_account_is_in_the_call() {
        let both = vec!["@bob:localhost".to_owned(), "@alice:localhost".to_owned()];
        assert!(ring_is_over(1000, &call(2000), Duration::from_secs(1), &both, Some("@alice:localhost")));
    }

    #[test]
    fn a_caller_who_left_gave_up_but_is_given_a_moment_to_arrive() {
        assert!(!ring_is_over(1000, &call(2000), Duration::from_secs(2), &[], Some("@alice:localhost")));
        assert!(ring_is_over(1000, &call(2000), Duration::from_secs(6), &[], Some("@alice:localhost")));
    }
}
