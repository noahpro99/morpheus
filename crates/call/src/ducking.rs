//! Windows: keep other apps at their volume during a call.
//!
//! Windows lowers every other app ("When Windows detects communications
//! activity: reduce other sounds by 80%") while a process plays or records
//! in the communications role, as WebRTC's device module does, until that
//! stream closes. Like Discord, the call opts out: each of this process's
//! audio sessions, on every active speaker and microphone, gets
//! `IAudioSessionControl2::SetDuckingPreference(TRUE)`. That includes the
//! process's default session on each device, which the call's streams join
//! when they open later.
//!
//! COM through raw calls: this module alone may use `unsafe`.
#![allow(unsafe_code)]

use tracing::{info, warn};
use windows::{
    Win32::{
        Media::Audio::{
            DEVICE_STATE_ACTIVE, IAudioSessionControl, IAudioSessionControl2,
            IAudioSessionManager2, IMMDeviceEnumerator, MMDeviceEnumerator, eAll,
        },
        System::Com::{
            CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
        },
    },
    core::Interface as _,
};

/// Opt this process's audio sessions out of ducking, on a thread of its own
/// (COM is set up there). `why` goes in the log line.
pub(crate) fn opt_out(why: &'static str) {
    let _ = std::thread::Builder::new()
        .name("ducking-opt-out".into())
        .spawn(move || {
            // SAFETY: COM for this thread, undone below.
            if unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_err() {
                warn!(why, "ducking opt-out: no COM");
                return;
            }
            match opt_out_all() {
                Ok((sessions, devices)) => {
                    info!(why, devices, "ducking opt-out: {sessions} sessions")
                }
                Err(err) => warn!(why, %err, "ducking opt-out failed"),
            }
            // SAFETY: matches the CoInitializeEx above.
            unsafe { CoUninitialize() };
        });
}

/// Sessions opted out, and the active devices looked at.
fn opt_out_all() -> windows::core::Result<(u32, u32)> {
    let pid = std::process::id();
    let mut sessions = 0;
    // SAFETY: plain COM calls on interfaces this function owns.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let devices = enumerator.EnumAudioEndpoints(eAll, DEVICE_STATE_ACTIVE)?;
        let count = devices.GetCount()?;
        for i in 0..count {
            let Ok(device) = devices.Item(i) else {
                continue;
            };
            let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
                continue;
            };
            // The process's default session on this device (made now if
            // no stream opened it yet).
            if let Ok(own) = manager.GetAudioSessionControl(None, 0)
                && opt_out_one(&own, None)
            {
                sessions += 1;
            }
            // And any other session of ours already there.
            let Ok(list) = manager.GetSessionEnumerator() else {
                continue;
            };
            for s in 0..list.GetCount().unwrap_or(0) {
                if let Ok(session) = list.GetSession(s)
                    && opt_out_one(&session, Some(pid))
                {
                    sessions += 1;
                }
            }
        }
        Ok((sessions, count))
    }
}

/// Opt `session` out, if it is this process's (`pid`, when given).
unsafe fn opt_out_one(session: &IAudioSessionControl, pid: Option<u32>) -> bool {
    let Ok(session) = session.cast::<IAudioSessionControl2>() else {
        return false;
    };
    // SAFETY: a live session interface.
    unsafe {
        if let Some(pid) = pid
            && session.GetProcessId().ok() != Some(pid)
        {
            return false;
        }
        session.SetDuckingPreference(true).is_ok()
    }
}
