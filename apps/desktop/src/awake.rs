//! What keeps the app running at full speed during a call.
//!
//! On macOS a window that is hidden (Mission Control, another full-screen
//! app) lets the system nap the process (App Nap: timers slowed, threads
//! throttled) and the window stops drawing, so a camera or a remote picture
//! freezes. During a call the process holds an activity that forbids the nap,
//! and the windows keep drawing even when the system calls them hidden.

/// What an "in a call" state turns on and off.
pub trait Backend {
    /// Forbid App Nap and idle sleep.
    fn begin(&mut self);
    /// Allow them again.
    fn end(&mut self);
    /// Keep drawing hidden windows, or stop.
    fn keep_drawing(&mut self, on: bool);
}

/// Follows "in a call": the activity is held from the first call to the last
/// and never begun twice.
pub struct Gate<B: Backend> {
    backend: B,
    held: bool,
}

impl<B: Backend> Gate<B> {
    pub fn new(backend: B) -> Self {
        Self { backend, held: false }
    }

    pub fn set(&mut self, in_call: bool) {
        if in_call == self.held {
            return;
        }
        self.held = in_call;
        if in_call {
            self.backend.begin();
        } else {
            self.backend.end();
        }
        self.backend.keep_drawing(in_call);
    }

    pub fn held(&self) -> bool {
        self.held
    }
}

/// The real thing: morpheus_call's App Nap activity (macOS), gpui's switch everywhere.
#[derive(Default)]
pub struct System {
    activity: Option<morpheus_call::Activity>,
}

impl Backend for System {
    fn begin(&mut self) {
        self.activity = morpheus_call::Activity::begin();
    }

    fn end(&mut self) {
        self.activity = None;
    }

    fn keep_drawing(&mut self, on: bool) {
        gpui::set_keep_rendering(on);
    }
}

thread_local! {
    static GATE: std::cell::RefCell<Gate<System>> = std::cell::RefCell::new(Gate::new(System::default()));
}

/// Tell the app whether a call is going (called on the main thread, every frame).
pub fn set_in_call(in_call: bool) {
    GATE.with(|g| g.borrow_mut().set(in_call));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake {
        log: Vec<&'static str>,
        drawing: bool,
    }
    impl Backend for Fake {
        fn begin(&mut self) {
            self.log.push("begin");
        }
        fn end(&mut self) {
            self.log.push("end");
        }
        fn keep_drawing(&mut self, on: bool) {
            self.drawing = on;
        }
    }

    #[test]
    fn the_activity_is_held_from_call_start_to_call_end() {
        let mut gate = Gate::new(Fake::default());
        gate.set(false);
        assert!(gate.backend.log.is_empty());
        gate.set(true);
        gate.set(true);
        assert_eq!(gate.backend.log, ["begin"]);
        assert!(gate.held());
        gate.set(false);
        gate.set(false);
        assert_eq!(gate.backend.log, ["begin", "end"]);
        assert!(!gate.held());
    }

    #[test]
    fn drawing_is_kept_going_exactly_while_in_a_call() {
        let mut gate = Gate::new(Fake::default());
        gate.set(true);
        assert!(gate.backend.drawing);
        gate.set(false);
        assert!(!gate.backend.drawing);
    }
}
