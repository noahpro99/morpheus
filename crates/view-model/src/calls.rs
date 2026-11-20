//! Calls with no UI toolkit in it: what the call is (who is in it, speaking,
//! muted, on camera or sharing a screen), the cards the call view shows and
//! how they fit the stage, the one that is focused, who is ringing, the
//! chimes, and the device lists. The media itself is `morpheus-call`; this
//! only turns its events and the room's call memberships into what is drawn.

use morpheus_core::{CallMedia, CallMembership, IncomingCall, OwnedRoomId, OwnedUserId, RoomSummary};

/// How long after a ring a caller may still be missing from the call (the
/// ring and the membership arrive in either order).
pub const RING_GRACE_MS: u64 = 5_000;

/// The key of the card standing for `user`'s share until it is watched.
pub fn unwatched_key(user: &OwnedUserId) -> String {
    format!("unwatched:{user}")
}

/// Whether `feed` is that stand-in (a Watch button), not a picture.
pub fn is_unwatched(feed: &VideoFeed) -> bool {
    feed.key.starts_with("unwatched:")
}

/// Someone's video, and where its pictures are (`morpheus_call::latest_frame`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFeed {
    pub user: OwnedUserId,
    pub key: String,
    /// A shared screen, not a camera.
    pub screen: bool,
}

/// The call this account is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallState {
    pub room_id: OwnedRoomId,
    pub room_name: String,
    /// Media flowing (credentials fetched, LiveKit connected).
    pub connected: bool,
    /// Who is speaking now, this account included.
    pub speaking: Vec<OwnedUserId>,
    /// Whose audio arrives, decrypted (test tones only).
    pub heard: Vec<OwnedUserId>,
    /// The video showing: screens, then cameras, this device's first of each.
    pub video: Vec<VideoFeed>,
    /// This device's camera is on.
    pub camera: bool,
    /// This device's screen is shared.
    pub screen: bool,
    /// The desktop is asking which screen to share.
    pub choosing_screen: bool,
    /// Others sharing their screen: nothing of it arrives until watched.
    pub shares: Vec<OwnedUserId>,
    /// The shares watched (asked for).
    pub watching: Vec<OwnedUserId>,
    /// How many people watch this device's shared screen.
    pub watchers: usize,
}

impl CallState {
    /// A call just started: nothing connected yet.
    pub fn joining(room_id: OwnedRoomId, room_name: String) -> Self {
        Self {
            room_id,
            room_name,
            connected: false,
            speaking: Vec::new(),
            heard: Vec::new(),
            video: Vec::new(),
            camera: false,
            screen: false,
            choosing_screen: false,
            shares: Vec::new(),
            watching: Vec::new(),
            watchers: 0,
        }
    }

    /// Someone started or stopped sharing their screen. A share that ended
    /// is no longer watched (the next one must be asked for).
    pub fn share_event(&mut self, user: OwnedUserId, on: bool) {
        self.shares.retain(|u| *u != user);
        if on {
            self.shares.push(user);
        } else {
            self.watching.retain(|u| *u != user);
        }
    }

    /// Watch (`on`) or stop watching `user`'s share.
    pub fn set_watching(&mut self, user: OwnedUserId, on: bool) {
        self.watching.retain(|u| *u != user);
        if on {
            self.watching.push(user);
        }
    }

    /// Video from someone started (`on`) or stopped arriving under `key`.
    pub fn video_event(&mut self, user: Option<OwnedUserId>, key: String, screen: bool, on: bool) {
        self.video.retain(|feed| feed.key != key);
        if let (true, Some(user)) = (on, user) {
            let feed = VideoFeed { user, key, screen };
            if screen {
                let at = self.video.iter().take_while(|f| f.screen).count();
                self.video.insert(at, feed);
            } else {
                self.video.push(feed);
            }
        }
    }

    /// This device's camera or screen switched on or off: its feed first
    /// among its kind (screens, then cameras).
    pub fn local_video(&mut self, me: Option<OwnedUserId>, key: &str, screen: bool, on: bool) {
        if screen {
            self.screen = on;
        } else {
            self.camera = on;
        }
        self.video.retain(|feed| feed.key != key);
        if let (true, Some(user)) = (on, me) {
            let at = if screen {
                0
            } else {
                self.video.iter().take_while(|feed| feed.screen).count()
            };
            self.video.insert(
                at,
                VideoFeed {
                    user,
                    key: key.to_owned(),
                    screen,
                },
            );
        }
    }

    /// Whose audio is arriving (or stopped).
    pub fn heard_event(&mut self, user: OwnedUserId, heard: bool) {
        self.heard.retain(|u| *u != user);
        if heard {
            self.heard.push(user);
        }
    }
}

/// Mute and deafen, kept across calls like Discord's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VoiceFlags {
    pub muted: bool,
    pub deafened: bool,
}

impl VoiceFlags {
    /// The microphone is off (deafening mutes too).
    pub fn mic_off(self) -> bool {
        self.muted || self.deafened
    }

    /// Mute, or unmute (which undeafens too, as in Discord).
    pub fn toggled_mute(self) -> Self {
        let muted = !self.mic_off();
        Self {
            muted,
            deafened: self.deafened && muted,
        }
    }

    /// Deafen (muted too), or undeafen (muted only if you were before).
    pub fn toggled_deafen(self) -> Self {
        Self {
            muted: self.muted,
            deafened: !self.deafened,
        }
    }

    /// What this device publishes in its call membership.
    pub fn media(self, call: &CallState) -> CallMedia {
        CallMedia {
            muted: self.mic_off(),
            deafened: self.deafened,
            camera: call.camera,
            screen: call.screen,
        }
    }

    /// The chime for going from `self` to `next` (a deafen change before a
    /// mute change).
    pub fn chime(self, next: Self) -> Option<Sound> {
        if next.deafened != self.deafened {
            Some(if next.deafened {
                Sound::Deafen
            } else {
                Sound::Undeafen
            })
        } else if next.muted != self.muted {
            Some(if next.muted {
                Sound::Mute
            } else {
                Sound::Unmute
            })
        } else {
            None
        }
    }

    pub fn mute_title(self) -> &'static str {
        if self.muted {
            "Unmute (Ctrl+Shift+M)"
        } else {
            "Mute (Ctrl+Shift+M)"
        }
    }

    pub fn deafen_title(self) -> &'static str {
        if self.deafened {
            "Undeafen (Ctrl+Shift+D)"
        } else {
            "Deafen (Ctrl+Shift+D)"
        }
    }
}

/// What someone in a call shows beside their name, as the marks to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark {
    pub class: &'static str,
    pub title: &'static str,
    /// The icon for it, or the text "LIVE" when there is none.
    pub glyph: Option<&'static str>,
}

/// Marks for someone in a call: deafened or muted, and with `video`, their
/// camera and LIVE for a shared screen.
pub fn marks(media: CallMedia, video: bool) -> Vec<Mark> {
    let mut marks = Vec::new();
    if media.deafened {
        marks.push(Mark {
            class: "voice-state deafened",
            title: "Deafened",
            glyph: Some("headphone-off"),
        });
    } else if media.muted {
        marks.push(Mark {
            class: "voice-state muted",
            title: "Muted",
            glyph: Some("mic-off"),
        });
    }
    if video && media.camera {
        marks.push(Mark {
            class: "media-camera",
            title: "Camera on",
            glyph: Some("video"),
        });
    }
    if video && media.screen {
        marks.push(Mark {
            class: "media-live",
            title: "Sharing their screen",
            glyph: None,
        });
    }
    marks
}

/// Who a room's call list shows: the people in it, and you at once (greyed
/// until connected) when you are joining.
pub fn listed(
    participants: &[OwnedUserId],
    joining: bool,
    me: Option<&OwnedUserId>,
) -> Vec<(OwnedUserId, bool)> {
    let mut people: Vec<OwnedUserId> = participants.to_vec();
    if joining
        && let Some(me) = me
        && !people.contains(me)
    {
        people.push(me.clone());
    }
    people
        .into_iter()
        .map(|user| {
            let connecting = joining && me == Some(&user);
            (user, connecting)
        })
        .collect()
}

/// What `user` has on in `room`'s call: this account's from its own flags (at
/// once), anyone else's as their membership publishes it.
pub fn media_of(
    room: &OwnedRoomId,
    user: &OwnedUserId,
    me: Option<&OwnedUserId>,
    call: Option<&CallState>,
    flags: VoiceFlags,
    rooms: &[RoomSummary],
) -> CallMedia {
    if me == Some(user)
        && let Some(call) = call.filter(|c| &c.room_id == room)
    {
        return flags.media(call);
    }
    rooms
        .iter()
        .find(|r| &r.id == room)
        .and_then(|r| {
            r.call_media
                .iter()
                .find(|(u, _)| u == user)
                .map(|(_, media)| *media)
        })
        .unwrap_or_default()
}

/// One card in a call, as Discord has them: a shared screen, or a person
/// (their camera when it is on, else their picture).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Card {
    Video(VideoFeed),
    Person(OwnedUserId),
}

impl Card {
    /// What a card is known by while focused: its video feed, or its person.
    pub fn key(&self) -> String {
        match self {
            Card::Video(feed) => feed.key.clone(),
            Card::Person(user) => user.to_string(),
        }
    }
}

/// A call's cards: shared screens first, then everyone in it (this account
/// first when not listed yet), each in their own card whether or not their
/// camera is on.
pub fn tiles(
    call: &CallState,
    participants: &[OwnedUserId],
    me: Option<OwnedUserId>,
) -> Vec<Card> {
    let mut people: Vec<OwnedUserId> = participants.to_vec();
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
    // Shares not received (yet): a card each, to press Watch on.
    cards.extend(
        call.shares
            .iter()
            .filter(|user| {
                !call
                    .video
                    .iter()
                    .any(|feed| feed.screen && &feed.user == *user)
            })
            .map(|user| {
                Card::Video(VideoFeed {
                    user: user.clone(),
                    key: unwatched_key(user),
                    screen: true,
                })
            }),
    );
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

/// Whether one of `cards` is focused (a focus on a card gone is no focus).
pub fn focused_card(focus: &Option<String>, cards: &[Card]) -> Option<String> {
    focus
        .clone()
        .filter(|key| cards.iter().any(|card| card.key() == *key))
}

/// The focus after a click on `card`: it, or, focused already, the grid.
pub fn toggled_focus(focus: &Option<String>, card: &str) -> Option<String> {
    (focus.as_deref() != Some(card)).then(|| card.to_owned())
}

/// The stage's gap between cards.
pub const CARD_GAP: f32 = 8.0;
/// A card is 16:9.
pub const CARD_RATIO: f32 = 16.0 / 9.0;
/// Shortest the stage may be dragged: its controls and a row of cards.
pub const STAGE_MIN: f32 = 200.0;
/// The stage's height before it is dragged: `clamp(160px, 35vh, 420px)`.
pub fn default_stage_height(window_height: f32) -> f32 {
    (window_height * 0.35).clamp(160.0, 420.0)
}

/// The stage's height after a drag of `delta` px from `start`, no taller than
/// 85% of the chat.
pub fn dragged_stage_height(start: f32, delta: f32, chat_height: f32) -> f32 {
    (start + delta).clamp(STAGE_MIN, (chat_height * 0.85).max(STAGE_MIN))
}

/// How `count` same-size 16:9 tiles sit in a stage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    /// Tiles in a full row.
    pub across: usize,
    /// Rows (the last may be short, and is centred).
    pub rows: usize,
    pub tile_w: f32,
    pub tile_h: f32,
}

impl Grid {
    /// How many tiles each row holds, for `count` tiles.
    pub fn row_counts(&self, count: usize) -> Vec<usize> {
        (0..self.rows)
            .map(|row| (count - row * self.across).min(self.across))
            .collect()
    }
}

/// Discord's grid: the number across (and so of rows) that makes the tiles
/// biggest in a `width` by `height` stage, each 16:9, `CARD_GAP` apart.
pub fn grid_layout(count: usize, width: f32, height: f32) -> Grid {
    let count = count.max(1);
    let mut best = Grid { across: 1, rows: count, tile_w: 0.0, tile_h: 0.0 };
    for across in 1..=count {
        let rows = count.div_ceil(across);
        let w = ((width - (across as f32 - 1.0) * CARD_GAP) / across as f32)
            .min(((height - (rows as f32 - 1.0) * CARD_GAP) / rows as f32) * CARD_RATIO);
        if w > best.tile_w {
            best = Grid { across, rows, tile_w: w, tile_h: 0.0 };
        }
    }
    best.tile_w = best.tile_w.floor().max(40.0);
    best.tile_h = (best.tile_w / CARD_RATIO).floor();
    best
}

/// How a stage with one card focused is shared: the focused card at the top,
/// then one row of the other cards (16:9) under it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocusLayout {
    /// The focused card's room (a 16:9 card fills it).
    pub main_w: f32,
    pub main_h: f32,
    /// The height of the row under it (0 with nothing there).
    pub strip_h: f32,
}

/// Share a `width` by `height` stage between the focused card and the
/// `others` in the row under it. The row gets at least about a quarter of the
/// height (as its width allows), the focused card the rest; when the focused
/// card is held by the width instead, the height it leaves goes to the row. The focused card is `ratio` (width
/// over height): its picture's shape, or 16:9.
pub fn focus_layout(others: usize, width: f32, height: f32, ratio: f32) -> FocusLayout {
    let fit = |w: f32, h: f32| {
        if w / h > ratio {
            ((h * ratio).floor(), h)
        } else {
            (w, (w / ratio).floor())
        }
    };
    if others == 0 {
        let (main_w, main_h) = fit(width, height);
        return FocusLayout { main_w, main_h, strip_h: 0.0 };
    }
    let n = others as f32;
    let by_width = ((width - (n - 1.0) * CARD_GAP) / n / CARD_RATIO).floor();
    let first = 81.0_f32.min(by_width).floor().max(40.0);
    let (main_w, main_h) = fit(width, (height - first - CARD_GAP).max(40.0));
    let strip_h = (height - main_h - CARD_GAP).min(by_width).floor().max(40.0);
    FocusLayout { main_w, main_h, strip_h }
}

/// A person's picture on a card `card_w` wide: about a third of its height,
/// from the strip's 40 up to 160.
pub fn card_avatar_size(card_w: f32) -> f32 {
    (card_w / CARD_RATIO * 0.34).round().clamp(40.0, 160.0)
}

/// Discord's cards: all the same size (16:9), as big as lets every one of
/// `count` fit a `width` by `height` stage, trying each number across.
pub fn card_width(count: usize, width: f32, height: f32) -> f32 {
    if count == 0 {
        return 40.0;
    }
    grid_layout(count, width, height).tile_w
}

/// The rings still worth answering: not expired, not for the call this
/// account is in, and the caller still waiting in it.
pub fn still_ringing(
    rings: &[IncomingCall],
    in_call: Option<&OwnedRoomId>,
    rooms: &[RoomSummary],
    now_ms: u64,
) -> Vec<IncomingCall> {
    rings
        .iter()
        .filter(|ring| ring.rings_until > now_ms)
        .filter(|ring| in_call != Some(&ring.room_id))
        .filter(|ring| {
            let rang_at = ring
                .rings_until
                .saturating_sub(morpheus_core::RING_FOR_MS);
            now_ms.saturating_sub(rang_at) < RING_GRACE_MS
                || rooms
                    .iter()
                    .find(|r| r.id == ring.room_id)
                    .is_some_and(|r| r.call_participants.contains(&ring.caller))
        })
        .cloned()
        .collect()
}

/// The call to join when asked to call in `room_id`. Someone can have
/// several direct-message rooms with you (old ones, ones started from
/// another app); if they are already calling you from one of the others,
/// that is the call to join, not a second, empty one beside it.
pub fn call_room_for(room_id: &OwnedRoomId, rooms: &[RoomSummary]) -> Option<(OwnedRoomId, String)> {
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

/// Whether your call in a DM is ringing the other person: connected, with
/// nobody but you in it.
pub fn ringing_out(call: Option<&CallState>, rooms: &[RoomSummary], me: Option<&OwnedUserId>) -> bool {
    call.filter(|c| c.connected).is_some_and(|c| {
        rooms.iter().any(|r| {
            r.id == c.room_id && r.is_dm && r.call_participants.iter().all(|u| Some(u) == me)
        })
    })
}

/// Who a room's call shows: what the server last said, less this account
/// where it has just hung up (`just_left`, and not back in it: `in_this`).
/// The leave reaches the server at once, but the sync saying so comes a
/// round trip or more later; until then the list and the badges on the
/// rail already go by the hang-up (#110).
pub fn shown_participants(
    room: &RoomSummary,
    me: Option<&OwnedUserId>,
    in_this: bool,
    just_left: bool,
) -> Vec<OwnedUserId> {
    let mut shown = room.call_participants.clone();
    if just_left && !in_this {
        shown.retain(|user| Some(user) != me);
    }
    shown
}

/// The call button in a room's header: what its tooltip says.
pub fn call_button_title(in_this_call: bool, someone_there: bool) -> &'static str {
    if in_this_call {
        "Leave the call"
    } else if someone_there {
        "Join the call"
    } else {
        "Start a call"
    }
}

/// The cards' count word for a stage (`data-cards`).
pub fn heard_list(call: &CallState) -> String {
    call.heard
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

/// A sound the app makes, Discord-style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    /// You, or someone else, joined the call (rising).
    Join,
    /// Someone left, or you hung up (falling).
    Leave,
    Mute,
    Unmute,
    Deafen,
    Undeafen,
    /// Your camera went on (rising), or off (falling).
    CameraOn,
    CameraOff,
    /// A screen share started (yours, or someone else's in the call), or
    /// yours stopped.
    ScreenOn,
    ScreenOff,
    /// Someone is calling (repeats while it rings).
    Ring,
    /// Your call ringing the other person (repeats until they answer).
    Ringback,
    /// A new message that notifies you (short and soft).
    Message,
}

/// The sample rate the sounds are made at.
pub const SOUND_RATE: u32 = 24_000;

impl Sound {
    pub fn key(self) -> &'static str {
        match self {
            Self::Join => "join",
            Self::Leave => "leave",
            Self::Mute => "mute",
            Self::Unmute => "unmute",
            Self::Deafen => "deafen",
            Self::Undeafen => "undeafen",
            Self::CameraOn => "camera-on",
            Self::CameraOff => "camera-off",
            Self::ScreenOn => "screen-on",
            Self::ScreenOff => "screen-off",
            Self::Ring => "ring",
            Self::Ringback => "ringback",
            Self::Message => "message",
        }
    }

    /// The notes (Hz) and how long each rings (ms).
    pub fn notes(self) -> &'static [(f32, u32)] {
        match self {
            Self::Join => &[(587.33, 110), (880.0, 260)],
            Self::Leave => &[(880.0, 110), (587.33, 260)],
            Self::Mute => &[(392.0, 140)],
            Self::Unmute => &[(659.25, 140)],
            Self::Deafen => &[(523.25, 90), (392.0, 160)],
            Self::Undeafen => &[(392.0, 90), (523.25, 160)],
            Self::CameraOn => &[(783.99, 90), (1174.66, 220)],
            Self::CameraOff => &[(1174.66, 90), (783.99, 220)],
            Self::ScreenOn => &[(523.25, 80), (659.25, 80), (1046.5, 240)],
            Self::ScreenOff => &[(1046.5, 80), (659.25, 80), (523.25, 240)],
            Self::Ring => &[(659.25, 120), (830.61, 120), (987.77, 120), (1318.51, 320)],
            Self::Ringback => &[(440.0, 380), (440.0, 380)],
            Self::Message => &[(987.77, 70), (1318.51, 150)],
        }
    }

    /// The notes as mono 16-bit samples at [`SOUND_RATE`], each struck and
    /// fading.
    pub fn samples(self) -> Vec<i16> {
        let mut samples: Vec<i16> = Vec::new();
        for &(freq, ms) in self.notes() {
            let n = (SOUND_RATE * ms / 1000) as usize;
            for i in 0..n {
                let t = i as f32 / SOUND_RATE as f32;
                // A quick attack (no click), then a bell's decay.
                let attack = (i as f32 / 120.0).min(1.0);
                let decay = (-t * 9.0).exp();
                let tone = (t * freq * std::f32::consts::TAU).sin()
                    + 0.3 * (t * freq * 2.0 * std::f32::consts::TAU).sin();
                samples.push((tone * attack * decay * 6_000.0) as i16);
            }
        }
        samples
    }
}

/// A device in a quick list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRow {
    /// `None` for "System default".
    pub id: Option<String>,
    pub name: String,
    pub checked: bool,
}

/// A device list as rows: "System default" first, the one in use ticked.
pub fn device_rows(devices: &[(String, String)], current: Option<&str>) -> Vec<DeviceRow> {
    std::iter::once((None, "System default".to_owned()))
        .chain(
            devices
                .iter()
                .map(|(id, name)| (Some(id.clone()), name.clone())),
        )
        .map(|(id, name)| DeviceRow {
            checked: id.as_deref() == current,
            id,
            name,
        })
        .collect()
}

/// The camera's quick list rows (no "System default"; "No camera found" when
/// there is none, which does nothing).
pub fn camera_rows(cameras: &[(String, String)], current: Option<&str>) -> Vec<DeviceRow> {
    cameras
        .iter()
        .map(|(id, name)| DeviceRow {
            checked: current == Some(id.as_str()),
            id: Some(id.clone()),
            name: name.clone(),
        })
        .collect()
}


/// Settings → Voice & Video's switch for the GPU's video encoder.
pub const HARDWARE_VIDEO_LABEL: &str = "Hardware video acceleration";
/// What the switch does, and when.
pub const HARDWARE_VIDEO_HINT: &str = "Sends your camera and screen through the graphics card (H.264) where it can; off sends VP8 in software. Applies the next time you turn on your camera or share your screen.";

/// Where the devices chosen for calls are kept (the Dioxus app's file, so
/// the two apps agree).
pub const AUDIO_DEVICES: &str = "audio_devices.json";

/// The microphone, speaker and camera chosen for calls (absent: the system's
/// defaults, the first camera that works).
#[derive(Debug, Clone, PartialEq)]
pub struct DevicePrefs {
    pub microphone: Option<String>,
    pub speaker: Option<String>,
    pub camera: Option<String>,
    pub input_volume: f32,
    pub output_volume: f32,
    /// Settings → Voice & Video → "Hardware video acceleration" (on unless
    /// turned off): the GPU encodes video where it can (H.264), else VP8 in
    /// software.
    pub hardware_video: bool,
}

impl Default for DevicePrefs {
    fn default() -> Self {
        Self {
            microphone: None,
            speaker: None,
            camera: None,
            input_volume: 1.0,
            output_volume: 1.0,
            hardware_video: true,
        }
    }
}

pub fn read_device_prefs(data_dir: &std::path::Path) -> DevicePrefs {
    let saved: serde_json::Value = std::fs::read(data_dir.join(AUDIO_DEVICES))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let field = |name: &str| saved.get(name).and_then(|v| v.as_str()).map(str::to_owned);
    let volume_field = |name: &str| {
        saved
            .get(name)
            .and_then(|v| v.as_f64())
            .map(|v| v as f32)
            .unwrap_or(1.0)
    };
    DevicePrefs {
        microphone: field("microphone"),
        speaker: field("speaker"),
        camera: field("camera"),
        input_volume: volume_field("input_volume"),
        output_volume: volume_field("output_volume"),
        hardware_video: saved
            .get("hardware_video")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
    }
}

pub fn write_device_prefs(data_dir: &std::path::Path, prefs: &DevicePrefs) -> std::io::Result<()> {
    let json = serde_json::json!({
        "microphone": prefs.microphone,
        "speaker": prefs.speaker,
        "camera": prefs.camera,
        "input_volume": prefs.input_volume,
        "output_volume": prefs.output_volume,
        "hardware_video": prefs.hardware_video,
    });
    std::fs::write(data_dir.join(AUDIO_DEVICES), json.to_string())
}

/// Where the per-person volumes are kept (by account, then by person).
pub const PERSON_VOLUMES: &str = "person-volumes.json";

/// What one account chose for one person: their voice's and their shared
/// screen's volume (1 as sent) and whether each is muted for them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PersonVolume {
    pub voice: f32,
    pub voice_muted: bool,
    pub stream: f32,
    pub stream_muted: bool,
}

impl Default for PersonVolume {
    fn default() -> Self {
        Self {
            voice: 1.0,
            voice_muted: false,
            stream: 1.0,
            stream_muted: false,
        }
    }
}

impl PersonVolume {
    /// How loud their voice plays on top of the output volume: nothing while muted for you.
    pub fn voice_gain(&self) -> f32 {
        if self.voice_muted { 0.0 } else { self.voice.clamp(0.0, 2.0) }
    }
}

fn read_person_file(data_dir: &std::path::Path) -> serde_json::Value {
    std::fs::read(data_dir.join(PERSON_VOLUMES))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// The volumes `account` gave each person, by their Matrix user ID (people at
/// the defaults are not in the file).
pub fn read_person_volumes(
    data_dir: &std::path::Path,
    account: &str,
) -> std::collections::HashMap<String, PersonVolume> {
    let file = read_person_file(data_dir);
    let mut all = std::collections::HashMap::new();
    let Some(people) = file.get(account).and_then(|v| v.as_object()) else {
        return all;
    };
    for (user, entry) in people {
        let gain = |key: &str| {
            entry
                .get(key)
                .and_then(|v| v.as_f64())
                .map_or(1.0, |v| (v as f32).clamp(0.0, 2.0))
        };
        let flag = |key: &str| entry.get(key).and_then(|v| v.as_bool()).unwrap_or(false);
        all.insert(
            user.clone(),
            PersonVolume {
                voice: gain("voice"),
                voice_muted: flag("voice_muted"),
                stream: gain("stream"),
                stream_muted: flag("stream_muted"),
            },
        );
    }
    all
}

/// Keeps what `account` chose for `user`; the defaults are dropped from the
/// file, so only what was changed is kept.
pub fn write_person_volume(
    data_dir: &std::path::Path,
    account: &str,
    user: &str,
    volume: PersonVolume,
) -> std::io::Result<()> {
    let mut file = read_person_file(data_dir);
    if !file.is_object() {
        file = serde_json::json!({});
    }
    let mine = file
        .as_object_mut()
        .expect("an object")
        .entry(account.to_owned())
        .or_insert_with(|| serde_json::json!({}));
    if !mine.is_object() {
        *mine = serde_json::json!({});
    }
    let people = mine.as_object_mut().expect("an object");
    if volume == PersonVolume::default() {
        people.remove(user);
    } else {
        people.insert(
            user.to_owned(),
            serde_json::json!({
                "voice": volume.voice,
                "voice_muted": volume.voice_muted,
                "stream": volume.stream,
                "stream_muted": volume.stream_muted,
            }),
        );
    }
    std::fs::write(data_dir.join(PERSON_VOLUMES), file.to_string())
}

#[cfg(test)]
mod person_volume_tests {
    use super::*;

    #[test]
    fn a_voice_muted_for_me_plays_at_nothing_and_a_volume_is_kept_within_range() {
        assert_eq!(PersonVolume::default().voice_gain(), 1.0);
        let muted = PersonVolume { voice: 1.5, voice_muted: true, ..Default::default() };
        assert_eq!(muted.voice_gain(), 0.0);
        let loud = PersonVolume { voice: 5.0, ..Default::default() };
        assert_eq!(loud.voice_gain(), 2.0);
    }

    #[test]
    fn person_volumes_round_trip_per_account_and_defaults_are_not_stored() {
        let dir = std::env::temp_dir().join(format!("morpheus-pv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let _ = std::fs::remove_file(dir.join(PERSON_VOLUMES));
        assert!(read_person_volumes(&dir, "@a:x").is_empty());
        let chosen = PersonVolume {
            voice: 1.0,
            voice_muted: true,
            stream: 0.3,
            stream_muted: false,
        };
        write_person_volume(&dir, "@a:x", "@bob:x", chosen).expect("write");
        write_person_volume(&dir, "@b:x", "@bob:x", PersonVolume::default()).expect("write");
        write_person_volume(&dir, "@a:x", "@cy:x", PersonVolume::default()).expect("write");
        let read = read_person_volumes(&dir, "@a:x");
        assert_eq!(read.len(), 1, "defaults are not kept: {read:?}");
        assert_eq!(read["@bob:x"], chosen);
        assert!(read_person_volumes(&dir, "@b:x").is_empty(), "per account");
        write_person_volume(&dir, "@a:x", "@bob:x", PersonVolume::default()).expect("reset");
        assert!(read_person_volumes(&dir, "@a:x").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// The quick list of microphones, speakers or cameras behind the profile
/// card's arrows and right-clicks, and the camera button's: the one in use
/// ticked, a pick switches to it, and the full settings below. A camera list
/// with no cameras says so.
pub fn device_menu(kind: crate::menu::DeviceKind, rows: &[DeviceRow]) -> Vec<crate::menu::MenuItem> {
    use crate::menu::{MenuAction, MenuItem};
    let mut items: Vec<MenuItem> = rows
        .iter()
        .map(|row| {
            MenuItem::new(row.name.clone(), MenuAction::Device(kind, row.id.clone()))
                .checked(row.checked)
        })
        .collect();
    if items.is_empty() {
        items.push(MenuItem {
            action: None,
            ..MenuItem::new("No camera found", MenuAction::VoiceSettings)
        });
    }
    items.push(MenuItem::new("Voice & Video settings", MenuAction::VoiceSettings));
    items
}

/// The bottom-left voice popups: the microphone button's menu is about input
/// only (the device list, the input volume), the headphones button's about
/// output only; both end with Voice Settings.
pub fn voice_popup_menu(
    kind: crate::menu::DeviceKind,
    devices: &[DeviceRow],
    volume: f32,
) -> Vec<crate::menu::MenuItem> {
    use crate::menu::{DeviceKind, MenuAction, MenuItem, MenuSlider};

    let children: Vec<MenuItem> = devices
        .iter()
        .map(|row| MenuItem::new(row.name.clone(), MenuAction::Device(kind, row.id.clone())).checked(row.checked))
        .collect();
    let (label, icon, volume_label, slider) = match kind {
        DeviceKind::Speaker => ("Output Device", "volume-2", "Output Volume", MenuSlider::OutputVolume(volume)),
        _ => ("Input Device", "mic", "Input Volume", MenuSlider::InputVolume(volume)),
    };
    let device = MenuItem {
        action: None,
        children,
        icon: Some(icon),
        ..MenuItem::new(label, MenuAction::VoiceSettings)
    };
    let mut items = vec![device, MenuItem::slider(volume_label, slider)];
    items.push(
        MenuItem {
            icon: Some("settings"),
            ..MenuItem::new("Voice Settings", MenuAction::VoiceSettings)
        },
    );
    items
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::test_room::room;

    #[test]
    fn a_focused_card_sits_at_the_top_and_the_strip_takes_the_height_left() {
        // Wide stage: the strip is far taller than the old 81 px and the
        // focused card fills the rest, from the top.
        let f = focus_layout(2, 1200.0, 700.0, CARD_RATIO);
        assert!(f.strip_h >= 81.0, "{f:?}");
        assert!(f.main_h + CARD_GAP + f.strip_h <= 700.0, "{f:?}");
        assert!(f.main_h + CARD_GAP + f.strip_h >= 698.0, "{f:?}");
        assert!(f.main_w <= 1200.0 && (f.main_w / f.main_h - CARD_RATIO).abs() < 0.02);
        // Narrow, tall stage: the focused card is held by the width, and all
        // the height it leaves goes to the strip (as far as its width allows).
        let f = focus_layout(1, 800.0, 900.0, CARD_RATIO);
        assert_eq!(f.main_w, 800.0);
        assert_eq!(
            f.strip_h,
            (900.0 - f.main_h - CARD_GAP).min(800.0 / CARD_RATIO).floor()
        );
        // Many in the strip: they stay on one row, so the width bounds them.
        let f = focus_layout(6, 1000.0, 800.0, CARD_RATIO);
        assert!(6.0 * f.strip_h * CARD_RATIO + 5.0 * CARD_GAP <= 1000.0 + 0.5, "{f:?}");
        assert!(f.strip_h >= 40.0);
        // A wide picture focused (a 21:9 screen) is held by the width sooner:
        // the row gets what it leaves.
        let f = focus_layout(1, 900.0, 700.0, 21.0 / 9.0);
        assert_eq!(f.main_w, 900.0);
        assert_eq!(f.strip_h, (700.0 - f.main_h - CARD_GAP).min(900.0 / CARD_RATIO).floor());
        // Nothing in the strip: the focused card has it all.
        let f = focus_layout(0, 1200.0, 700.0, CARD_RATIO);
        assert_eq!((f.strip_h, f.main_h), (0.0, 675.0));
    }

    #[test]
    fn a_share_is_a_card_to_watch_until_it_is_received() {
        let mut call = call();
        let bob = user("bob");
        call.share_event(bob.clone(), true);
        let cards = tiles(&call, &[bob.clone()], None);
        // The share first (a Watch card), then bob's own card.
        assert_eq!(cards.len(), 2);
        let Card::Video(feed) = &cards[0] else { panic!("{cards:?}") };
        assert!(feed.screen && is_unwatched(feed) && feed.user == bob);
        // Watched and received: the real feed replaces the stand-in.
        call.set_watching(bob.clone(), true);
        call.video_event(Some(bob.clone()), "bob/TR_1".into(), true, true);
        let cards = tiles(&call, &[bob.clone()], None);
        assert_eq!(cards.len(), 2);
        let Card::Video(feed) = &cards[0] else { panic!("{cards:?}") };
        assert!(!is_unwatched(feed));
        // Stopped watching: the stand-in is back; the share ending drops it
        // and the choice.
        call.video_event(None, "bob/TR_1".into(), true, false);
        call.set_watching(bob.clone(), false);
        assert_eq!(tiles(&call, &[bob.clone()], None).len(), 2);
        call.set_watching(bob.clone(), true);
        call.share_event(bob.clone(), false);
        assert!(call.watching.is_empty());
        assert_eq!(tiles(&call, &[bob], None).len(), 1);
    }

    #[test]
    fn a_focused_card_takes_the_full_width_or_all_the_height() {
        let min_strip = 81.0;
        // Wide window, 16:9 picture: the height is the limit, the strip is
        // only the smallest card tall.
        let f = focus_layout(2, 1600.0, 600.0, CARD_RATIO);
        assert_eq!(f.strip_h, min_strip, "{f:?}");
        assert_eq!(f.main_h, 600.0 - min_strip - CARD_GAP, "{f:?}");
        // Tall window: the width is the limit, full width, and the strip
        // gets the height left.
        let f = focus_layout(2, 800.0, 1200.0, CARD_RATIO);
        assert_eq!(f.main_w, 800.0, "{f:?}");
        assert!(f.main_h + CARD_GAP + f.strip_h <= 1200.0, "{f:?}");
        assert!(f.strip_h > min_strip, "{f:?}");
        // Chat open (a narrower call area): still the full width when it can.
        let f = focus_layout(1, 700.0, 700.0, CARD_RATIO);
        assert_eq!(f.main_w, 700.0, "{f:?}");
        assert_eq!(f.main_h, (700.0f32 / CARD_RATIO).floor(), "{f:?}");
        // Never stretched: the card keeps the picture's shape.
        for (w, h, r) in [(1600.0, 600.0, 4.0 / 3.0), (700.0, 700.0, 21.0 / 9.0)] {
            let f = focus_layout(2, w, h, r);
            assert!((f.main_w / f.main_h / r - 1.0).abs() < 0.02, "{f:?}");
        }
    }

    fn user(name: &str) -> OwnedUserId {
        OwnedUserId::try_from(format!("@{name}:x")).expect("user id")
    }

    fn call() -> CallState {
        CallState::joining(OwnedRoomId::try_from("!a:x").expect("id"), "hangout".into())
    }

    fn dm(id: &str, with: &str, in_call: &[&str]) -> RoomSummary {
        let mut r = room(id, with);
        r.is_dm = true;
        r.dm_user = Some(OwnedUserId::try_from(with).expect("user id"));
        r.call_participants = in_call
            .iter()
            .map(|u| OwnedUserId::try_from(*u).expect("user id"))
            .collect();
        r
    }

    #[test]
    fn a_hang_up_shows_at_once_before_the_server_says_so() {
        let me = OwnedUserId::try_from("@me:x").expect("user id");
        let mut voice = room("!v:x", "voice");
        voice.call_participants = vec![me.clone(), user("bob")];
        // In the call: as the server says.
        assert_eq!(shown_participants(&voice, Some(&me), true, false).len(), 2);
        // Just hung up: gone at once, bob still there.
        assert_eq!(
            shown_participants(&voice, Some(&me), false, true),
            [user("bob")]
        );
        // Alone in it: no call left to show (the server's badge goes too).
        voice.call_participants = vec![me.clone()];
        assert!(shown_participants(&voice, Some(&me), false, true).is_empty());
        // Back in it: shown again.
        assert_eq!(shown_participants(&voice, Some(&me), true, true), [me.clone()]);
        // Not hung up here (another device of mine is in it): shown.
        assert_eq!(shown_participants(&voice, Some(&me), false, false), [me]);
    }

    #[test]
    fn muting_and_deafening_follow_discord() {
        let none = VoiceFlags::default();
        let deaf = none.toggled_deafen();
        assert!(deaf.deafened && !deaf.muted && deaf.mic_off());
        // Unmuting while deafened undeafens.
        let un = deaf.toggled_mute();
        assert_eq!(un, VoiceFlags::default());
        // Muted, then deafened and undeafened: still muted.
        let muted = none.toggled_mute();
        assert!(muted.muted);
        assert!(muted.toggled_deafen().toggled_deafen().muted);
        assert_eq!(muted.toggled_mute(), VoiceFlags::default());
    }

    #[test]
    fn chimes_say_what_changed() {
        let none = VoiceFlags::default();
        assert_eq!(none.chime(none.toggled_mute()), Some(Sound::Mute));
        assert_eq!(none.toggled_mute().chime(none), Some(Sound::Unmute));
        assert_eq!(none.chime(none.toggled_deafen()), Some(Sound::Deafen));
        assert_eq!(none.toggled_deafen().chime(none), Some(Sound::Undeafen));
        assert_eq!(none.chime(none), None);
    }

    #[test]
    fn what_a_device_publishes() {
        let mut c = call();
        c.camera = true;
        let media = VoiceFlags {
            muted: false,
            deafened: true,
        }
        .media(&c);
        assert!(media.muted && media.deafened && media.camera && !media.screen);
    }

    #[test]
    fn marks_show_deafened_over_muted_and_video() {
        let media = CallMedia {
            muted: true,
            deafened: true,
            camera: true,
            screen: true,
        };
        let classes: Vec<_> = marks(media, true).iter().map(|m| m.class).collect();
        assert_eq!(
            classes,
            ["voice-state deafened", "media-camera", "media-live"]
        );
        let on_a_card: Vec<_> = marks(media, false).iter().map(|m| m.class).collect();
        assert_eq!(on_a_card, ["voice-state deafened"]);
        assert!(marks(CallMedia::default(), true).is_empty());
    }

    #[test]
    fn joining_lists_you_greyed_until_connected() {
        let bob = user("bob");
        let alice = user("alice");
        let listed = listed(&[bob.clone()], true, Some(&alice));
        assert_eq!(listed, vec![(bob, false), (alice, true)]);
        let again = super::listed(&[user("bob")], false, Some(&user("alice")));
        assert_eq!(again.len(), 1);
    }

    #[test]
    fn screens_come_first_then_people_with_you_first() {
        let mut c = call();
        c.video_event(Some(user("bob")), "b/cam".into(), false, true);
        c.video_event(Some(user("bob")), "b/screen".into(), true, true);
        c.video_event(Some(user("zed")), "z/cam".into(), false, true);
        let cards = tiles(&c, &[user("bob"), user("carol")], Some(user("alice")));
        let keys: Vec<String> = cards.iter().map(Card::key).collect();
        // The screen, then alice (not listed yet), bob's camera, carol, and
        // zed's camera (not listed in the call).
        assert_eq!(
            keys,
            ["b/screen", "@alice:x", "b/cam", "@carol:x", "z/cam"]
        );
    }

    #[test]
    fn local_video_sits_first_of_its_kind() {
        let mut c = call();
        c.video_event(Some(user("bob")), "b/cam".into(), false, true);
        c.video_event(Some(user("bob")), "b/screen".into(), true, true);
        c.local_video(Some(user("alice")), "local/camera", false, true);
        c.local_video(Some(user("alice")), "local/screen", true, true);
        let keys: Vec<&str> = c.video.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, ["local/screen", "b/screen", "local/camera", "b/cam"]);
        assert!(c.camera && c.screen);
        c.local_video(Some(user("alice")), "local/camera", false, false);
        assert!(!c.camera && c.video.len() == 3);
        c.video_event(None, "b/cam".into(), false, false);
        assert_eq!(c.video.len(), 2);
    }

    #[test]
    fn focus_toggles_and_forgets_cards_gone() {
        let cards = vec![Card::Person(user("alice")), Card::Person(user("bob"))];
        let focus = toggled_focus(&None, "@bob:x");
        assert_eq!(focus.as_deref(), Some("@bob:x"));
        assert_eq!(focused_card(&focus, &cards).as_deref(), Some("@bob:x"));
        assert_eq!(toggled_focus(&focus, "@bob:x"), None);
        assert_eq!(
            toggled_focus(&focus, "@alice:x").as_deref(),
            Some("@alice:x")
        );
        assert_eq!(focused_card(&Some("@gone:x".into()), &cards), None);
    }

    #[test]
    fn the_grid_picks_the_split_with_the_biggest_tiles() {
        let g = grid_layout(1, 1600.0, 800.0);
        assert_eq!((g.across, g.rows), (1, 1));
        assert_eq!((g.tile_w, g.tile_h), (1422.0, 799.0));
        let g = grid_layout(2, 1600.0, 800.0);
        assert_eq!((g.across, g.rows), (2, 1));
        assert_eq!(g.tile_w, 796.0);
        let g = grid_layout(2, 748.0, 564.0);
        assert_eq!((g.across, g.rows), (1, 2));
        assert_eq!(g.tile_w, 494.0);
        let g = grid_layout(4, 1000.0, 600.0);
        assert_eq!((g.across, g.rows), (2, 2));
        let g = grid_layout(5, 1200.0, 500.0);
        assert_eq!((g.across, g.rows), (3, 2));
        assert_eq!(g.row_counts(5), vec![3, 2]);
        assert_eq!(grid_layout(7, 1200.0, 700.0).row_counts(7), vec![3, 3, 1]);
        for count in 1..=12 {
            for (w, h) in [(1600.0, 800.0), (500.0, 900.0), (1100.0, 300.0), (90.0, 60.0)] {
                let g = grid_layout(count, w, h);
                assert!(g.tile_w >= 40.0);
                if g.tile_w > 40.0 {
                    let used_w = g.across as f32 * g.tile_w + (g.across as f32 - 1.0) * CARD_GAP;
                    let used_h = g.rows as f32 * g.tile_h + (g.rows as f32 - 1.0) * CARD_GAP;
                    assert!(used_w <= w + 0.01 && used_h <= h + 0.01, "{count} in {w}x{h}: {g:?}");
                }
            }
        }
    }

    #[test]
    fn a_cards_picture_grows_with_it_within_limits() {
        assert_eq!(card_avatar_size(144.0), 40.0);
        assert!(card_avatar_size(494.0) > 72.0);
        assert_eq!(card_avatar_size(494.0), (494.0f32 / CARD_RATIO * 0.34).round());
        assert_eq!(card_avatar_size(3000.0), 160.0);
    }

    #[test]
    fn cards_are_as_big_as_lets_all_fit() {
        // One card fills the height (16:9) or the width, whichever is less.
        assert_eq!(card_width(1, 1000.0, 300.0), 533.0);
        assert_eq!(card_width(1, 400.0, 600.0), 400.0);
        // Two across when that is the larger.
        assert_eq!(card_width(2, 1000.0, 400.0), 496.0);
        // Four in a short wide stage go in a row; in a tall one, two by two.
        assert_eq!(card_width(4, 1000.0, 200.0), 244.0);
        assert!(card_width(4, 800.0, 500.0) > card_width(4, 800.0, 150.0));
        assert_eq!(card_width(0, 800.0, 500.0), 40.0);
        assert_eq!(card_width(3, 10.0, 10.0), 40.0);
    }

    #[test]
    fn the_stage_is_dragged_within_its_limits() {
        assert_eq!(default_stage_height(600.0), 210.0);
        assert_eq!(default_stage_height(200.0), 160.0);
        assert_eq!(default_stage_height(2000.0), 420.0);
        assert_eq!(dragged_stage_height(300.0, 100.0, 800.0), 400.0);
        assert_eq!(dragged_stage_height(300.0, -400.0, 800.0), 200.0);
        assert_eq!(dragged_stage_height(300.0, 900.0, 800.0), 680.0);
    }

    fn ring(room: &str, caller: &str, until: u64) -> IncomingCall {
        IncomingCall {
            room_id: OwnedRoomId::try_from(room).expect("id"),
            caller: OwnedUserId::try_from(caller).expect("id"),
            notification: morpheus_core::OwnedEventId::try_from("$n:x").expect("id"),
            rings_until: until,
        }
    }

    #[test]
    fn a_ring_lasts_while_its_caller_waits_and_not_in_your_call() {
        let now = 1_000_000;
        let until = now + 10_000;
        let waiting = {
            let mut r = dm("!d:x", "@bob:x", &["@bob:x"]);
            r.call_participants = vec![user("bob")];
            r
        };
        let empty = dm("!d:x", "@bob:x", &[]);
        let rings = [ring("!d:x", "@bob:x", until)];
        // Fresh rings show even before the caller is listed.
        let fresh = still_ringing(&rings, None, &[empty.clone()], until - morpheus_core::RING_FOR_MS + 1_000);
        assert_eq!(fresh.len(), 1);
        // Old rings need the caller in the call.
        let late = now + 9_000;
        assert!(still_ringing(&rings, None, &[empty], late).is_empty());
        assert_eq!(still_ringing(&rings, None, &[waiting], late).len(), 1);
        // Expired, or for the call you are in.
        assert!(still_ringing(&rings, None, &[], until + 1).is_empty());
        let here = OwnedRoomId::try_from("!d:x").expect("id");
        assert!(still_ringing(&rings, Some(&here), &[], now).is_empty());
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
        let both = [
            dm("!new:x", "@bob:x", &["@bob:x"]),
            dm("!old:x", "@bob:x", &["@bob:x"]),
        ];
        assert_eq!(call_room_for(&asked, &both), None);
    }

    #[test]
    fn your_dm_call_rings_out_until_the_other_joins() {
        let mut c = call();
        let rooms = [{
            let mut r = dm("!a:x", "@bob:x", &["@alice:x"]);
            r.id = c.room_id.clone();
            r
        }];
        let me = user("alice");
        assert!(!ringing_out(Some(&c), &rooms, Some(&me)));
        c.connected = true;
        assert!(ringing_out(Some(&c), &rooms, Some(&me)));
        let joined = [{
            let mut r = rooms[0].clone();
            r.call_participants.push(user("bob"));
            r
        }];
        assert!(!ringing_out(Some(&c), &joined, Some(&me)));
        assert!(!ringing_out(None, &rooms, Some(&me)));
    }

    #[test]
    fn the_quick_device_list_ticks_the_one_in_use_and_ends_with_the_settings() {
        use crate::menu::{DeviceKind, MenuAction};
        let list = vec![("a".to_owned(), "Mic A".to_owned())];
        let items = device_menu(DeviceKind::Microphone, &device_rows(&list, Some("a")));
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["System default", "Mic A", "Voice & Video settings"]);
        assert!(!items[0].checked && items[1].checked);
        assert_eq!(
            items[1].action,
            Some(MenuAction::Device(DeviceKind::Microphone, Some("a".into())))
        );
        assert_eq!(items[2].action, Some(MenuAction::VoiceSettings));
        let none = device_menu(DeviceKind::Camera, &camera_rows(&[], None));
        assert_eq!(none[0].label, "No camera found");
        assert!(none[0].action.is_none());
    }

    #[test]
    fn call_button_titles() {
        assert_eq!(call_button_title(true, true), "Leave the call");
        assert_eq!(call_button_title(false, true), "Join the call");
        assert_eq!(call_button_title(false, false), "Start a call");
    }

    #[test]
    fn chimes_have_their_length() {
        let n = Sound::Join.samples().len();
        assert_eq!(
            n,
            (SOUND_RATE as usize * 110 / 1000) + (SOUND_RATE as usize * 260 / 1000)
        );
        assert_eq!(Sound::Ring.key(), "ring");
    }

    #[test]
    fn device_lists_start_with_the_default_and_tick_the_one_in_use() {
        let list = vec![("a".to_owned(), "Mic A".to_owned()), ("b".to_owned(), "Mic B".to_owned())];
        let rows = device_rows(&list, Some("b"));
        assert_eq!(rows[0].name, "System default");
        assert!(!rows[0].checked && rows[2].checked);
        assert!(device_rows(&list, None)[0].checked);
        let cams = camera_rows(&list, Some("a"));
        assert!(cams[0].checked && !cams[1].checked && cams.len() == 2);
    }

    #[test]
    fn chosen_devices_are_kept_in_the_data_directory() {
        let dir = std::env::temp_dir().join(format!("morpheus-devprefs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        assert_eq!(read_device_prefs(&dir), DevicePrefs::default());
        let chosen = DevicePrefs {
            microphone: Some("mic".into()),
            speaker: None,
            camera: Some("cam".into()),
            input_volume: 1.5,
            output_volume: 0.8,
            hardware_video: false,
        };
        write_device_prefs(&dir, &chosen).expect("write");
        assert_eq!(read_device_prefs(&dir), chosen);
        // A file from before the switch: hardware video stays on.
        std::fs::write(dir.join(AUDIO_DEVICES), r#"{"microphone":"mic"}"#).expect("write");
        assert!(read_device_prefs(&dir).hardware_video);
        std::fs::remove_dir_all(&dir).expect("clean");
    }

    #[test]
    fn voice_popups_are_input_only_or_output_only() {
        use crate::menu::{DeviceKind, MenuSlider};
        let mics = vec![DeviceRow { id: Some("mic1".into()), name: "Mic 1".into(), checked: true }];
        let items = voice_popup_menu(DeviceKind::Microphone, &mics, 1.2);
        assert_eq!(items.len(), 3, "device, volume, settings: {items:?}");
        assert_eq!(items[0].label, "Input Device");
        assert_eq!(items[0].children[0].label, "Mic 1");
        assert_eq!(items[1].slider, Some(MenuSlider::InputVolume(1.2)));
        assert!(items.iter().all(|i| !i.label.contains("Noise")), "no noise suppression in the mic menu");
        assert_eq!(items[2].label, "Voice Settings");
        assert_eq!(items[2].icon, Some("settings"));
        let items = voice_popup_menu(DeviceKind::Speaker, &[], 0.7);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].label, "Output Device");
        assert_eq!(items[1].slider, Some(MenuSlider::OutputVolume(0.7)));
    }
}

/// What a right-click on someone's card in a call adds to their user menu:
/// their volume for you and mute for you (not on your own card), and focus.
/// On their shared screen (`stream`) the volume and mute are for its sound
/// alone: "Stream volume" and "Mute stream".
pub fn call_card_items(
    user: &morpheus_core::OwnedUserId,
    own: bool,
    stream: bool,
    volume: f32,
    muted: bool,
    hidden: bool,
    watching: bool,
) -> Vec<crate::menu::MenuItem> {
    use crate::menu::{MenuAction, MenuItem, MenuSlider};
    let mut items = Vec::new();
    if !own && stream {
        items.push(MenuItem::slider("Stream volume", MenuSlider::StreamVolume(user.clone(), volume)));
        items.push(
            MenuItem::new(
                if muted { "Unmute stream" } else { "Mute stream" },
                MenuAction::CallMuteStream(user.clone(), !muted),
            )
            .icon(if muted { "volume-2" } else { "volume-x" }),
        );
    } else if !own {
        items.push(MenuItem::slider("Volume", MenuSlider::UserVolume(user.clone(), volume)));
        items.push(
            MenuItem::new(
                if muted { "Unmute for me" } else { "Mute for me" },
                MenuAction::CallMute(user.clone(), !muted),
            )
            .icon(if muted { "volume-2" } else { "volume-x" }),
        );
    }
    if stream && watching && !own {
        items.push(
            MenuItem::new("Stop watching", MenuAction::CallStopWatching(user.clone()))
                .icon("eye-off"),
        );
    }
    // Someone else's camera: shown or hidden (a check mark says which). Clicking
    // the card focuses it, so there is no Focus item.
    if !own && !stream {
        items.push(
            MenuItem::new("Hide video", MenuAction::CallHideVideo(user.clone()))
                .icon("video-off")
                .checked(hidden),
        );
    }
    items
}

/// The hint under the share picker's sources: on Windows (where the picker
/// is the app's own and nobody has seen that a double click shares), a line
/// saying so; nowhere else.
pub fn screen_picker_hint(on_windows: bool) -> Option<&'static str> {
    on_windows.then_some("Double-click a screen or window to share it")
}

#[cfg(test)]
mod screen_picker_hint_tests {
    use super::*;

    #[test]
    fn the_share_picker_hints_at_double_click_on_windows_only() {
        assert_eq!(
            screen_picker_hint(true),
            Some("Double-click a screen or window to share it")
        );
        assert_eq!(screen_picker_hint(false), None);
    }
}

/// When the call began, in ms since the epoch: the earliest join among the
/// memberships active now. A membership that has left (empty content) or has
/// expired (`expires` passed, counted from when it began) is not part of the
/// call, however old; when everyone left and someone joined again, the call
/// starts from that join. A join's time is its `created_ts`, else the event's
/// own time. `None` when nobody is in the call.
pub fn call_start(now_ms: u64, memberships: &[CallMembership]) -> Option<u64> {
    memberships
        .iter()
        .filter(|m| !m.left)
        .filter_map(|m| {
            let began = m.created_ts.or(m.origin_server_ts)?;
            let lapsed = m.expires_ms.is_some_and(|lasts| began.saturating_add(lasts) <= now_ms);
            (!lapsed).then_some(began)
        })
        .min()
}

/// How long a call has been going, as a timer: `m:ss`, `h:mm:ss` from an
/// hour. `None` for a call whose start is not known or lies ahead of the
/// clock by more than a few seconds (clocks differ a little: a start up to
/// 5 s ahead reads as 0:00).
pub fn call_timer(now_ms: u64, started_ms: Option<u64>) -> Option<String> {
    let started = started_ms?;
    if started > now_ms + 5_000 {
        return None;
    }
    let seconds = now_ms.saturating_sub(started) / 1000;
    let (h, m, s) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    Some(if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    })
}

#[cfg(test)]
mod call_timer_tests {
    use super::*;

    #[test]
    fn the_timer_counts_from_the_calls_start() {
        let now = 1_000_000_000;
        assert_eq!(call_timer(now, Some(now)), Some("0:00".into()));
        assert_eq!(call_timer(now, Some(now - 65_000)), Some("1:05".into()));
        assert_eq!(call_timer(now, Some(now - 3_599_000)), Some("59:59".into()));
        assert_eq!(call_timer(now, Some(now - 3_600_000)), Some("1:00:00".into()));
        assert_eq!(call_timer(now, Some(now - 36_125_000)), Some("10:02:05".into()));
    }

    #[test]
    fn an_unknown_or_future_start_shows_nothing_or_zero() {
        let now = 1_000_000_000;
        assert_eq!(call_timer(now, None), None);
        assert_eq!(call_timer(now, Some(now + 3_000)), Some("0:00".into()));
        assert_eq!(call_timer(now, Some(now + 60_000)), None);
    }
}

#[cfg(test)]
mod call_start_tests {
    use super::*;

    const NOW: u64 = 1_000_000_000_000;
    const MIN: u64 = 60_000;

    fn joined(created_ago_min: u64) -> CallMembership {
        CallMembership {
            created_ts: Some(NOW - created_ago_min * MIN),
            origin_server_ts: Some(NOW - created_ago_min * MIN + 500),
            ..Default::default()
        }
    }

    fn left(ago_min: u64) -> CallMembership {
        CallMembership {
            left: true,
            // A left event carries no created_ts.
            origin_server_ts: Some(NOW - ago_min * MIN),
            ..Default::default()
        }
    }

    #[test]
    fn the_earliest_join_among_the_present_is_the_start() {
        let members = [joined(3), joined(10), joined(1)];
        assert_eq!(call_start(NOW, &members), Some(NOW - 10 * MIN));
    }

    #[test]
    fn nobody_in_the_call_has_no_start() {
        assert_eq!(call_start(NOW, &[]), None);
        assert_eq!(call_start(NOW, &[left(5), left(500)]), None);
    }

    #[test]
    fn a_stale_left_membership_is_ignored() {
        // An old device left long ago; its event's time must not count.
        let members = [left(600), joined(2)];
        assert_eq!(call_start(NOW, &members), Some(NOW - 2 * MIN));
    }

    #[test]
    fn an_expired_membership_is_ignored() {
        let expired = CallMembership {
            expires_ms: Some(2 * 60 * MIN),
            ..joined(5 * 60)
        };
        // Began 5 h ago and lasted 2 h: over 3 h ago.
        let alive = CallMembership {
            expires_ms: Some(2 * 60 * MIN),
            ..joined(60)
        };
        assert_eq!(call_start(NOW, &[expired.clone(), joined(1)]), Some(NOW - MIN));
        assert_eq!(call_start(NOW, &[expired]), None);
        // Not yet expired: counts.
        assert_eq!(call_start(NOW, &[alive, joined(1)]), Some(NOW - 60 * MIN));
    }

    #[test]
    fn a_rejoin_after_the_call_emptied_starts_again() {
        // An earlier call (its members left), then someone joined just now.
        let members = [left(180), left(170), joined(0)];
        assert_eq!(call_start(NOW, &members), Some(NOW));
        // The same device rejoining replaces its event: only the new join exists.
        assert_eq!(call_start(NOW, &[joined(0)]), Some(NOW));
    }

    #[test]
    fn several_devices_count_from_the_earliest_present() {
        let phone = joined(7);
        let laptop = joined(4);
        let gone = left(1);
        assert_eq!(call_start(NOW, &[laptop, gone, phone]), Some(NOW - 7 * MIN));
    }

    #[test]
    fn without_created_ts_the_events_own_time_counts() {
        let bare = CallMembership {
            origin_server_ts: Some(NOW - 9 * MIN),
            ..Default::default()
        };
        assert_eq!(call_start(NOW, &[bare, joined(2)]), Some(NOW - 9 * MIN));
        // Neither time known: not counted as a start.
        assert_eq!(call_start(NOW, &[CallMembership::default()]), None);
    }
}
