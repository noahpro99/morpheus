//! Windows: what other apps play, by WASAPI process loopback (Windows 10
//! version 2004, build 19041, and later): the system's mix less this
//! process and its children, so the call's voices, chimes and videos played
//! here are not in it. See `system_audio.rs`.
//!
//! COM through raw calls: this module alone may use `unsafe`.
#![allow(unsafe_code)]

use std::{
    mem::ManuallyDrop,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
    },
    time::Duration,
};

use tracing::{info, warn};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
        Media::Audio::{
            AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
            AUDCLNT_STREAMFLAGS_LOOPBACK, AUDIOCLIENT_ACTIVATION_PARAMS,
            AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, ActivateAudioInterfaceAsync,
            IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
            IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient,
            PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVE_FORMAT_PCM, WAVEFORMATEX,
        },
        System::{
            Com::{
                BLOB, COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize,
                StructuredStorage::{
                    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
                },
            },
            Threading::{CreateEventW, WaitForSingleObject},
            Variant::VT_BLOB,
        },
    },
    core::{HRESULT, IUnknown, Interface as _, PCWSTR, Ref, implement},
};

use super::system_audio::{CHANNELS, FRAME, Out, RATE};

/// Samples in a 10 ms stereo frame.
const SAMPLES: usize = FRAME * CHANNELS;

/// Tells the waiting thread that activation finished (it may call back on
/// any thread).
#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Activated {
    done: Sender<()>,
}

impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
    fn ActivateCompleted(
        &self,
        _operation: Ref<IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        let _ = self.done.send(());
        Ok(())
    }
}

/// Capture until `stop`; `started` hears once whether it could.
pub(crate) fn run(stop: &AtomicBool, mut out: Out, started: Sender<Result<(), String>>) {
    // SAFETY: COM for this thread, undone below.
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if com.is_err() {
        let _ = started.send(Err(format!("COM: {com:?}")));
        return;
    }
    match open() {
        Ok((client, capture, event)) => {
            let _ = started.send(Ok(()));
            info!("screen audio: capturing the system's sound less this app (process loopback)");
            pump(stop, &client, &capture, event, &mut out);
            // SAFETY: the client and event are ours, used no more.
            unsafe {
                let _ = client.Stop();
                let _ = CloseHandle(event);
            }
        }
        Err(why) => {
            warn!(%why, "screen audio: no process loopback");
            let _ = started.send(Err(why));
        }
    }
    // SAFETY: matches the CoInitializeEx above.
    unsafe { CoUninitialize() };
}

/// A started loopback client of everything but this process tree, at 48 kHz
/// stereo 16-bit, signalling `event` when there is sound.
fn open() -> Result<(IAudioClient, IAudioCaptureClient, HANDLE), String> {
    let needs = "sharing the computer's sound needs Windows 10 version 2004 or later";
    let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: std::process::id(),
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
            },
        },
    };
    // The parameters as a blob in a PROPVARIANT (not owned by it: nothing
    // to clear).
    let prop = ManuallyDrop::new(PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_BLOB,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: PROPVARIANT_0_0_0 {
                    blob: BLOB {
                        cbSize: size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                        pBlobData: (&raw mut params).cast::<u8>(),
                    },
                },
            }),
        },
    });
    let (done, finished) = mpsc::channel();
    let handler: IActivateAudioInterfaceCompletionHandler = Activated { done }.into();
    // SAFETY: `params` and `prop` outlive the activation, which is waited
    // for below.
    let operation = unsafe {
        ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&raw const *prop),
            &handler,
        )
    }
    .map_err(|err| format!("{needs} ({err})"))?;
    finished
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| format!("{needs} (no answer)"))?;
    let mut result = HRESULT::default();
    let mut activated: Option<IUnknown> = None;
    // SAFETY: the operation has completed.
    unsafe { operation.GetActivateResult(&mut result, &mut activated) }
        .map_err(|err| format!("{needs} ({err})"))?;
    result.ok().map_err(|err| format!("{needs} ({err})"))?;
    let client: IAudioClient = activated
        .ok_or_else(|| needs.to_owned())?
        .cast()
        .map_err(|err| err.to_string())?;
    let block = (CHANNELS * 2) as u16;
    let format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM as u16,
        nChannels: CHANNELS as u16,
        nSamplesPerSec: RATE,
        nAvgBytesPerSec: RATE * u32::from(block),
        nBlockAlign: block,
        wBitsPerSample: 16,
        cbSize: 0,
    };
    // SAFETY: plain calls on the client just made; `format` outlives them.
    unsafe {
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK
                    | AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                    | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
                // 20 ms, in 100 ns units.
                200_000,
                0,
                &format,
                None,
            )
            .map_err(|err| format!("starting the loopback: {err}"))?;
        let capture: IAudioCaptureClient = client
            .GetService()
            .map_err(|err| format!("the loopback's capture: {err}"))?;
        let event =
            CreateEventW(None, false, false, PCWSTR::null()).map_err(|err| err.to_string())?;
        if let Err(err) = client.SetEventHandle(event).and_then(|()| client.Start()) {
            let _ = CloseHandle(event);
            return Err(format!("starting the loopback: {err}"));
        }
        Ok((client, capture, event))
    }
}

/// Packets as they come, out in 10 ms frames, until `stop`.
fn pump(
    stop: &AtomicBool,
    _client: &IAudioClient,
    capture: &IAudioCaptureClient,
    event: HANDLE,
    out: &mut Out,
) {
    let mut pending: Vec<i16> = Vec::with_capacity(8 * SAMPLES);
    while !stop.load(Ordering::Relaxed) {
        // SAFETY: our event; a timeout to look at `stop`.
        if unsafe { WaitForSingleObject(event, 100) } != WAIT_OBJECT_0 {
            continue;
        }
        loop {
            // SAFETY: the capture client's own calls, each buffer released
            // after it is copied.
            let taken = unsafe {
                match capture.GetNextPacketSize() {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(err) => {
                        warn!(%err, "screen audio: the loopback ended");
                        return;
                    }
                }
                let mut data = std::ptr::null_mut();
                let mut frames = 0u32;
                let mut flags = 0u32;
                if let Err(err) = capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None)
                {
                    warn!(%err, "screen audio: reading the loopback");
                    return;
                }
                let samples = frames as usize * CHANNELS;
                if data.is_null() || flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 {
                    pending.resize(pending.len() + samples, 0);
                } else {
                    pending
                        .extend_from_slice(std::slice::from_raw_parts(data.cast::<i16>(), samples));
                }
                capture.ReleaseBuffer(frames)
            };
            if let Err(err) = taken {
                warn!(%err, "screen audio: releasing the loopback's buffer");
                return;
            }
        }
        let whole = pending.len() / SAMPLES * SAMPLES;
        for frame in pending[..whole].chunks_exact(SAMPLES) {
            out(frame);
        }
        pending.drain(..whole);
        // Behind by more than 80 ms (a stall): the oldest goes.
        if pending.len() > 8 * SAMPLES {
            let excess = pending.len() - SAMPLES;
            pending.drain(..excess);
        }
    }
}
