#![allow(clippy::expect_used)]
//! A second person in a call, for the phone's end-to-end tests (they run
//! this as a child process and talk to it over its input and output). It
//! signs in as `bob`, sends a test tone for a microphone and a test pattern
//! for its camera and screen, and does what the lines on its input say:
//!
//! - `join <room name>`, `mute`, `deafen`, `camera on|off`, `screen on|off`,
//!   `leave` (then it exits);
//! - each change of the call is a line on its output:
//!   `call <phase> watchers=<n> <user>[mds] ...` where the letters say what
//!   the person has on (m muted, d deafened, s speaking, c camera, w sharing
//!   their screen), and `ready` once signed in.
//!
//! `MORPHEUS_TEST_HOMESERVER` names the homeserver.

use std::{io::BufRead, sync::Arc};

use morpheus_ffi::{CallListener, CallPhase, CallSnapshot, Morpheus, Sidebar, SidebarListener};
use tokio::sync::mpsc;

struct Calls;
impl CallListener for Calls {
    fn on_call(&self, snapshot: CallSnapshot) {
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
    app.login(homeserver, "bob".into(), "bob-password".into()).await.expect("bob signs in");
    let (tx, mut rooms) = mpsc::unbounded_channel();
    let _watch = app.watch_sidebar(Arc::new(Rooms(std::sync::Mutex::new(tx)))).await.expect("the list");
    println!("ready");

    let (lines_tx, mut lines) = mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines().map_while(Result::ok) {
            let _ = lines_tx.send(line);
        }
    });
    let mut call = None;
    let mut list: Option<Sidebar> = None;
    // A join asked for before the room is in the list waits for it.
    let mut pending: Option<String> = None;
    loop {
        tokio::select! {
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
                    "mute" => call.iter().for_each(|c| c.toggle_mute()),
                    "deafen" => call.iter().for_each(|c| c.toggle_deafen()),
                    "camera" => call.iter().for_each(|c| c.set_camera(rest == "on")),
                    "screen" => call.iter().for_each(|c| c.set_screen_share(rest == "on")),
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
