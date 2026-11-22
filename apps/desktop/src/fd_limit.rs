//! Raises the soft limit on open files to the hard limit at startup (Zed and others do
//! the same). A terminal's soft limit is often 1024, which a signed-in app with several
//! accounts' stores, sockets and a call can reach, and some C code (`select`) aborts on
//! a descriptor above 1023 (#339).

/// The soft limit to ask for given the hard one: all of it, but macOS refuses more than
/// `OPEN_MAX` even when the hard limit says unlimited.
pub fn wanted(soft: u64, hard: u64, mac: bool) -> u64 {
    let ceiling = if mac { hard.min(10_240) } else { hard };
    ceiling.max(soft)
}

#[cfg(unix)]
#[allow(unsafe_code)]
pub fn raise() {
    #[repr(C)]
    struct Rlimit {
        cur: u64,
        max: u64,
    }
    unsafe extern "C" {
        fn getrlimit(resource: i32, rlim: *mut Rlimit) -> i32;
        fn setrlimit(resource: i32, rlim: *const Rlimit) -> i32;
    }
    #[cfg(target_os = "macos")]
    const RLIMIT_NOFILE: i32 = 8;
    #[cfg(not(target_os = "macos"))]
    const RLIMIT_NOFILE: i32 = 7;
    let mut limit = Rlimit { cur: 0, max: 0 };
    // SAFETY: plain libc calls on a struct with the C layout (two 64-bit words).
    unsafe {
        if getrlimit(RLIMIT_NOFILE, &mut limit) != 0 {
            return;
        }
        let cur = wanted(limit.cur, limit.max, cfg!(target_os = "macos"));
        if cur > limit.cur {
            let raised = Rlimit { cur, max: limit.max };
            if setrlimit(RLIMIT_NOFILE, &raised) != 0 {
                eprintln!("morpheus-desktop: could not raise the open files limit");
            }
        }
    }
}

#[cfg(not(unix))]
pub fn raise() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_soft_limit_goes_to_the_hard_one() {
        assert_eq!(wanted(1024, 524_288, false), 524_288);
        assert_eq!(wanted(1024, u64::MAX, true), 10_240);
        assert_eq!(wanted(2_000_000, 524_288, false), 2_000_000, "never lowered");
    }
}
