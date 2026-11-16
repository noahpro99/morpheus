//! Windows: the microphone's capture thread as a "Pro Audio" task of the
//! Multimedia Class Scheduler (MMCSS), as audio apps run theirs, so a busy
//! CPU (the window drawing, video encoding) does not leave it unrun long
//! enough for the microphone to fall behind (#101). cpal's own WASAPI
//! thread is already time-critical; this is the thread that cleans and
//! sends the frames.
//!
//! Raw calls into avrt.dll: this module alone may use `unsafe`.
#![allow(unsafe_code)]

use tracing::{info, warn};
use windows::{
    Win32::{
        Foundation::HANDLE,
        System::Threading::{AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW},
    },
    core::w,
};

/// The calling thread registered with MMCSS until this is dropped (on the
/// same thread).
pub(crate) struct ProAudio(HANDLE);

impl ProAudio {
    /// Register the calling thread; `None` (logged) if MMCSS refuses (its
    /// service off).
    pub(crate) fn join() -> Option<Self> {
        let mut task = 0u32;
        // SAFETY: a constant task name and a local index for the call.
        match unsafe { AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut task) } {
            Ok(handle) => {
                info!(task, "microphone thread: MMCSS Pro Audio");
                Some(Self(handle))
            }
            Err(err) => {
                warn!(%err, "microphone thread: no MMCSS priority");
                None
            }
        }
    }
}

impl Drop for ProAudio {
    fn drop(&mut self) {
        // SAFETY: the handle AvSetMmThreadCharacteristicsW gave this thread.
        let _ = unsafe { AvRevertMmThreadCharacteristics(self.0) };
    }
}
