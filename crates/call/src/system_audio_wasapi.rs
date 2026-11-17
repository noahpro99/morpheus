//! Windows: what other apps play, by WASAPI process loopback (Windows 10
//! version 2004, build 19041, and later). See `system_audio.rs`.
//!
//! Morpheus is always left out: one loopback of the system's mix less this
//! process and its children, so the call's voices, chimes and videos played
//! here are not in it. A loopback can leave out only one process tree, so
//! while Discord is to be left out too (Settings, on by default) and is
//! playing, the capture switches to a loopback of each other app playing
//! (each process tree with an active audio session, include mode), mixed
//! here; system sounds (which belong to no app) are then not shared. The
//! apps playing are looked at again every second, so apps starting and
//! stopping, Discord coming and going and the setting changing are
//! followed. Where an app's own loopback cannot be had, it is left out and
//! the rest goes on; where looking at the apps fails, everything but
//! Morpheus is shared (Discord too) and the log says why.
//!
//! COM through raw calls: this module alone may use `unsafe`.
#![allow(unsafe_code)]

use std::{
    collections::{HashMap, HashSet},
    mem::ManuallyDrop,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
    },
    time::{Duration, Instant},
};

use tracing::{debug, info, warn};
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_TIMEOUT},
        Media::Audio::{
            AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
            AUDCLNT_STREAMFLAGS_LOOPBACK, AUDIOCLIENT_ACTIVATION_PARAMS,
            AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, ActivateAudioInterfaceAsync,
            AudioSessionStateActive, DEVICE_STATE_ACTIVE, IActivateAudioInterfaceAsyncOperation,
            IActivateAudioInterfaceCompletionHandler,
            IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient,
            IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
            MMDeviceEnumerator, PROCESS_LOOPBACK_MODE, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
            PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            WAVE_FORMAT_PCM, WAVEFORMATEX, eRender,
        },
        System::{
            Com::{
                BLOB, CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoUninitialize,
                StructuredStorage::{
                    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
                },
            },
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                TH32CS_SNAPPROCESS,
            },
            Threading::{CreateEventW, WaitForMultipleObjects},
            Variant::VT_BLOB,
        },
    },
    core::{HRESULT, IUnknown, Interface as _, PCWSTR, Ref, implement},
};

use super::system_audio::{CHANNELS, Mixer, Out, Plan, Process, RATE, plan};

/// How often the apps playing are looked at again.
const RESCAN: Duration = Duration::from_secs(1);
/// The mixer's key for the loopback of everything but this process (no
/// process has id 0 but the system's idle one).
const ALL_BUT_ME: u32 = 0;

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

/// One loopback, running until dropped.
struct Loopback {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    event: HANDLE,
}

impl Drop for Loopback {
    fn drop(&mut self) {
        // SAFETY: the client and event are ours, used no more.
        unsafe {
            let _ = self.client.Stop();
            let _ = CloseHandle(self.event);
        }
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
    let mut capturing = Capturing {
        loopbacks: HashMap::new(),
        failed: HashSet::new(),
        mixer: Mixer::new(),
        plan: None,
        why: None,
    };
    capturing.follow();
    // Every app's own loopback failed: the next look falls back.
    if capturing.loopbacks.is_empty() && matches!(&capturing.plan, Some(Plan::Only(p)) if !p.is_empty()) {
        capturing.follow();
    }
    if capturing.loopbacks.is_empty() && capturing.plan != Some(Plan::Only(Vec::new())) {
        // Not even everything but this app: the share goes on without sound.
        let why = capturing.why.take().unwrap_or_else(|| NEEDS.to_owned());
        warn!(%why, "screen audio: no process loopback");
        let _ = started.send(Err(why));
    } else {
        let _ = started.send(Ok(()));
        capturing.pump(stop, &mut out);
        info!(frames = capturing.mixer.sent, "screen audio: loopback stopped");
    }
    drop(capturing);
    // SAFETY: matches the CoInitializeEx above.
    unsafe { CoUninitialize() };
}

const NEEDS: &str = "sharing the computer's sound needs Windows 10 version 2004 or later";

struct Capturing {
    /// Running loopbacks, by mixer key ([`ALL_BUT_ME`] or a process id).
    loopbacks: HashMap<u32, Loopback>,
    /// Processes whose loopback could not be had (not tried again).
    failed: HashSet<u32>,
    mixer: Mixer,
    /// What is captured now.
    plan: Option<Plan>,
    /// Why everything but this app could not be had, if it could not.
    why: Option<String>,
}

impl Capturing {
    /// Look at what plays and capture what the plan says, keeping the
    /// loopbacks that are still wanted.
    fn follow(&mut self) {
        let wanted = match self.choose() {
            Ok(plan) => plan,
            Err(why) => {
                if self.plan != Some(Plan::AllButMe) {
                    warn!(%why, "screen audio: could not see which apps play; sharing everything but Morpheus");
                }
                Plan::AllButMe
            }
        };
        // No app's own loopback could be had: Discord is shared after all
        // rather than nothing.
        let wanted = match wanted {
            Plan::Only(pids) if !pids.is_empty() && pids.iter().all(|p| self.failed.contains(p)) => {
                if self.plan != Some(Plan::AllButMe) {
                    warn!("screen audio: no app's own loopback; sharing everything but Morpheus (Discord too)");
                }
                Plan::AllButMe
            }
            wanted => wanted,
        };
        if self.plan.as_ref() != Some(&wanted) {
            match &wanted {
                Plan::AllButMe => info!("screen audio: everything but Morpheus (process loopback)"),
                Plan::Only(pids) => info!(
                    ?pids,
                    "screen audio: Discord left out; each other app on its own loopback"
                ),
            }
        }
        let keys: Vec<u32> = match &wanted {
            Plan::AllButMe => vec![ALL_BUT_ME],
            Plan::Only(pids) => pids.clone(),
        };
        let gone: Vec<u32> = self
            .loopbacks
            .keys()
            .copied()
            .filter(|key| !keys.contains(key))
            .collect();
        for key in gone {
            self.loopbacks.remove(&key);
            self.mixer.remove(key);
        }
        for key in keys {
            if self.loopbacks.contains_key(&key) || self.failed.contains(&key) {
                continue;
            }
            let opened = if key == ALL_BUT_ME {
                open(std::process::id(), PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE)
            } else {
                open(key, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE)
            };
            match opened {
                Ok(loopback) => {
                    debug!(key, "screen audio: a loopback started");
                    self.loopbacks.insert(key, loopback);
                    self.mixer.add(key);
                }
                Err(why) if key == ALL_BUT_ME => {
                    self.why = Some(why);
                }
                Err(why) => {
                    warn!(pid = key, %why, "screen audio: an app's sound could not be had; left out");
                    self.failed.insert(key);
                }
            }
        }
        self.plan = Some(wanted);
    }

    /// The plan for what plays now (with Discord left out or not, as
    /// Settings says). Processes already captured stay while they have any
    /// session at all (paused music comes back at once).
    fn choose(&self) -> Result<Plan, String> {
        let leave_out = crate::screen_audio_leaves_out_discord();
        if !leave_out {
            return Ok(Plan::AllButMe);
        }
        let (active, any) = sessions().map_err(|err| format!("audio sessions: {err}"))?;
        let mut playing = active;
        playing.extend(
            self.loopbacks
                .keys()
                .copied()
                .filter(|key| *key != ALL_BUT_ME && any.contains(key)),
        );
        let processes = processes().map_err(|err| format!("processes: {err}"))?;
        Ok(plan(&processes, &playing, std::process::id(), leave_out))
    }

    /// Packets as they come, out in 10 ms frames, until `stop`; what plays
    /// looked at again every [`RESCAN`].
    fn pump(&mut self, stop: &AtomicBool, out: &mut Out) {
        let mut looked = Instant::now();
        let mut samples: Vec<i16> = Vec::with_capacity(8 * FRAME_SAMPLES);
        while !stop.load(Ordering::Relaxed) {
            if looked.elapsed() >= RESCAN {
                looked = Instant::now();
                self.follow();
            }
            let events: Vec<HANDLE> = self.loopbacks.values().map(|l| l.event).collect();
            if events.is_empty() {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            // SAFETY: our events; a timeout to look at `stop`.
            let woke = unsafe { WaitForMultipleObjects(&events, false, 100) };
            if woke == WAIT_TIMEOUT {
                continue;
            }
            let mut ended = Vec::new();
            for (&key, loopback) in &self.loopbacks {
                samples.clear();
                if let Err(why) = drain(loopback, &mut samples) {
                    warn!(key, %why, "screen audio: a loopback ended");
                    ended.push(key);
                    continue;
                }
                if !samples.is_empty() {
                    self.mixer.push(key, samples.iter().copied(), out);
                }
            }
            for key in ended {
                self.loopbacks.remove(&key);
                self.mixer.remove(key);
                self.failed.insert(key);
            }
        }
    }
}

/// Samples in a 10 ms stereo frame.
const FRAME_SAMPLES: usize = super::system_audio::FRAME * CHANNELS;

/// Every packet a loopback has, appended to `into` (silence as zeros).
fn drain(loopback: &Loopback, into: &mut Vec<i16>) -> Result<(), String> {
    loop {
        // SAFETY: the capture client's own calls, each buffer released
        // after it is copied.
        unsafe {
            match loopback.capture.GetNextPacketSize() {
                Ok(0) => return Ok(()),
                Ok(_) => {}
                Err(err) => return Err(err.to_string()),
            }
            let mut data = std::ptr::null_mut();
            let mut frames = 0u32;
            let mut flags = 0u32;
            loopback
                .capture
                .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
                .map_err(|err| format!("reading: {err}"))?;
            let samples = frames as usize * CHANNELS;
            if data.is_null() || flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 {
                into.resize(into.len() + samples, 0);
            } else {
                into.extend_from_slice(std::slice::from_raw_parts(data.cast::<i16>(), samples));
            }
            loopback
                .capture
                .ReleaseBuffer(frames)
                .map_err(|err| format!("releasing its buffer: {err}"))?;
        }
    }
}

/// The processes with an audio session on a speaker now: those playing
/// (active), and all of them.
fn sessions() -> windows::core::Result<(Vec<u32>, HashSet<u32>)> {
    let mut active = Vec::new();
    let mut any = HashSet::new();
    // SAFETY: plain COM calls on interfaces this function owns.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let devices = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        for i in 0..devices.GetCount()? {
            let Ok(device) = devices.Item(i) else {
                continue;
            };
            let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
                continue;
            };
            let Ok(list) = manager.GetSessionEnumerator() else {
                continue;
            };
            for s in 0..list.GetCount().unwrap_or(0) {
                let Ok(session) = list.GetSession(s) else {
                    continue;
                };
                let Ok(session) = session.cast::<IAudioSessionControl2>() else {
                    continue;
                };
                let Ok(pid) = session.GetProcessId() else {
                    continue;
                };
                any.insert(pid);
                if session.GetState().is_ok_and(|state| state == AudioSessionStateActive) {
                    active.push(pid);
                }
            }
        }
    }
    Ok((active, any))
}

/// Every running process: its id, its parent's and its program's file name.
fn processes() -> windows::core::Result<Vec<Process>> {
    let mut all = Vec::new();
    // SAFETY: a snapshot of our own, walked with an entry of the size it
    // says, and closed.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?;
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut more = Process32FirstW(snapshot, &mut entry).is_ok();
        while more {
            let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
            all.push(Process {
                pid: entry.th32ProcessID,
                parent: entry.th32ParentProcessID,
                exe: String::from_utf16_lossy(&entry.szExeFile[..len]),
            });
            more = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    Ok(all)
}

/// A started loopback of `target`'s process tree (`mode`: all but it, or it
/// alone) at 48 kHz stereo 16-bit, signalling its event when there is sound.
fn open(target: u32, mode: PROCESS_LOOPBACK_MODE) -> Result<Loopback, String> {
    let needs = NEEDS;
    let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: target,
                ProcessLoopbackMode: mode,
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
        Ok(Loopback {
            client,
            capture,
            event,
        })
    }
}
