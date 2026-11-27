#![allow(clippy::expect_used)]
//! A second person in a call, for the phone's end-to-end tests (they run
//! this as a child process and talk to it over its input and output). It
//! signs in as `bob`, sends a test tone for a microphone and a test pattern
//! for its camera and screen, and does what the lines on its input say:
//!
//! - `callroom <room id>` joins the call of that room (a direct message rings the other person);
//! - `join <room name>`, `mute`, `deafen`, `camera on|off`, `screen on|off`, `watch <user id> on|off`,
//!   `leave` (then it exits);
//! - each change of the call is a line on its output:
//!   `call <phase> watchers=<n> <user>[mds] ...` where the letters say what
//!   the person has on (m muted, d deafened, s speaking, c camera, w sharing
//!   their screen), and `ready` once signed in;
//! - `frame <user id> <width>x<height> seq=<n>` about twice a second for each
//!   camera or shared screen it receives (what the others' video looks like
//!   from here).
//!
//! `MORPHEUS_TEST_HOMESERVER` names the homeserver.

use std::{io::BufRead, sync::Arc};

use morpheus_ffi::{CallListener, CallPhase, CallSnapshot, Morpheus, Sidebar, SidebarListener};
use tokio::sync::mpsc;

/// The video feeds of the latest snapshot: (user id, feed).
static FEEDS: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

struct Calls;
impl CallListener for Calls {
    fn on_call(&self, snapshot: CallSnapshot) {
        *FEEDS.lock().expect("lock") = snapshot
            .cards
            .iter()
            .filter_map(|c| c.feed.clone().map(|f| (c.user_id.clone(), f)))
            .collect();
        let people: Vec<String> = snapshot
            .people
            .iter()
            .map(|p| {
                let flags: String = [(p.muted, 'm'), (p.deafened, 'd'), (p.speaking, 's'), (p.camera, 'c'), (p.screen, 'w')]
                    .iter()
                    .filter(|(on, _)| *on)
                    .map(|(_, c)| *c)
                    .collect();
                format!("{}[{flags}]", p.user_id)
            })
            .collect();
        let phase = match snapshot.phase {
            CallPhase::Joining => "joining",
            CallPhase::Connected => "connected",
            CallPhase::Failed => "failed",
            CallPhase::Ended => "ended",
        };
        println!("call {phase} watchers={} {}", snapshot.watchers, people.join(" "));
        // What the call screen would draw: a kind and a user for each card, `+` for a card with a feed (a picture to receive).
        let cards: Vec<String> = snapshot.cards.iter().map(|c| format!("{:?}:{}{}", c.kind, c.user_id, if c.feed.is_some() { "+" } else { "" })).collect();
        println!("cards {}", cards.join(" "));
    }
}

struct Rooms(std::sync::Mutex<mpsc::UnboundedSender<Sidebar>>);
impl SidebarListener for Rooms {
    fn on_sidebar(&self, sidebar: Sidebar) {
        let _ = self.0.lock().expect("lock").send(sidebar);
    }
}

fn room_id(list: &Option<Sidebar>, name: &str) -> Option<String> {
    list.as_ref()?
        .scopes
        .iter()
        .flat_map(|sc| sc.sections.iter().flat_map(|sec| sec.rooms.iter()))
        .find(|r| r.name == name)
        .map(|r| r.id.clone())
}

#[tokio::main]
async fn main() {
    morpheus_ffi::call_use_test_tone();
    let homeserver = std::env::var("MORPHEUS_TEST_HOMESERVER").expect("MORPHEUS_TEST_HOMESERVER");
    let dir = tempfile::tempdir().expect("dir");
    let app = Morpheus::new(dir.path().display().to_string()).expect("app");
    // Bob, unless MORPHEUS_PEER_USER names someone else (their password is "<name>-password").
    let user = std::env::var("MORPHEUS_PEER_USER").unwrap_or_else(|_| "bob".into());
    app.login(homeserver, user.clone(), format!("{user}-password")).await.expect("bob signs in");
    let (tx, mut rooms) = mpsc::unbounded_channel();
    let _watch = app.watch_sidebar(Arc::new(Rooms(std::sync::Mutex::new(tx)))).await.expect("the list");
    println!("ready");

    let (lines_tx, mut lines) = mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            let _ = lines_tx.send(line);
        }
    });
    let mut call: Option<Arc<morpheus_ffi::CallHandle>> = None;
    let mut list: Option<Sidebar> = None;
    // A join asked for before the room is in the list waits for it.
    let mut pending: Option<String> = None;
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(500));
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let feeds = FEEDS.lock().expect("lock").clone();
                if let Some(diag) = morpheus_call::call_diag() {
                    let tracks: Vec<String> = diag.tracks.iter().map(|t| format!("{}:{}:{}:{}x{}:{:.0}fps:{:.0}bps", t.direction, t.label, t.implementation, t.width, t.height, t.fps, t.bitrate.unwrap_or(0.0))).collect();
                    if !tracks.is_empty() {
                        println!("tracks {}", tracks.join(" "));
                    }
                }
                for (user, feed) in feeds {
                    if let Some(frame) = call.as_ref().and_then(|c| c.frame(feed)) {
                        println!("frame {user} {}x{} seq={}", frame.width, frame.height, frame.seq);
                    }
                }
            }
            Some(sidebar) = rooms.recv() => {
                list = Some(sidebar);
                if let Some(name) = pending.clone() && let Some(id) = room_id(&list, &name) {
                    pending = None;
                    call = Some(app.join_call(id, Arc::new(Calls)).await.expect("joins"));
                }
            }
            line = lines.recv() => {
                let Some(line) = line else { break };
                let (word, rest) = line.split_once(' ').unwrap_or((line.as_str(), ""));
                match word {
                    "join" => {
                        match room_id(&list, rest) {
                            Some(id) => call = Some(app.join_call(id, Arc::new(Calls)).await.expect("joins")),
                            None => pending = Some(rest.to_owned()),
                        }
                    }
                    // A room by its id.
                    "callroom" => {
                        // An invite to it (the DM was just made) is accepted first.
                        for _ in 0..15 {
                            if app.accept_invite(rest.to_owned()).await.is_ok() {
                                break;
                            }
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        }
                        call = Some(app.join_call(rest.to_owned(), Arc::new(Calls)).await.expect("joins"));
                    }
                    "mute" => call.iter().for_each(|c| c.toggle_mute()),
                    "deafen" => call.iter().for_each(|c| c.toggle_deafen()),
                    "camera" => call.iter().for_each(|c| c.set_camera(rest == "on")),
                    "screen" => call.iter().for_each(|c| c.set_screen_share(rest == "on")),
                    // `watch <user id> on|off`: watch (or stop watching) that person's shared screen, so its pictures are received.
                    "watch" => {
                        if let Some((user, on)) = rest.split_once(' ') {
                            call.iter().for_each(|c| c.watch(user.to_owned(), on == "on"));
                        }
                    }
                    "leave" => {
                        if let Some(c) = call.take() {
                            c.leave().await;
                        }
                        break;
                    }
                    _ => println!("error unknown {line}"),
                }
            }
        }
    }
}
