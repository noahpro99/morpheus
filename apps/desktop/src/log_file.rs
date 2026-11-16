//! The log in the data dir (`morpheus.log`), read after something went
//! wrong; Settings' "Report a bug" sends its end. Each start begins a fresh
//! one and keeps the last runs' as `morpheus.log.1` (the one before) to
//! `morpheus.log.3`: a crash's log used to be lost as soon as the app was
//! started again (issue #76). A panic writes its message and backtrace
//! into the log too, not only to the terminal.

use std::{
    fs, io,
    io::Write as _,
    path::{Path, PathBuf},
};

/// The log's file name in the data dir.
pub const NAME: &str = "morpheus.log";

/// How many earlier runs' logs are kept.
const KEPT: usize = 3;

/// The log of `runs_ago` runs before this one (0: this run's).
pub fn path(dir: &Path, runs_ago: usize) -> PathBuf {
    match runs_ago {
        0 => dir.join(NAME),
        n => dir.join(format!("{NAME}.{n}")),
    }
}

/// Moves the earlier runs' logs along (the oldest goes) and opens a fresh
/// log for this run, appended to (so a panic written beside it is not
/// written over).
pub fn start(dir: &Path) -> io::Result<fs::File> {
    fs::create_dir_all(dir)?;
    rotate(dir);
    let log = path(dir, 0);
    fs::File::create(&log)?;
    fs::OpenOptions::new().append(true).open(log)
}

fn rotate(dir: &Path) {
    for n in (1..=KEPT).rev() {
        let older = path(dir, n);
        let newer = path(dir, n - 1);
        if newer.exists() {
            let _ = fs::rename(&newer, &older);
        }
    }
}

/// From now on a panic (on any thread) is written to the log at `log`, with
/// its backtrace, before the usual report on the terminal.
pub fn log_panics(log: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = write_panic(&log, &panic_entry(info));
        previous(info);
    }));
}

fn panic_entry(info: &std::panic::PanicHookInfo<'_>) -> String {
    let thread = std::thread::current();
    let name = thread.name().unwrap_or("<unnamed>");
    let backtrace = std::backtrace::Backtrace::force_capture();
    format!("PANIC: thread '{name}' {info}\nbacktrace:\n{backtrace}\n")
}

fn write_panic(log: &Path, entry: &str) -> io::Result<()> {
    let mut file = fs::OpenOptions::new().append(true).open(log)?;
    file.write_all(entry.as_bytes())?;
    file.flush()
}

/// matrix-sdk logs every failed request at error level, the ones that are
/// answers too: account data never set, a picture gone from its server, a
/// server that makes no thumbnails. In a big room those were most of the
/// log (#112). This leaves them out; the callers handle them (and log what
/// they did at debug), and other failures still show.
pub struct QuietExpected;

impl<S> tracing_subscriber::layer::Filter<S> for QuietExpected {
    fn enabled(
        &self,
        _: &tracing::Metadata<'_>,
        _: &tracing_subscriber::layer::Context<'_, S>,
    ) -> bool {
        true
    }

    fn event_enabled(
        &self,
        event: &tracing::Event<'_>,
        _: &tracing_subscriber::layer::Context<'_, S>,
    ) -> bool {
        let meta = event.metadata();
        if *meta.level() != tracing::Level::ERROR || !meta.target().starts_with("matrix_sdk::") {
            return true;
        }
        struct Message(String);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = format!("{value:?}");
                }
            }
        }
        let mut message = Message(String::new());
        event.record(&mut message);
        !expected_error(meta.target(), &message.0)
    }
}

/// matrix-sdk's error-level logs that are no error: a request's answer (see
/// [`expected_failure`]), and latest_events' "Room is unknown" (#115). That
/// one comes for every room a sync brings that the client has not stored
/// yet (all of them on the first sync after logging in, a room just joined
/// later): matrix-sdk 0.19 registers a sync's rooms for their latest events
/// (`subscribe_to_room_latest_events`) before it processes the sync into
/// its store, and the first computation runs in that gap. It computes them
/// again once the events are in (the event cache's update), so nothing is
/// lost. (It also comes after logging out: the client is gone.)
fn expected_error(target: &str, message: &str) -> bool {
    match target {
        "matrix_sdk::http_client" => expected_failure(message),
        "matrix_sdk::latest_events::room_latest_events" => message == "Room is unknown",
        _ => false,
    }
}

/// A request's failure that is an answer: not found, or no thumbnail.
fn expected_failure(message: &str) -> bool {
    message.contains("status_code: 404")
        || (message.contains("status_code: 400") && message.contains("thumbnail"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("morpheus-log-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn read(dir: &Path, runs_ago: usize) -> Option<String> {
        fs::read_to_string(path(dir, runs_ago)).ok()
    }

    /// Each start keeps the runs before it, newest first, at most three.
    #[test]
    fn each_start_keeps_the_last_three_runs_logs() {
        let dir = dir("rotate");
        for run in 1..=5 {
            let mut log = start(&dir).expect("log");
            writeln!(log, "run {run}").expect("write");
        }
        assert_eq!(read(&dir, 0).as_deref(), Some("run 5\n"));
        assert_eq!(read(&dir, 1).as_deref(), Some("run 4\n"));
        assert_eq!(read(&dir, 2).as_deref(), Some("run 3\n"));
        assert_eq!(read(&dir, 3).as_deref(), Some("run 2\n"));
        assert_eq!(read(&dir, 4), None, "no more than three kept");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The first start has nothing to keep, and its log starts empty.
    #[test]
    fn the_first_start_begins_an_empty_log() {
        let dir = dir("first");
        drop(start(&dir).expect("log"));
        assert_eq!(read(&dir, 0).as_deref(), Some(""));
        assert_eq!(read(&dir, 1), None);
        let _ = fs::remove_dir_all(&dir);
    }

    /// A panic written beside the log's own writes is not written over.
    #[test]
    fn a_panic_lands_after_what_was_logged() {
        let dir = dir("panic");
        let mut log = start(&dir).expect("log");
        writeln!(log, "before").expect("write");
        write_panic(&path(&dir, 0), "PANIC: boom\n").expect("panic entry");
        writeln!(log, "after").expect("write");
        assert_eq!(
            read(&dir, 0).as_deref(),
            Some("before\nPANIC: boom\nafter\n")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// latest_events' "Room is unknown" before a new room is stored is left
    /// out; its other errors, and other crates' errors, stay (#115).
    #[test]
    fn latest_events_unknown_room_is_quiet() {
        assert!(expected_error(
            "matrix_sdk::latest_events::room_latest_events",
            "Room is unknown"
        ));
        assert!(!expected_error(
            "matrix_sdk::latest_events::room_latest_events",
            "Failed to fetch the `RoomEventCache`"
        ));
        assert!(!expected_error("matrix_sdk::sliding_sync", "Room is unknown"));
        assert!(expected_error(
            "matrix_sdk::http_client",
            r#"Error while sending request: Api(Server(MatrixError(Error { status_code: 404, body: Json("") })))"#
        ));
    }

    /// matrix-sdk's not-found and no-thumbnail answers are left out of the
    /// log; its other failures stay (#112).
    #[test]
    fn expected_request_failures_are_quiet() {
        assert!(expected_failure(
            r#"Error while sending request: Api(Server(MatrixError(Error { status_code: 404, body: Standard(StandardErrorBody { kind: NotFound, message: "Account data not found" }) })))"#
        ));
        assert!(expected_failure(
            r#"Error while sending request: Api(Server(MatrixError(Error { status_code: 400, body: Standard(StandardErrorBody { kind: Unknown, message: "Dynamic thumbnails are disabled on this server" }) })))"#
        ));
        assert!(!expected_failure(
            r#"Error while sending request: Api(Server(MatrixError(Error { status_code: 400, body: Standard(StandardErrorBody { kind: BadJson, message: "bad" }) })))"#
        ));
        assert!(!expected_failure(
            r#"Error while sending request: Api(Server(MatrixError(Error { status_code: 502, body: Json("") })))"#
        ));
        assert!(!expected_failure("Error while sending request: Reqwest(timed out)"));
    }
}
