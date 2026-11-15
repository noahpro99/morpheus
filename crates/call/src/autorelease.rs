//! Autorelease pools for the threads of our own that call into libwebrtc's
//! Objective-C code on Apple platforms.
//!
//! An Objective-C object returned "autoreleased" is freed when the innermost
//! autorelease pool of its thread drains. The main thread (AppKit's run loop)
//! and GCD queues drain pools of their own; a thread started with
//! `std::thread::spawn`, or a Tokio worker, has none, so whatever is
//! autoreleased there is never freed. libwebrtc's macOS screen capturers
//! (ScreenCaptureKit, CGDisplayStream, the cursor monitor) and its
//! CVPixelBuffer frame buffers are Objective-C, and Chromium, which they are
//! written for, runs them on threads with a pool. Each call from such a thread
//! of ours goes in [`pool`] (issue #20: memory grew to 34 GB while sharing).
//! Elsewhere [`pool`] just calls `f`.

// The two Objective-C runtime calls below.
#![allow(unsafe_code)]

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[link(name = "objc", kind = "dylib")]
unsafe extern "C" {
    fn objc_autoreleasePoolPush() -> *mut std::ffi::c_void;
    fn objc_autoreleasePoolPop(pool: *mut std::ffi::c_void);
}

/// Runs `f` inside an autorelease pool of its own, drained when it returns.
#[inline]
pub(crate) fn pool<R>(f: impl FnOnce() -> R) -> R {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        struct Pop(*mut std::ffi::c_void);
        impl Drop for Pop {
            fn drop(&mut self) {
                // SAFETY: the token is the one objc_autoreleasePoolPush gave
                // on this thread, popped once, in push order (this frame).
                unsafe { objc_autoreleasePoolPop(self.0) }
            }
        }
        // SAFETY: plain runtime call; the pool is popped by `Pop` even if
        // `f` unwinds.
        let _pop = Pop(unsafe { objc_autoreleasePoolPush() });
        f()
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    {
        f()
    }
}
