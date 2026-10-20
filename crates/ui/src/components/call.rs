//! Calls, Discord-style. Any room can hold a call: the phone button in its
//! header joins; the people in it are listed under the room in the sidebar
//! (ringed while they speak); the voice panel at the bottom of the sidebar
//! names the call and hangs up; mute and deafen sit in the user pill.
//!
//! The core does the Matrix side (credentials, membership); `morpheus-call`
//! carries the audio. One call at a time per account, as in Discord.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use dioxus::core::spawn_forever;
use dioxus::prelude::*;
use morpheus_call::{AudioMode, Call, CallEvent, DeviceChoice};
use morpheus_core::{OwnedRoomId, OwnedUserId, Session};

use super::shell::go_to;
use super::toast::report;
use crate::icons::{Icon, PHONE_OFF};
use crate::media::Avatar;

/// The call this account is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallState {
    pub room_id: OwnedRoomId,
    pub room_name: String,
    /// Media flowing (credentials fetched, LiveKit connected).
    pub connected: bool,
    /// Who is speaking now, this account included.
    pub speaking: Vec<OwnedUserId>,
    /// Whose audio arrives, decrypted (test tones only; see `AudioMode::Tone`).
    pub heard: Vec<OwnedUserId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CallSlot(pub Option<CallState>);
pub type ActiveCall = Signal<CallSlot>;

/// Mute and deafen, kept across calls like Discord's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VoiceFlags {
    pub muted: bool,
    pub deafened: bool,
}
pub type Voice = Signal<VoiceFlags>;

/// The media connection, while there is one.
#[derive(Clone, Default)]
struct CallMedia(Rc<RefCell<Option<Call>>>);

/// Install in the shell. The call hangs up when the shell goes (signing
/// out, switching accounts).
pub fn provide_calls(session: Session) {
    let active: ActiveCall = use_context_provider(|| Signal::new(CallSlot::default()));
    use_context_provider(|| Signal::new(VoiceFlags::default()) as Voice);
    let media = use_context_provider(CallMedia::default);
    let data_dir = use_context::<crate::AppConfig>().data_dir;
    use_hook(|| morpheus_call::choose_audio_devices(crate::device_prefs::audio_devices(&data_dir)));
    use_drop(move || {
        let call = media.0.borrow_mut().take();
        let room = active.peek().0.as_ref().map(|c| c.room_id.clone());
        spawn_forever(async move {
            if let Some(call) = call {
                call.leave().await;
            }
            if let Some(room) = room {
                let _ = session.leave_call(room).await;
            }
        });
    });
}

/// Join `room_id`'s call, leaving any other first.
pub fn join_call(session: Session, room_id: OwnedRoomId, room_name: String) {
    let mut active = consume_context::<ActiveCall>();
    let voice = consume_context::<Voice>();
    let media = consume_context::<CallMedia>();
    if active
        .peek()
        .0
        .as_ref()
        .is_some_and(|c| c.room_id == room_id)
    {
        return;
    }
    let microphone = try_consume_context::<crate::AppConfig>()
        .and_then(|config| config.microphone)
        .filter(|_| AudioMode::from_env() == AudioMode::Platform);
    // Outlives whatever button started it.
    spawn_forever(async move {
        leave(&session, active, &media).await;
        active.set(CallSlot(Some(CallState {
            room_id: room_id.clone(),
            room_name,
            connected: false,
            speaking: Vec::new(),
            heard: Vec::new(),
        })));
        let wanted = room_id.clone();
        let still_wanted = move || {
            active
                .try_peek()
                .is_ok_and(|slot| slot.0.as_ref().is_some_and(|c| c.room_id == wanted))
        };

        // The phone asks once; without the microphone there is no call.
        if let Some(ask) = microphone {
            let allowed = tokio::task::spawn_blocking(move || ask())
                .await
                .unwrap_or(false);
            if !allowed {
                super::toast::toast("Calls need the microphone: allow it in the system settings.");
                if still_wanted() {
                    active.set(CallSlot(None));
                }
                return;
            }
        }
        let credentials = match session.join_call(room_id.clone()).await {
            Ok(credentials) => credentials,
            Err(err) => {
                report("joining the call", &err);
                if still_wanted() {
                    active.set(CallSlot(None));
                }
                return;
            }
        };
        // Keys others share, subscribed before ours goes out so no answer is missed.
        let mut keys = match &credentials.media_key {
            Some(_) => session.call_keys(room_id.clone()).await.ok(),
            None => None,
        };
        let connected = match credentials.media_key.clone() {
            Some(key) => {
                Call::connect_encrypted(
                    &credentials.livekit_url,
                    &credentials.jwt,
                    AudioMode::from_env(),
                    key,
                )
                .await
            }
            None => {
                Call::connect(
                    &credentials.livekit_url,
                    &credentials.jwt,
                    AudioMode::from_env(),
                )
                .await
            }
        };
        let (call, mut events) = match connected {
            Ok(connected) => connected,
            Err(err) => {
                report("connecting the call", &err);
                let _ = session.leave_call(room_id.clone()).await;
                if still_wanted() {
                    active.set(CallSlot(None));
                }
                return;
            }
        };
        if !still_wanted() {
            // Hung up (or moved on) while connecting.
            call.leave().await;
            let _ = session.leave_call(room_id.clone()).await;
            return;
        }
        let flags = *voice.peek();
        call.set_muted(flags.muted || flags.deafened);
        call.set_deafened(flags.deafened);
        let mut people: HashMap<String, OwnedUserId> = HashMap::new();
        if let Some(me) = session.user_id() {
            people.insert(call.identity(), me);
        }
        *media.0.borrow_mut() = Some(call);
        active.with_mut(|slot| {
            if let Some(state) = slot.0.as_mut() {
                state.connected = true;
            }
        });
        // Our media key, to everyone in the call (again whenever someone joins).
        let share_key = {
            let (session, room_id, key) = (
                session.clone(),
                room_id.clone(),
                credentials.media_key.clone(),
            );
            move || {
                let (session, room_id) = (session.clone(), room_id.clone());
                let key = key.clone();
                async move {
                    if let Some(key) = key
                        && let Err(err) = session.send_call_key(room_id, key).await
                    {
                        report("sharing the call's media key", &err);
                    }
                }
            }
        };
        share_key().await;

        loop {
            let event = tokio::select! {
                event = events.recv() => match event {
                    Some(event) => event,
                    None => return,
                },
                Some(key) = async {
                    match keys.as_mut() {
                        Some(keys) => keys.next().await,
                        None => std::future::pending().await,
                    }
                } => {
                    if let Some(call) = media.0.borrow().as_ref() {
                        for identity in &key.livekit_identities {
                            call.set_key(identity, key.key.clone());
                        }
                    }
                    continue;
                }
            };
            if !still_wanted() {
                return;
            }
            match event {
                CallEvent::Joined(_) => {
                    refresh_people(&session, &room_id, &mut people).await;
                    share_key().await;
                }
                CallEvent::Speaking(identities) => {
                    if identities.iter().any(|id| !people.contains_key(id)) {
                        refresh_people(&session, &room_id, &mut people).await;
                    }
                    let speaking: Vec<OwnedUserId> = identities
                        .iter()
                        .filter_map(|id| people.get(id).cloned())
                        .collect();
                    active.with_mut(|slot| {
                        if let Some(state) = slot.0.as_mut() {
                            state.speaking = speaking;
                        }
                    });
                }
                // Test tones only: whose audio actually arrives (decrypted).
                CallEvent::Heard(ref identity) | CallEvent::Quiet(ref identity) => {
                    if !people.contains_key(identity) {
                        refresh_people(&session, &room_id, &mut people).await;
                    }
                    let heard = matches!(event, CallEvent::Heard(_));
                    if let Some(user) = people.get(identity).cloned() {
                        active.with_mut(|slot| {
                            if let Some(state) = slot.0.as_mut() {
                                state.heard.retain(|u| *u != user);
                                if heard {
                                    state.heard.push(user);
                                }
                            }
                        });
                    }
                }
                CallEvent::Disconnected(reason) => {
                    report("the call dropped", &reason);
                    leave(&session, active, &media).await;
                    return;
                }
                CallEvent::Left(_) => {}
            }
        }
    });
}

/// Hang up.
pub fn hang_up(session: Session) {
    let active = consume_context::<ActiveCall>();
    let media = consume_context::<CallMedia>();
    spawn_forever(async move { leave(&session, active, &media).await });
}

async fn leave(session: &Session, mut active: ActiveCall, media: &CallMedia) {
    let call = media.0.borrow_mut().take();
    let room = active
        .try_peek()
        .ok()
        .and_then(|slot| slot.0.as_ref().map(|c| c.room_id.clone()));
    if active.try_peek().is_ok() {
        active.set(CallSlot(None));
    }
    // Matrix first: that is what everyone else sees.
    if let Some(room) = room
        && let Err(err) = session.leave_call(room).await
    {
        report("leaving the call", &err);
    }
    if let Some(call) = call {
        call.leave().await;
    }
}

/// Match LiveKit identities to people, from the room's call memberships.
async fn refresh_people(
    session: &Session,
    room_id: &OwnedRoomId,
    people: &mut HashMap<String, OwnedUserId>,
) {
    if let Ok(members) = session.call_members(room_id.clone()).await {
        for member in members {
            for identity in member.livekit_identities {
                people.insert(identity, member.user_id.clone());
            }
        }
    }
}

/// Toggle mute or deafen (deafening mutes too, as in Discord).
pub fn set_voice(muted: bool, deafened: bool) {
    let mut voice = consume_context::<Voice>();
    let media = consume_context::<CallMedia>();
    voice.set(VoiceFlags { muted, deafened });
    if let Some(call) = media.0.borrow().as_ref() {
        call.set_muted(muted || deafened);
        call.set_deafened(deafened);
    }
}

/// Calls use these devices, the one going now included.
pub fn use_devices(choice: DeviceChoice) {
    morpheus_call::choose_audio_devices(choice);
    let media = consume_context::<CallMedia>();
    if let Some(call) = media.0.borrow().as_ref()
        && let Err(err) = call.use_chosen_devices()
    {
        report("switching audio devices", &err);
    }
}

/// The people in a room's call, under its row in the sidebar. Clicking
/// joins, like a Discord voice channel.
#[component]
pub fn RoomCall(
    room_id: OwnedRoomId,
    room_name: String,
    participants: Vec<OwnedUserId>,
) -> Element {
    let session = use_context::<Session>();
    let active = use_context::<ActiveCall>();
    let speaking = active()
        .0
        .filter(|c| c.room_id == room_id)
        .map(|c| c.speaking)
        .unwrap_or_default();
    rsx! {
        div {
            class: "room-call",
            "data-room": "{room_id}",
            title: "Join the call",
            onclick: move |_| join_call(session.clone(), room_id.clone(), room_name.clone()),
            for user in participants {
                CallParticipant {
                    key: "{user}",
                    speaking: speaking.contains(&user),
                    user_id: user,
                }
            }
        }
    }
}

#[component]
fn CallParticipant(user_id: OwnedUserId, speaking: bool) -> Element {
    let session = use_context::<Session>();
    let profile = use_resource({
        let user_id = user_id.clone();
        move || {
            let (session, user_id) = (session.clone(), user_id.clone());
            async move { session.profile(user_id).await.ok() }
        }
    });
    let fallback = user_id.localpart().to_owned();
    let (name, avatar) = match profile().flatten() {
        Some(profile) => (profile.name, profile.avatar),
        None => (fallback, None),
    };
    let name = try_use_context::<super::nicknames::Nicknames>()
        .map(|map| super::nicknames::shown_name(&map(), user_id.as_str(), &name))
        .unwrap_or(name);
    rsx! {
        div { class: "call-participant", class: if speaking { "speaking" },
            Avatar { media: avatar, fallback: name.clone(), size: 20 }
            span { class: "call-participant-name", "{name}" }
        }
    }
}

/// At the bottom of the sidebar while in a call: which call, and hang up.
#[component]
pub fn VoicePanel() -> Element {
    let session = use_context::<Session>();
    let active = use_context::<ActiveCall>();
    let Some(call) = active().0 else {
        return rsx! {};
    };
    let room_id = call.room_id.clone();
    let heard = call
        .heard
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    rsx! {
        div { class: "voice-panel", "data-heard": "{heard}",
            div { class: "voice-status",
                span { class: "voice-state", class: if call.connected { "connected" },
                    if call.connected { "Voice connected" } else { "Connecting…" }
                }
                button {
                    class: "voice-room",
                    title: "Go to the room",
                    onclick: move |_| go_to(room_id.clone()),
                    "{call.room_name}"
                }
            }
            button {
                class: "ghost hang-up",
                title: "Disconnect",
                onclick: move |_| hang_up(session.clone()),
                Icon { glyph: PHONE_OFF, size: 18 }
            }
        }
    }
}
