//! Windows: what other apps play, by WASAPI process loopback. See
//! `system_audio.rs`.

use std::sync::{atomic::AtomicBool, mpsc::Sender};

use super::system_audio::Out;

/// Capture until `stop`; `started` hears once whether it could.
pub(crate) fn run(_stop: &AtomicBool, _out: Out, started: Sender<Result<(), String>>) {
    let _ = started.send(Err("sharing the computer's sound is not written for Windows yet".into()));
}
