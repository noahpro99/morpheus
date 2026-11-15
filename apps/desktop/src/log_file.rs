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
}
