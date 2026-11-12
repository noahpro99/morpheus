//! A record of how long frames take to draw, for the e2e probe.
//!
//! Every frame's time to build and paint the scene (`Window::draw`) and to
//! hand it to the GPU (`present`) is pushed here, with the draw's two big
//! parts (prepaint: render, layout; and paint); the app reads and clears it.

use std::{cell::Cell, sync::Mutex};

/// One frame: all in ms.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameTime {
    /// Build and paint the scene.
    pub draw: f32,
    /// Hand it to the GPU.
    pub present: f32,
    /// Render, layout and prepaint, part of `draw`.
    pub prepaint: f32,
    /// Paint, part of `draw`.
    pub paint: f32,
}

static FRAMES: Mutex<Vec<FrameTime>> = Mutex::new(Vec::new());
const KEEP: usize = 20_000;

thread_local! {
    static PHASES: Cell<(f32, f32)> = const { Cell::new((0.0, 0.0)) };
}

/// Set by `draw_roots`: the prepaint and paint times of the frame being drawn.
pub(crate) fn phases(prepaint: f32, paint: f32) {
    PHASES.with(|p| p.set((prepaint, paint)));
}

pub(crate) fn record(draw: f32, present: f32) {
    let (prepaint, paint) = PHASES.with(|p| p.get());
    if let Ok(mut frames) = FRAMES.lock()
        && frames.len() < KEEP
    {
        frames.push(FrameTime { draw, present, prepaint, paint });
    }
}

/// The frames drawn since the last call.
pub fn take_frame_times() -> Vec<FrameTime> {
    FRAMES.lock().map(|mut f| std::mem::take(&mut *f)).unwrap_or_default()
}
