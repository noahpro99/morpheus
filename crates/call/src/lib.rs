//! Call media for Morpheus: a LiveKit room (the MatrixRTC transport; the
//! core gets the URL and token) with this device's microphone in it and
//! everyone else's audio coming out of the speakers.
//!
//! Real calls use the platform's audio devices through WebRTC's own device
//! module, which also cancels echo and suppresses noise. Tests use
//! [`AudioMode::Tone`]: a steady tone instead of the microphone, nothing
//! played, and [`CallEvent::Heard`]/[`CallEvent::Quiet`] reporting what
//! arrives from each participant.
//!
//! Like the core, the crate owns its Tokio runtime, so callers can await its
//! futures from any executor.
//!
//! A browser (apps/web) has no native WebRTC: there the same interface says
//! calls are not available yet (they will go through the browser's own).

#[cfg(not(target_arch = "wasm32"))]
#[path = "native.rs"]
mod imp;
#[cfg(target_arch = "wasm32")]
#[path = "web.rs"]
mod imp;

pub use imp::*;
