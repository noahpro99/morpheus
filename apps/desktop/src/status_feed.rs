//! Activities that set my status (#329): Steam's running game, found by a
//! gentle poll on a background thread, and other programs (a browser
//! extension for music, say) pushing over a local socket, see
//! docs/activity-ipc.md. What they say is kept here; `Shell` picks the one to
//! show and sets the status (`start_activity_status`).

use std::{
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    sync::{Mutex, Once, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::view_model::{
    activity::{self, Activity, Kind, Sources},
    steam,
};

fn sources() -> &'static Mutex<Sources> {
    static SOURCES: OnceLock<Mutex<Sources>> = OnceLock::new();
    SOURCES.get_or_init(Default::default)
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The status the activities now say to show (None: none, or all switched off).
pub fn shown_text() -> Option<String> {
    let dir = crate::data_dir();
    let now = now_ms();
    let mut sources = sources().lock().ok()?;
    sources.expire(now);
    sources
        .shown(now, |source| activity::source_enabled(&dir, source))
        .map(Activity::text)
}

fn env_ms(name: &str, default: u64) -> Duration {
    Duration::from_millis(
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default),
    )
}

/// Where other programs connect: `MORPHEUS_ACTIVITY_SOCK`, else a socket in
/// `$XDG_RUNTIME_DIR` (private to the user), else in the data folder; on
/// Windows a named pipe.
pub fn socket_path() -> PathBuf {
    if let Some(path) = std::env::var_os("MORPHEUS_ACTIVITY_SOCK") {
        return PathBuf::from(path);
    }
    if cfg!(windows) {
        return PathBuf::from(r"\\.\pipe\morpheus-activity");
    }
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(run) if !run.is_empty() => PathBuf::from(run).join("morpheus-activity.sock"),
        _ => crate::data_dir().join("activity.sock"),
    }
}

/// Starts watching Steam and listening on the socket, once per run.
pub fn start() {
    static START: Once = Once::new();
    START.call_once(|| {
        std::thread::Builder::new()
            .name("steam-status".into())
            .spawn(poll_steam)
            .ok();
        #[cfg(unix)]
        std::thread::Builder::new()
            .name("activity-socket".into())
            .spawn(listen)
            .ok();
    });
}

/// Removes the socket file (on quit).
pub fn shutdown() {
    #[cfg(unix)]
    if SOCKET_BOUND.load(std::sync::atomic::Ordering::SeqCst) {
        std::fs::remove_file(socket_path()).ok();
    }
}

// ---- Steam -------------------------------------------------------------------

fn steam_dirs() -> (steam::SteamDirs, Option<u32>) {
    if let Some(dir) = std::env::var_os("MORPHEUS_STEAM_DIR") {
        return (steam::SteamDirs::in_folder(&PathBuf::from(dir)), None);
    }
    #[cfg(target_os = "windows")]
    {
        let (id, path) = steam::windows_running();
        let dirs = steam::SteamDirs {
            registries: Vec::new(),
            roots: path.into_iter().collect(),
        };
        return (dirs, id);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        (steam::SteamDirs::standard(&home), None)
    }
}

fn poll_steam() {
    let every = env_ms("MORPHEUS_STEAM_POLL_MS", 12_000);
    loop {
        if activity::group_enabled(&crate::data_dir(), steam::SOURCE) {
            let (dirs, app_id) = steam_dirs();
            let game = steam::detect(&dirs, app_id);
            if let Ok(mut sources) = sources().lock() {
                match game {
                    Some(game) => {
                        let unchanged = sources.shown(now_ms(), |s| s == steam::SOURCE)
                            .is_some_and(|a| a.name == game.name);
                        if !unchanged {
                            sources.set(
                                Activity {
                                    source: steam::SOURCE.into(),
                                    kind: Kind::Playing,
                                    name: game.name,
                                    details: None,
                                    url: None,
                                },
                                None,
                                now_ms(),
                            );
                        }
                    }
                    None => sources.clear(steam::SOURCE),
                }
            }
        }
        std::thread::sleep(every);
    }
}

// ---- the socket ----------------------------------------------------------------

#[cfg(unix)]
static SOCKET_BOUND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(unix)]
fn listen() {
    use std::os::unix::{fs::PermissionsExt, net::{UnixListener, UnixStream}};
    let path = socket_path();
    if UnixStream::connect(&path).is_ok() {
        // Another Morpheus is serving it.
        return;
    }
    std::fs::remove_file(&path).ok();
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("morpheus-desktop: no activity socket at {}: {err}", path.display());
            return;
        }
    };
    // Only this user may connect.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).ok();
    SOCKET_BOUND.store(true, std::sync::atomic::Ordering::SeqCst);
    for stream in listener.incoming().flatten() {
        std::thread::Builder::new()
            .name("activity-client".into())
            .spawn(move || serve(stream))
            .ok();
    }
}

#[cfg(unix)]
fn serve(stream: std::os::unix::net::UnixStream) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        let read = reader
            .by_ref()
            .take(activity::MAX_LINE as u64 + 1)
            .read_line(&mut line);
        match read {
            Ok(0) | Err(_) => break,
            Ok(_) if line.len() > activity::MAX_LINE => {
                writeln!(writer, "{}", activity::reply_error("line too long")).ok();
                break;
            }
            Ok(_) => {}
        }
        if line.trim().is_empty() {
            continue;
        }
        let reply = match activity::parse_request(&line) {
            Ok(activity::Request::Hello { .. }) => activity::reply_hello(),
            Ok(request) => {
                match sources().lock() {
                    Ok(mut sources) => match sources.apply(request, now_ms()) {
                        Ok(()) => activity::reply_ok(),
                        Err(err) => activity::reply_error(&err),
                    },
                    Err(_) => activity::reply_error("busy"),
                }
            }
            Err(err) => activity::reply_error(&err),
        };
        if writeln!(writer, "{reply}").is_err() {
            break;
        }
    }
}

// ---- Chrome's native messaging --------------------------------------------------------

/// Whether Chrome started this process as a native-messaging host (it passes the
/// calling extension's origin), or `--native-messaging` was given.
pub fn native_messaging_requested() -> bool {
    std::env::args().skip(1).any(|a| a == "--native-messaging" || a.starts_with("chrome-extension://"))
}

/// Forwards each message Chrome sends (JSON on stdin, length-prefixed) as a line
/// to the activity socket, and each reply line back the same way on stdout.
pub fn run_native_host() {
    let mut stdin = std::io::stdin().lock();
    let mut stdout = std::io::stdout().lock();
    let mut connection = connect_client();
    while let Ok(Some(message)) = activity::native_read(&mut stdin) {
        let message: String = message.split_whitespace().collect::<Vec<_>>().join(" ");
        let reply = match connection.as_mut() {
            Some((writer, reader)) => {
                let mut line = String::new();
                if writeln!(writer, "{message}").is_ok() && reader.read_line(&mut line).is_ok() && !line.is_empty() {
                    line.trim().to_owned()
                } else {
                    connection = None;
                    activity::reply_error("Morpheus closed the connection")
                }
            }
            None => {
                connection = connect_client();
                activity::reply_error("Morpheus is not running")
            }
        };
        if stdout.write_all(&activity::native_encode(&reply)).is_err() || stdout.flush().is_err() {
            break;
        }
    }
}

type Client = (Box<dyn Write>, Box<dyn BufRead>);

#[cfg(unix)]
fn connect_client() -> Option<Client> {
    let stream = std::os::unix::net::UnixStream::connect(socket_path()).ok()?;
    let reader = stream.try_clone().ok()?;
    Some((Box::new(stream), Box::new(BufReader::new(reader))))
}

#[cfg(windows)]
fn connect_client() -> Option<Client> {
    let pipe = std::fs::OpenOptions::new().read(true).write(true).open(socket_path()).ok()?;
    let reader = pipe.try_clone().ok()?;
    Some((Box::new(pipe), Box::new(BufReader::new(reader))))
}

// ---- the shell's side -----------------------------------------------------------------

impl crate::shell::Shell {
    /// Every few seconds: sets the status from the activity that shows (and puts it back
    /// when it ends, unless it was typed over by hand), and tells Discord what to show.
    pub fn start_activity_status(&mut self, session: &morpheus_core::Session, cx: &mut gpui::Context<Self>) {
        use crate::view_model::{activity::Tracker, presence};
        start();
        let session = session.clone();
        let tick = env_ms("MORPHEUS_ACTIVITY_TICK_MS", 2_000);
        cx.spawn(async move |this, cx| {
            let mut tracker = Tracker::default();
            // The room whose public link is known, and that link.
            let mut linked: (Option<morpheus_core::OwnedRoomId>, Option<String>) = (None, None);
            loop {
                cx.background_executor().timer(tick).await;
                let wanted = shown_text();
                if tracker.changed(wanted.as_deref()) {
                    let current = match session.own_profile().await {
                        Ok(profile) => profile.status,
                        Err(_) => continue,
                    };
                    if let Some(activity::Change(text)) = tracker.step(wanted.as_deref(), current.as_deref()) {
                        if session.set_status(text).await.is_ok()
                            && this.update(cx, |this, cx| this.reload_own_profile(cx)).is_err()
                        {
                            return;
                        }
                    }
                }
                if !crate::rich_presence::available() {
                    continue;
                }
                let prefs = presence::load(&crate::data_dir());
                let Ok(selected) = this.update(cx, |this, _| this.selected.clone()) else {
                    return;
                };
                if prefs.link == presence::LinkChoice::Room && linked.0 != selected {
                    linked.1 = match &selected {
                        Some(room) => session.public_room_link(room.clone()).await.ok().flatten(),
                        None => None,
                    };
                    linked.0 = selected.clone();
                }
                let own = session.user_id().map(|u| u.to_string()).unwrap_or_default();
                let room = linked.1.clone().map(|target| presence::RoomLink { public: true, target });
                crate::rich_presence::want(crate::rich_presence::Wanted {
                    on: prefs.on,
                    button: presence::choose_link(prefs.link, &own, room.as_ref()),
                    state: wanted,
                });
            }
        })
        .detach();
    }
}
