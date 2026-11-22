//! Discord Rich Presence (#330): the activity "Chatting on Morpheus" with a
//! button to a Matrix link, sent over Discord's local IPC. This is the part
//! with no I/O: which link is shown, the activity's JSON, the frames, the
//! places Discord's socket may be, and the settings.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// The Discord application ("Morpheus", with a logo asset named "morpheus") whose
/// name Discord shows after "Playing". Create it at discord.com/developers and put
/// its Application ID here; `MORPHEUS_DISCORD_CLIENT_ID` overrides it. Empty: the
/// feature does nothing.
pub const CLIENT_ID: &str = "";

pub const SETTING_LABEL: &str = "Show \"Chatting on Morpheus\" in my Discord activity";
pub const SETTING_HINT: &str = "While Morpheus runs and Discord is open, your Discord status says you are chatting on Morpheus, with a button that opens the link below. It is cleared when you quit.";
pub const DETAILS: &str = "Chatting on Morpheus";

/// The Application ID in use: the environment's, else [`CLIENT_ID`]; None when unset.
pub fn client_id() -> Option<String> {
    let id = std::env::var("MORPHEUS_DISCORD_CLIENT_ID").unwrap_or_else(|_| CLIENT_ID.to_owned());
    let id = id.trim();
    (!id.is_empty()).then(|| id.to_owned())
}

// ---- settings -------------------------------------------------------------------

/// Which link the activity's button opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinkChoice {
    /// My matrix.to profile link.
    #[default]
    Profile,
    /// The room I'm in, only when it is public (else no button).
    Room,
    /// No button.
    None,
}

impl LinkChoice {
    pub const ALL: [LinkChoice; 3] = [LinkChoice::Profile, LinkChoice::Room, LinkChoice::None];

    pub fn key(self) -> &'static str {
        match self {
            LinkChoice::Profile => "profile",
            LinkChoice::Room => "room",
            LinkChoice::None => "none",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LinkChoice::Profile => "My profile",
            LinkChoice::Room => "The room I'm in (public rooms only)",
            LinkChoice::None => "No link",
        }
    }
}

/// What the user chose for Rich Presence, kept on this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prefs {
    pub on: bool,
    pub link: LinkChoice,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { on: true, link: LinkChoice::Profile }
    }
}

const OFF: &str = "discord-presence-off";
const LINK: &str = "discord-presence-link";

pub fn load(data_dir: &Path) -> Prefs {
    let kept = std::fs::read_to_string(data_dir.join(LINK)).unwrap_or_default();
    Prefs {
        on: !data_dir.join(OFF).exists(),
        link: LinkChoice::ALL
            .into_iter()
            .find(|l| l.key() == kept.trim())
            .unwrap_or_default(),
    }
}

pub fn save(data_dir: &Path, prefs: Prefs) -> std::io::Result<()> {
    let off = data_dir.join(OFF);
    if prefs.on {
        match std::fs::remove_file(&off) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(err),
            _ => {}
        }
    } else {
        std::fs::write(off, "")?;
    }
    std::fs::write(data_dir.join(LINK), prefs.link.key())
}

// ---- the link -------------------------------------------------------------------

/// A room as far as the link goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomLink {
    /// Whether anyone may join it.
    pub public: bool,
    /// Its address (`#alias:server`) or, with none, its id with a server to find it by
    /// (`!id:server?via=server`): only ever used for a public room.
    pub target: String,
}

/// The activity's button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub label: String,
    pub url: String,
}

pub fn matrix_to(target: &str) -> String {
    format!("https://matrix.to/#/{target}")
}

/// The button for `choice`: a private room (or none) never gives one.
pub fn choose_link(choice: LinkChoice, own_id: &str, room: Option<&RoomLink>) -> Option<Button> {
    match choice {
        LinkChoice::None => None,
        LinkChoice::Profile => own_id.starts_with('@').then(|| Button {
            label: "My Matrix profile".into(),
            url: matrix_to(own_id),
        }),
        LinkChoice::Room => room.filter(|r| r.public).map(|r| Button {
            label: "Join this room".into(),
            url: matrix_to(&r.target),
        }),
    }
}

// ---- the activity -----------------------------------------------------------------

/// What to show, or nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    pub button: Option<Button>,
    /// The line under the title: what my status shows (a game, a song), if anything.
    pub state: Option<String>,
    /// Seconds since the epoch when it began (Discord's "elapsed").
    pub started: u64,
}

/// The `SET_ACTIVITY` command; `activity` None clears it.
pub fn set_activity(pid: u32, activity: Option<&Activity>, nonce: &str) -> Value {
    let activity = activity.map(|a| {
        let mut value = json!({
            "details": DETAILS,
            "assets": { "large_image": "morpheus", "large_text": "Morpheus" },
            "timestamps": { "start": a.started },
        });
        if let Some(state) = &a.state {
            value["state"] = json!(state);
        }
        if let Some(button) = &a.button {
            value["buttons"] = json!([{ "label": button.label, "url": button.url }]);
        }
        value
    });
    json!({
        "cmd": "SET_ACTIVITY",
        "args": { "pid": pid, "activity": activity },
        "nonce": nonce,
    })
}

// ---- the wire -------------------------------------------------------------------------

pub const OP_HANDSHAKE: u32 = 0;
pub const OP_FRAME: u32 = 1;
pub const OP_CLOSE: u32 = 2;

/// An IPC message: the opcode and the length (little endian), then the JSON.
pub fn frame(op: u32, payload: &Value) -> Vec<u8> {
    let body = payload.to_string().into_bytes();
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

pub fn handshake(client_id: &str) -> Vec<u8> {
    frame(OP_HANDSHAKE, &json!({ "v": 1, "client_id": client_id }))
}

/// The opcode and body length of a frame's 8-byte header.
pub fn header(bytes: [u8; 8]) -> (u32, usize) {
    let op = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let len = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    (op, len as usize)
}

/// Where Discord's socket may be, best first: `MORPHEUS_DISCORD_IPC` alone when set,
/// else `discord-ipc-0..9` in each of the runtime folders (Linux: `$XDG_RUNTIME_DIR`
/// with the snap and flatpak ones under it; macOS: `$TMPDIR`; Windows: the pipes).
pub fn ipc_candidates(
    override_path: Option<&str>,
    runtime_dir: Option<&Path>,
    tmp_dir: Option<&Path>,
) -> Vec<PathBuf> {
    if let Some(path) = override_path.filter(|p| !p.is_empty()) {
        return vec![PathBuf::from(path)];
    }
    if cfg!(windows) {
        return (0..10)
            .map(|n| PathBuf::from(format!(r"\\.\pipe\discord-ipc-{n}")))
            .collect();
    }
    let mut folders: Vec<PathBuf> = Vec::new();
    if let Some(run) = runtime_dir {
        folders.push(run.to_path_buf());
        folders.push(run.join("snap.discord"));
        folders.push(run.join("app/com.discordapp.Discord"));
        folders.push(run.join("app/com.discordapp.DiscordCanary"));
    }
    for dir in [tmp_dir, Some(Path::new("/tmp"))].into_iter().flatten() {
        folders.push(dir.to_path_buf());
    }
    let mut out = Vec::new();
    for folder in &folders {
        for n in 0..10 {
            let path = folder.join(format!("discord-ipc-{n}"));
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(public: bool) -> RoomLink {
        RoomLink { public, target: "#lobby:example.org".into() }
    }

    #[test]
    fn the_profile_link_is_a_matrix_to_user_link() {
        let button = choose_link(LinkChoice::Profile, "@me:example.org", None).expect("button");
        assert_eq!(button.url, "https://matrix.to/#/@me:example.org");
    }

    #[test]
    fn a_private_room_never_gives_a_link() {
        let private = RoomLink { public: false, target: "!secret:example.org".into() };
        assert_eq!(choose_link(LinkChoice::Room, "@me:example.org", Some(&private)), None);
        assert_eq!(choose_link(LinkChoice::Room, "@me:example.org", None), None);
        let button = choose_link(LinkChoice::Room, "@me:example.org", Some(&room(true))).expect("public");
        assert_eq!(button.url, "https://matrix.to/#/#lobby:example.org");
    }

    #[test]
    fn no_link_means_no_button() {
        assert_eq!(choose_link(LinkChoice::None, "@me:example.org", Some(&room(true))), None);
    }

    #[test]
    fn the_activity_says_chatting_with_its_button() {
        let activity = Activity {
            button: choose_link(LinkChoice::Profile, "@me:example.org", None),
            state: Some("Playing Hades".into()),
            started: 100,
        };
        let v = set_activity(42, Some(&activity), "n1");
        assert_eq!(v["cmd"], "SET_ACTIVITY");
        assert_eq!(v["args"]["pid"], 42);
        assert_eq!(v["args"]["activity"]["details"], "Chatting on Morpheus");
        assert_eq!(v["args"]["activity"]["assets"]["large_image"], "morpheus");
        assert_eq!(v["args"]["activity"]["buttons"][0]["url"], "https://matrix.to/#/@me:example.org");
        assert_eq!(v["args"]["activity"]["timestamps"]["start"], 100);
        assert_eq!(v["args"]["activity"]["state"], "Playing Hades");
    }

    #[test]
    fn without_a_button_there_is_no_buttons_field_and_clearing_sends_null() {
        let v = set_activity(1, Some(&Activity { button: None, state: None, started: 1 }), "n");
        assert!(v["args"]["activity"].get("buttons").is_none());
        let cleared = set_activity(1, None, "n");
        assert!(cleared["args"]["activity"].is_null());
    }

    #[test]
    fn frames_have_an_opcode_and_a_length() {
        let bytes = handshake("123");
        let (op, len) = header(bytes[..8].try_into().expect("header"));
        assert_eq!(op, OP_HANDSHAKE);
        assert_eq!(len, bytes.len() - 8);
        let body: Value = serde_json::from_slice(&bytes[8..]).expect("json");
        assert_eq!(body, json!({ "v": 1, "client_id": "123" }));
    }

    #[test]
    fn the_sockets_are_looked_for_in_the_usual_folders() {
        let c = ipc_candidates(None, Some(Path::new("/run/user/1000")), Some(Path::new("/var/tmp")));
        if !cfg!(windows) {
            assert_eq!(c[0], PathBuf::from("/run/user/1000/discord-ipc-0"));
            assert!(c.contains(&PathBuf::from("/run/user/1000/snap.discord/discord-ipc-3")));
            assert!(c.contains(&PathBuf::from("/run/user/1000/app/com.discordapp.Discord/discord-ipc-0")));
            assert!(c.contains(&PathBuf::from("/var/tmp/discord-ipc-9")));
        }
        assert_eq!(ipc_candidates(Some("/x/sock"), None, None), vec![PathBuf::from("/x/sock")]);
    }

    #[test]
    fn the_settings_default_on_with_the_profile_link_and_are_kept() {
        let dir = std::env::temp_dir().join(format!("morpheus-presence-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        assert_eq!(load(&dir), Prefs { on: true, link: LinkChoice::Profile });
        save(&dir, Prefs { on: false, link: LinkChoice::Room }).expect("save");
        assert_eq!(load(&dir), Prefs { on: false, link: LinkChoice::Room });
        save(&dir, Prefs { on: true, link: LinkChoice::None }).expect("save");
        assert_eq!(load(&dir), Prefs { on: true, link: LinkChoice::None });
        std::fs::remove_dir_all(&dir).ok();
    }
}
