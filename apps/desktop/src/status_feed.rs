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

/// What each account's activity status knows of its server refusing to keep a status (#384).
fn notices() -> &'static Mutex<std::collections::HashMap<String, activity::KeepNotice>> {
    static NOTICES: OnceLock<Mutex<std::collections::HashMap<String, activity::KeepNotice>>> = OnceLock::new();
    NOTICES.get_or_init(Default::default)
}

/// Whether `user`'s server refused the last status an activity set (the line in
/// Settings > Activity status shows).
pub fn status_not_kept(user: &str) -> bool {
    notices().lock().ok().and_then(|n| n.get(user).map(activity::KeepNotice::showing)).unwrap_or(false)
}

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
        #[cfg(windows)]
        std::thread::Builder::new()
            .name("activity-pipe".into())
            .spawn(listen_windows)
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
        let mut dirs = steam::SteamDirs::in_folder(&PathBuf::from(dir));
        // Test hook: a fake /proc for the launcher scan (the real one is never scanned here).
        dirs.proc_dir = std::env::var_os("MORPHEUS_STEAM_PROC").map(PathBuf::from);
        return (dirs, None);
    }
    #[cfg(target_os = "windows")]
    {
        let (id, path) = steam::windows_running();
        let dirs = steam::SteamDirs {
            registries: Vec::new(),
            roots: path.into_iter().collect(),
            proc_dir: None,
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
    let mut logged: Option<(u32, String)> = None;
    loop {
        if activity::group_enabled(&crate::data_dir(), steam::SOURCE) {
            let (dirs, app_id) = steam_dirs();
            let game = steam::detect(&dirs, app_id);
            let now = game.as_ref().map(|g| (g.id, g.name.clone()));
            if now != logged {
                match &now {
                    Some((id, name)) => tracing::info!(app_id = id, name = name.as_str(), "activity status: Steam game detected"),
                    None => tracing::info!("activity status: Steam game gone"),
                }
                logged = now;
            }
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
            .spawn(move || serve_unix(stream))
            .ok();
    }
}

#[cfg(unix)]
fn serve_unix(stream: std::os::unix::net::UnixStream) {
    let Ok(writer) = stream.try_clone() else {
        return;
    };
    serve(stream, writer);
}

/// Answers one client: a line in, a line out.
fn serve(input: impl Read, mut writer: impl Write) {
    let mut reader = BufReader::new(input);
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
            let own = session.user_id().map(|u| u.to_string()).unwrap_or_default();
            // A status left up by a run that quit or crashed while an activity showed.
            if let Some(marker) = activity::Marker::load(&crate::data_dir())
                && marker.user == own
            {
                if let Some(Ok(status)) = within(cx, STATUS_CALL_LIMIT, session.own_status()).await
                    && let Some(activity::Change(text)) = marker.recover(&own, status.as_deref())
                {
                    if let Some(Err(err)) = within(cx, STATUS_CALL_LIMIT, session.set_status(text)).await {
                        tracing::warn!(error = %err, "activity status: putting the old status back failed");
                    }
                    this.update(cx, |this, cx| this.reload_own_profile(cx)).ok();
                }
                activity::Marker::store(&crate::data_dir(), None);
            }
            // The room whose public link is known, and that link.
            let mut linked: (Option<morpheus_core::OwnedRoomId>, Option<String>) = (None, None);
            let mut announced: Option<String> = None;
            let mut last_failure: Option<String> = None;
            tracing::info!("activity status: watching for activities");
            loop {
                cx.background_executor().timer(tick).await;
                // The shell is gone (another account opened): so is this, with its session.
                if this.update(cx, |_, _| ()).is_err() {
                    return;
                }
                let wanted = shown_text();
                if tracker.changed(wanted.as_deref()) {
                    // Each change of wish is logged once; a read that keeps failing, once per kind of failure.
                    if announced.as_deref() != Some(wanted.as_deref().unwrap_or("")) {
                        tracing::info!(wanted = wanted.as_deref().unwrap_or("(nothing)"), "activity status: wanted status changed");
                        announced = Some(wanted.clone().unwrap_or_default());
                    }
                    let current = match within(cx, STATUS_CALL_LIMIT, session.own_status()).await {
                        Some(Ok(status)) => Some(status),
                        Some(Err(err)) => {
                            warn_once(&mut last_failure, format!("reading the status failed: {err}"));
                            None
                        }
                        None => {
                            warn_once(&mut last_failure, "reading the status timed out".into());
                            None
                        }
                    };
                    let change = tracker.step_read(wanted.as_deref(), current.as_ref().map(|c| c.as_deref()));
                    if current.is_some() {
                        last_failure = None;
                        activity::Marker::store(&crate::data_dir(), tracker.marker(&own).as_ref());
                    }
                    if let Some(activity::Change(text)) = change {
                        match within(cx, STATUS_CALL_LIMIT, session.set_status(text.clone())).await {
                            Some(Ok(())) => {
                                tracing::info!(set = text.is_some(), "activity status: status set");
                                if text.is_some() {
                                    notices().lock().ok().map(|mut n| n.entry(own.clone()).or_default().kept());
                                    refresh_settings(&this, cx);
                                }
                                if this.update(cx, |this, cx| this.reload_own_profile(cx)).is_err() {
                                    return;
                                }
                            }
                            Some(Err(err)) => {
                                let refused = format!("{err}");
                                if matches!(err, morpheus_core::Error::StatusUnavailable) {
                                    let first = notices().lock().ok().is_some_and(|mut n| n.entry(own.clone()).or_default().refused());
                                    let shown = this.update(cx, |this, cx| {
                                        if first {
                                            this.toast(activity::NOT_KEPT_NOTICE, cx);
                                        }
                                    });
                                    if shown.is_err() {
                                        return;
                                    }
                                    refresh_settings(&this, cx);
                                }
                                tracing::warn!(error = %refused, "activity status: the server did not take the status (presence refused or failing)");
                            }
                            None => tracing::warn!("activity status: setting the status timed out"),
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

impl crate::shell::Shell {
    /// On quit: puts the status back if an activity set it (at most a few seconds).
    pub fn restore_status_before_quit(&mut self) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let own = session.user_id().map(|u| u.to_string()).unwrap_or_default();
        let dir = crate::data_dir();
        let Some(marker) = activity::Marker::load(&dir).filter(|m| m.user == own) else {
            return;
        };
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(4));
            std::process::exit(0);
        });
        futures::executor::block_on(async {
            if let Ok(profile) = session.own_profile().await
                && let Some(activity::Change(text)) = marker.recover(&own, profile.status.as_deref())
            {
                session.set_status(text).await.ok();
            }
        });
        activity::Marker::store(&dir, None);
    }
}

// ---- Windows: the named pipe -----------------------------------------------------------

/// Serves `\\.\pipe\morpheus-activity` (or `MORPHEUS_ACTIVITY_SOCK`): one pipe instance
/// per client, local clients only (the pipe's default access is read-only for others).
#[cfg(windows)]
#[allow(unsafe_code)]
fn listen_windows() {
    use std::os::windows::io::{FromRawHandle as _, RawHandle};

    use windows::{
        Win32::{
            Foundation::{ERROR_PIPE_CONNECTED, INVALID_HANDLE_VALUE},
            Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX},
            System::Pipes::{
                ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
            },
        },
        core::PCWSTR,
    };

    let name: Vec<u16> = socket_path().as_os_str().to_string_lossy().encode_utf16().chain([0]).collect();
    let mut first = true;
    loop {
        let open_mode = if first { PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE } else { PIPE_ACCESS_DUPLEX };
        // SAFETY: `name` is a NUL-terminated wide string that outlives the call.
        let handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(name.as_ptr()),
                open_mode,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                4096,
                4096,
                0,
                None,
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_invalid() {
            if first {
                // Another Morpheus serves it already, or the pipe cannot be made.
                return;
            }
            std::thread::sleep(Duration::from_secs(1));
            continue;
        }
        first = false;
        // SAFETY: the handle is a pipe instance made above, owned by the File from here on.
        let pipe = unsafe { std::fs::File::from_raw_handle(handle.0 as RawHandle) };
        // SAFETY: a valid pipe handle, no overlapped I/O.
        let connected = unsafe { ConnectNamedPipe(handle, None) };
        let ok = match connected {
            Ok(()) => true,
            Err(err) => err.code() == ERROR_PIPE_CONNECTED.to_hresult(),
        };
        if !ok {
            continue;
        }
        let Ok(writer) = pipe.try_clone() else {
            continue;
        };
        std::thread::Builder::new()
            .name("activity-client".into())
            .spawn(move || serve(pipe, writer))
            .ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_is_answered_line_by_line() {
        let input = "{\"op\":\"hello\",\"version\":1}\nnonsense\n\n{\"op\":\"clear\",\"source\":\"test-serve\"}\n";
        let mut out = Vec::new();
        serve(std::io::Cursor::new(input.as_bytes().to_vec()), &mut out);
        let replies: Vec<serde_json::Value> = String::from_utf8(out)
            .expect("utf8")
            .lines()
            .map(|l| serde_json::from_str(l).expect("json"))
            .collect();
        assert_eq!(replies.len(), 3);
        assert_eq!(replies[0]["op"], "hello");
        assert_eq!(replies[1]["ok"], false);
        assert_eq!(replies[2]["ok"], true);
    }
}

/// How long one read or set of the status may take before it is given up on.
const STATUS_CALL_LIMIT: Duration = Duration::from_secs(30);

/// `fut`'s result, or None when it takes longer than `limit`.
async fn within<T>(
    cx: &mut gpui::AsyncApp,
    limit: Duration,
    fut: impl std::future::Future<Output = T>,
) -> Option<T> {
    use futures::future::{Either, select};
    let timer = cx.background_executor().timer(limit);
    match select(Box::pin(fut), Box::pin(timer)).await {
        Either::Left((value, _)) => Some(value),
        Either::Right(_) => None,
    }
}

/// Redraws Settings, if open, for the notice that came or went.
fn refresh_settings(this: &gpui::WeakEntity<crate::shell::Shell>, cx: &mut gpui::AsyncApp) {
    this.update(cx, |this, cx| {
        if let Some(settings) = &this.settings {
            settings.update(cx, |_, cx| cx.notify());
        }
    })
    .ok();
}

/// Logs `message` as a warning unless it is the one logged last.
fn warn_once(last: &mut Option<String>, message: String) {
    if last.as_deref() != Some(message.as_str()) {
        tracing::warn!("activity status: {message}");
        *last = Some(message);
    }
}
