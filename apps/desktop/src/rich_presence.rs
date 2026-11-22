//! Discord Rich Presence (#330): a thread that keeps the activity "Chatting on
//! Morpheus" on Discord's local IPC while Morpheus runs. `Shell` says what is
//! wanted (`want`); the thread connects when Discord is there (trying again every
//! few seconds, quietly), sends only what changed, and clears it on quit.
//! Does nothing without a Discord application id (`view_model::presence::CLIENT_ID`
//! or `MORPHEUS_DISCORD_CLIENT_ID`).

use std::{
    io::{Read, Write},
    sync::{Condvar, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::view_model::presence::{self, Activity, Button};

/// What the user wants shown right now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Wanted {
    pub on: bool,
    pub button: Option<Button>,
    pub state: Option<String>,
}

#[derive(Default)]
struct Shared {
    wanted: Wanted,
    quit: bool,
    done: bool,
}

fn shared() -> &'static (Mutex<Shared>, Condvar) {
    static SHARED: OnceLock<(Mutex<Shared>, Condvar)> = OnceLock::new();
    SHARED.get_or_init(Default::default)
}

/// Whether there is an application id to talk with.
pub fn available() -> bool {
    presence::client_id().is_some()
}

/// Says what to show; starts the thread the first time.
pub fn want(wanted: Wanted) {
    let Some(client_id) = presence::client_id() else {
        return;
    };
    static START: std::sync::Once = std::sync::Once::new();
    START.call_once(|| {
        std::thread::Builder::new()
            .name("discord-presence".into())
            .spawn(move || run(client_id))
            .ok();
    });
    let (lock, signal) = shared();
    if let Ok(mut shared) = lock.lock()
        && shared.wanted != wanted
    {
        shared.wanted = wanted;
        signal.notify_all();
    }
}

/// Clears the activity and lets go of the connection (at most a second).
pub fn shutdown() {
    let (lock, signal) = shared();
    let Ok(mut shared) = lock.lock() else {
        return;
    };
    shared.quit = true;
    signal.notify_all();
    let deadline = Instant::now() + Duration::from_secs(1);
    while !shared.done {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match signal.wait_timeout(shared, left) {
            Ok((guard, _)) => shared = guard,
            Err(_) => break,
        }
    }
}

trait Wire: Read + Write {}
impl<T: Read + Write> Wire for T {}

#[cfg(unix)]
fn open(path: &std::path::Path) -> Option<Box<dyn Wire>> {
    let stream = std::os::unix::net::UnixStream::connect(path).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    stream.set_write_timeout(Some(Duration::from_secs(3))).ok()?;
    Some(Box::new(stream))
}

#[cfg(windows)]
fn open(path: &std::path::Path) -> Option<Box<dyn Wire>> {
    let pipe = std::fs::OpenOptions::new().read(true).write(true).open(path).ok()?;
    Some(Box::new(pipe))
}

fn read_frame(wire: &mut dyn Wire) -> Option<(u32, Vec<u8>)> {
    let mut head = [0u8; 8];
    wire.read_exact(&mut head).ok()?;
    let (op, len) = presence::header(head);
    if len > 1 << 20 {
        return None;
    }
    let mut body = vec![0u8; len];
    wire.read_exact(&mut body).ok()?;
    Some((op, body))
}

/// Connects to the first Discord that answers the handshake.
fn connect(client_id: &str) -> Option<Box<dyn Wire>> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from);
    let tmp = std::env::var_os("TMPDIR").map(std::path::PathBuf::from);
    let over = std::env::var("MORPHEUS_DISCORD_IPC").ok();
    for path in presence::ipc_candidates(over.as_deref(), runtime.as_deref(), tmp.as_deref()) {
        let Some(mut wire) = open(&path) else {
            continue;
        };
        if wire.write_all(&presence::handshake(client_id)).is_err() {
            continue;
        }
        // READY, or an error frame (the id is not one Discord knows).
        match read_frame(&mut *wire) {
            Some((presence::OP_FRAME, _)) => return Some(wire),
            _ => continue,
        }
    }
    None
}

fn send(wire: &mut dyn Wire, activity: Option<&Activity>, nonce: u64) -> bool {
    let command = presence::set_activity(std::process::id(), activity, &nonce.to_string());
    wire.write_all(&presence::frame(presence::OP_FRAME, &command)).is_ok()
        && read_frame(wire).is_some()
}

fn run(client_id: String) {
    let retry = Duration::from_millis(
        std::env::var("MORPHEUS_DISCORD_POLL_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(15_000),
    );
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (lock, signal) = shared();
    let mut wire: Option<Box<dyn Wire>> = None;
    // What Discord shows from this connection: None until it is known.
    let mut sent: Option<Option<Activity>> = None;
    let mut nonce = 0u64;
    loop {
        let (wanted, quit) = {
            let Ok(mut guard) = lock.lock() else {
                return;
            };
            if !guard.quit {
                guard = match signal.wait_timeout(guard, retry) {
                    Ok((guard, _)) => guard,
                    Err(_) => return,
                };
            }
            (guard.wanted.clone(), guard.quit)
        };
        let target = wanted.on.then(|| Activity {
            button: wanted.button.clone(),
            state: wanted.state.clone(),
            started,
        });
        if quit {
            if let Some(wire) = wire.as_mut()
                && sent.as_ref().is_some_and(Option::is_some)
            {
                nonce += 1;
                send(&mut **wire, None, nonce);
            }
            if let Ok(mut guard) = lock.lock() {
                guard.done = true;
                signal.notify_all();
            }
            return;
        }
        if wire.is_none() {
            if target.is_none() {
                continue;
            }
            wire = connect(&client_id);
            sent = wire.is_some().then_some(None);
        }
        let Some(connection) = wire.as_mut() else {
            continue;
        };
        if sent.as_ref() != Some(&target) {
            nonce += 1;
            if send(&mut **connection, target.as_ref(), nonce) {
                sent = Some(target);
            } else {
                // Discord went away: connect again on the next round.
                wire = None;
                sent = None;
            }
        }
    }
}
