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
use morpheus_call::{AudioMode, Call, CallEvent, DeviceChoice, LOCAL_CAMERA, LOCAL_SCREEN};
use morpheus_core::{IncomingCall, OwnedRoomId, OwnedUserId, Ringing, RoomSummary, Session};

use super::shell::go_to;
use super::toast::report;
use crate::icons::{
    CHEVRON_DOWN, CHEVRON_UP, HEADPHONE_OFF, HEADPHONES, Icon, MIC, MIC_OFF, PHONE, PHONE_OFF,
    SCREEN_SHARE, SCREEN_SHARE_OFF, VIDEO, VIDEO_OFF, VOLUME_2,
};
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
    /// The video showing: screens, then cameras, this device's first of each.
    pub video: Vec<VideoFeed>,
    /// This device's camera is on.
    pub camera: bool,
    /// This device's screen is shared.
    pub screen: bool,
    /// The desktop is asking which screen to share.
    pub choosing_screen: bool,
}

/// Someone's video, and where its pictures are ([`morpheus_call::latest_frame`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFeed {
    pub user: OwnedUserId,
    pub key: String,
    /// A shared screen, not a camera.
    pub screen: bool,
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

/// Phones: the call fills the screen (else it is a bar above the message box).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CallScreenShown(pub bool);
pub type CallScreenOpen = Signal<CallScreenShown>;

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
    let voice: Voice = use_context_provider(|| Signal::new(VoiceFlags::default()));
    // What this device has on, published in its call membership for everyone
    // to show (on its card, and beside its name under the room).
    let mut published = use_signal(|| None::<(OwnedRoomId, morpheus_core::CallMedia)>);
    use_effect({
        let session = session.clone();
        move || {
            let flags = voice();
            let Some(call) = active().0.filter(|call| call.connected) else {
                if published.peek().is_some() {
                    published.set(None);
                }
                return;
            };
            let media = morpheus_core::CallMedia {
                muted: flags.muted || flags.deafened,
                deafened: flags.deafened,
                camera: call.camera,
                screen: call.screen,
            };
            let now = (call.room_id.clone(), media);
            if published.peek().as_ref() == Some(&now) {
                return;
            }
            published.set(Some(now));
            let sending = session.set_call_media(call.room_id, media);
            spawn(async move {
                if let Err(err) = sending.await {
                    tracing::warn!(%err, "call media state not published");
                }
            });
        }
    });
    use_context_provider(|| Signal::new(CallScreenShown::default()) as CallScreenOpen);
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

/// The call to join when asked to call in `room_id`. Someone can have
/// several direct-message rooms with you (old ones, ones started from
/// another app); if they are already calling you from one of the others,
/// that is the call to join, not a second, empty one beside it.
fn call_room_for(room_id: &OwnedRoomId, rooms: &[RoomSummary]) -> Option<(OwnedRoomId, String)> {
    let here = rooms.iter().find(|r| &r.id == room_id)?;
    let them = here.dm_user.as_ref().filter(|_| here.is_dm)?;
    if here.call_participants.contains(them) {
        return None;
    }
    rooms
        .iter()
        .find(|r| {
            r.is_dm
                && r.id != *room_id
                && r.dm_user.as_ref() == Some(them)
                && r.call_participants.contains(them)
        })
        .map(|r| (r.id.clone(), r.name.clone()))
}

/// Join `room_id`'s call, leaving any other first.
pub fn join_call(session: Session, room_id: OwnedRoomId, room_name: String) {
    let mut active = consume_context::<ActiveCall>();
    let voice = consume_context::<Voice>();
    let media = consume_context::<CallMedia>();
    let (room_id, room_name) = match try_consume_context::<super::shell::Rooms>().and_then(
        |rooms| call_room_for(&room_id, &rooms.peek().iter().cloned().collect::<Vec<_>>()),
    ) {
        Some((elsewhere, name)) => {
            tracing::info!(asked = %room_id, joining = %elsewhere, "they are calling from another DM: joining that call");
            go_to(elsewhere.clone());
            (elsewhere, name)
        }
        None => (room_id, room_name),
    };
    if active
        .peek()
        .0
        .as_ref()
        .is_some_and(|c| c.room_id == room_id)
    {
        return;
    }
    // A phone shows the call full screen, as Discord's app does.
    if try_consume_context::<super::shell::NarrowWindow>().is_some_and(|narrow| narrow.peek().0) {
        show_call_screen(true);
    }
    let microphone = try_consume_context::<crate::AppConfig>()
        .and_then(|config| config.microphone)
        .filter(|_| AudioMode::from_env() == AudioMode::Platform);
    // Outlives whatever button started it.
    spawn_forever(async move {
        leave(&session, active, &media).await;
        // This call's number: its events stop counting once another call (or a
        // hang-up) has come since.
        let generation = CALL_GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        let current =
            move || CALL_GENERATION.load(std::sync::atomic::Ordering::SeqCst) == generation;
        active.set(CallSlot(Some(CallState {
            room_id: room_id.clone(),
            room_name,
            connected: false,
            speaking: Vec::new(),
            heard: Vec::new(),
            video: Vec::new(),
            camera: false,
            screen: false,
            choosing_screen: false,
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
        // This device's name in the call, sent along with its key: the token
        // service's names cannot be worked out everywhere (matrix.org's
        // cannot), so the others learn it from us.
        let own_identity = call.identity();
        if let Some(me) = session.user_id() {
            people.insert(own_identity.clone(), me);
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
                let (key, identity) = (key.clone(), own_identity.clone());
                async move {
                    if let Some(key) = key
                        && let Err(err) = session.send_call_key(room_id, key, identity).await
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
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)), if !current() => return,
                Some(key) = async {
                    match keys.as_mut() {
                        Some(keys) => keys.next().await,
                        None => std::future::pending().await,
                    }
                } => {
                    // Its sender's name in the call, when it says: who is
                    // speaking under that name.
                    if let Some(identity) = &key.livekit_identity {
                        people.insert(identity.clone(), key.user_id.clone());
                    }
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
                CallEvent::Video {
                    identity,
                    key,
                    screen,
                    on,
                } => {
                    if !people.contains_key(&identity) {
                        refresh_people(&session, &room_id, &mut people).await;
                    }
                    let user = people.get(&identity).cloned();
                    active.with_mut(|slot| {
                        if let Some(state) = slot.0.as_mut() {
                            state.video.retain(|feed| feed.key != key);
                            if on && let Some(user) = user {
                                let feed = VideoFeed { user, key, screen };
                                if screen {
                                    let at = state.video.iter().take_while(|f| f.screen).count();
                                    state.video.insert(at, feed);
                                } else {
                                    state.video.push(feed);
                                }
                            }
                        }
                    });
                }
                CallEvent::Disconnected(reason) => {
                    // A call left already (hung up, or replaced by a rejoin) says
                    // goodbye late: not the call going now.
                    if !current() {
                        return;
                    }
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
    CALL_GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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

/// Phones: fill the screen with the call, or shrink it to a bar.
pub fn show_call_screen(shown: bool) {
    if let Some(mut screen) = try_consume_context::<CallScreenOpen>() {
        screen.set(CallScreenShown(shown));
    }
}

/// Mute, or unmute (which undeafens too, as in Discord): the mic buttons
/// and Ctrl+Shift+M.
pub fn toggle_mute() {
    let flags = *consume_context::<Voice>().peek();
    let muted = !(flags.muted || flags.deafened);
    set_voice(muted, flags.deafened && muted);
}

/// Deafen (muted too), or undeafen (muted only if you were before): the
/// headphones button and Ctrl+Shift+D.
pub fn toggle_deafen() {
    let flags = *consume_context::<Voice>().peek();
    set_voice(flags.muted, !flags.deafened);
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
    set_video(session, false, on);
}

/// Share this device's screen in the call going, or stop. The desktop may
/// ask which screen or window first; cancelling there just leaves it off.
pub fn set_screen_share(session: Session, on: bool) {
    set_video(session, true, on);
}

fn set_video(session: Session, screen: bool, on: bool) {
    let mut active = consume_context::<ActiveCall>();
    let media = consume_context::<CallMedia>();
    type Switch = std::pin::Pin<Box<dyn Future<Output = morpheus_call::Result<()>>>>;
    let Some(switch) = media.0.borrow().as_ref().map(|call| -> Switch {
        if screen {
            Box::pin(call.set_screen_share(on))
        } else {
            Box::pin(call.set_camera(on))
        }
    }) else {
        return;
    };
    let key = if screen { LOCAL_SCREEN } else { LOCAL_CAMERA };
    if screen && on {
        active.with_mut(|slot| {
            if let Some(state) = slot.0.as_mut() {
                state.choosing_screen = true;
            }
        });
    }
    spawn_forever(async move {
        let switched = switch.await;
        let me = session.user_id();
        active.with_mut(|slot| {
            if let Some(state) = slot.0.as_mut() {
                if screen {
                    state.choosing_screen = false;
                }
                if switched.is_err() {
                    return;
                }
                if screen {
                    state.screen = on;
                } else {
                    state.camera = on;
                }
                state.video.retain(|feed| feed.key != key);
                if on && let Some(user) = me {
                    // Screens first, then cameras; this device's first of each.
                    let at = if screen {
                        0
                    } else {
                        state.video.iter().take_while(|feed| feed.screen).count()
                    };
                    state.video.insert(
                        at,
                        VideoFeed {
                            user,
                            key: key.to_owned(),
                            screen,
                        },
                    );
                }
            }
        });
        match switched {
            // Changing one's mind in the screen picker is not a failure.
            Err(err) if err.to_string().contains("cancelled") => {}
            Err(err) if screen => report("sharing the screen", &err),
            Err(err) => report("turning the camera on", &err),
            Ok(()) => {}
        }
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
                    room_id: room_id.clone(),
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
fn CallParticipant(room_id: OwnedRoomId, user_id: OwnedUserId, speaking: bool) -> Element {
    let (name, avatar) = use_person(&user_id);
    let media = use_call_media(&room_id, &user_id);
    rsx! {
        div { class: "call-participant", class: if speaking { "speaking" }, "data-user": "{user_id}",
            Avatar { media: avatar, fallback: name.clone(), size: 20 }
            span { class: "call-participant-name", "{name}" }
            MediaMarks { media, video: true }
        }
    }
}

/// What someone in `room_id`'s call has on: this account's from its own
/// flags (at once), anyone else's as their membership publishes it.
fn use_call_media(room_id: &OwnedRoomId, user_id: &OwnedUserId) -> morpheus_core::CallMedia {
    let session = use_context::<Session>();
    let rooms = use_context::<super::shell::Rooms>();
    let active = use_context::<ActiveCall>();
    let voice = use_context::<Voice>();
    if session.user_id().as_ref() == Some(user_id)
        && let Some(call) = active().0.filter(|c| &c.room_id == room_id)
    {
        let flags = voice();
        return morpheus_core::CallMedia {
            muted: flags.muted || flags.deafened,
            deafened: flags.deafened,
            camera: call.camera,
            screen: call.screen,
        };
    }
    rooms
        .read()
        .iter()
        .find(|room| &room.id == room_id)
        .and_then(|room| {
            room.call_media
                .iter()
                .find(|(user, _)| user == user_id)
                .map(|(_, media)| *media)
        })
        .unwrap_or_default()
}

/// Marks for someone in a call: deafened or muted, and with `video`, their
/// camera and LIVE for a shared screen.
#[component]
fn MediaMarks(media: morpheus_core::CallMedia, video: bool) -> Element {
    rsx! {
        if media.deafened {
            span { class: "voice-state deafened", title: "Deafened", Icon { glyph: HEADPHONE_OFF, size: 14 } }
        } else if media.muted {
            span { class: "voice-state muted", title: "Muted", Icon { glyph: MIC_OFF, size: 14 } }
        }
        if video && media.camera {
            span { class: "media-camera", title: "Camera on", Icon { glyph: VIDEO, size: 14 } }
        }
        if video && media.screen {
            span { class: "media-live", title: "Sharing their screen", "LIVE" }
        }
    }
}

/// Above the chat of the room this account is calling in: a card each
/// (their camera, or their picture), and the call's controls along the
/// bottom, cameras on or not.
#[component]
pub fn CallStage(room_id: OwnedRoomId) -> Element {
    let session = use_context::<Session>();
    let active = use_context::<ActiveCall>();
    let rooms = use_context::<super::shell::Rooms>();
    let Some(call) = active().0.filter(|call| call.room_id == room_id) else {
        return rsx! {};
    };
    let cards = tiles(&call, &rooms.read(), session.user_id());
    let count = cards.len();
    rsx! {
        div { class: "call-stage", "data-cards": "{count}",
            div { class: "call-cards",
                Cards { room_id: room_id.clone(), cards, speaking: call.speaking.clone() }
            }
            StageControls { call: call.clone() }
        }
        // Dragged to make the stage taller or shorter (hotkeys.rs does the
        // dragging and remembers the height).
        div { class: "call-stage-resize", title: "Drag to resize" }
    }
}

/// The call's controls under its cards, as Discord has them (the same as
/// the profile card's, there too): mute, deafen, camera, screen, leave.
#[component]
fn StageControls(call: CallState) -> Element {
    let session = use_context::<Session>();
    let voice = use_context::<Voice>();
    let flags = voice();
    let muted = flags.muted || flags.deafened;
    rsx! {
        div { class: "call-controls stage-controls",
            button {
                class: "call-control mute-button",
                class: if muted { "active" },
                title: if muted { "Unmute (Ctrl+Shift+M)" } else { "Mute (Ctrl+Shift+M)" },
                onclick: move |_| toggle_mute(),
                Icon { glyph: if muted { MIC_OFF } else { MIC }, size: 20 }
            }
            button {
                class: "call-control deafen-button",
                class: if flags.deafened { "active" },
                title: if flags.deafened { "Undeafen (Ctrl+Shift+D)" } else { "Deafen (Ctrl+Shift+D)" },
                onclick: move |_| toggle_deafen(),
                Icon { glyph: if flags.deafened { HEADPHONE_OFF } else { HEADPHONES }, size: 20 }
            }
            if morpheus_call::can_send_video() {
                button {
                    class: "call-control camera-button",
                    class: if call.camera { "active" },
                    title: if call.camera { "Turn off camera" } else { "Turn on camera" },
                    disabled: !call.connected,
                    onclick: {
                        let (session, on) = (session.clone(), !call.camera);
                        move |_| set_camera(session.clone(), on)
                    },
                    Icon { glyph: if call.camera { VIDEO } else { VIDEO_OFF }, size: 20 }
                }
            }
            if morpheus_call::can_share_screen() {
                button {
                    class: "call-control screen-button",
                    class: if call.screen { "active" },
                    class: if call.choosing_screen { "pending" },
                    title: if call.screen { "Stop sharing your screen" } else { "Share your screen" },
                    disabled: !call.connected || call.choosing_screen,
                    onclick: {
                        let (session, on) = (session.clone(), !call.screen);
                        move |_| set_screen_share(session.clone(), on)
                    },
                    Icon { glyph: if call.screen { SCREEN_SHARE_OFF } else { SCREEN_SHARE }, size: 20 }
                }
            }
            button {
                class: "call-control hang-up",
                title: "Disconnect",
                onclick: move |_| hang_up(session.clone()),
                Icon { glyph: PHONE_OFF, size: 20 }
            }
        }
    }
}

/// One card in a call, as Discord has them: a shared screen, or a person
/// (their camera when it is on, else their picture).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Card {
    Video(VideoFeed),
    Person(OwnedUserId),
}

/// A call's cards: shared screens first, then everyone in it (this account
/// first), each in their own card whether or not their camera is on.
fn tiles(
    call: &CallState,
    rooms: &morpheus_core::Vector<RoomSummary>,
    me: Option<OwnedUserId>,
) -> Vec<Card> {
    let mut people = rooms
        .iter()
        .find(|room| room.id == call.room_id)
        .map(|room| room.call_participants.clone())
        .unwrap_or_default();
    if let Some(me) = me
        && !people.contains(&me)
    {
        people.insert(0, me);
    }
    let camera = |user: &OwnedUserId| {
        call.video
            .iter()
            .find(|feed| !feed.screen && &feed.user == user)
    };
    let mut cards: Vec<Card> = call
        .video
        .iter()
        .filter(|feed| feed.screen)
        .cloned()
        .map(Card::Video)
        .collect();
    cards.extend(people.iter().map(|user| match camera(user) {
        Some(feed) => Card::Video(feed.clone()),
        None => Card::Person(user.clone()),
    }));
    // A camera from someone not (yet) listed in the call.
    cards.extend(
        call.video
            .iter()
            .filter(|feed| !feed.screen && !people.contains(&feed.user))
            .cloned()
            .map(Card::Video),
    );
    cards
}

/// The cards, each its own size (the stage's script fits them to it).
#[component]
fn Cards(room_id: OwnedRoomId, cards: Vec<Card>, speaking: Vec<OwnedUserId>) -> Element {
    rsx! {
        {cards.into_iter().map(|card| match card {
            Card::Video(feed) => rsx! {
                VideoTile { key: "{feed.key}", room_id: room_id.clone(), user: feed.user, feed: feed.key, screen: feed.screen }
            },
            Card::Person(user) => rsx! {
                CallTile { key: "{user}", room_id: room_id.clone(), speaking: speaking.contains(&user), user_id: user }
            },
        })}
    }
}

/// How often a tile looks for a new picture.
const VIDEO_POLL_MS: u64 = 1000 / 30;

/// One person's video: the latest picture, as it comes.
#[component]
fn VideoTile(room_id: OwnedRoomId, user: OwnedUserId, feed: String, screen: bool) -> Element {
    let (name, _) = use_person(&user);
    let media = use_call_media(&room_id, &user);
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
            class: if feed == LOCAL_CAMERA || feed == LOCAL_SCREEN { "self" },
            class: if screen { "screen" },
            "data-feed": "{feed}",
            "data-user": "{user}",
            img { class: "video", alt: "" }
            span { class: "video-name",
                if screen { "{name}'s screen" } else { "{name}" }
            }
            if !screen {
                span { class: "card-marks", MediaMarks { media, video: false } }
            }
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
            if morpheus_call::can_share_screen() {
                button {
                    class: "ghost screen-button",
                    class: if call.screen { "active" },
                    class: if call.choosing_screen { "pending" },
                    title: if call.screen { "Stop sharing your screen" } else { "Share your screen" },
                    disabled: !call.connected || call.choosing_screen,
                    onclick: {
                        let (session, on) = (session.clone(), !call.screen);
                        move |_| set_screen_share(session.clone(), on)
                    },
                    Icon { glyph: if call.screen { SCREEN_SHARE_OFF } else { SCREEN_SHARE }, size: 18 }
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

/// Phones: the call over everything. A tile per person (their camera when
/// it is on, else their picture, ringed while they speak), shared screens
/// first; mute, speaker, camera, and hang up along the bottom. Minimizing
/// (or back) leaves the [`CallBar`].
#[component]
pub fn CallScreen() -> Element {
    let session = use_context::<Session>();
    let active = use_context::<ActiveCall>();
    let shown = use_context::<CallScreenOpen>();
    let voice = use_context::<Voice>();
    let rooms = use_context::<super::shell::Rooms>();
    let Some(call) = active().0.filter(|_| shown().0) else {
        return rsx! {};
    };
    let cards = tiles(&call, &rooms.read(), session.user_id());
    let flags = voice();
    let muted = flags.muted || flags.deafened;
    let room_id = call.room_id.clone();
    rsx! {
        div { class: "call-screen",
            header { class: "call-screen-header",
                button {
                    class: "ghost call-minimize",
                    title: "Back to the chat",
                    onclick: move |_| show_call_screen(false),
                    Icon { glyph: CHEVRON_DOWN, size: 24 }
                }
                button {
                    class: "call-screen-room",
                    title: "Go to the room",
                    onclick: move |_| {
                        show_call_screen(false);
                        go_to(room_id.clone());
                    },
                    span { class: "call-screen-title", "{call.room_name}" }
                    span { class: "call-screen-state", class: if call.connected { "connected" },
                        if call.connected { "Connected" } else { "Connecting…" }
                    }
                }
            }
            div { class: "call-tiles",
                Cards { room_id: call.room_id.clone(), cards, speaking: call.speaking.clone() }
            }
            div { class: "call-controls",
                button {
                    class: "call-control mute-button",
                    class: if muted { "active" },
                    title: if muted { "Unmute" } else { "Mute" },
                    onclick: move |_| toggle_mute(),
                    Icon { glyph: if muted { MIC_OFF } else { MIC }, size: 24 }
                }
                if has_earpiece() {
                    button {
                        class: "call-control speaker-button",
                        class: if !flags.earpiece { "active" },
                        title: if flags.earpiece { "Earpiece (tap for the speaker)" } else { "Speaker (tap for the earpiece)" },
                        onclick: move |_| set_earpiece(!flags.earpiece),
                        Icon { glyph: if flags.earpiece { PHONE } else { VOLUME_2 }, size: 24 }
                    }
                }
                if morpheus_call::can_send_video() {
                    button {
                        class: "call-control camera-button",
                        class: if call.camera { "active" },
                        title: if call.camera { "Turn off camera" } else { "Turn on camera" },
                        disabled: !call.connected,
                        onclick: {
                            let (session, on) = (session.clone(), !call.camera);
                            move |_| set_camera(session.clone(), on)
                        },
                        Icon { glyph: if call.camera { VIDEO } else { VIDEO_OFF }, size: 24 }
                    }
                }
                button {
                    class: "call-control hang-up",
                    title: "Disconnect",
                    onclick: move |_| {
                        show_call_screen(false);
                        hang_up(session.clone());
                    },
                    Icon { glyph: PHONE_OFF, size: 24 }
                }
            }
        }
    }
}

/// Someone in the call without a camera on: their picture, large.
#[component]
fn CallTile(room_id: OwnedRoomId, user_id: OwnedUserId, speaking: bool) -> Element {
    let (name, avatar) = use_person(&user_id);
    let media = use_call_media(&room_id, &user_id);
    rsx! {
        div { class: "call-tile", class: if speaking { "speaking" }, "data-user": "{user_id}",
            Avatar { media: avatar, fallback: name.clone(), size: 72 }
            span { class: "video-name", "{name}" }
            span { class: "card-marks", MediaMarks { media, video: false } }
        }
    }
}

/// Phones, with the call minimized: above the message box, back to it.
#[component]
pub fn CallBar() -> Element {
    let active = use_context::<ActiveCall>();
    let shown = use_context::<CallScreenOpen>();
    let Some(call) = active().0.filter(|_| !shown().0) else {
        return rsx! {};
    };
    rsx! {
        button {
            class: "call-bar",
            class: if call.connected { "connected" },
            title: "Open the call",
            onclick: move |_| show_call_screen(true),
            Icon { glyph: VOLUME_2, size: 16 }
            span { class: "call-bar-room", "{call.room_name}" }
            span { class: "call-bar-state",
                if call.connected { "In call" } else { "Connecting…" }
            }
            Icon { glyph: CHEVRON_UP, size: 16 }
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
            } else if *tick.peek() % 2 == 0 && active.peek().0.is_none() {
                // In a call already: the card shows, quietly, so the call
                // you are in is not drowned out by a ringtone.
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

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn dm(id: &str, with: &str, in_call: &[&str]) -> RoomSummary {
        RoomSummary {
            id: OwnedRoomId::try_from(id).expect("room id"),
            name: with.to_owned(),
            topic: None,
            avatar: None,
            is_space: false,
            is_dm: true,
            dm_user: Some(OwnedUserId::try_from(with).expect("user id")),
            member_count: 2,
            unread_messages: 0,
            highlight_count: 0,
            marked_unread: false,
            muted: false,
            parent_spaces: Vec::new(),
            invited: false,
            inviter: None,
            call_participants: in_call
                .iter()
                .map(|u| OwnedUserId::try_from(*u).expect("user id"))
                .collect(),
            favorite: false,
            latest_ms: 0,
            space_children: Vec::new(),
            can_edit_children: false,
            is_voice: false,
            is_encrypted: false,
            call_media: Vec::new(),
        }
    }

    #[test]
    fn calling_someone_already_calling_from_another_dm_joins_that_call() {
        let rooms = [
            dm("!new:x", "@bob:x", &[]),
            dm("!old:x", "@bob:x", &["@bob:x"]),
            dm("!carol:x", "@carol:x", &["@carol:x"]),
        ];
        let asked = OwnedRoomId::try_from("!new:x").expect("id");
        assert_eq!(
            call_room_for(&asked, &rooms).map(|(id, _)| id.to_string()),
            Some("!old:x".to_owned())
        );
    }

    #[test]
    fn a_call_already_here_or_nowhere_else_stays_here() {
        let both = [
            dm("!new:x", "@bob:x", &["@bob:x"]),
            dm("!old:x", "@bob:x", &["@bob:x"]),
        ];
        let asked = OwnedRoomId::try_from("!new:x").expect("id");
        assert_eq!(call_room_for(&asked, &both), None);
        let alone = [dm("!new:x", "@bob:x", &[]), dm("!old:x", "@bob:x", &[])];
        assert_eq!(call_room_for(&asked, &alone), None);
    }
}

/// Bumped by every join and hang-up: a call's event loop goes on only while
/// its number is the latest, so a call already left cannot end the next one.
static CALL_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
