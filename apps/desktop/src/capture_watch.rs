//! Streamer mode also turns on while another program is capturing the screen
//! (a Discord stream, OBS), and off when that stops (#143).
//!
//! Linux: PipeWire's graph is read with `pw-dump` every few seconds, only
//! when the tool is there, and `view_model::capture::external_capture` says
//! whether a screen cast is linked to a program that is not this one.
//! Windows and macOS have no public way to ask whether another program is
//! capturing the screen, so nothing is watched there (a guess would misfire).
//!
//! Test runs never ask the real PipeWire: `MORPHEUS_FAKE_EXTERNAL_SHARE` names
//! a file, and while it exists another program is capturing.

use std::time::Duration;

/// How often PipeWire's graph is read.
const POLL: Duration = Duration::from_secs(3);
/// How often the test file is looked at.
const POLL_FAKE: Duration = Duration::from_millis(200);

/// Starts the watch (at startup).
pub fn start() {
    let fake = std::env::var_os("MORPHEUS_FAKE_EXTERNAL_SHARE").map(std::path::PathBuf::from);
    #[cfg(target_os = "linux")]
    let real = std::env::var_os("MORPHEUS_E2E_PORT").is_none() && pw_dump_found();
    #[cfg(not(target_os = "linux"))]
    let real = false;
    if fake.is_none() && !real {
        return;
    }
    std::thread::spawn(move || {
        loop {
            let on = match &fake {
                Some(file) => file.exists(),
                #[cfg(target_os = "linux")]
                None => pw_dump().is_some_and(|dump| {
                    crate::view_model::capture::external_capture(&dump, std::process::id())
                }),
                #[cfg(not(target_os = "linux"))]
                None => false,
            };
            if crate::streamer::set_external(on) {
                crate::send_command(crate::Command::Redraw);
            }
            std::thread::sleep(if fake.is_some() { POLL_FAKE } else { POLL });
        }
    });
}

#[cfg(target_os = "linux")]
fn pw_dump_found() -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join("pw-dump").is_file()))
}

/// PipeWire's graph as JSON, or `None` where it could not be read.
#[cfg(target_os = "linux")]
fn pw_dump() -> Option<String> {
    let out = std::process::Command::new("pw-dump")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|out| out.status.success())?;
    String::from_utf8(out.stdout).ok()
}
