//! How long switching rooms takes (#312): from the moment a room is asked for
//! to the first frame that lays out real message rows (not skeletons). The
//! last switch is shown on the timeline (`data-switch-*`, read by the e2e
//! benchmark) and logged.

use std::sync::Mutex;
use std::time::{Duration, Instant};

struct Switch {
    room: String,
    started: Instant,
    cached: bool,
    done: Option<Duration>,
    /// The first frame drawn after the click: the wait for it is the platform's
    /// (vsync, or the headless 60 Hz timer), not the app's work.
    first_frame: Option<Duration>,
}

static LAST: Mutex<Option<Switch>> = Mutex::new(None);

/// A room is asked for; `cached` when a kept timeline is shown again.
pub fn start(room: &str, cached: bool) {
    if let Ok(mut last) = LAST.lock() {
        *last = Some(Switch {
            room: room.to_owned(),
            started: Instant::now(),
            cached,
            done: None,
            first_frame: None,
        });
    }
}

/// A frame of the window begins.
pub fn frame() {
    if let Ok(mut last) = LAST.lock()
        && let Some(switch) = last.as_mut()
        && switch.first_frame.is_none()
    {
        switch.first_frame = Some(switch.started.elapsed());
    }
}

/// A real message row of `room`'s own timeline was laid out; the first one
/// after [`start`] ends the switch.
pub fn content(room: &str) {
    let Ok(mut last) = LAST.lock() else { return };
    if let Some(switch) = last.as_mut()
        && switch.done.is_none()
        && switch.room == room
    {
        let took = switch.started.elapsed();
        switch.done = Some(took);
        tracing::info!(
            "switch to {room} ({}): {:.1} ms to the first rows",
            if switch.cached { "cached" } else { "fresh" },
            took.as_secs_f64() * 1000.0
        );
    }
}

/// `(room, milliseconds, cached, work)` of the last finished switch: `work` is
/// the milliseconds without the wait for the first frame.
pub fn last() -> Option<(String, f64, bool, f64)> {
    let last = LAST.lock().ok()?;
    let switch = last.as_ref()?;
    Some((
        switch.room.clone(),
        switch.done?.as_secs_f64() * 1000.0,
        switch.cached,
        (switch.done? - switch.first_frame.unwrap_or_default()).as_secs_f64() * 1000.0,
    ))
}
