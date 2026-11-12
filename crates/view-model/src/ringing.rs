//! How long an incoming call rings, for the app's own ringing outside the
//! window (the ringtone with the window closed to the tray;
//! `crates/ui/src/ringing.rs`): until its time is up, it is declined, the
//! caller has left the call, or this account is in it (answered here or on
//! another device).

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use morpheus_core::{IncomingCall, Session};

/// Rings declined (here or on another device): theirs stop.
pub type Declined = Arc<Mutex<HashSet<String>>>;

/// Whether a ring is over, from what is known of the call: its time is up,
/// it was declined, this account is in the call, or (after a few seconds, for
/// the caller's own membership to arrive) the caller has gone.
pub fn over(
    now_ms: u64,
    call: &IncomingCall,
    declined: bool,
    me_in_call: bool,
    caller_in_call: bool,
    seconds_ringing: u64,
) -> bool {
    now_ms >= call.rings_until || declined || me_in_call || (seconds_ringing > 5 && !caller_in_call)
}

/// Call `tick` every other second while `call` rings; returns once it doesn't.
pub async fn while_ringing(
    session: &Session,
    call: &IncomingCall,
    declined: &Declined,
    mut tick: impl FnMut(),
) {
    let me = session.user_id();
    let started = morpheus_core::rt::Instant::now();
    loop {
        let now = morpheus_core::rt::SystemTime::now()
            .duration_since(morpheus_core::rt::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or_default();
        let was_declined = declined
            .lock()
            .is_ok_and(|d| d.contains(call.notification.as_str()));
        let in_call = session
            .call_participants(call.room_id.clone())
            .await
            .unwrap_or_default();
        if over(
            now,
            call,
            was_declined,
            me.as_ref().is_some_and(|me| in_call.contains(me)),
            in_call.contains(&call.caller),
            started.elapsed().as_secs(),
        ) {
            return;
        }
        tick();
        morpheus_core::rt::sleep(std::time::Duration::from_secs(2)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(until: u64) -> IncomingCall {
        IncomingCall {
            room_id: "!r:x".try_into().expect("room"),
            caller: "@a:x".try_into().expect("user"),
            notification: "$n".try_into().expect("event"),
            rings_until: until,
        }
    }

    #[test]
    fn a_ring_ends_for_each_reason() {
        let c = call(1000);
        assert!(!over(500, &c, false, false, true, 1));
        assert!(over(1000, &c, false, false, true, 1), "time is up");
        assert!(over(500, &c, true, false, true, 1), "declined");
        assert!(over(500, &c, false, true, true, 1), "answered here");
        assert!(!over(500, &c, false, false, false, 3), "the caller's membership is still on its way");
        assert!(over(500, &c, false, false, false, 6), "the caller gave up");
    }
}
