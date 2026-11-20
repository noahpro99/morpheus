//! Keeps the process out of App Nap while it is held (macOS): a call whose
//! window is hidden (Mission Control, another full-screen app) would otherwise
//! have its timers and threads slowed and its picture frozen. Nothing
//! elsewhere.
#![allow(unsafe_code)]

/// Held while a call is going; dropping it lets the system nap the app again.
pub struct Activity {
    #[cfg(target_os = "macos")]
    token: objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2_foundation::NSObjectProtocol>>,
}

impl Activity {
    /// Begin the activity (user-initiated, latency-critical: no App Nap, no
    /// timer coalescing, no idle sleep). `None` where there is no such thing.
    pub fn begin() -> Option<Self> {
        #[cfg(target_os = "macos")]
        {
            use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
            let options = NSActivityOptions::UserInitiated | NSActivityOptions::LatencyCritical;
            let reason = NSString::from_str("Morpheus call in progress");
            let token = NSProcessInfo::processInfo().beginActivityWithOptions_reason(options, &reason);
            Some(Self { token })
        }
        #[cfg(not(target_os = "macos"))]
        None
    }
}

#[cfg(target_os = "macos")]
impl Drop for Activity {
    fn drop(&mut self) {
        // SAFETY: the object `beginActivity` returned, ended once.
        unsafe { objc2_foundation::NSProcessInfo::processInfo().endActivity(&self.token) };
    }
}
