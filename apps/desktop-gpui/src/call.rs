//! Calls, the engine (`crates/ui/src/components/call.rs`, its first half):
//! joining and leaving, mute and deafen, camera and screen, who is in the
//! call and speaking, the rings. The core does the Matrix side (credentials,
//! membership); `morpheus-call` carries the media. One call at a time per
//! account, as in Discord. What the call looks like is `call_ui.rs`; what
//! its state means is `view_model::calls`.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    pin::Pin,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use futures::FutureExt as _;
use gpui::{AsyncApp, Context, WeakEntity};
use morpheus_call::{AudioMode, Call, CallEvent, LOCAL_CAMERA, LOCAL_SCREEN};
use morpheus_core::{CallMedia, IncomingCall, MediaRef, OwnedRoomId, OwnedUserId, Ringing, Session};

use crate::{
    shell::Shell,
    sounds,
    view_model::calls::{CallState, Sound, VoiceFlags, call_room_for, ringing_out, still_ringing},
};

/// Bumped by every join and hang-up: a call's event loop goes on only while
/// its number is the latest, so a call already left cannot end the next one.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// A person in a call, as the lists show them.
#[derive(Debug, Clone)]
pub struct Person {
    pub name: String,
    pub avatar: Option<MediaRef>,
}

/// Everything about calls the shell keeps.
#[derive(Default)]
pub struct Calls {
    /// The call this account is in.
    pub state: Option<CallState>,
    /// Mute and deafen, kept across calls.
    pub voice: VoiceFlags,
    /// The media connection, while there is one.
    media: Option<Call>,
    /// The card focused in the call view (a feed's key, or a person's id).
    pub focus: Option<String>,
    /// People's names and pictures, as profiles say.
    pub people: HashMap<OwnedUserId, Person>,
    asked: HashSet<OwnedUserId>,
    /// What this device last published in its call membership.
    published: Option<(OwnedRoomId, CallMedia)>,
    /// Calls ringing this account (newest last).
    pub rings: Vec<IncomingCall>,
    /// Your call ringing the other person: since when.
    pub ringing_out_since: Option<std::time::Instant>,
    /// Ticks of the rings' clock.
    pub tick: u64,
    /// Every sound played, in order (a test build shows them, `.sound-log`).
    pub sounds: Vec<&'static str>,
    /// Where a test build also writes them.
    pub sound_log: Option<PathBuf>,
    /// The call stage's height, once dragged.
    pub stage_height: Option<f32>,
    /// A voice channel's text chat is open beside its call.
    pub voice_chat: bool,
    /// The call stage's inner size as last painted (to size its cards).
    pub stage_measure: crate::call_ui::StageMeasure,
    /// The stage's edge being dragged: where the pointer was, the height then.
    pub stage_drag: Option<(f32, f32)>,
    /// One entity per video feed shown (a new picture redraws only its card).
    pub frames: crate::call_ui::Frames,
    /// The audio devices chosen, as kept in the data directory.
    pub devices: crate::view_model::calls::DevicePrefs,
}

impl Calls {
    /// Whether this account is in `room`'s call.
    pub fn in_call(&self, room: &OwnedRoomId) -> bool {
        self.state.as_ref().is_some_and(|c| &c.room_id == room)
    }

    /// The name to show for `user`: your nickname, else their profile's.
    pub fn name_of(&self, user: &OwnedUserId) -> String {
        self.people
            .get(user)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| user.localpart().to_owned())
    }
}

impl Shell {
    /// A sound: noted, and played where there is sound.
    pub fn chime(&mut self, sound: Sound) {
        self.calls.sounds.push(sound.key());
        sounds::play(sound, self.calls.sound_log.as_ref());
    }

    /// Set up what calls need once the account is known: the chosen devices
    /// and the rings.
    pub fn start_calls(&mut self, session: &Session, cx: &mut Context<Self>) {
        let dir = crate::data_dir();
        let prefs = crate::view_model::calls::read_device_prefs(&dir);
        morpheus_call::choose_audio_devices(morpheus_call::DeviceChoice {
            microphone: prefs.microphone.clone(),
            speaker: prefs.speaker.clone(),
        });
        morpheus_call::choose_camera(prefs.camera.clone());
        self.calls.devices = prefs;
        // The frames are drawn here as raw pixels: no JPEGs for nobody.
        morpheus_call::skip_jpeg_frames();
        if std::env::var_os("MORPHEUS_E2E_PORT").is_some() {
            self.calls.sound_log = Some(dir.join("played-sounds.log"));
        }
        let listening = session.ring_updates();
        let session = session.clone();
        cx.spawn(async move |this, cx| {
            let mut updates = match listening.await {
                Ok(updates) => updates,
                Err(err) => {
                    this.update(cx, |this, cx| this.fail("listening for calls", &err.to_string(), cx))
                        .ok();
                    return;
                }
            };
            while let Some(update) = updates.next().await {
                let alive = match update {
                    Ringing::Incoming(call) => {
                        let caller = call.caller.clone();
                        let alive = this
                            .update(cx, |this, cx| {
                                this.calls.rings.retain(|c| c.room_id != call.room_id);
                                this.calls.rings.push(call);
                                cx.notify();
                            })
                            .is_ok();
                        // The window in the background: the system is told too.
                        let background = cx
                            .update(|cx| {
                                cx.windows()
                                    .first()
                                    .and_then(|w| w.update(cx, |_, window, _| !window.is_window_active()).ok())
                                    .unwrap_or(true)
                            })
                            .unwrap_or(true);
                        let wanted = crate::view_model::settings::notifications_enabled(&crate::data_dir());
                        if alive && background && wanted {
                            let name = match session.profile(caller.clone()).await {
                                Ok(profile) => profile.name,
                                Err(_) => caller.localpart().to_owned(),
                            };
                            let hidden = crate::view_model::settings::notification_content(&crate::data_dir())
                                == crate::view_model::settings::NotificationContent::Hidden;
                            notify_ring(format!("{} is calling", if hidden { "Someone" } else { &name }));
                        }
                        alive
                    }
                    Ringing::Declined {
                        room_id,
                        by,
                        notification,
                    } => {
                        let alive = this
                            .update(cx, |this, cx| {
                                this.calls.rings.retain(|c| c.notification != notification);
                                cx.notify();
                            })
                            .is_ok();
                        if alive && session.user_id().as_ref() != Some(&by) {
                            declined(&session, &this, cx, room_id, by).await;
                        }
                        alive
                    }
                };
                if !alive {
                    break;
                }
            }
        })
        .detach();
        // The rings' clock: rings expire and callers give up, so look again
        // every second; ring every other second, and your own call to a DM
        // rings back every third.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if this.update(cx, |this, cx| this.ring_tick(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// One second of the rings' clock.
    fn ring_tick(&mut self, cx: &mut Context<Self>) {
        let me = self.session.as_ref().and_then(|s| s.user_id());
        if ringing_out(self.calls.state.as_ref(), &self.rooms, me.as_ref()) {
            let since = *self
                .calls
                .ringing_out_since
                .get_or_insert_with(std::time::Instant::now);
            let within = since.elapsed().as_millis() < u128::from(morpheus_core::RING_FOR_MS);
            if within && self.calls.tick % 3 == 0 {
                self.chime(Sound::Ringback);
            }
        } else {
            self.calls.ringing_out_since = None;
        }
        let ringing = self.ringing(now_ms());
        if ringing.is_empty() {
            if !self.calls.rings.is_empty() {
                self.calls.rings.clear();
            }
        } else if self.calls.tick % 2 == 0 && self.calls.state.is_none() {
            // In a call already: the card shows, quietly, so the call you
            // are in is not drowned out by a ringtone.
            self.chime(Sound::Ring);
        }
        self.calls.tick += 1;
        cx.notify();
    }

    /// The rings still worth answering.
    pub fn ringing(&self, now: u64) -> Vec<IncomingCall> {
        still_ringing(
            &self.calls.rings,
            self.calls.state.as_ref().map(|c| &c.room_id),
            &self.rooms,
            now,
        )
    }

    /// Reads the profile of everyone the call lists and cards show.
    pub fn fetch_people(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let mut wanted: Vec<OwnedUserId> = self
            .rooms
            .iter()
            .flat_map(|r| r.call_participants.iter().cloned())
            .collect();
        wanted.extend(self.calls.rings.iter().map(|r| r.caller.clone()));
        if let Some(state) = &self.calls.state {
            wanted.extend(state.video.iter().map(|f| f.user.clone()));
        }
        wanted.extend(session.user_id());
        for user in wanted {
            if !self.calls.asked.insert(user.clone()) {
                continue;
            }
            let asking = session.profile(user.clone());
            cx.spawn(async move |this, cx| {
                if let Ok(profile) = asking.await {
                    this.update(cx, |this, cx| {
                        this.calls.people.insert(
                            user,
                            Person {
                                name: profile.name,
                                avatar: profile.avatar,
                            },
                        );
                        cx.notify();
                    })
                    .ok();
                }
            })
            .detach();
        }
    }

    /// Join `room_id`'s call, leaving any other first.
    pub fn join_call(&mut self, room_id: OwnedRoomId, room_name: String, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let (room_id, room_name) = match call_room_for(&room_id, &self.rooms) {
            Some((elsewhere, name)) => {
                self.open_room(elsewhere.clone(), cx);
                (elsewhere, name)
            }
            None => (room_id, room_name),
        };
        if self.calls.in_call(&room_id) {
            return;
        }
        // This call's number: its events stop counting once another call (or
        // a hang-up) has come since.
        let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        let previous = (self.calls.media.take(), self.calls.state.take());
        if previous.1.is_some() {
            self.chime(Sound::Leave);
        }
        self.calls.state = Some(CallState::joining(room_id.clone(), room_name));
        self.calls.published = None;
        cx.notify();
        let leaving = previous.1.map(|state| (state.room_id, previous.0));
        cx.spawn(async move |this, cx| {
            if let Some((old_room, call)) = leaving {
                leave_media(&session, old_room, call).await;
            }
            run_call(session, this, cx, room_id, generation).await;
        })
        .detach();
    }

    /// Hang up.
    pub fn hang_up(&mut self, cx: &mut Context<Self>) {
        GENERATION.fetch_add(1, Ordering::SeqCst);
        let call = self.calls.media.take();
        let room = self.calls.state.take().map(|state| state.room_id);
        self.calls.published = None;
        if room.is_some() {
            self.chime(Sound::Leave);
        }
        cx.notify();
        let (Some(session), Some(room)) = (self.session.clone(), room) else {
            return;
        };
        cx.spawn(async move |_, _| leave_media(&session, room, call).await)
            .detach();
    }

    /// Mute and deafen as asked, with the chime and what the call hears.
    pub fn set_voice(&mut self, next: VoiceFlags, cx: &mut Context<Self>) {
        let before = self.calls.voice;
        if let Some(sound) = before.chime(next) {
            self.chime(sound);
        }
        self.calls.voice = next;
        if let Some(call) = &self.calls.media {
            call.set_muted(next.mic_off());
            call.set_deafened(next.deafened);
        }
        self.sync_published(cx);
        cx.notify();
    }

    /// Mute, or unmute (which undeafens too): the mic buttons and Ctrl+Shift+M.
    pub fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        self.set_voice(self.calls.voice.toggled_mute(), cx);
    }

    /// Deafen, or undeafen: the headphones button and Ctrl+Shift+D.
    pub fn toggle_deafen(&mut self, cx: &mut Context<Self>) {
        self.set_voice(self.calls.voice.toggled_deafen(), cx);
    }

    /// Publish what this device has on (muted, deafened, camera, screen) in
    /// its call membership, for everyone to show, when it changed.
    pub fn sync_published(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let Some(call) = self.calls.state.as_ref().filter(|c| c.connected) else {
            self.calls.published = None;
            return;
        };
        let now = (call.room_id.clone(), self.calls.voice.media(call));
        if self.calls.published.as_ref() == Some(&now) {
            return;
        }
        self.calls.published = Some(now.clone());
        let sending = session.set_call_media(now.0, now.1);
        cx.spawn(async move |_, _| {
            if let Err(err) = sending.await {
                eprintln!("morpheus-gpui: call media state not published: {err}");
            }
        })
        .detach();
    }

    /// Turn this device's camera on or off in the call going.
    pub fn set_camera(&mut self, on: bool, cx: &mut Context<Self>) {
        self.set_video(false, on, cx);
    }

    /// Share this device's screen in the call going, or stop.
    pub fn set_screen_share(&mut self, on: bool, cx: &mut Context<Self>) {
        self.set_video(true, on, cx);
    }

    fn set_video(&mut self, screen: bool, on: bool, cx: &mut Context<Self>) {
        type Switch = Pin<Box<dyn std::future::Future<Output = morpheus_call::Result<()>>>>;
        let Some(switch) = self.calls.media.as_ref().map(|call| -> Switch {
            if screen {
                Box::pin(call.set_screen_share(on))
            } else {
                Box::pin(call.set_camera(on))
            }
        }) else {
            return;
        };
        let key = if screen { LOCAL_SCREEN } else { LOCAL_CAMERA };
        if screen
            && on
            && let Some(state) = self.calls.state.as_mut()
        {
            state.choosing_screen = true;
            cx.notify();
        }
        let me = self.session.as_ref().and_then(|s| s.user_id());
        cx.spawn(async move |this, cx| {
            let switched = switch.await;
            this.update(cx, |this, cx| {
                if let Some(state) = this.calls.state.as_mut() {
                    if screen {
                        state.choosing_screen = false;
                    }
                    if switched.is_ok() {
                        state.local_video(me, key, screen, on);
                    }
                }
                match &switched {
                    // Changing one's mind in the screen picker is not a failure.
                    Err(err) if err.to_string().contains("cancelled") => {}
                    Err(err) if screen => this.fail("sharing the screen", &err.to_string(), cx),
                    Err(err) => this.fail("turning the camera on", &err.to_string(), cx),
                    Ok(()) => {}
                }
                this.sync_published(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Calls use these devices, the one going now included.
    pub fn use_devices(&mut self, cx: &mut Context<Self>) {
        let prefs = self.calls.devices.clone();
        morpheus_call::choose_audio_devices(morpheus_call::DeviceChoice {
            microphone: prefs.microphone,
            speaker: prefs.speaker,
        });
        if let Some(call) = &self.calls.media
            && let Err(err) = call.use_chosen_devices()
        {
            self.fail("switching audio devices", &err.to_string(), cx);
        }
    }

    /// Keeps the devices chosen for the next start.
    pub fn save_devices(&mut self, cx: &mut Context<Self>) {
        let saved =
            crate::view_model::calls::write_device_prefs(&crate::data_dir(), &self.calls.devices);
        if let Err(err) = saved {
            self.fail("saving the audio devices", &err.to_string(), cx);
        }
    }

    /// Answers a ring: opens the room and joins.
    pub fn answer_call(&mut self, call: &IncomingCall, cx: &mut Context<Self>) {
        self.calls.rings.retain(|r| r.notification != call.notification);
        let name = self
            .rooms
            .iter()
            .find(|r| r.id == call.room_id)
            .map(|r| r.name.clone())
            .unwrap_or_else(|| call.caller.to_string());
        self.open_room(call.room_id.clone(), cx);
        self.join_call(call.room_id.clone(), name, cx);
    }

    /// Declines a ring.
    pub fn decline_ring(&mut self, call: &IncomingCall, cx: &mut Context<Self>) {
        self.calls.rings.retain(|r| r.notification != call.notification);
        cx.notify();
        let Some(session) = self.session.clone() else {
            return;
        };
        let declining = session.decline_call(call.room_id.clone(), call.notification.clone());
        cx.spawn(async move |this, cx| {
            if let Err(err) = declining.await {
                this.update(cx, |this, cx| {
                    this.fail("declining the call", &err.to_string(), cx)
                })
                .ok();
            }
        })
        .detach();
    }
}

/// Matrix first (that is what everyone else sees), then the media.
async fn leave_media(session: &Session, room: OwnedRoomId, call: Option<Call>) {
    if let Err(err) = session.leave_call(room).await {
        eprintln!("morpheus-gpui: leaving the call: {err}");
    }
    if let Some(call) = call {
        call.leave().await;
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// The person called said no: say so, and hang up if nobody else came.
async fn declined(
    session: &Session,
    this: &WeakEntity<Shell>,
    cx: &mut AsyncApp,
    room_id: OwnedRoomId,
    by: OwnedUserId,
) {
    let in_that_call = this
        .read_with(cx, |this, _| this.calls.in_call(&room_id))
        .unwrap_or(false);
    if !in_that_call {
        return;
    }
    let name = match session.profile(by.clone()).await {
        Ok(profile) => profile.name,
        Err(_) => by.localpart().to_owned(),
    };
    let me = session.user_id();
    let room_for_log = room_id.clone();
    let alone = session
        .call_members(room_id)
        .await
        .map(|members| members.iter().all(|m| Some(&m.user_id) == me.as_ref()))
        .unwrap_or(true);
    this.update(cx, |this, cx| {
        eprintln!("morpheus-gpui: {name} declined the call in {room_for_log} (alone: {alone})");
        this.toast(format!("{name} declined the call"), cx);
        if alone {
            this.hang_up(cx);
        }
    })
    .ok();
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

/// What the call's loop waits on.
enum Next {
    Event(Option<CallEvent>),
    Key(Option<morpheus_core::CallKey>),
    Tick,
}

/// A call joined: credentials, the media connection, then its events until
/// it ends.
async fn run_call(
    session: Session,
    this: WeakEntity<Shell>,
    cx: &mut AsyncApp,
    room_id: OwnedRoomId,
    generation: u64,
) {
    let current = move || GENERATION.load(Ordering::SeqCst) == generation;
    let wanted = room_id.clone();
    let still_wanted = |this: &WeakEntity<Shell>, cx: &mut AsyncApp| {
        this.read_with(cx, |this, _| this.calls.in_call(&wanted))
            .unwrap_or(false)
    };
    // Gives the call up (still the one asked for): hung up in state.
    let give_up = |this: &WeakEntity<Shell>, cx: &mut AsyncApp, what: &str, err: String| {
        let wanted = room_id.clone();
        this.update(cx, |this, cx| {
            this.fail(what, &err, cx);
            if this.calls.in_call(&wanted) {
                this.calls.state = None;
                cx.notify();
            }
        })
        .ok();
    };
    let credentials = match session.join_call(room_id.clone()).await {
        Ok(credentials) => credentials,
        Err(err) => return give_up(&this, cx, "joining the call", err.to_string()),
    };
    // Keys others share, subscribed before ours goes out so no answer is missed.
    let mut keys = match &credentials.media_key {
        Some(_) => match session.call_keys(room_id.clone()).await {
            Ok(keys) => Some(keys),
            Err(_) => None,
        },
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
            let _ = session.leave_call(room_id.clone()).await;
            return give_up(&this, cx, "connecting the call", err.to_string());
        }
    };
    if !still_wanted(&this, cx) {
        // Hung up (or moved on) while connecting.
        call.leave().await;
        let _ = session.leave_call(room_id.clone()).await;
        return;
    }
    // This device's name in the call, sent along with its key: the token
    // service's names cannot be worked out everywhere (matrix.org's cannot),
    // so the others learn it from us.
    let own_identity = call.identity();
    let mut people: HashMap<String, OwnedUserId> = HashMap::new();
    if let Some(me) = session.user_id() {
        people.insert(own_identity.clone(), me);
    }
    let kept = this.update(cx, |this, cx| {
        let flags = this.calls.voice;
        call.set_muted(flags.mic_off());
        call.set_deafened(flags.deafened);
        this.calls.media = Some(call);
        if let Some(state) = this.calls.state.as_mut() {
            state.connected = true;
        }
        this.chime(Sound::Join);
        this.sync_published(cx);
        cx.notify();
    });
    if kept.is_err() {
        return;
    }
    // Our media key, to everyone in the call (again whenever someone joins).
    let share_key = || {
        let sending = credentials.media_key.clone().map(|key| {
            session.send_call_key(room_id.clone(), key, own_identity.clone())
        });
        async move {
            if let Some(sending) = sending
                && let Err(err) = sending.await
            {
                eprintln!("morpheus-gpui: sharing the call's media key: {err}");
            }
        }
    };
    share_key().await;

    loop {
        let next = {
            let event = events.recv().fuse();
            let key = async {
                match keys.as_mut() {
                    Some(keys) => keys.next().await,
                    None => std::future::pending().await,
                }
            }
            .fuse();
            let tick = cx.background_executor().timer(Duration::from_secs(1)).fuse();
            futures::pin_mut!(event, key, tick);
            futures::select_biased! {
                event = event => Next::Event(event),
                key = key => Next::Key(key),
                _ = tick => Next::Tick,
            }
        };
        if !current() || !still_wanted(&this, cx) {
            return;
        }
        let event = match next {
            Next::Tick => continue,
            Next::Event(None) => return,
            Next::Event(Some(event)) => event,
            Next::Key(None) => {
                keys = None;
                continue;
            }
            Next::Key(Some(key)) => {
                // Its sender's name in the call, when it says: who is
                // speaking under that name.
                if let Some(identity) = &key.livekit_identity {
                    people.insert(identity.clone(), key.user_id.clone());
                }
                let applied = this.read_with(cx, |this, _| {
                    if let Some(call) = &this.calls.media {
                        for identity in &key.livekit_identities {
                            call.set_key(identity, key.key.clone());
                        }
                    }
                });
                if applied.is_err() {
                    return;
                }
                continue;
            }
        };
        match event {
            CallEvent::Joined(_) => {
                this.update(cx, |this, _| this.chime(Sound::Join)).ok();
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
                this.update(cx, |this, cx| {
                    if let Some(state) = this.calls.state.as_mut()
                        && state.speaking != speaking
                    {
                        state.speaking = speaking;
                        cx.notify();
                    }
                })
                .ok();
            }
            // Test tones only: whose audio actually arrives (decrypted).
            CallEvent::Heard(ref identity) | CallEvent::Quiet(ref identity) => {
                if !people.contains_key(identity) {
                    refresh_people(&session, &room_id, &mut people).await;
                }
                let heard = matches!(event, CallEvent::Heard(_));
                if let Some(user) = people.get(identity).cloned() {
                    this.update(cx, |this, cx| {
                        if let Some(state) = this.calls.state.as_mut() {
                            state.heard_event(user, heard);
                            cx.notify();
                        }
                    })
                    .ok();
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
                this.update(cx, |this, cx| {
                    if let Some(state) = this.calls.state.as_mut() {
                        state.video_event(user, key, screen, on);
                        cx.notify();
                    }
                })
                .ok();
            }
            CallEvent::Disconnected(reason) => {
                // A call left already (hung up, or replaced by a rejoin) says
                // goodbye late: not the call going now.
                if !current() {
                    return;
                }
                this.update(cx, |this, cx| {
                    this.fail("the call dropped", &reason, cx);
                    this.hang_up(cx);
                })
                .ok();
                return;
            }
            CallEvent::Left(_) => {
                this.update(cx, |this, _| this.chime(Sound::Leave)).ok();
            }
        }
    }
}

impl Shell {
    /// The camera button's right-click: the cameras, to pick from.
    pub fn open_camera_menu(&mut self, at: gpui::Point<gpui::Pixels>, cx: &mut Context<Self>) {
        use crate::view_model::{calls::{camera_rows, device_menu}, menu::DeviceKind};
        let cameras: Vec<(String, String)> =
            morpheus_call::cameras(AudioMode::from_env())
                .into_iter()
                .map(|c| (c.id, c.name))
                .collect();
        let rows = camera_rows(&cameras, self.calls.devices.camera.as_deref());
        let items = device_menu(DeviceKind::Camera, &rows);
        self.open_menu(crate::menu::OpenMenu::new(at, items, false, crate::view_model::menu::MenuTarget::Other), cx);
    }

    /// The quick list of microphones (or, with `speakers`, speakers).
    pub fn open_audio_menu(
        &mut self,
        speakers: bool,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        use crate::view_model::{calls::{device_menu, device_rows}, menu::DeviceKind};
        let found = morpheus_call::audio_devices(AudioMode::from_env());
        cx.spawn(async move |this, cx| {
            let found = found.await;
            this.update(cx, |this, cx| {
                let (list, current, kind) = if speakers {
                    (found.speakers, this.calls.devices.speaker.clone(), DeviceKind::Speaker)
                } else {
                    (found.microphones, this.calls.devices.microphone.clone(), DeviceKind::Microphone)
                };
                let list: Vec<(String, String)> = list.into_iter().map(|d| (d.id, d.name)).collect();
                let items = device_menu(kind, &device_rows(&list, current.as_deref()));
                this.open_menu(
                    crate::menu::OpenMenu::new(at, items, false, crate::view_model::menu::MenuTarget::Other),
                    cx,
                );
            })
            .ok();
        })
        .detach();
    }

    /// A device chosen from a quick list: used from now on, a call going
    /// now switches over, and kept for the next start.
    pub fn pick_device(
        &mut self,
        kind: crate::view_model::menu::DeviceKind,
        id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        use crate::view_model::menu::DeviceKind;
        match kind {
            DeviceKind::Microphone => self.calls.devices.microphone = id,
            DeviceKind::Speaker => self.calls.devices.speaker = id,
            DeviceKind::Camera => {
                self.calls.devices.camera = id.clone();
                morpheus_call::choose_camera(id);
                self.save_devices(cx);
                // On now: turned off and on again with the new one.
                if self.calls.state.as_ref().is_some_and(|c| c.camera) {
                    self.set_camera(false, cx);
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(Duration::from_millis(400)).await;
                        this.update(cx, |this, cx| this.set_camera(true, cx)).ok();
                    })
                    .detach();
                }
                return;
            }
        }
        self.save_devices(cx);
        self.use_devices(cx);
    }
}

/// Someone calling while the window is in the background: say who (the
/// ringtone plays in the app either way). Test runs never notify (they would
/// pile real pop-ups on the developer's desktop): they log what would show
/// and write it to `shown-notifications.jsonl`.
fn notify_ring(title: String) {
    crate::notify::show(
        title,
        "Open Morpheus to answer".into(),
        None,
        None,
        Some(crate::notify::RING_SOUND),
    );
}
