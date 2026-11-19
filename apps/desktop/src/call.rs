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
use morpheus_core::{
    CallMedia, IncomingCall, MediaRef, OwnedRoomId, OwnedUserId, Ringing, RoomSummary, Session,
};

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
    /// The strip of the other cards under the focused one is hidden (kept for the session).
    pub strip_hidden: bool,
    /// The LiveKit identity of each user sharing their screen (to watch it by).
    pub share_identity: HashMap<OwnedUserId, String>,
    /// People's names and pictures, as profiles say.
    pub people: HashMap<OwnedUserId, Person>,
    asked: HashSet<OwnedUserId>,
    /// What this device last published in its call membership.
    published: Option<(OwnedRoomId, CallMedia)>,
    /// Calls ringing this account (newest last).
    pub rings: Vec<IncomingCall>,
    /// Your call ringing the other person: since when.
    pub ringing_out_since: Option<std::time::Instant>,
    /// When this account joined the call it is in (ms since the epoch): a decline of a
    /// ring sent before that is a stale one.
    pub joined_at_ms: u64,
    /// Calls just hung up: the room and when (ms), so its list stops showing
    /// this account at once rather than when the server has answered.
    pub left: Vec<(OwnedRoomId, u64)>,
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
    /// How the screen is shared, as last chosen (kept in the data directory).
    pub screen_quality: morpheus_call::ScreenQuality,
    /// Whose shared screen has sound now, and their name in the call.
    pub screen_audio: HashMap<OwnedUserId, String>,
    /// The volume given to each person's screen sound (1 as sent; up to
    /// [`morpheus_call::MAX_SCREEN_VOLUME`]), for as long as the app runs.
    pub screen_volumes: HashMap<OwnedUserId, f32>,
    /// The volume given to each person's voice (1 as sent) and who is muted
    /// for you, for as long as the app runs.
    pub user_volumes: HashMap<OwnedUserId, f32>,
    pub muted_for_me: std::collections::HashSet<OwnedUserId>,
    /// Whose shared screen's sound is muted for you (apart from their voice).
    pub stream_muted: std::collections::HashSet<OwnedUserId>,
    /// The signed-in account, whose per-person volumes are kept (`person-volumes.json`).
    pub account: Option<OwnedUserId>,
    /// Share screen's question is open, over this point.
    pub screen_popover: Option<gpui::Point<gpui::Pixels>>,
    /// The noise suppression card is open, above this point (the button's
    /// top left, [`Self::noise_button_at`]).
    pub noise_card: Option<gpui::Point<gpui::Pixels>>,
    /// Where the noise button was last drawn.
    pub noise_button_at: std::rc::Rc<std::cell::Cell<gpui::Point<gpui::Pixels>>>,
    /// Mute and deafen as they were before the mic test deafened you.
    pub mic_test_before: Option<VoiceFlags>,
    /// A redraw for the mic's meter or processing is on its way.
    pub mic_refresh: std::cell::Cell<bool>,
    /// The last join that did not come off (shown with a way to try again).
    pub join_error: Option<JoinError>,
    /// The mini call window (issue #86): which feed it shows over time,
    /// where it sits (kept on this device), a press on it going on, and
    /// when a redraw is due for a speaker's hold.
    pub mini_pick: crate::view_model::mini_call::MiniPick,
    pub mini_place: crate::view_model::mini_call::MiniPlace,
    pub mini_drag: Option<crate::mini_call::MiniDrag>,
    pub mini_wake: Option<u64>,
}

/// A join that failed: which call, and why in plain words.
#[derive(Debug, Clone)]
pub struct JoinError {
    pub room_id: OwnedRoomId,
    pub room_name: String,
    pub message: String,
}

impl Calls {
    /// Whether this account just hung up in `room`: its list should not show
    /// it yet, whatever the server has said so far.
    pub fn just_left(&self, room: &OwnedRoomId) -> bool {
        self.left
            .iter()
            .any(|(left, at)| left == room && now_ms().saturating_sub(*at) < LEFT_FOR_MS)
    }

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
    /// Who `room`'s call shows: the server's list, less this account right
    /// after it hung up there (the list, and the badges on the rail).
    /// How long the room's call has been going (`m:ss`), when someone is in it.
    pub fn call_timer(&self, room: &RoomSummary) -> Option<String> {
        if self.call_shown(room).is_empty() {
            return None;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or_default();
        crate::view_model::calls::call_timer(now, room.call_started_ms)
    }

    pub fn call_shown(&self, room: &RoomSummary) -> Vec<OwnedUserId> {
        let me = self.session.as_ref().and_then(|s| s.user_id());
        crate::view_model::calls::shown_participants(
            room,
            me.as_ref(),
            self.calls.in_call(&room.id),
            self.calls.just_left(&room.id),
        )
    }

    /// The server has said this account left: its call lists go by the
    /// server again (so another device of yours joining shows at once).
    pub fn forget_confirmed_leaves(&mut self) {
        let me = self.session.as_ref().and_then(|s| s.user_id());
        let rooms = &self.rooms;
        self.calls.left.retain(|(left, _)| {
            rooms
                .iter()
                .find(|r| &r.id == left)
                .is_some_and(|r| me.as_ref().is_some_and(|me| r.call_participants.contains(me)))
        });
    }

    /// Whether `room` has a call to show (see [`Self::call_shown`]).
    pub fn has_call_shown(&self, room: &RoomSummary) -> bool {
        !room.call_participants.is_empty() && !self.call_shown(room).is_empty()
    }

    /// A sound: noted, and played where there is sound.
    pub fn chime(&mut self, sound: Sound) {
        self.calls.sounds.push(sound.key());
        sounds::play(sound, self.calls.sound_log.as_ref());
    }

    /// Have what joining waits for ready: the call service and an OpenID
    /// token, and for a voice channel or a DM its LiveKit token.
    pub fn prepare_call(&mut self, room: Option<OwnedRoomId>, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let wanted = room.filter(|id| {
            self.rooms
                .iter()
                .any(|r| &r.id == id && (r.is_voice || r.is_dm))
        });
        let preparing = session.prepare_calls(wanted);
        cx.spawn(async move |_, _| {
            let _ = preparing.await;
        })
        .detach();
    }

    /// Set up what calls need once the account is known: the chosen devices
    /// and the rings.
    pub fn start_calls(&mut self, session: &Session, cx: &mut Context<Self>) {
        // Ready before anyone clicks, and kept ready (a token lasts about an hour).
        let preparing = session.prepare_calls(None);
        cx.spawn(async move |_, _| {
            let _ = preparing.await;
        })
        .detach();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(10 * 60))
                    .await;
                if this
                    .update(cx, |this, cx| this.prepare_call(None, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let dir = crate::data_dir();
        let prefs = crate::view_model::calls::read_device_prefs(&dir);
        morpheus_call::choose_audio_devices(morpheus_call::DeviceChoice {
            microphone: prefs.microphone.clone(),
            speaker: prefs.speaker.clone(),
        });
        morpheus_call::choose_camera(prefs.camera.clone());
        morpheus_call::set_input_volume(prefs.input_volume);
        morpheus_call::set_output_volume(prefs.output_volume);
        morpheus_call::set_hardware_video(prefs.hardware_video);
        morpheus_call::set_screen_audio_leaves_out_discord(
            crate::view_model::settings::leaves_out_discord(&dir),
        );
        morpheus_call::set_noise_suppression(crate::noise::read());
        morpheus_call::set_noise_tuning(crate::noise::read_tuning());
        self.calls.devices = prefs;
        self.calls.account = session.user_id();
        self.load_person_volumes();
        self.calls.screen_quality = crate::screen_quality::read();
        self.calls.mini_place = crate::mini_call::load_place();
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
                    this.update(cx, |this, cx| {
                        this.fail("listening for calls", &err.to_string(), cx)
                    })
                    .ok();
                    return;
                }
            };
            while let Some(update) = updates.next().await {
                let alive = match update {
                    Ringing::Incoming(call) => {
                        let caller = call.caller.clone();
                        let ring_room = call.room_id.clone();
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
                                    .and_then(|w| {
                                        w.update(cx, |_, window, _| !window.is_window_active()).ok()
                                    })
                                    .unwrap_or(true)
                            })
                            .unwrap_or(true);
                        let wanted =
                            crate::view_model::settings::notifications_enabled(&crate::data_dir());
                        if alive && background && wanted {
                            let name = match session.profile(caller.clone()).await {
                                Ok(profile) => profile.name,
                                Err(_) => caller.localpart().to_owned(),
                            };
                            let hidden = crate::view_model::settings::notification_content(
                                &crate::data_dir(),
                            )
                                == crate::view_model::settings::NotificationContent::Hidden;
                            let title = if crate::locks::is_locked(&ring_room) {
                                crate::locks::LOCKED_CALL.to_owned()
                            } else {
                                format!("{} is calling", if hidden { "Someone" } else { &name })
                            };
                            notify_ring(title);
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
                            declined(&session, &this, cx, room_id, by, notification).await;
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
                self.open_room_now(elsewhere.clone(), cx);
                (elsewhere, name)
            }
            None => (room_id, room_name),
        };
        if self.calls.in_call(&room_id) {
            return;
        }
        self.calls.join_error = None;
        // This call's number: its events stop counting once another call (or
        // a hang-up) has come since.
        let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        let previous = (self.calls.media.take(), self.calls.state.take());
        self.calls.screen_audio.clear();
        if previous.1.is_some() {
            self.chime(Sound::Leave);
        }
        self.calls.state = Some(CallState::joining(room_id.clone(), room_name));
        // The channel's chat loads meanwhile, so its panel opens full.
        self.warm_timeline(room_id.clone(), cx);
        self.calls.joined_at_ms = now_ms();
        self.calls.left.retain(|(left, _)| *left != room_id);
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
        // The mic test deafened you for this call: as you were.
        self.calls.noise_card = None;
        if let Some(before) = self.calls.mic_test_before.take() {
            morpheus_call::stop_mic_test();
            self.calls.voice = before;
        }
        GENERATION.fetch_add(1, Ordering::SeqCst);
        self.calls.join_error = None;
        let call = self.calls.media.take();
        let room = self.calls.state.take().map(|state| state.room_id);
        self.calls.screen_audio.clear();
        self.calls.published = None;
        if let Some(room) = &room {
            self.calls
                .left
                .retain(|(_, at)| now_ms().saturating_sub(*at) < LEFT_FOR_MS);
            self.calls.left.push((room.clone(), now_ms()));
        }
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

    /// Hang up before the app quits, waiting for it (this blocks the caller)
    /// up to [`QUIT_LEAVE_FOR`]: the call membership's leave, then LiveKit.
    /// Without it the others see a ghost in the call until the homeserver's
    /// delayed leave fires, or for hours where delayed events are off
    /// (matrix.org). A quit must never hang, so after that it gives up.
    pub fn leave_before_quit(&mut self) {
        GENERATION.fetch_add(1, Ordering::SeqCst);
        let call = self.calls.media.take();
        let room = self.calls.state.take().map(|state| state.room_id);
        let (Some(session), Some(room)) = (self.session.clone(), room) else {
            return;
        };
        eprintln!("morpheus-desktop: quitting: leaving the call first");
        // Run here, on the caller: a watchdog ends the app if it takes too long.
        std::thread::spawn(|| {
            std::thread::sleep(QUIT_LEAVE_FOR);
            eprintln!("morpheus-desktop: quitting: leaving the call timed out");
            std::process::exit(0);
        });
        futures::executor::block_on(leave_media(&session, room, call));
        eprintln!("morpheus-desktop: quitting: left the call");
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
                eprintln!("morpheus-desktop: call media state not published: {err}");
            }
        })
        .detach();
    }

    /// Turn this device's camera on or off in the call going.
    pub fn set_camera(&mut self, on: bool, cx: &mut Context<Self>) {
        // The picture shows in the call's view: go there from another room.
        if on
            && let Some(room) = self.calls.state.as_ref().map(|c| c.room_id.clone())
            && self.selected.as_ref() != Some(&room)
        {
            self.open_room_now(room, cx);
        }
        self.set_video(false, on, cx);
    }

    /// The Share screen button, pressed at `at`: sharing stops at once; where
    /// it is off, asks how to share first (Go Live starts it).
    pub fn press_screen(
        &mut self,
        on: bool,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        if on {
            self.ask_screen_quality(at, cx);
        } else {
            self.set_screen_share(false, cx);
        }
    }

    /// Share `source` (picked in the app), or with `None` what the system's
    /// picker says (Wayland's portal, macOS's).
    pub fn share_screen_from(
        &mut self,
        source: Option<morpheus_call::ScreenSource>,
        cx: &mut Context<Self>,
    ) {
        if let Some(room) = self.calls.state.as_ref().map(|c| c.room_id.clone())
            && self.selected.as_ref() != Some(&room)
        {
            self.open_room_now(room, cx);
        }
        self.set_video_from(true, true, source, cx);
    }

    /// Share this device's screen in the call going, or stop.
    pub fn set_screen_share(&mut self, on: bool, cx: &mut Context<Self>) {
        // The picture shows in the call's view: go there from another room.
        if on
            && let Some(room) = self.calls.state.as_ref().map(|c| c.room_id.clone())
            && self.selected.as_ref() != Some(&room)
        {
            self.open_room_now(room, cx);
        }
        self.set_video(true, on, cx);
    }

    fn set_video(&mut self, screen: bool, on: bool, cx: &mut Context<Self>) {
        self.set_video_from(screen, on, None, cx);
    }

    fn set_video_from(
        &mut self,
        screen: bool,
        on: bool,
        source: Option<morpheus_call::ScreenSource>,
        cx: &mut Context<Self>,
    ) {
        type Switch = Pin<Box<dyn std::future::Future<Output = morpheus_call::Result<()>>>>;
        let quality = self.calls.screen_quality;
        let Some(switch) = self.calls.media.as_ref().map(|call| -> Switch {
            if screen {
                Box::pin(call.set_screen_share_from(on, quality, source))
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
                    Ok(()) if screen && on => {
                        this.chime(Sound::ScreenOn);
                        // The screen went, but its sound could not.
                        if let Some(Err(why)) = morpheus_call::sent_screen_audio() {
                            this.toast(format!("Sharing without sound: {why}"), cx);
                        }
                    }
                    Ok(()) => this.chime(match (screen, on) {
                        (false, true) => Sound::CameraOn,
                        (false, false) => Sound::CameraOff,
                        (true, true) => Sound::ScreenOn,
                        (true, false) => Sound::ScreenOff,
                    }),
                }
                this.sync_published(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// How loud `user`'s shared screen plays here (1 as sent), now and the
    /// next time they share with sound.
    pub fn set_screen_volume(&mut self, user: &OwnedUserId, volume: f32, cx: &mut Context<Self>) {
        let volume = volume.clamp(0.0, morpheus_call::MAX_SCREEN_VOLUME);
        self.calls.screen_volumes.insert(user.clone(), volume);
        self.apply_stream_volume(user);
        self.save_person_volume(user);
        if let Some(menu) = &mut self.menu {
            for item in &mut menu.items {
                if let Some(crate::view_model::menu::MenuSlider::StreamVolume(who, v)) =
                    &mut item.slider
                    && who == user
                {
                    *v = volume;
                }
            }
        }
        cx.notify();
    }

    /// Start or stop watching `user`'s shared screen: nothing of it is
    /// received until asked for, and its sender sends nothing while nobody asks.
    pub fn watch_share(&mut self, user: &OwnedUserId, on: bool, cx: &mut Context<Self>) {
        let Some(state) = self.calls.state.as_mut() else {
            return;
        };
        state.set_watching(user.clone(), on);
        if let (Some(call), Some(identity)) = (&self.calls.media, self.calls.share_identity.get(user)) {
            call.watch(identity, on);
        }
        cx.notify();
    }

    /// Silences the sound of `user`'s shared screen for you alone, or brings
    /// it back at its volume. Their voice is not touched.
    pub fn set_stream_muted(&mut self, user: &OwnedUserId, muted: bool, cx: &mut Context<Self>) {
        if muted {
            self.calls.stream_muted.insert(user.clone());
        } else {
            self.calls.stream_muted.remove(user);
        }
        self.apply_stream_volume(user);
        self.save_person_volume(user);
        cx.notify();
    }

    /// The volumes kept for the signed-in account: applied now, so they hold
    /// when that person's tracks appear.
    fn load_person_volumes(&mut self) {
        let Some(account) = self.calls.account.clone() else {
            return;
        };
        let all = crate::view_model::calls::read_person_volumes(&crate::data_dir(), account.as_str());
        for (user, v) in all {
            let Ok(user) = OwnedUserId::try_from(user) else {
                continue;
            };
            self.calls.user_volumes.insert(user.clone(), v.voice);
            self.calls.screen_volumes.insert(user.clone(), v.stream);
            if v.voice_muted {
                self.calls.muted_for_me.insert(user.clone());
            }
            if v.stream_muted {
                self.calls.stream_muted.insert(user.clone());
            }
            self.apply_user_volume(&user);
            self.apply_stream_volume(&user);
        }
    }

    /// Keeps what is now chosen for `user` (the defaults are not kept).
    fn save_person_volume(&self, user: &OwnedUserId) {
        let Some(account) = &self.calls.account else {
            return;
        };
        let volume = crate::view_model::calls::PersonVolume {
            voice: self.calls.user_volumes.get(user).copied().unwrap_or(1.0),
            voice_muted: self.calls.muted_for_me.contains(user),
            stream: self.calls.screen_volumes.get(user).copied().unwrap_or(1.0),
            stream_muted: self.calls.stream_muted.contains(user),
        };
        if let Err(err) = crate::view_model::calls::write_person_volume(
            &crate::data_dir(),
            account.as_str(),
            user.as_str(),
            volume,
        ) {
            eprintln!("morpheus-desktop: saving volumes: {err}");
        }
    }

    pub(crate) fn apply_stream_volume(&self, user: &OwnedUserId) {
        let volume = if self.calls.stream_muted.contains(user) {
            0.0
        } else {
            self.calls.screen_volumes.get(user).copied().unwrap_or(1.0)
        };
        morpheus_call::set_user_track_volume(
            user.as_str(),
            morpheus_call::TrackSource::ScreenShare,
            volume,
        );
        if let (Some(call), Some(identity)) = (&self.calls.media, self.calls.screen_audio.get(user))
        {
            call.set_screen_volume(identity, volume);
        }
    }

    /// How loud `user`'s voice plays here (1 as sent), unless muted for you.
    pub fn set_user_volume(&mut self, user: &OwnedUserId, volume: f32, cx: &mut Context<Self>) {
        let volume = volume.clamp(0.0, morpheus_call::MAX_VOLUME);
        self.calls.user_volumes.insert(user.clone(), volume);
        self.apply_user_volume(user);
        self.save_person_volume(user);
        if let Some(menu) = &mut self.menu {
            for item in &mut menu.items {
                if let Some(crate::view_model::menu::MenuSlider::UserVolume(who, v)) =
                    &mut item.slider
                    && who == user
                {
                    *v = volume;
                }
            }
        }
        cx.notify();
    }

    /// Silences `user` for you alone, or brings them back at their volume.
    pub fn set_user_muted(&mut self, user: &OwnedUserId, muted: bool, cx: &mut Context<Self>) {
        if muted {
            self.calls.muted_for_me.insert(user.clone());
        } else {
            self.calls.muted_for_me.remove(user);
        }
        self.apply_user_volume(user);
        self.save_person_volume(user);
        cx.notify();
    }

    fn apply_user_volume(&self, user: &OwnedUserId) {
        let volume = if self.calls.muted_for_me.contains(user) {
            0.0
        } else {
            self.calls.user_volumes.get(user).copied().unwrap_or(1.0)
        };
        match &self.calls.media {
            Some(call) => call.set_user_volume(user.as_str(), volume),
            None => morpheus_call::set_user_volume(user.as_str(), volume),
        }
    }

    /// Sets input (microphone) volume multiplier (0.0 to 2.0).
    pub fn set_input_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
        let volume = volume.clamp(0.0, morpheus_call::MAX_VOLUME);
        morpheus_call::set_input_volume(volume);
        self.calls.devices.input_volume = volume;
        self.save_devices(cx);
        if let Some(menu) = &mut self.menu {
            for item in &mut menu.items {
                if let Some(crate::view_model::menu::MenuSlider::InputVolume(v)) = &mut item.slider
                {
                    *v = volume;
                }
            }
        }
        cx.notify();
    }

    /// Sets output (speaker) volume multiplier (0.0 to 2.0).
    pub fn set_output_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
        let volume = volume.clamp(0.0, morpheus_call::MAX_VOLUME);
        morpheus_call::set_output_volume(volume);
        self.calls.devices.output_volume = volume;
        if let Some(call) = &self.calls.media {
            call.set_output_volume(volume);
        }
        self.save_devices(cx);
        if let Some(menu) = &mut self.menu {
            for item in &mut menu.items {
                if let Some(crate::view_model::menu::MenuSlider::OutputVolume(v)) = &mut item.slider
                {
                    *v = volume;
                }
            }
        }
        cx.notify();
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
        self.calls
            .rings
            .retain(|r| r.notification != call.notification);
        let name = self
            .rooms
            .iter()
            .find(|r| r.id == call.room_id)
            .map(|r| r.name.clone())
            .unwrap_or_else(|| call.caller.to_string());
        self.open_room_now(call.room_id.clone(), cx);
        self.join_call(call.room_id.clone(), name, cx);
    }

    /// Declines a ring.
    pub fn decline_ring(&mut self, call: &IncomingCall, cx: &mut Context<Self>) {
        self.calls
            .rings
            .retain(|r| r.notification != call.notification);
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
        eprintln!("morpheus-desktop: leaving the call: {err}");
    }
    if let Some(call) = call {
        call.leave().await;
    }
}

/// How long a hung-up room's list and badges hide this account at most while
/// the server catches up: until its sync says so ([`Shell::forget_confirmed_leaves`]),
/// which on a busy homeserver (matrix.org) can take well over ten seconds.
const LEFT_FOR_MS: u64 = 60_000;

/// How long quitting waits to hang up cleanly before it ends the app anyway.
const QUIT_LEAVE_FOR: Duration = Duration::from_secs(2);

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
    notification: morpheus_core::OwnedEventId,
) {
    let in_that_call = this
        .read_with(cx, |this, _| this.calls.in_call(&room_id))
        .unwrap_or(false);
    if !in_that_call {
        return;
    }
    // A decline of a ring from before this call (one of an earlier call, still
    // arriving as the app catches up) is not this call's: the ring this call
    // sent goes out after it joined.
    let joined = this
        .read_with(cx, |this, _| this.calls.joined_at_ms)
        .unwrap_or(0);
    if let Ok(Some(sent)) = session.event_sent_at(room_id.clone(), notification).await
        && sent + 5_000 < joined
    {
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
        eprintln!("morpheus-desktop: {name} declined the call in {room_for_log} (alone: {alone})");
        this.toast(
            format!("{} declined the call", crate::streamer::name(&name)),
            cx,
        );
        if alone {
            this.hang_up(cx);
        }
    })
    .ok();
}

/// Match LiveKit identities to people, from the room's call memberships.
/// Returns the names newly matched.
async fn refresh_people(
    session: &Session,
    room_id: &OwnedRoomId,
    people: &mut HashMap<String, OwnedUserId>,
) -> Vec<String> {
    let mut learned = Vec::new();
    if let Ok(members) = session.call_members(room_id.clone()).await {
        for member in members {
            for identity in member.livekit_identities {
                if people.get(&identity) != Some(&member.user_id) {
                    people.insert(identity.clone(), member.user_id.clone());
                    learned.push(identity);
                }
            }
        }
    }
    learned
}

/// What the call said under each LiveKit name, kept as it said it. A name
/// is not always known when its voice or picture arrives: when the token
/// service's names cannot be worked out from user and device (matrix.org),
/// the name comes with the person's media key, which can land after their
/// voice; what was said under it must show once it is known.
#[derive(Default)]
struct ByIdentity {
    /// Who is speaking, the last the call said.
    speaking: Vec<String>,
    /// Whose test tone is heard (true) or quiet.
    heard: HashMap<String, bool>,
    /// Feeds whose sender is not known yet: feed key -> (name, screen).
    unplaced: HashMap<String, (String, bool)>,
}

/// Names just matched to people: what the call said under them shows now.
fn show_learned(
    this: &WeakEntity<Shell>,
    cx: &mut AsyncApp,
    people: &HashMap<String, OwnedUserId>,
    said: &mut ByIdentity,
    learned: &[String],
) {
    if learned.is_empty() {
        return;
    }
    // Their volumes play on these names from now on (a name is no user ID).
    let mut owners: Vec<OwnedUserId> = Vec::new();
    for identity in learned {
        if let Some(user) = people.get(identity) {
            morpheus_call::note_voice_identity(identity, user.as_str());
            if !owners.contains(user) {
                owners.push(user.clone());
            }
        }
    }
    this.update(cx, |this, _| {
        for user in &owners {
            this.apply_user_volume(user);
            this.apply_stream_volume(user);
        }
    })
    .ok();
    let speaking: Vec<OwnedUserId> = said
        .speaking
        .iter()
        .filter_map(|id| people.get(id).cloned())
        .collect();
    let heard: Vec<(OwnedUserId, bool)> = learned
        .iter()
        .filter_map(|id| Some((people.get(id)?.clone(), *said.heard.get(id)?)))
        .collect();
    let placed: Vec<String> = said
        .unplaced
        .iter()
        .filter(|(_, (id, _))| people.contains_key(id))
        .map(|(key, _)| key.clone())
        .collect();
    let feeds: Vec<(OwnedUserId, String, bool)> = placed
        .into_iter()
        .filter_map(|key| {
            let (id, screen) = said.unplaced.remove(&key)?;
            Some((people.get(&id)?.clone(), key, screen))
        })
        .collect();
    if learned.iter().any(|id| said.speaking.contains(id)) || !heard.is_empty() || !feeds.is_empty()
    {
        eprintln!(
            "morpheus-desktop: call names learned after their media: {learned:?} (speaking {}, heard {}, feeds {})",
            speaking.len(),
            heard.len(),
            feeds.len()
        );
    }
    this.update(cx, |this, cx| {
        if let Some(state) = this.calls.state.as_mut() {
            state.speaking = speaking;
            for (user, heard) in heard {
                state.heard_event(user, heard);
            }
            for (user, key, screen) in feeds {
                state.video_event(Some(user), key, screen, true);
            }
            cx.notify();
        }
    })
    .ok();
}

/// What the call's loop waits on.
enum Next {
    Event(Option<CallEvent>),
    Key(Option<morpheus_core::CallKey>),
    /// A token for another SFU of the call, or why there is none.
    Focus(String, Result<morpheus_core::CallCredentials, String>),
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
    // Gives the call up (still the one asked for): hung up in state, and
    // why shown where the call's controls were, with a way to try again.
    let give_up = |this: &WeakEntity<Shell>, cx: &mut AsyncApp, what: &str, err: String| {
        let wanted = room_id.clone();
        this.update(cx, |this, cx| {
            eprintln!("morpheus-desktop: {what}: {err}");
            if this.calls.in_call(&wanted) {
                let room_name = this
                    .calls
                    .state
                    .take()
                    .map(|state| state.room_name)
                    .unwrap_or_default();
                this.calls.join_error = Some(JoinError {
                    room_id: wanted,
                    room_name,
                    message: crate::view_model::toast::failure_text(what, &err),
                });
                cx.notify();
            }
        })
        .ok();
    };
    let credentials = match session.join_call(room_id.clone()).await {
        Ok(credentials) => {
            crate::diagnostics::set_call_service(&credentials.service_url);
            credentials
        }
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
    // Our media key goes out while LiveKit is being reached, under the name
    // the token gives us (checked against LiveKit's own once connected).
    let early_key = match (credentials.media_key.clone(), credentials.identity()) {
        (Some(key), Some(identity)) => Some((
            identity.clone(),
            session.send_call_key(room_id.clone(), key, identity),
        )),
        _ => None,
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
    let mut said = ByIdentity::default();
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
        let sending = credentials
            .media_key
            .clone()
            .map(|key| session.send_call_key(room_id.clone(), key, own_identity.clone()));
        async move {
            if let Some(sending) = sending
                && let Err(err) = sending.await
            {
                eprintln!("morpheus-desktop: sharing the call's media key: {err}");
            }
        }
    };
    match early_key {
        Some((identity, sending)) if identity == own_identity => {
            if let Err(err) = sending.await {
                eprintln!("morpheus-desktop: sharing the call's media key: {err}");
            }
        }
        // Not the name LiveKit gave us: say it again under the right one.
        _ => share_key().await,
    }

    let learned = refresh_people(&session, &room_id, &mut people).await;
    show_learned(&this, cx, &people, &mut said, &learned);

    // The devices in the call our key went to. Element Call takes keys only
    // sent to its device, and its membership can arrive after its LiveKit
    // join: a device seen in the call for the first time gets the key too.
    let devices_of = |members: &[morpheus_core::CallMember]| -> HashSet<(OwnedUserId, String)> {
        members
            .iter()
            .map(|m| (m.user_id.clone(), m.device_id.clone()))
            .collect()
    };
    // People on other homeservers publish on their own SFUs (#91): this
    // device listens on each of them too, and its own stays the one it
    // publishes on. Their tokens come back here to connect.
    let (focus_tx, mut focus_rx) = futures::channel::mpsc::unbounded::<(
        String,
        Result<morpheus_core::CallCredentials, String>,
    )>();
    let mut focus_asked: HashMap<String, std::time::Instant> = HashMap::new();
    let me_device = session.device_id().unwrap_or_default();
    let me_user = session.user_id();
    let sync_foci = |this: &WeakEntity<Shell>,
                     cx: &mut AsyncApp,
                     members: &[morpheus_core::CallMember],
                     asked: &mut HashMap<String, std::time::Instant>| {
        let Some(me) = me_user.as_ref() else {
            return;
        };
        let wanted =
            morpheus_core::foci_to_subscribe(members, &credentials.service_url, (me, &me_device));
        let listening = this
            .read_with(cx, |this, _| {
                this.calls.media.as_ref().map(Call::listening).unwrap_or_default()
            })
            .unwrap_or_default();
        for (service, _) in listening.iter().filter(|(s, _)| !wanted.contains(s)) {
            eprintln!("morpheus-desktop: nobody publishes on {service} any more: left");
            let leaving = this
                .read_with(cx, |this, _| {
                    this.calls.media.as_ref().map(|call| call.unsubscribe(service))
                })
                .ok()
                .flatten();
            if let Some(leaving) = leaving {
                cx.background_executor().spawn(leaving).detach();
            }
        }
        asked.retain(|service, _| wanted.contains(service));
        for service in wanted {
            if listening.iter().any(|(s, live)| *s == service && *live) {
                continue;
            }
            // Asked lately (or connecting now): not again yet.
            if asked
                .get(&service)
                .is_some_and(|at| at.elapsed() < Duration::from_secs(5))
            {
                continue;
            }
            asked.insert(service.clone(), std::time::Instant::now());
            let token = session.call_focus_token(room_id.clone(), service.clone());
            let tx = focus_tx.clone();
            cx.background_executor()
                .spawn(async move {
                    let _ = tx.unbounded_send((service, token.await.map_err(|e| e.to_string())));
                })
                .detach();
        }
    };
    let members = session.call_members(room_id.clone()).await.unwrap_or_default();
    let mut keyed = devices_of(&members);
    sync_foci(&this, cx, &members, &mut focus_asked);
    let mut members_checked = std::time::Instant::now();

    // A key that reaches someone before the room key it is encrypted with is
    // read once the event cache decrypts it (core's `call_keys`); ours goes
    // out again whenever someone joins, as in Element Call.
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
            let focus = futures::StreamExt::next(&mut focus_rx).fuse();
            let tick = cx
                .background_executor()
                .timer(Duration::from_secs(1))
                .fuse();
            futures::pin_mut!(event, key, focus, tick);
            futures::select_biased! {
                event = event => Next::Event(event),
                key = key => Next::Key(key),
                focus = focus => match focus {
                    Some((service, token)) => Next::Focus(service, token),
                    None => Next::Tick,
                },
                _ = tick => Next::Tick,
            }
        };
        if !current() || !still_wanted(&this, cx) {
            return;
        }
        if members_checked.elapsed() >= Duration::from_secs(2) {
            members_checked = std::time::Instant::now();
            if let Ok(members) = session.call_members(room_id.clone()).await {
                let devices = devices_of(&members);
                if credentials.media_key.is_some()
                    && devices.iter().any(|device| !keyed.contains(device))
                {
                    cx.background_executor().spawn(share_key()).detach();
                }
                keyed = devices;
                sync_foci(&this, cx, &members, &mut focus_asked);
            }
        }
        let event = match next {
            Next::Focus(service, Err(err)) => {
                eprintln!("morpheus-desktop: no token to listen on {service}: {err}");
                continue;
            }
            Next::Focus(service, Ok(token)) => {
                let connecting = this
                    .read_with(cx, |this, _| {
                        this.calls
                            .media
                            .as_ref()
                            .map(|call| call.subscribe(&service, &token.livekit_url, &token.jwt))
                    })
                    .ok()
                    .flatten();
                if let Some(connecting) = connecting {
                    cx.background_executor()
                        .spawn(async move {
                            match connecting.await {
                                Ok(()) => eprintln!("morpheus-desktop: listening on {service} too"),
                                Err(err) => eprintln!(
                                    "morpheus-desktop: could not listen on {service}: {err}"
                                ),
                            }
                        })
                        .detach();
                }
                continue;
            }
            // A screen's sound: its figures on its card, once a second.
            Next::Tick => {
                this.update(cx, |this, cx| {
                    if !this.calls.screen_audio.is_empty() {
                        cx.notify();
                    }
                })
                .ok();
                continue;
            }
            Next::Event(None) => return,
            Next::Event(Some(event)) => event,
            Next::Key(None) => {
                keys = None;
                continue;
            }
            Next::Key(Some(key)) => {
                // Its sender's name in the call, when it says: who is
                // speaking under that name.
                let mut learned = Vec::new();
                if let Some(identity) = &key.livekit_identity
                    && people.get(identity) != Some(&key.user_id)
                {
                    people.insert(identity.clone(), key.user_id.clone());
                    learned.push(identity.clone());
                }
                let applied = this.read_with(cx, |this, _| {
                    if let Some(call) = &this.calls.media {
                        for identity in &key.livekit_identities {
                            call.set_key(identity, key.index, key.key.clone());
                        }
                    }
                });
                if applied.is_err() {
                    return;
                }
                show_learned(&this, cx, &people, &mut said, &learned);
                continue;
            }
        };
        match event {
            CallEvent::Joined(_) => {
                this.update(cx, |this, _| this.chime(Sound::Join)).ok();
                cx.background_executor().spawn(share_key()).detach();
                let learned = refresh_people(&session, &room_id, &mut people).await;
                show_learned(&this, cx, &people, &mut said, &learned);
            }
            CallEvent::Speaking(identities) => {
                said.speaking = identities.clone();
                if identities.iter().any(|id| !people.contains_key(id)) {
                    let learned = refresh_people(&session, &room_id, &mut people).await;
                    show_learned(&this, cx, &people, &mut said, &learned);
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
                let heard = matches!(event, CallEvent::Heard(_));
                said.heard.insert(identity.clone(), heard);
                if !people.contains_key(identity) {
                    let learned = refresh_people(&session, &room_id, &mut people).await;
                    show_learned(&this, cx, &people, &mut said, &learned);
                }
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
                    let learned = refresh_people(&session, &room_id, &mut people).await;
                    show_learned(&this, cx, &people, &mut said, &learned);
                }
                let user = people.get(&identity).cloned();
                // Its sender not known yet: shown once their name is.
                if on && user.is_none() {
                    said.unplaced
                        .insert(key.clone(), (identity.clone(), screen));
                } else {
                    said.unplaced.remove(&key);
                }
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
            CallEvent::Screen { identity, on } => {
                if !people.contains_key(&identity) {
                    let learned = refresh_people(&session, &room_id, &mut people).await;
                    show_learned(&this, cx, &people, &mut said, &learned);
                }
                let Some(user) = people.get(&identity).cloned() else {
                    continue;
                };
                this.update(cx, |this, cx| {
                    // Someone starts sharing their screen: heard, as in Discord.
                    if on && this.calls.state.as_ref().is_some_and(|c| !c.shares.contains(&user)) {
                        this.chime(Sound::ScreenOn);
                    }
                    if on {
                        this.calls.share_identity.insert(user.clone(), identity);
                    } else {
                        this.calls.share_identity.remove(&user);
                    }
                    if let Some(state) = this.calls.state.as_mut() {
                        state.share_event(user.clone(), on);
                    }
                    // The test runs watch every share as it appears (they check the
                    // pictures); MORPHEUS_WATCH_ASK=1 gives the Watch buttons.
                    if on && auto_watch() {
                        this.watch_share(&user, true, cx);
                    }
                    cx.notify();
                })
                .ok();
            }
            CallEvent::Watchers(n) => {
                this.update(cx, |this, cx| {
                    if let Some(state) = this.calls.state.as_mut() {
                        state.watchers = n;
                        cx.notify();
                    }
                })
                .ok();
            }
            CallEvent::ScreenAudio { identity, on } => {
                if !people.contains_key(&identity) {
                    let learned = refresh_people(&session, &room_id, &mut people).await;
                    show_learned(&this, cx, &people, &mut said, &learned);
                }
                let Some(user) = people.get(&identity).cloned() else {
                    continue;
                };
                this.update(cx, |this, cx| {
                    if on {
                        // At the volume you gave their screen before.
                        this.calls.screen_audio.insert(user.clone(), identity);
                        this.apply_stream_volume(&user);
                    } else {
                        this.calls.screen_audio.remove(&user);
                    }
                    cx.notify();
                })
                .ok();
            }
        }
    }
}

/// E2e runs watch shares by themselves (see `CallEvent::Screen`).
fn auto_watch() -> bool {
    std::env::var_os("MORPHEUS_E2E_PORT").is_some() && std::env::var_os("MORPHEUS_WATCH_ASK").is_none()
}

impl Shell {
    /// The camera button's right-click: the cameras, to pick from.
    pub fn open_camera_menu(&mut self, at: gpui::Point<gpui::Pixels>, cx: &mut Context<Self>) {
        use crate::view_model::{
            calls::{camera_rows, device_menu},
            menu::DeviceKind,
        };
        let cameras: Vec<(String, String)> = morpheus_call::cameras(AudioMode::from_env())
            .into_iter()
            .map(|c| (c.id, c.name))
            .collect();
        let rows = camera_rows(&cameras, self.calls.devices.camera.as_deref());
        let items = device_menu(DeviceKind::Camera, &rows);
        self.open_menu(
            crate::menu::OpenMenu::new(
                at,
                items,
                false,
                crate::view_model::menu::MenuTarget::Other,
            ),
            cx,
        );
    }

    /// The quick list of microphones (or, with `speakers`, speakers).
    pub fn open_audio_menu(
        &mut self,
        speakers: bool,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        use crate::view_model::{
            calls::{device_rows, voice_popup_menu},
            menu::DeviceKind,
        };
        let found = morpheus_call::audio_devices(AudioMode::from_env());
        cx.spawn(async move |this, cx| {
            let found = found.await;
            this.update(cx, |this, cx| {
                let mics: Vec<(String, String)> = found
                    .microphones
                    .into_iter()
                    .map(|d| (d.id, d.name))
                    .collect();
                let spks: Vec<(String, String)> =
                    found.speakers.into_iter().map(|d| (d.id, d.name)).collect();
                let items = if speakers {
                    let rows = device_rows(&spks, this.calls.devices.speaker.as_deref());
                    voice_popup_menu(DeviceKind::Speaker, &rows, morpheus_call::output_volume(), "")
                } else {
                    let rows = device_rows(&mics, this.calls.devices.microphone.as_deref());
                    voice_popup_menu(DeviceKind::Microphone, &rows, morpheus_call::input_volume(), crate::noise::read().key())
                };
                let mut menu = crate::menu::OpenMenu::new(
                    at,
                    items,
                    false,
                    crate::view_model::menu::MenuTarget::Other,
                );
                menu.open = Some(0);
                this.open_menu(menu, cx);
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
                        cx.background_executor()
                            .timer(Duration::from_millis(400))
                            .await;
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
