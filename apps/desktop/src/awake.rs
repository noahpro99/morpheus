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

/// The real thing: an NSProcessInfo activity on macOS, gpui's switch everywhere.
#[derive(Default)]
pub struct System {
    #[cfg(target_os = "macos")]
    activity: Option<objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2_foundation::NSObjectProtocol>>>,
}

impl Backend for System {
    fn begin(&mut self) {
        #[cfg(target_os = "macos")]
        {
            use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
            // User-initiated (no App Nap, no idle sleep) with low latency, so
            // timers and capture threads are not coalesced while hidden.
            let options = NSActivityOptions::UserInitiated | NSActivityOptions::LatencyCritical;
            let reason = NSString::from_str("Morpheus call in progress");
            self.activity = Some(NSProcessInfo::processInfo().beginActivityWithOptions_reason(options, &reason));
        }
    }

    fn end(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(activity) = self.activity.take() {
            // SAFETY: the object `beginActivity` returned, ended once.
            unsafe { objc2_foundation::NSProcessInfo::processInfo().endActivity(&activity) };
        }
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
