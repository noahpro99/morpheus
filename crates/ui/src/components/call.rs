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
use morpheus_call::{AudioMode, Call, CallEvent, DeviceChoice, LOCAL_CAMERA};
use morpheus_core::{IncomingCall, OwnedRoomId, OwnedUserId, Ringing, RoomSummary, Session};

use super::shell::go_to;
use super::toast::report;
use crate::icons::{Icon, PHONE, PHONE_OFF, VIDEO, VIDEO_OFF, VOLUME_2};
use crate::media::Avatar;
use crate::sounds::{Sound, play};

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
    /// The video showing, this device's camera first.
    pub video: Vec<VideoFeed>,
    /// This device's camera is on.
    pub camera: bool,
}

/// Someone's video, and where its pictures are ([`morpheus_call::latest_frame`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFeed {
    pub user: OwnedUserId,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CallSlot(pub Option<CallState>);
pub type ActiveCall = Signal<CallSlot>;

/// Mute and deafen, kept across calls like Discord's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VoiceFlags {
    pub muted: bool,
    pub deafened: bool,
    /// Phones: call audio from the earpiece instead of the loudspeaker.
    pub earpiece: bool,
}
pub type Voice = Signal<VoiceFlags>;

/// Calls ringing this account (newest last).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RingList(pub Vec<IncomingCall>);
pub type Rings = Signal<RingList>;

/// How long a ring may show before its caller turns up in the call (the
/// ring and the membership can arrive in either order).
const RING_GRACE_MS: u64 = 5_000;

/// The platform's loudspeaker/earpiece switch (phones), for call tasks
/// running outside any component (and on whichever thread the UI runs them).
static SPEAKER_ROUTE: std::sync::Mutex<Option<crate::SpeakerRoute>> = std::sync::Mutex::new(None);

/// Call audio to the loudspeaker or the earpiece, where the platform can.
fn route_audio(loudspeaker: bool) {
    let route = SPEAKER_ROUTE.lock().ok().and_then(|route| route.clone());
    if let Some(route) = route {
        route(loudspeaker);
    }
}

/// Whether this platform can switch between loudspeaker and earpiece.
pub fn has_earpiece() -> bool {
    SPEAKER_ROUTE.lock().is_ok_and(|route| route.is_some())
}

/// The media connection, while there is one.
#[derive(Clone, Default)]
struct CallMedia(Rc<RefCell<Option<Call>>>);

/// Install in the shell. The call hangs up when the shell goes (signing
/// out, switching accounts).
pub fn provide_calls(session: Session) {
    let active: ActiveCall = use_context_provider(|| Signal::new(CallSlot::default()));
    use_context_provider(|| Signal::new(VoiceFlags::default()) as Voice);
    let media = use_context_provider(CallMedia::default);
    let config = use_context::<crate::AppConfig>();
    let data_dir = config.data_dir.clone();
    use_hook(|| {
        if let Ok(mut route) = SPEAKER_ROUTE.lock() {
            route.clone_from(&config.speakerphone);
        }
    });
    use_hook(|| morpheus_call::choose_audio_devices(crate::device_prefs::audio_devices(&data_dir)));
    let mut rings: Rings = use_context_provider(|| Signal::new(RingList::default()));
    use_future({
        let (session, media) = (session.clone(), media.clone());
        move || {
            let (session, media) = (session.clone(), media.clone());
            async move {
                let mut updates = match session.ring_updates().await {
                    Ok(updates) => updates,
                    Err(err) => return report("listening for calls", &err),
                };
                while let Some(update) = updates.next().await {
                    match update {
                        Ringing::Incoming(call) => rings.with_mut(|list| {
                            list.0.retain(|c| c.room_id != call.room_id);
                            list.0.push(call);
                        }),
                        Ringing::Declined {
                            room_id,
                            by,
                            notification,
                        } => {
                            rings
                                .with_mut(|list| list.0.retain(|c| c.notification != notification));
                            if session.user_id().as_ref() != Some(&by) {
                                declined(&session, active, &media, room_id, by).await;
                            }
                        }
                    }
                }
            }
        }
    });
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

/// The person called said no: say so, and hang up if nobody else came.
async fn declined(
    session: &Session,
    active: ActiveCall,
    media: &CallMedia,
    room_id: OwnedRoomId,
    by: OwnedUserId,
) {
    let in_that_call = active
        .peek()
        .0
        .as_ref()
        .is_some_and(|c| c.room_id == room_id);
    if !in_that_call {
        return;
    }
    let name = match session.profile(by.clone()).await {
        Ok(profile) => profile.name,
        Err(_) => by.localpart().to_owned(),
    };
    super::toast::toast(format!("{name} declined the call"));
    let me = session.user_id();
    let alone = session
        .call_members(room_id)
        .await
        .map(|members| members.iter().all(|m| Some(&m.user_id) == me.as_ref()))
        .unwrap_or(true);
    if alone {
        leave(session, active, media).await;
    }
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
            video: Vec::new(),
            camera: false,
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
        route_audio(!flags.earpiece);
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
        play(Sound::Join);
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
                    play(Sound::Join);
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
                CallEvent::Video { identity, key, on } => {
                    if !people.contains_key(&identity) {
                        refresh_people(&session, &room_id, &mut people).await;
                    }
                    let user = people.get(&identity).cloned();
                    active.with_mut(|slot| {
                        if let Some(state) = slot.0.as_mut() {
                            state.video.retain(|feed| feed.key != key);
                            if on && let Some(user) = user {
                                state.video.push(VideoFeed { user, key });
                            }
                        }
                    });
                }
                CallEvent::Disconnected(reason) => {
                    report("the call dropped", &reason);
                    leave(&session, active, &media).await;
                    return;
                }
                CallEvent::Left(_) => play(Sound::Leave),
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
    if room.is_some() {
        play(Sound::Leave);
    }
    // Matrix first: that is what everyone else sees.
    if let Some(room) = room
        && let Err(err) = session.leave_call(room).await
    {
        report("leaving the call", &err);
    }
    if let Some(call) = call {
        call.leave().await;
        route_audio(false);
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
    let before = *voice.peek();
    if deafened != before.deafened {
        play(if deafened {
            Sound::Deafen
        } else {
            Sound::Undeafen
        });
    } else if muted != before.muted {
        play(if muted { Sound::Mute } else { Sound::Unmute });
    }
    voice.with_mut(|flags| {
        flags.muted = muted;
        flags.deafened = deafened;
    });
    if let Some(call) = media.0.borrow().as_ref() {
        call.set_muted(muted || deafened);
        call.set_deafened(deafened);
    }
}

/// Phones: call audio from the earpiece (held to the ear) or the loudspeaker.
pub fn set_earpiece(earpiece: bool) {
    let mut voice = consume_context::<Voice>();
    let active = consume_context::<ActiveCall>();
    voice.with_mut(|flags| flags.earpiece = earpiece);
    if active.peek().0.as_ref().is_some_and(|c| c.connected) {
        route_audio(!earpiece);
    }
}

/// Turn this device's camera on or off in the call going.
pub fn set_camera(session: Session, on: bool) {
    let mut active = consume_context::<ActiveCall>();
    let media = consume_context::<CallMedia>();
    let Some(switch) = media.0.borrow().as_ref().map(|call| call.set_camera(on)) else {
        return;
    };
    spawn_forever(async move {
        if let Err(err) = switch.await {
            report("turning the camera on", &err);
            return;
        }
        let me = session.user_id();
        active.with_mut(|slot| {
            if let Some(state) = slot.0.as_mut() {
                state.camera = on;
                state.video.retain(|feed| feed.key != LOCAL_CAMERA);
                if on && let Some(user) = me {
                    state.video.insert(
                        0,
                        VideoFeed {
                            user,
                            key: LOCAL_CAMERA.to_owned(),
                        },
                    );
                }
            }
        });
    });
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

/// Someone's shown name (nickname first) and avatar.
fn use_person(user_id: &OwnedUserId) -> (String, Option<morpheus_core::MediaRef>) {
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
    (name, avatar)
}

#[component]
fn CallParticipant(user_id: OwnedUserId, speaking: bool) -> Element {
    let (name, avatar) = use_person(&user_id);
    rsx! {
        div { class: "call-participant", class: if speaking { "speaking" },
            Avatar { media: avatar, fallback: name.clone(), size: 20 }
            span { class: "call-participant-name", "{name}" }
        }
    }
}

/// Above the chat of the room this account is calling in, while anyone's
/// camera is on: a tile each.
#[component]
pub fn CallStage(room_id: OwnedRoomId) -> Element {
    let active = use_context::<ActiveCall>();
    let Some(call) = active().0.filter(|call| call.room_id == room_id) else {
        return rsx! {};
    };
    if call.video.is_empty() {
        return rsx! {};
    }
    rsx! {
        div { class: "call-stage",
            for feed in call.video {
                VideoTile { key: "{feed.key}", user: feed.user, feed: feed.key }
            }
        }
    }
}

/// How often a tile looks for a new picture.
const VIDEO_POLL_MS: u64 = 1000 / 30;

/// One person's video: the latest picture, as it comes.
#[component]
fn VideoTile(user: OwnedUserId, feed: String) -> Element {
    let (name, _) = use_person(&user);
    use_future({
        let feed = feed.clone();
        move || {
            let feed = feed.clone();
            async move {
                let quoted = serde_json::to_string(&feed).unwrap_or_default();
                let mut shown = 0;
                loop {
                    if let Some(frame) = morpheus_call::latest_frame(&feed)
                        && frame.seq != shown
                    {
                        shown = frame.seq;
                        let data = base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD,
                            frame.jpeg.as_slice(),
                        );
                        document::eval(&format!(
                            "const t = [...document.querySelectorAll('.video-tile')].find(t => t.dataset.feed === {quoted}); \
                             if (t) t.querySelector('img').src = 'data:image/jpeg;base64,{data}';"
                        ));
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(VIDEO_POLL_MS)).await;
                }
            }
        }
    });
    rsx! {
        div {
            class: "video-tile",
            class: if feed == LOCAL_CAMERA { "self" },
            "data-feed": "{feed}",
            "data-user": "{user}",
            img { class: "video", alt: "" }
            span { class: "video-name", "{name}" }
        }
    }
}

/// At the bottom of the sidebar while in a call: which call, and hang up.
#[component]
pub fn VoicePanel() -> Element {
    let session = use_context::<Session>();
    let active = use_context::<ActiveCall>();
    let earpiece = use_context::<Voice>()().earpiece;
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
            if has_earpiece() {
                button {
                    class: "ghost speaker-button",
                    class: if !earpiece { "active" },
                    title: if earpiece { "Earpiece (tap for the speaker)" } else { "Speaker (tap for the earpiece)" },
                    onclick: move |_| set_earpiece(!earpiece),
                    Icon { glyph: if earpiece { PHONE } else { VOLUME_2 }, size: 18 }
                }
            }
            if morpheus_call::can_send_video() {
            button {
                class: "ghost camera-button",
                class: if call.camera { "active" },
                title: if call.camera { "Turn off camera" } else { "Turn on camera" },
                disabled: !call.connected,
                onclick: {
                    let (session, on) = (session.clone(), !call.camera);
                    move |_| set_camera(session.clone(), on)
                },
                Icon { glyph: if call.camera { VIDEO } else { VIDEO_OFF }, size: 18 }
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

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// The rings still worth answering: not expired, not for the call this
/// account is in, and the caller still waiting in it.
fn still_ringing(rings: &RingList, active: &CallSlot, rooms: &[RoomSummary]) -> Vec<IncomingCall> {
    let now = now_ms();
    rings
        .0
        .iter()
        .filter(|ring| ring.rings_until > now)
        .filter(|ring| active.0.as_ref().is_none_or(|c| c.room_id != ring.room_id))
        .filter(|ring| {
            let rang_at = ring.rings_until.saturating_sub(morpheus_core::RING_FOR_MS);
            now.saturating_sub(rang_at) < RING_GRACE_MS
                || rooms
                    .iter()
                    .find(|r| r.id == ring.room_id)
                    .is_some_and(|r| r.call_participants.contains(&ring.caller))
        })
        .cloned()
        .collect()
}

/// Someone calling: who, with answer and decline, while the ringtone plays.
#[component]
pub fn IncomingCalls() -> Element {
    let mut rings = use_context::<Rings>();
    let active = use_context::<ActiveCall>();
    let rooms = use_context::<super::shell::Rooms>();
    // Re-checked every second (rings expire, callers give up), ringing
    // every other second.
    let mut tick = use_signal(|| 0u64);
    use_future(move || async move {
        loop {
            let ringing = still_ringing(
                &rings.peek(),
                &active.peek(),
                &rooms.peek().iter().cloned().collect::<Vec<_>>(),
            );
            if ringing.is_empty() {
                if !rings.peek().0.is_empty() {
                    rings.with_mut(|list| list.0.retain(|r| ringing.contains(r)));
                }
            } else if *tick.peek() % 2 == 0 {
                play(Sound::Ring);
            }
            tick += 1;
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
    tick();
    let rooms_now: Vec<RoomSummary> = rooms().iter().cloned().collect();
    let ringing = still_ringing(&rings(), &active(), &rooms_now);
    rsx! {
        if !ringing.is_empty() {
            div { class: "incoming-calls",
                for call in ringing {
                    IncomingCallCard {
                        key: "{call.notification}",
                        room: rooms_now.iter().find(|r| r.id == call.room_id).cloned(),
                        call,
                    }
                }
            }
        }
    }
}

#[component]
fn IncomingCallCard(call: IncomingCall, room: Option<RoomSummary>) -> Element {
    let session = use_context::<Session>();
    let mut rings = use_context::<Rings>();
    let (name, avatar) = match &room {
        Some(room) => (room.name.clone(), room.avatar.clone()),
        None => (call.caller.localpart().to_owned(), None),
    };
    let accept = {
        let (session, call, name) = (session.clone(), call.clone(), name.clone());
        move |_| {
            rings.with_mut(|list| list.0.retain(|r| r.notification != call.notification));
            go_to(call.room_id.clone());
            join_call(session.clone(), call.room_id.clone(), name.clone());
        }
    };
    let decline = {
        let (session, call) = (session.clone(), call.clone());
        move |_| {
            rings.with_mut(|list| list.0.retain(|r| r.notification != call.notification));
            let future = session.decline_call(call.room_id.clone(), call.notification.clone());
            spawn(async move {
                if let Err(err) = future.await {
                    report("declining the call", &err);
                }
            });
        }
    };
    rsx! {
        div { class: "incoming-call", "data-room": "{call.room_id}",
            Avatar { media: avatar, fallback: name.clone(), size: 48 }
            div { class: "incoming-call-text",
                span { class: "incoming-call-name", "{name}" }
                span { class: "incoming-call-what", "Incoming call…" }
            }
            button { class: "decline-call", title: "Decline", onclick: decline,
                Icon { glyph: PHONE_OFF, size: 20 }
            }
            button { class: "accept-call", title: "Accept", onclick: accept,
                Icon { glyph: PHONE, size: 20 }
            }
        }
    }
}
