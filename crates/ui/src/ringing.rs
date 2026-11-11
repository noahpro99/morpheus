//! How long an incoming call rings, for the app's own ringing outside the
//! window (a desktop's ringtone in the tray, a phone's call through
//! Android): until its time is up, it is declined, the caller has left the
//! call, or this account is in it (answered here or on another device).

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use morpheus_core::{IncomingCall, Session};

/// Rings declined (here or on another device): theirs stop.
pub type Declined = Arc<Mutex<HashSet<String>>>;

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
        if now >= call.rings_until
            || declined
                .lock()
                .is_ok_and(|d| d.contains(call.notification.as_str()))
        {
            return;
        }
        let in_call = session
            .call_participants(call.room_id.clone())
            .await
            .unwrap_or_default();
        if me.as_ref().is_some_and(|me| in_call.contains(me)) {
            return;
        }
        // A moment for the caller's own membership to arrive, then: gone,
        // they gave up.
        if started.elapsed() > std::time::Duration::from_secs(5) && !in_call.contains(&call.caller)
        {
            return;
        }
        tick();
        morpheus_core::rt::sleep(std::time::Duration::from_secs(2)).await;
    }
}
