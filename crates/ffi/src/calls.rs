//! Voice and video calls: the same pieces the desktop app joins a call with
//! (the core's MatrixRTC signalling, `morpheus-call`'s LiveKit media), run
//! here as one task per call, and what the call screen draws as whole
//! snapshots (`morpheus-view-model::calls`: the cards, the speaking set, who
//! watches which shared screen, the timer).
//!
//! A call is joined with [`Morpheus::join_call`], which hands back a
//! [`CallHandle`] and pushes [`CallSnapshot`]s to the listener until the call
//! ends. Pictures of video are fetched by the app with
//! [`CallHandle::frame`] while it draws them.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use futures_util::FutureExt;
use morpheus_call::{AudioMode, Call, CallEvent, LOCAL_CAMERA};
use morpheus_view_model::calls::CallStep;
use morpheus_core::{CallMedia, OwnedRoomId, OwnedUserId, RoomSummary, Session};
use morpheus_view_model::{
    calls::{self, CallState, Card, VoiceFlags},
    diffs,
};
use tokio::sync::{mpsc, oneshot};

use crate::{FfiError, MediaHandle, Morpheus, runtime};

/// How far a call has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CallPhase {
    /// Credentials and the media connection are being made.
    Joining,
    Connected,
    /// The call could not be joined, or dropped: `error` says why. The task has ended.
    Failed,
    /// Hung up. The last snapshot of a call.
    Ended,
}

/// Someone in the call.
#[derive(uniffi::Record)]
pub struct CallPerson {
    pub user_id: String,
    pub name: String,
    pub avatar: Option<Arc<MediaHandle>>,
    pub me: bool,
    pub speaking: bool,
    pub muted: bool,
    pub deafened: bool,
    pub camera: bool,
    pub screen: bool,
    /// Joining, not connected yet (this account only).
    pub connecting: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CardKind {
    /// A person: their picture (no camera on).
    Person,
    /// A person's camera.
    Camera,
    /// A shared screen being watched.
    Screen,
    /// A shared screen nobody here asked for: a Watch button, nothing received.
    UnwatchedScreen,
}

/// One tile of the call screen, in the order the desktop shows them.
#[derive(uniffi::Record)]
pub struct CallCard {
    /// Stable: what focus and lists key by.
    pub key: String,
    pub kind: CardKind,
    pub user_id: String,
    pub name: String,
    pub avatar: Option<Arc<MediaHandle>>,
    pub speaking: bool,
    /// For a camera or screen: ask [`CallHandle::frame`] with this.
    pub feed: Option<String>,
}

/// What the call screen shows now.
#[derive(uniffi::Record)]
pub struct CallSnapshot {
    pub room_id: String,
    pub room_name: String,
    pub phase: CallPhase,
    /// Why the call failed (a sentence for people).
    pub error: Option<String>,
    pub muted: bool,
    pub deafened: bool,
    /// This device's camera is sending.
    pub camera: bool,
    /// Whether this device can send video at all.
    pub can_send_video: bool,
    /// Whether this device can share its screen.
    pub can_share_screen: bool,
    pub people: Vec<CallPerson>,
    pub cards: Vec<CallCard>,
    /// How many people watch the screen this device shares.
    pub watchers: u32,
    /// When the call began (milliseconds since the Unix epoch), if known:
    /// [`call_timer`] turns it and the time into the timer's text.
    pub started_ms: Option<u64>,
}

/// Gets a whole new [`CallSnapshot`] whenever something on the call screen changed.
#[uniffi::export(with_foreign)]
pub trait CallListener: Send + Sync {
    fn on_call(&self, snapshot: CallSnapshot);
}

/// One picture of someone's video.
#[derive(uniffi::Record)]
pub struct VideoFrame {
    /// Goes up with each new picture under the same feed.
    pub seq: u64,
    pub jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// The call's timer text (`1:05`, `1:00:00`) at `now_ms` for a call that began at `started_ms`.
#[uniffi::export]
pub fn call_timer(now_ms: u64, started_ms: Option<u64>) -> Option<String> {
    calls::call_timer(now_ms, started_ms)
}

/// Calls send a test tone and play nothing, and cameras send a test pattern
/// (test builds, where there is no microphone to hear).
#[uniffi::export]
pub fn call_use_test_tone() {
    morpheus_call::use_test_tone();
}

/// Who a room's call lists now, and what they have on: for the channel list.
#[derive(uniffi::Record)]
pub struct CallListing {
    pub user_id: String,
    pub muted: bool,
    pub deafened: bool,
    /// Their camera is on.
    pub camera: bool,
    /// They share their screen (a LIVE mark).
    pub screen: bool,
    /// When the room's call began (ms since the Unix epoch), the same for each person: the row's timer (`call_timer`).
    pub started_ms: Option<u64>,
}

pub(crate) fn listing(room: &RoomSummary) -> Vec<CallListing> {
    let started_ms = calls::call_start(now_ms(), &room.call_memberships);
    room.call_participants
        .iter()
        .map(|user| {
            let media = room
                .call_media
                .iter()
                .find(|(u, _)| u == user)
                .map(|(_, m)| *m)
                .unwrap_or_default();
            CallListing {
                user_id: user.to_string(),
                muted: media.muted,
                deafened: media.deafened,
                camera: media.camera,
                screen: media.screen,
                started_ms,
            }
        })
        .collect()
}

enum Command {
    Voice(VoiceFlags),
    Camera(bool),
    Screen(bool),
    Watch(OwnedUserId, bool),
    PersonVolume(String, f32),
    HideVideo(String, bool),
    OutputVolume(f32),
    Leave(oneshot::Sender<()>),
}

/// A call going: its controls. Dropping it does not hang up; [`CallHandle::leave`] does.
#[derive(uniffi::Object)]
pub struct CallHandle {
    commands: mpsc::UnboundedSender<Command>,
    flags: std::sync::Mutex<VoiceFlags>,
}

#[uniffi::export]
impl CallHandle {
    /// Mute, or unmute (which undeafens too).
    pub fn toggle_mute(&self) {
        self.change(VoiceFlags::toggled_mute);
    }

    /// Deafen, or undeafen.
    pub fn toggle_deafen(&self) {
        self.change(VoiceFlags::toggled_deafen);
    }

    /// Turn this device's camera on or off.
    pub fn set_camera(&self, on: bool) {
        let _ = self.commands.send(Command::Camera(on));
    }

    /// Share this device's screen, or stop. Only where the call can (desktops, test builds: `can_share_screen`).
    pub fn set_screen_share(&self, on: bool) {
        let _ = self.commands.send(Command::Screen(on));
    }

    /// Watch (or stop watching) someone's shared screen: nothing of it is
    /// received until asked for, and the sharer sends nothing while nobody asks.
    pub fn watch(&self, user_id: String, on: bool) {
        if let Ok(user) = OwnedUserId::try_from(user_id) {
            let _ = self.commands.send(Command::Watch(user, on));
        }
    }

    /// Play a person's voice at this volume (0 to 2; 0 mutes them for you alone), live.
    pub fn set_person_volume(&self, user_id: String, gain: f32) {
        let _ = self.commands.send(Command::PersonVolume(user_id, gain));
    }

    /// Hide (or show again) someone's camera: nothing of it is downloaded while hidden (#436).
    pub fn hide_video(&self, user_id: String, on: bool) {
        let _ = self.commands.send(Command::HideVideo(user_id, on));
    }

    /// Play someone's shared screen's sound at this volume (0 to 2; 0 silences it for you alone), live (#436).
    pub fn set_stream_volume(&self, user_id: String, gain: f32) {
        morpheus_call::set_user_track_volume(&user_id, morpheus_call::TrackSource::ScreenShare, gain);
    }

    /// Play the call at this volume (0 to 2), live.
    pub fn set_output_volume(&self, volume: f32) {
        let _ = self.commands.send(Command::OutputVolume(volume));
    }

    /// The latest picture of a camera or screen (a card's `feed`).
    pub fn frame(&self, feed: String) -> Option<VideoFrame> {
        let frame = morpheus_call::latest_frame(&feed)?;
        Some(VideoFrame {
            seq: frame.seq,
            jpeg: frame.jpeg.as_ref().clone(),
            width: frame.width,
            height: frame.height,
        })
    }

    /// Hang up: the membership is left and the media closed before this returns.
    pub async fn leave(&self) {
        let (done, wait) = oneshot::channel();
        if self.commands.send(Command::Leave(done)).is_ok() {
            // On this crate's runtime: the caller's executor (Kotlin's) has no timers.
            let _ = runtime().spawn(async move { tokio::time::timeout(Duration::from_secs(8), wait).await }).await;
        }
    }
}

impl CallHandle {
    fn change(&self, f: impl Fn(VoiceFlags) -> VoiceFlags) {
        let next = match self.flags.lock() {
            Ok(mut flags) => {
                *flags = f(*flags);
                *flags
            }
            Err(_) => return,
        };
        let _ = self.commands.send(Command::Voice(next));
    }
}

#[uniffi::export]
impl Morpheus {
    /// Join the call in `room_id` (leaving none: the app hangs up the one
    /// going first) and tell `listener` how it goes until it ends. A call
    /// that cannot be joined ends with a [`CallPhase::Failed`] snapshot.
    pub async fn join_call(
        &self,
        room_id: String,
        listener: Arc<dyn CallListener>,
    ) -> Result<Arc<CallHandle>, FfiError> {
        let session = self.session()?;
        let room_id = OwnedRoomId::try_from(room_id.as_str())
            .map_err(|_| FfiError::Failed { reason: format!("{room_id} is not a room id") })?;
        let rooms = session.room_list().await?;
        let (commands, inbox) = mpsc::unbounded_channel();
        let handle = Arc::new(CallHandle {
            commands,
            flags: std::sync::Mutex::new(VoiceFlags::default()),
        });
        self.apply_saved_volumes();
        runtime().spawn(run_call(session, room_id, listener, rooms, inbox));
        Ok(handle)
    }

    /// Get ready for a call in this room (the call service, the tokens), so
    /// joining does not wait for them. Quiet and cheap to repeat.
    pub async fn prepare_call(&self, room_id: String) {
        if let (Ok(session), Ok(room)) = (self.session(), OwnedRoomId::try_from(room_id.as_str())) {
            let _ = session.prepare_calls(Some(room)).await;
        }
    }

    /// A person's name (their display name, else their ID's local part).
    pub async fn display_name(&self, user_id: String) -> String {
        let fallback = user_id.trim_start_matches('@').split(':').next().unwrap_or("").to_owned();
        let (Ok(session), Ok(user)) = (self.session(), OwnedUserId::try_from(user_id.as_str())) else {
            return fallback;
        };
        let name = session.profile(user.clone()).await.map(|p| p.name).unwrap_or(fallback);
        morpheus_view_model::nicknames::shown_name(&self.nicks.snapshot(), user.as_str(), &name)
    }
}

/// What the loop knows of the people: their names, and which LiveKit name is who.
#[derive(Default)]
struct People {
    /// LiveKit identity -> person.
    by_identity: HashMap<String, OwnedUserId>,
    profiles: HashMap<OwnedUserId, (String, Option<morpheus_core::MediaRef>)>,
    asked: HashSet<OwnedUserId>,
}

/// What the call said under each LiveKit name, kept as it said it: a name is
/// not always known when its voice or picture arrives (see the desktop's
/// `ByIdentity`).
#[derive(Default)]
struct Said {
    speaking: Vec<String>,
    heard: HashMap<String, bool>,
    /// Feeds whose sender is not known yet: key -> (identity, screen).
    unplaced: HashMap<String, (String, bool)>,
    /// A person -> their LiveKit name, for watching their screen.
    share_identity: HashMap<OwnedUserId, String>,
}

/// Match LiveKit names to people from the room's call memberships.
async fn refresh_people(session: &Session, room: &OwnedRoomId, people: &mut People) -> Vec<String> {
    let mut learned = Vec::new();
    if let Ok(members) = session.call_members(room.clone()).await {
        for member in members {
            for identity in member.livekit_identities {
                if people.by_identity.get(&identity) != Some(&member.user_id) {
                    people.by_identity.insert(identity.clone(), member.user_id.clone());
                    learned.push(identity);
                }
            }
        }
    }
    learned
}

enum Next {
    Event(Option<CallEvent>),
    Key(Option<morpheus_core::CallKey>),
    Rooms(Option<Vec<morpheus_core::VectorDiff<RoomSummary>>>),
    Command(Option<Command>),
    /// A token for listening on another homeserver's SFU (its service, the token).
    Focus(String, Result<morpheus_core::CallCredentials, String>),
    Tick,
}

struct Model {
    session: Session,
    room_id: OwnedRoomId,
    state: CallState,
    flags: VoiceFlags,
    rooms: Vec<RoomSummary>,
    people: People,
    said: Said,
    phase: CallPhase,
    error: Option<String>,
    published: Option<(OwnedRoomId, CallMedia)>,
    listener: Arc<dyn CallListener>,
    told: Option<String>,
    fetched: Fetched,
}

impl Model {
    fn room(&self) -> Option<&RoomSummary> {
        self.rooms.iter().find(|r| r.id == self.room_id)
    }

    fn me(&self) -> Option<OwnedUserId> {
        self.session.user_id()
    }

    /// Names of people not asked for yet are fetched in the background.
    fn want_profiles(&mut self, wake: &mpsc::UnboundedSender<()>) {
        let mut wanted: Vec<OwnedUserId> = self.room().map(|r| r.call_participants.clone()).unwrap_or_default();
        wanted.extend(self.state.video.iter().map(|f| f.user.clone()));
        wanted.extend(self.state.shares.iter().cloned());
        wanted.extend(self.me());
        for user in wanted {
            if !self.people.asked.insert(user.clone()) {
                continue;
            }
            let (asking, wake) = (self.session.profile(user.clone()), wake.clone());
            let slot = self.fetched.clone();
            runtime().spawn(async move {
                if let Ok(profile) = asking.await {
                    if let Ok(mut slot) = slot.lock() {
                        slot.push((user, profile.name, profile.avatar));
                    }
                    let _ = wake.send(());
                }
            });
        }
    }

    fn take_profiles(&mut self) {
        if let Ok(mut slot) = self.fetched.lock() {
            for (user, name, avatar) in slot.drain(..) {
                self.people.profiles.insert(user, (name, avatar));
            }
        }
    }

    fn name_of(&self, user: &OwnedUserId) -> String {
        self.people
            .profiles
            .get(user)
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| user.localpart().to_owned())
    }

    fn avatar_of(&self, user: &OwnedUserId) -> Option<Arc<MediaHandle>> {
        let (_, avatar) = self.people.profiles.get(user)?;
        Some(MediaHandle::new(avatar.clone()?, &self.session))
    }

    fn snapshot(&self) -> CallSnapshot {
        let me = self.me();
        let participants: Vec<OwnedUserId> = self.room().map(|r| r.call_participants.clone()).unwrap_or_default();
        let joining = self.phase == CallPhase::Joining;
        let listed = calls::listed(&participants, joining || self.phase == CallPhase::Connected, me.as_ref());
        let media_of = |user: &OwnedUserId| {
            calls::media_of(&self.room_id, user, me.as_ref(), Some(&self.state), self.flags, &self.rooms)
        };
        let people = listed
            .iter()
            .map(|(user, connecting)| {
                let media = media_of(user);
                CallPerson {
                    user_id: user.to_string(),
                    name: self.name_of(user),
                    avatar: self.avatar_of(user),
                    me: me.as_ref() == Some(user),
                    speaking: self.state.speaking.contains(user),
                    muted: media.muted,
                    deafened: media.deafened,
                    camera: media.camera,
                    screen: media.screen,
                    connecting: *connecting,
                }
            })
            .collect();
        let cards = calls::tiles(&self.state, &participants, me.clone())
            .into_iter()
            .map(|card| match card {
                Card::Person(user) => CallCard {
                    key: user.to_string(),
                    kind: CardKind::Person,
                    name: self.name_of(&user),
                    avatar: self.avatar_of(&user),
                    speaking: self.state.speaking.contains(&user),
                    user_id: user.to_string(),
                    feed: None,
                },
                Card::Video(feed) => {
                    let unwatched = calls::is_unwatched(&feed);
                    CallCard {
                        key: feed.key.clone(),
                        kind: match (unwatched, feed.screen) {
                            (true, _) => CardKind::UnwatchedScreen,
                            (false, true) => CardKind::Screen,
                            (false, false) => CardKind::Camera,
                        },
                        name: self.name_of(&feed.user),
                        avatar: self.avatar_of(&feed.user),
                        speaking: self.state.speaking.contains(&feed.user),
                        user_id: feed.user.to_string(),
                        feed: (!unwatched).then(|| feed.key.clone()),
                    }
                }
            })
            .collect();
        CallSnapshot {
            room_id: self.room_id.to_string(),
            room_name: self.state.room_name.clone(),
            phase: self.phase,
            error: self.error.clone(),
            muted: self.flags.mic_off(),
            deafened: self.flags.deafened,
            camera: self.state.camera,
            can_send_video: morpheus_call::can_send_video(),
            can_share_screen: morpheus_call::can_share_screen(),
            people,
            cards,
            watchers: u32::try_from(self.state.watchers).unwrap_or(u32::MAX),
            started_ms: self.room().and_then(|r| calls::call_start(now_ms(), &r.call_memberships)),
        }
    }

    /// Tell the listener, when what it shows changed.
    fn tell(&mut self) {
        let snapshot = self.snapshot();
        // The timer is the app's own clock; everything else is compared.
        let signature = format!(
            "{:?}|{:?}|{}|{}|{}|{}|{}|{}|{}",
            snapshot.phase,
            snapshot.error,
            snapshot.muted,
            snapshot.deafened,
            snapshot.camera,
            snapshot.watchers,
            snapshot.started_ms.is_some(),
            snapshot
                .people
                .iter()
                .map(|p| format!("{}{}{}{}{}{}{}{}", p.user_id, p.name, p.speaking, p.muted, p.deafened, p.camera, p.screen, p.connecting))
                .collect::<Vec<_>>()
                .join(","),
            snapshot
                .cards
                .iter()
                .map(|c| format!("{}{:?}{}", c.key, c.kind, c.speaking))
                .collect::<Vec<_>>()
                .join(","),
        );
        if self.told.as_deref() == Some(signature.as_str()) {
            return;
        }
        self.told = Some(signature);
        self.listener.on_call(snapshot);
    }

    /// Publish what this device has on in its membership, when it changed.
    fn sync_published(&mut self) {
        if self.phase != CallPhase::Connected {
            return;
        }
        let now = (self.room_id.clone(), self.flags.media(&self.state));
        if self.published.as_ref() == Some(&now) {
            return;
        }
        self.published = Some(now.clone());
        let sending = self.session.set_call_media(now.0, now.1);
        runtime().spawn(async move {
            let _ = sending.await;
        });
    }

    /// Names just matched: what the call said under them shows now.
    fn show_learned(&mut self, learned: &[String]) {
        if learned.is_empty() {
            return;
        }
        for identity in learned {
            if let Some(user) = self.people.by_identity.get(identity) {
                morpheus_call::note_voice_identity(identity, user.as_str());
            }
        }
        let by = &self.people.by_identity;
        self.state.speaking = self.said.speaking.iter().filter_map(|id| by.get(id).cloned()).collect();
        for id in learned {
            if let (Some(user), Some(heard)) = (by.get(id).cloned(), self.said.heard.get(id).copied()) {
                self.state.heard_event(user, heard);
            }
        }
        let placed: Vec<String> = self
            .said
            .unplaced
            .iter()
            .filter(|(_, (id, _))| self.people.by_identity.contains_key(id))
            .map(|(key, _)| key.clone())
            .collect();
        for key in placed {
            if let Some((id, screen)) = self.said.unplaced.remove(&key)
                && let Some(user) = self.people.by_identity.get(&id).cloned()
            {
                self.state.video_event(Some(user), key, screen, true);
            }
        }
    }
}

type Fetched = Arc<std::sync::Mutex<Vec<(OwnedUserId, String, Option<morpheus_core::MediaRef>)>>>;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(0))
        .unwrap_or(0)
}

async fn leave_media(session: &Session, room: OwnedRoomId, call: Option<Call>) {
    let _ = session.leave_call(room).await;
    if let Some(call) = call {
        call.leave().await;
    }
}

/// The room's name for the call screen, from the room list when it has come.
fn room_name(rooms: &[RoomSummary], id: &OwnedRoomId) -> String {
    rooms.iter().find(|r| &r.id == id).map(|r| r.name.clone()).unwrap_or_default()
}

/// A call: credentials, the media connection, then its events until it ends.
async fn run_call(
    session: Session,
    room_id: OwnedRoomId,
    listener: Arc<dyn CallListener>,
    mut room_updates: morpheus_core::RoomListUpdates,
    mut inbox: mpsc::UnboundedReceiver<Command>,
) {
    let mut model = Model {
        session: session.clone(),
        room_id: room_id.clone(),
        state: CallState::joining(room_id.clone(), String::new()),
        flags: VoiceFlags::default(),
        rooms: Vec::new(),
        people: People::default(),
        said: Said::default(),
        phase: CallPhase::Joining,
        error: None,
        published: None,
        listener,
        told: None,
        fetched: Fetched::default(),
    };
    let (wake, mut woken) = mpsc::unbounded_channel::<()>();
    // The room list's first batch names the room.
    if let Ok(Some(batch)) = tokio::time::timeout(Duration::from_secs(2), room_updates.next()).await {
        for diff in batch {
            diffs::apply(&mut model.rooms, diff);
        }
    }
    model.state.room_name = room_name(&model.rooms, &room_id);
    model.want_profiles(&wake);
    model.tell();

    let fail = |model: &mut Model, step: morpheus_view_model::calls::CallStep, err: String| {
        model.phase = CallPhase::Failed;
        model.error = Some(morpheus_view_model::calls::call_failure(step, &err));
        model.tell();
    };

    let credentials = match session.join_call(room_id.clone()).await {
        Ok(credentials) => credentials,
        Err(err) => return fail(&mut model, CallStep::Joining, err.to_string()),
    };
    // Keys others share, subscribed before ours goes out so no answer is missed.
    let mut keys = match &credentials.media_key {
        Some(_) => session.call_keys(room_id.clone()).await.ok(),
        None => None,
    };
    let early_key = match (credentials.media_key.clone(), credentials.identity()) {
        (Some(key), Some(identity)) => Some((identity.clone(), session.send_call_key(room_id.clone(), key, identity))),
        _ => None,
    };
    let connected = match credentials.media_key.clone() {
        Some(key) => {
            Call::connect_encrypted(&credentials.livekit_url, &credentials.jwt, AudioMode::from_env(), key).await
        }
        None => Call::connect(&credentials.livekit_url, &credentials.jwt, AudioMode::from_env()).await,
    };
    let (call, mut events) = match connected {
        Ok(connected) => connected,
        Err(err) => {
            let _ = session.leave_call(room_id.clone()).await;
            return fail(&mut model, CallStep::Connecting, err.to_string());
        }
    };
    let mut call = Some(call);
    let own_identity = call.as_ref().map(Call::identity).unwrap_or_default();
    if let Some(me) = session.user_id() {
        model.people.by_identity.insert(own_identity.clone(), me);
    }
    if let Some(call) = &call {
        call.set_muted(model.flags.mic_off());
        call.set_deafened(model.flags.deafened);
    }
    model.phase = CallPhase::Connected;
    model.state.connected = true;
    model.sync_published();
    model.tell();

    // Our media key, to everyone in the call (again whenever someone joins).
    let share_key = || {
        let sending = credentials
            .media_key
            .clone()
            .map(|key| session.send_call_key(room_id.clone(), key, own_identity.clone()));
        async move {
            if let Some(sending) = sending {
                let _ = sending.await;
            }
        }
    };
    match early_key {
        Some((identity, sending)) if identity == own_identity => {
            let _ = sending.await;
        }
        _ => share_key().await,
    }
    let learned = refresh_people(&session, &room_id, &mut model.people).await;
    model.show_learned(&learned);
    // Devices our key went to: a device seen for the first time gets it too.
    let devices_of = |members: &[morpheus_core::CallMember]| -> HashSet<(OwnedUserId, String)> {
        members.iter().map(|m| (m.user_id.clone(), m.device_id.clone())).collect()
    };
    let members = session.call_members(room_id.clone()).await.unwrap_or_default();
    let mut keyed = devices_of(&members);
    let mut members_checked = std::time::Instant::now();
    // People on other homeservers publish on their own SFUs: this call listens on each of
    // them too, and its own stays the one it publishes on (the desktop does the same).
    let (focus_tx, mut focus_rx) = mpsc::unbounded_channel::<(String, Result<morpheus_core::CallCredentials, String>)>();
    let mut focus_asked: HashMap<String, std::time::Instant> = HashMap::new();
    let me_device = session.device_id().unwrap_or_default();
    let me_user = session.user_id();
    let mut sync_foci = |call: &Call, members: &[morpheus_core::CallMember], asked: &mut HashMap<String, std::time::Instant>| {
        let Some(me) = me_user.as_ref() else { return };
        let wanted = morpheus_core::foci_to_subscribe(members, &credentials.service_url, (me, &me_device));
        let listening = call.listening();
        for (service, _) in listening.iter().filter(|(s, _)| !wanted.contains(s)) {
            runtime().spawn(call.unsubscribe(service));
        }
        asked.retain(|service, _| wanted.contains(service));
        for service in wanted {
            if listening.iter().any(|(s, live)| *s == service && *live) {
                continue;
            }
            if asked.get(&service).is_some_and(|at| at.elapsed() < Duration::from_secs(5)) {
                continue;
            }
            asked.insert(service.clone(), std::time::Instant::now());
            let token = session.call_focus_token(room_id.clone(), service.clone());
            let tx = focus_tx.clone();
            runtime().spawn(async move {
                let _ = tx.send((service, token.await.map_err(|e| e.to_string())));
            });
        }
    };
    if let Some(call) = &call {
        sync_foci(call, &members, &mut focus_asked);
    }
    let mut leaving: Option<oneshot::Sender<()>> = None;

    'call: loop {
        let next = {
            let event = async {
                match events.recv().await {
                    Some(event) => Next::Event(Some(event)),
                    None => Next::Event(None),
                }
            }
            .fuse();
            let key = async {
                match keys.as_mut() {
                    Some(keys) => Next::Key(keys.next().await),
                    None => std::future::pending().await,
                }
            }
            .fuse();
            let rooms = async { Next::Rooms(room_updates.next().await) }.fuse();
            let focus = async {
                match focus_rx.recv().await {
                    Some((service, token)) => Next::Focus(service, token),
                    None => std::future::pending().await,
                }
            }
            .fuse();
            let command = async { Next::Command(inbox.recv().await) }.fuse();
            let tick = async {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                    _ = woken.recv() => {}
                }
                Next::Tick
            }
            .fuse();
            futures_util::pin_mut!(event, key, rooms, command, focus, tick);
            futures_util::select_biased! {
                next = command => next,
                next = event => next,
                next = key => next,
                next = rooms => next,
                next = focus => next,
                next = tick => next,
            }
        };
        if members_checked.elapsed() >= Duration::from_secs(2) {
            members_checked = std::time::Instant::now();
            if let Ok(members) = session.call_members(room_id.clone()).await {
                let devices = devices_of(&members);
                if credentials.media_key.is_some() && devices.iter().any(|device| !keyed.contains(device)) {
                    runtime().spawn(share_key());
                }
                keyed = devices;
                if let Some(call) = &call {
                    sync_foci(call, &members, &mut focus_asked);
                }
            }
        }
        match next {
            Next::Tick => {}
            Next::Focus(service, Err(err)) => tracing::warn!(%service, %err, "no token to listen on another homeserver's call service"),
            Next::Focus(service, Ok(token)) => {
                if let Some(call) = &call {
                    let connecting = call.subscribe(&service, &token.livekit_url, &token.jwt);
                    runtime().spawn(async move {
                        if let Err(err) = connecting.await {
                            tracing::warn!(%service, %err, "could not listen on another homeserver's call service");
                        }
                    });
                }
            }
            Next::Rooms(None) => {}
            Next::Rooms(Some(batch)) => {
                for diff in batch {
                    diffs::apply(&mut model.rooms, diff);
                }
                if model.state.room_name.is_empty() {
                    model.state.room_name = room_name(&model.rooms, &room_id);
                }
            }
            Next::Command(None) => break 'call,
            Next::Command(Some(Command::Leave(done))) => {
                leaving = Some(done);
                break 'call;
            }
            Next::Command(Some(Command::Voice(next))) => {
                model.flags = next;
                if let Some(call) = &call {
                    call.set_muted(next.mic_off());
                    call.set_deafened(next.deafened);
                }
                model.sync_published();
            }
            Next::Command(Some(Command::Camera(on))) => {
                if let Some(call) = &call {
                    let switched = call.set_camera(on);
                    match switched.await {
                        Ok(()) => {
                            let me = model.me();
                            model.state.local_video(me, LOCAL_CAMERA, false, on);
                            model.sync_published();
                        }
                        Err(err) => {
                            model.error = Some(morpheus_view_model::calls::call_failure(CallStep::CameraOn, &err.to_string()));
                        }
                    }
                }
            }
            Next::Command(Some(Command::Screen(on))) => {
                if let Some(call) = &call {
                    match call.set_screen_share(on).await {
                        Ok(()) => {
                            let me = model.me();
                            model.state.local_video(me, morpheus_call::LOCAL_SCREEN, true, on);
                            model.sync_published();
                        }
                        Err(err) => {
                            model.error = Some(morpheus_view_model::calls::call_failure(CallStep::ScreenShare, &err.to_string()));
                        }
                    }
                }
            }
            Next::Command(Some(Command::Watch(user, on))) => {
                model.state.set_watching(user.clone(), on);
                if let (Some(call), Some(identity)) = (&call, model.said.share_identity.get(&user)) {
                    call.watch(identity, on);
                }
            }
            Next::Command(Some(Command::PersonVolume(user, gain))) => {
                if let Some(call) = &call {
                    call.set_user_volume(&user, gain);
                } else {
                    morpheus_call::set_user_volume(&user, gain);
                }
            }
            Next::Command(Some(Command::HideVideo(user, on))) => {
                if let Some(call) = &call {
                    call.hide_video(&user, on);
                }
            }
            Next::Command(Some(Command::OutputVolume(volume))) => {
                if let Some(call) = &call {
                    call.set_output_volume(volume);
                }
            }
            Next::Key(None) => keys = None,
            Next::Key(Some(key)) => {
                let mut learned = Vec::new();
                if let Some(identity) = &key.livekit_identity
                    && model.people.by_identity.get(identity) != Some(&key.user_id)
                {
                    model.people.by_identity.insert(identity.clone(), key.user_id.clone());
                    learned.push(identity.clone());
                }
                if let Some(call) = &call {
                    for identity in &key.livekit_identities {
                        call.set_key(identity, key.index, key.key.clone());
                    }
                }
                model.show_learned(&learned);
            }
            Next::Event(None) => break 'call,
            Next::Event(Some(event)) => {
                if handle_event(&mut model, &call, &session, &room_id, event, &share_key).await {
                    // The call dropped.
                    model.phase = CallPhase::Failed;
                    break 'call;
                }
            }
        }
        model.want_profiles(&wake);
        model.take_profiles();
        model.tell();
    }

    if model.phase != CallPhase::Failed {
        model.phase = CallPhase::Ended;
    }
    model.state.connected = false;
    model.tell();
    leave_media(&session, room_id, call.take()).await;
    if let Some(done) = leaving {
        let _ = done.send(());
    }
}

/// One thing the media said. True when the call is gone.
async fn handle_event<F, Fut>(
    model: &mut Model,
    call: &Option<Call>,
    session: &Session,
    room_id: &OwnedRoomId,
    event: CallEvent,
    share_key: &F,
) -> bool
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    // A name not known yet: look the room's memberships up again.
    macro_rules! known {
        ($identity:expr) => {
            if !model.people.by_identity.contains_key($identity) {
                let learned = refresh_people(session, room_id, &mut model.people).await;
                model.show_learned(&learned);
            }
        };
    }
    match event {
        CallEvent::Joined(_) => {
            runtime().spawn(share_key());
            let learned = refresh_people(session, room_id, &mut model.people).await;
            model.show_learned(&learned);
        }
        CallEvent::Left(_) => {}
        CallEvent::Speaking(identities) => {
            model.said.speaking = identities.clone();
            if identities.iter().any(|id| !model.people.by_identity.contains_key(id)) {
                let learned = refresh_people(session, room_id, &mut model.people).await;
                model.show_learned(&learned);
            }
            model.state.speaking = identities
                .iter()
                .filter_map(|id| model.people.by_identity.get(id).cloned())
                .collect();
        }
        CallEvent::Heard(ref identity) | CallEvent::Quiet(ref identity) => {
            let heard = matches!(event, CallEvent::Heard(_));
            model.said.heard.insert(identity.clone(), heard);
            known!(identity);
            if let Some(user) = model.people.by_identity.get(identity).cloned() {
                model.state.heard_event(user, heard);
            }
        }
        CallEvent::Video { identity, key, screen, on } => {
            known!(&identity);
            let user = model.people.by_identity.get(&identity).cloned();
            if on && user.is_none() {
                model.said.unplaced.insert(key.clone(), (identity.clone(), screen));
            } else {
                model.said.unplaced.remove(&key);
            }
            model.state.video_event(user, key, screen, on);
        }
        CallEvent::Disconnected(reason) => {
            model.error = Some(morpheus_view_model::calls::call_failure(CallStep::Dropped, &reason));
            return true;
        }
        CallEvent::Screen { identity, on } => {
            known!(&identity);
            if let Some(user) = model.people.by_identity.get(&identity).cloned() {
                if on {
                    model.said.share_identity.insert(user.clone(), identity);
                } else {
                    model.said.share_identity.remove(&user);
                }
                model.state.share_event(user, on);
            }
        }
        CallEvent::Publications { identity, camera, screen, muted } => {
            known!(&identity);
            if let Some(user) = model.people.by_identity.get(&identity).cloned() {
                model.state.publications_event(
                    user,
                    morpheus_view_model::calls::MemberMedia { camera, screen, muted },
                );
            }
        }
        CallEvent::Watchers(n) => model.state.watchers = n,

        CallEvent::ScreenAudio { .. } | CallEvent::Thumbnail { .. } => {}
    }
    let _ = call;
    false
}

/// The phone's camera (Camera2, in Kotlin): the call asks it to start and
/// stop, and it hands its pictures back through [`call_camera_frame`].
#[uniffi::export(with_foreign)]
pub trait CameraDriver: Send + Sync {
    /// Open the camera and start sending pictures; whether it opened.
    fn start(&self) -> bool;
    fn stop(&self);
}

/// Register the camera calls send from (once; later ones are ignored).
#[uniffi::export]
pub fn call_set_camera_driver(driver: Arc<dyn CameraDriver>) {
    let (on, off) = (driver.clone(), driver);
    morpheus_call::set_platform_camera(morpheus_call::PlatformCamera {
        start: Box::new(move || on.start()),
        stop: Box::new(move || off.stop()),
    });
}

/// One picture from the camera, in the planes Android's YUV_420_888 gives
/// (a pixel stride of 1 for planar chroma, 2 where U and V interleave);
/// `rotation` is how far (0, 90, 180, 270 degrees) it must turn to stand up.
#[uniffi::export]
#[allow(clippy::too_many_arguments)]
pub fn call_camera_frame(y: Vec<u8>, u: Vec<u8>, v: Vec<u8>, y_stride: u32, uv_stride: u32, uv_pixel_stride: u32, width: u32, height: u32, rotation: u32) {
    morpheus_call::camera_frame(&y, &u, &v, y_stride as usize, uv_stride as usize, uv_pixel_stride as usize, width, height, rotation);
}

fn sound_view(s: &CallSnapshot) -> calls::SoundView {
    let others = s.people.iter().filter(|p| !p.me);
    calls::SoundView {
        connected: s.phase == CallPhase::Connected,
        over: s.phase == CallPhase::Ended,
        muted: s.muted,
        deafened: s.deafened,
        camera: s.camera,
        sharing: s.people.iter().any(|p| p.me && p.screen),
        their_screens: others.clone().filter(|p| p.screen).count(),
        others: others.map(|p| p.user_id.clone()).collect(),
    }
}

/// The call sounds (#439) for the step from the last snapshot to this one, as
/// [`call_sound_samples`] keys, in the order to play them.
#[uniffi::export]
pub fn call_sounds(before: Option<CallSnapshot>, after: CallSnapshot) -> Vec<String> {
    let before = before.as_ref().map(sound_view);
    calls::sounds_for_change(before.as_ref(), &sound_view(&after))
        .into_iter()
        .map(|s| s.key().to_owned())
        .collect()
}

/// A sound as mono 16-bit samples at [`call_sound_rate`] (`join`, `leave`,
/// `mute`, `unmute`, `deafen`, `undeafen`, `camera-on`, `camera-off`,
/// `screen-on`, `screen-off`, `ring`, `ringback`, `message`); empty for an
/// unknown key.
#[uniffi::export]
pub fn call_sound_samples(key: String) -> Vec<i16> {
    calls::Sound::from_key(&key).map(calls::Sound::samples).unwrap_or_default()
}

/// The sample rate of [`call_sound_samples`].
#[uniffi::export]
pub fn call_sound_rate() -> u32 {
    calls::SOUND_RATE
}

/// Whether the ringback is due now (see `view_model::calls::ringback_due`).
#[uniffi::export]
pub fn call_ringback_due(ringing_out: bool, since_ms: u64, last_ms: Option<u64>) -> bool {
    calls::ringback_due(ringing_out, since_ms, last_ms)
}

/// One row of a call card's menu (#436): the desktop's `call_card_items`.
#[derive(uniffi::Record)]
pub struct CardMenuRow {
    /// What the row does: `stop-watching`, `volume`, `mute`, `stream-volume`, `mute-stream` or `hide-video`.
    pub action: String,
    /// In English, for tags.
    pub id: String,
    /// In the app's language.
    pub label: String,
    /// A Lucide glyph name.
    pub icon: Option<String>,
    pub checked: bool,
    /// For a volume row: where the slider stands (1 as sent, up to 2).
    pub slider: Option<f32>,
}

/// What the menu of one call card offers (the desktop's list: stop watching, the volume or stream
/// volume with its mute, hide video). `kind` is the card's; `volume` and `muted` are for the voice, or
/// for the screen's sound on a screen card.
#[uniffi::export]
pub fn call_card_menu(user_id: String, kind: CardKind, own: bool, volume: f32, muted: bool, hidden: bool, watching: bool) -> Vec<CardMenuRow> {
    use morpheus_view_model::menu::MenuAction;
    let Ok(user) = OwnedUserId::try_from(user_id) else { return Vec::new() };
    let stream = matches!(kind, CardKind::Screen | CardKind::UnwatchedScreen);
    calls::call_card_items(&user, own, stream, volume, muted, hidden, watching)
        .into_iter()
        .filter_map(|item| {
            let action = match (&item.action, &item.slider) {
                (_, Some(morpheus_view_model::menu::MenuSlider::StreamVolume(..))) => "stream-volume",
                (_, Some(_)) => "volume",
                (Some(MenuAction::CallStopWatching(_)), _) => "stop-watching",
                (Some(MenuAction::CallMute(..)), _) => "mute",
                (Some(MenuAction::CallMuteStream(..)), _) => "mute-stream",
                (Some(MenuAction::CallHideVideo(_)), _) if kind == CardKind::Camera => "hide-video",
                _ => return None,
            };
            Some(CardMenuRow {
                action: action.to_owned(),
                id: item.label.clone(),
                label: morpheus_view_model::i18n::t(&item.label),
                icon: item.glyph().map(str::to_owned),
                checked: item.checked,
                slider: item.slider.as_ref().map(|s| s.value()),
            })
        })
        .collect()
}

#[uniffi::export]
impl Morpheus {
    /// Invite a friend to the call of `room_id` (#438): in a direct message the room's own invite, in a
    /// voice channel a message to them with a link that opens it (the desktop's invite).
    pub async fn invite_to_call(&self, room_id: String, user_id: String) -> Result<(), FfiError> {
        let session = self.session()?;
        let room = OwnedRoomId::try_from(room_id.as_str()).map_err(|_| FfiError::Failed { reason: format!("{room_id} is not a room") })?;
        let user = OwnedUserId::try_from(user_id.as_str()).map_err(|_| FfiError::Failed { reason: format!("{user_id} is not a Matrix ID") })?;
        let (name, direct) = self
            .rooms_now
            .lock()
            .ok()
            .and_then(|rooms| rooms.iter().find(|r| r.id == room).map(|r| (r.name.clone(), r.is_dm)))
            .unwrap_or_default();
        if direct {
            session.invite(room, user).await?;
        } else {
            let home = session.user_id().map(|u| u.server_name().to_string());
            let dm = session.dm_with(user).await?;
            let link = calls::channel_link(room.as_str(), home.as_deref());
            session.say(dm, calls::channel_invite_text(&name, &link)).await?;
        }
        Ok(())
    }
}

/// How good the call's connection is, for the signal bars in the call header (#440).
#[derive(uniffi::Record)]
pub struct ConnectionSignal {
    /// "unknown" (nothing measured yet), "good", "fair" or "poor".
    pub quality: String,
    /// Lit bars of three.
    pub bars: u32,
    /// The round-trip delay, like "42 ms" (with the loss when there is some), or "Measuring…".
    pub tooltip: String,
}

fn signal_of(rtt_ms: Option<f64>, loss_percent: Option<f64>) -> ConnectionSignal {
    use morpheus_view_model::connection_quality::{Quality, quality, tooltip};
    let q = quality(rtt_ms, loss_percent);
    ConnectionSignal {
        quality: match q {
            Quality::Unknown => "unknown",
            Quality::Good => "good",
            Quality::Fair => "fair",
            Quality::Poor => "poor",
        }
        .to_owned(),
        bars: u32::try_from(q.bars()).unwrap_or(0),
        tooltip: tooltip(rtt_ms, loss_percent),
    }
}

/// The call's connection now (the call stack measures it about once a second): ask again about once a second.
#[uniffi::export]
pub fn call_connection_signal() -> ConnectionSignal {
    let diag = morpheus_call::call_diag();
    signal_of(diag.as_ref().and_then(|d| d.rtt_ms), diag.as_ref().and_then(|d| d.loss_percent))
}

#[cfg(test)]
mod signal_tests {
    use super::signal_of;

    #[test]
    fn the_signal_follows_the_measurement() {
        let none = signal_of(None, None);
        assert_eq!((none.quality.as_str(), none.bars), ("unknown", 0));
        let good = signal_of(Some(42.0), Some(0.0));
        assert_eq!((good.quality.as_str(), good.bars, good.tooltip.as_str()), ("good", 3, "42 ms"));
        assert_eq!(signal_of(Some(150.0), None).quality, "fair");
        assert_eq!(signal_of(Some(400.0), None).quality, "poor");
    }
}
