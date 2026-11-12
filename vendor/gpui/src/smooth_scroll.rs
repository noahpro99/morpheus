//! Eased wheel scrolling, for every scroll view of a window at once.
//!
//! A mouse wheel reports notches (`ScrollDelta::Lines`) and gpui's scroll
//! views move by the whole notch in one frame. The window absorbs those events
//! here instead and replays them over the next frames: each frame moves the
//! share `1 - exp(-dt / TAU)` of what is still to go (an ease-out), so a notch
//! takes about 150 ms and the sum of the steps is exactly the notch. A notch
//! that arrives meanwhile adds to what is still to go (the animation goes on
//! to the new target rather than restarting); one the other way round drops
//! what was left. The steps are replayed as `Lines` events too, so each
//! view applies its own line height (the total distance is what it always
//! was). Pixel deltas (touchpads, already smooth) never come here.

use std::time::Duration;

use crate::{Modifiers, Pixels, Point, ScrollDelta, ScrollWheelEvent, TouchPhase, point};

/// The ease-out's time constant: 95 % of a notch is done after three of
/// these (about 100 ms), all of it after about 150 ms.
const TAU_SECS: f32 = 0.032;
/// What is left under this many lines is delivered at once, ending the animation.
const REST: f32 = 0.02;

#[derive(Default)]
pub(crate) struct WheelEase {
    remaining: Point<f32>,
    position: Point<Pixels>,
    modifiers: Modifiers,
    last: Option<std::time::Instant>,
    running: bool,
}

impl WheelEase {
    /// Takes a notch in. True when the animation was not running, so the
    /// caller starts it.
    pub(crate) fn add(&mut self, event: &ScrollWheelEvent, lines: Point<f32>) -> bool {
        fn merge(left: f32, new: f32) -> f32 {
            if left * new < 0.0 { new } else { left + new }
        }
        self.remaining = point(
            merge(self.remaining.x, lines.x),
            merge(self.remaining.y, lines.y),
        );
        self.position = event.position;
        self.modifiers = event.modifiers;
        let start = !self.running;
        if start {
            self.running = true;
            self.last = Some(std::time::Instant::now());
        }
        start
    }

    /// The next step, in lines: `(event for it, whether the animation is over)`.
    pub(crate) fn tick(&mut self) -> (Option<ScrollWheelEvent>, bool) {
        let now = std::time::Instant::now();
        let dt = self
            .last
            .map_or(Duration::from_millis(16), |last| now - last)
            .clamp(Duration::from_millis(1), Duration::from_millis(50));
        self.last = Some(now);
        let (step, done) = step(self.remaining, dt.as_secs_f32());
        self.remaining = point(self.remaining.x - step.x, self.remaining.y - step.y);
        if done {
            self.running = false;
            self.remaining = Point::default();
        }
        let event = (step.x != 0.0 || step.y != 0.0).then(|| ScrollWheelEvent {
            position: self.position,
            delta: ScrollDelta::Lines(step),
            modifiers: self.modifiers,
            touch_phase: TouchPhase::Moved,
        });
        (event, done)
    }
}

/// One step of the ease-out over `dt` seconds: how much of `remaining` to
/// scroll now, and whether that finishes the animation (then the step is all
/// of what remained).
pub(crate) fn step(remaining: Point<f32>, dt: f32) -> (Point<f32>, bool) {
    if remaining.x.abs() < REST && remaining.y.abs() < REST {
        return (remaining, true);
    }
    let share = 1.0 - (-dt / TAU_SECS).exp();
    let part = |left: f32| if left.abs() < REST { left } else { left * share };
    let step = point(part(remaining.x), part(remaining.y));
    let left = point(remaining.x - step.x, remaining.y - step.y);
    if left.x.abs() < REST && left.y.abs() < REST {
        (remaining, true)
    } else {
        (step, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notch_arrives_in_full_over_several_frames() {
        let mut left = point(0.0, 3.0);
        let (mut sum, mut frames) = (0.0, 0);
        loop {
            let (step, done) = step(left, 0.016);
            sum += step.y;
            left = point(0.0, left.y - step.y);
            frames += 1;
            if done {
                break;
            }
        }
        assert!((sum - 3.0).abs() < 1e-4, "{sum}");
        assert!((6..=14).contains(&frames), "{frames} frames");
    }
}
