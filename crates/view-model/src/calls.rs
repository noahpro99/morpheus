//! Calls with no UI toolkit in it: what the call is (who is in it, speaking,
//! muted, on camera or sharing a screen), the cards the call view shows and
//! how they fit the stage, the one that is focused, who is ringing, the
//! chimes, and the device lists. The media itself is `morpheus-call`; this
//! only turns its events and the room's call memberships into what is drawn.

use morpheus_core::{CallMedia, IncomingCall, OwnedRoomId, OwnedUserId, RoomSummary};

/// How long after a ring a caller may still be missing from the call (the
/// ring and the membership arrive in either order).
pub const RING_GRACE_MS: u64 = 5_000;

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

/// Discord's cards: all the same size (16:9), as big as lets every one of
/// `count` fit a `width` by `height` stage, trying each number across.
pub fn card_width(count: usize, width: f32, height: f32) -> f32 {
    if count == 0 {
        return 40.0;
    }
    let mut best = 0.0f32;
    for across in 1..=count {
        let rows = count.div_ceil(across);
        let w = ((width - (across as f32 - 1.0) * CARD_GAP) / across as f32)
            .min(((height - (rows as f32 - 1.0) * CARD_GAP) / rows as f32) * CARD_RATIO);
        best = best.max(w);
    }
    best.floor().max(40.0)
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
    /// Someone is calling (repeats while it rings).
    Ring,
    /// Your call ringing the other person (repeats until they answer).
    Ringback,
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
            Self::Ring => "ring",
            Self::Ringback => "ringback",
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
            Self::Ring => &[(659.25, 120), (830.61, 120), (987.77, 120), (1318.51, 320)],
            Self::Ringback => &[(440.0, 380), (440.0, 380)],
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


/// Where the devices chosen for calls are kept (the Dioxus app's file, so
/// the two apps agree).
pub const AUDIO_DEVICES: &str = "audio_devices.json";

/// The microphone, speaker and camera chosen for calls (absent: the system's
/// defaults, the first camera that works).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DevicePrefs {
    pub microphone: Option<String>,
    pub speaker: Option<String>,
    pub camera: Option<String>,
}

pub fn read_device_prefs(data_dir: &std::path::Path) -> DevicePrefs {
    let saved: serde_json::Value = std::fs::read(data_dir.join(AUDIO_DEVICES))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let field = |name: &str| saved.get(name).and_then(|v| v.as_str()).map(str::to_owned);
    DevicePrefs {
        microphone: field("microphone"),
        speaker: field("speaker"),
        camera: field("camera"),
    }
}

pub fn write_device_prefs(data_dir: &std::path::Path, prefs: &DevicePrefs) -> std::io::Result<()> {
    let json = serde_json::json!({
        "microphone": prefs.microphone,
        "speaker": prefs.speaker,
        "camera": prefs.camera,
    });
    std::fs::write(data_dir.join(AUDIO_DEVICES), json.to_string())
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

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::test_room::room;

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
        };
        write_device_prefs(&dir, &chosen).expect("write");
        assert_eq!(read_device_prefs(&dir), chosen);
        std::fs::remove_dir_all(&dir).expect("clean");
    }
}
