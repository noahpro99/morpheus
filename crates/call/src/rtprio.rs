//! Linux and macOS: the microphone's capture thread as a real-time audio
//! thread (#332), as Windows has it through MMCSS (`mmcss.rs`). The thread
//! cleans each 10 ms frame (Strong's model takes 0.2 ms of it) and gives it
//! to the call, but at the ordinary priority a busy CPU leaves it unrun for
//! tens of milliseconds, the frames then go out late and bunched, and a
//! listener hears the voice chop (a stall of 120 ms every second made 5% of
//! it concealed; 40 ms was absorbed). Real-time priority needs the user's
//! limit (`RLIMIT_RTPRIO`, which an `audio` group usually gets); without it
//! the thread takes a better nice value, and without that it stays as it was.
//!
//! Raw calls into libc and libSystem: this module alone may use `unsafe`.
#![allow(unsafe_code)]

use tracing::info;

/// What the calling thread was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Raised {
    /// Round-robin real-time priority (Linux), or the interactive quality of
    /// service class (macOS).
    Realtime,
    /// A lower nice value only (Linux).
    Nice,
    /// Neither was allowed.
    No,
}

#[cfg(target_os = "linux")]
mod sys {
    #[repr(C)]
    pub struct SchedParam {
        pub priority: i32,
    }
    unsafe extern "C" {
        pub fn sched_setscheduler(pid: i32, policy: i32, param: *const SchedParam) -> i32;
        pub fn setpriority(which: i32, who: u32, priority: i32) -> i32;
    }
    pub const SCHED_RR: i32 = 2;
    pub const PRIO_PROCESS: i32 = 0;
}

/// The calling thread, at a priority a busy CPU does not starve.
pub fn raise() -> Raised {
    let got = raise_thread();
    info!(?got, "microphone capture thread priority");
    got
}

#[cfg(target_os = "linux")]
fn raise_thread() -> Raised {
    // A moderate real-time priority: over everything ordinary, under the
    // sound server's own.
    let param = sys::SchedParam { priority: 10 };
    // SAFETY: a pointer to a local struct for the call; pid 0 is this thread.
    if unsafe { sys::sched_setscheduler(0, sys::SCHED_RR, &param) } == 0 {
        return Raised::Realtime;
    }
    // SAFETY: plain integers; who 0 is this thread.
    if unsafe { sys::setpriority(sys::PRIO_PROCESS, 0, -11) } == 0 {
        return Raised::Nice;
    }
    Raised::No
}

#[cfg(target_os = "macos")]
fn raise_thread() -> Raised {
    unsafe extern "C" {
        fn pthread_set_qos_class_self_np(class: u32, relative: i32) -> i32;
    }
    /// QOS_CLASS_USER_INTERACTIVE.
    const USER_INTERACTIVE: u32 = 0x21;
    // SAFETY: plain integers.
    if unsafe { pthread_set_qos_class_self_np(USER_INTERACTIVE, 0) } == 0 {
        Raised::Realtime
    } else {
        Raised::No
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn raise_thread() -> Raised {
    Raised::No
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        thread,
        time::{Duration, Instant},
    };

    /// How late (ms) a thread asking for 10 ms of sleep, 300 times, is run:
    /// the 99th percentile and the worst.
    fn lateness(raise_it: bool) -> (f64, f64, Option<Raised>) {
        let mut late = Vec::new();
        let mut got = None;
        for _ in 0..300 {
            // (the priority is the thread's: set it once, on this thread)
            if raise_it && got.is_none() {
                got = Some(raise());
            }
            let start = Instant::now();
            thread::sleep(Duration::from_millis(10));
            late.push(start.elapsed().as_secs_f64() * 1000.0 - 10.0);
        }
        late.sort_by(f64::total_cmp);
        (late[late.len() * 99 / 100], late[late.len() - 1], got)
    }

    /// With every core oversubscribed, a plain thread is run late and the
    /// capture thread, raised, is not.
    #[test]
    fn a_raised_thread_is_run_on_time_on_a_busy_cpu() {
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let busy: Vec<_> = (0..thread::available_parallelism().map_or(8, |n| n.get()) * 2)
            .map(|_| {
                let stop = stop.clone();
                thread::spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        std::hint::spin_loop();
                    }
                })
            })
            .collect();
        thread::sleep(Duration::from_millis(200));
        let plain = thread::spawn(|| lateness(false)).join().expect("plain");
        let raised = thread::spawn(|| lateness(true)).join().expect("raised");
        stop.store(true, Ordering::Relaxed);
        busy.into_iter().for_each(|b| b.join().expect("busy"));
        eprintln!(
            "RTPRIO plain: p99 {:.2} ms late, worst {:.2}; raised ({:?}): p99 {:.2}, worst {:.2}",
            plain.0, plain.1, raised.2, raised.0, raised.1
        );
        if raised.2 == Some(Raised::No) {
            eprintln!("no real-time priority or nice allowed here; skipping the check");
            return;
        }
        assert!(
            raised.0 < 1.0 && raised.0 <= plain.0,
            "raised p99 {:.2} ms late (plain {:.2})",
            raised.0,
            plain.0
        );
    }
}
