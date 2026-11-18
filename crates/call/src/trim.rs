//! Hand freed memory back to the system after a call (#190). WebRTC's C++
//! side (codecs, frame buffers, audio devices) allocates from glibc's malloc
//! or the Windows heap, which keep freed blocks, so the process stayed as
//! large as the call made it.
#![allow(unsafe_code)]

use std::time::Duration;

/// A few seconds from now, once the call's tasks have let go.
pub(crate) fn release_memory_later() {
    let _ = std::thread::Builder::new()
        .name("morpheus-trim".into())
        .spawn(|| {
            std::thread::sleep(Duration::from_secs(3));
            release_memory();
        });
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn release_memory() {
    unsafe extern "C" {
        fn malloc_trim(pad: usize) -> i32;
    }
    // SAFETY: glibc's own function; it takes no pointers.
    unsafe { malloc_trim(0) };
}

#[cfg(target_os = "windows")]
fn release_memory() {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetProcessHeap() -> *mut c_void;
        fn HeapCompact(heap: *mut c_void, flags: u32) -> usize;
        fn GetCurrentProcess() -> *mut c_void;
        fn K32EmptyWorkingSet(process: *mut c_void) -> i32;
    }
    // SAFETY: kernel32's functions on this process's own heap and handle (a
    // pseudo handle); nothing is passed in or out but those.
    unsafe {
        HeapCompact(GetProcessHeap(), 0);
        // What is left resident is trimmed: it faults back in when used.
        K32EmptyWorkingSet(GetCurrentProcess());
    }
}

#[cfg(not(any(all(target_os = "linux", target_env = "gnu"), target_os = "windows")))]
fn release_memory() {}
