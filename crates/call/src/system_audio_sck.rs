//! macOS: what other apps play, by ScreenCaptureKit (macOS 13 and later):
//! an `SCStream` with `capturesAudio` and `excludesCurrentProcessAudio`, so
//! the call's voices, chimes and videos played here are not in it. See
//! `system_audio.rs`.
//!
//! The shared screen's pictures come from libwebrtc's own ScreenCaptureKit
//! stream, which this crate cannot reach; the sound is a second stream, of
//! sound alone (its pictures, 2x2 at one a second, are dropped):
//!
//! - macOS 14 and later: libwebrtc puts up the system's picker
//!   (`SCContentSharingPicker`). An observer of that picker ([`watch_picker`],
//!   before the capturer starts) keeps the content filter picked, and the
//!   sound stream uses the same filter: a picked app or window shares that
//!   app's sound, a picked screen everything's. The picker's grant covers it,
//!   with no Screen Recording permission asked.
//! - Otherwise (macOS 13, or no pick seen): the main display's filter, which
//!   needs the Screen Recording permission (sharing the screen asked for it
//!   already on macOS 13).
//!
//! The sound comes as 32-bit float, one buffer per channel, converted by
//! `system_audio_frames.rs`. Objective-C through raw calls: this module alone
//! may use `unsafe`.
#![allow(unsafe_code)]

use std::{
    ptr::NonNull,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
    },
    time::Duration,
};

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::{
    AnyThread, DefinedClass, Message, define_class, msg_send, rc::Retained, runtime::ProtocolObject,
};
use objc2_core_audio_types::AudioBuffer;
use objc2_core_foundation::CFRetained;
use objc2_core_media::{
    CMAudioFormatDescriptionGetStreamBasicDescription, CMBlockBuffer, CMSampleBuffer, CMTime,
    kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment,
};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol, NSProcessInfo};
use objc2_screen_capture_kit::{
    SCContentFilter, SCContentSharingPicker, SCContentSharingPickerObserver, SCShareableContent,
    SCStream, SCStreamConfiguration, SCStreamDelegate, SCStreamErrorCode, SCStreamOutput,
    SCStreamOutputType,
};
use tracing::{info, warn};

use super::{
    autorelease,
    system_audio::{CHANNELS, Out, RATE},
    system_audio_frames::{Framer, Layout},
};

/// An Objective-C object handed between threads. Only immutable objects
/// (filters) and ones ScreenCaptureKit itself calls from its own queues
/// (streams, our observer) go in one.
struct Shared<T>(Retained<T>);
// SAFETY: see above; ScreenCaptureKit's objects are used from any thread.
unsafe impl<T> Send for Shared<T> {}

/// The filter last picked in the system's picker, while a share is going.
static PICKED: Mutex<Option<Shared<SCContentFilter>>> = Mutex::new(None);
/// The picker's observer, made once and kept (the picker holds it weakly).
static OBSERVER: Mutex<Option<Shared<PickerObserver>>> = Mutex::new(None);
/// The sound stream running, to follow a change of what is picked.
static LIVE: Mutex<Option<Shared<SCStream>>> = Mutex::new(None);

/// This Mac's macOS major version (11, 12, ... 26).
pub(crate) fn macos_major() -> isize {
    static MAJOR: OnceLock<isize> = OnceLock::new();
    *MAJOR.get_or_init(|| {
        autorelease::pool(|| NSProcessInfo::processInfo().operatingSystemVersion().majorVersion)
    })
}

/// Before libwebrtc's capturer starts (it puts up the picker on macOS 14 and
/// later): watch the picker for what is picked, forgetting an earlier share's.
pub(crate) fn watch_picker() {
    if let Ok(mut picked) = PICKED.lock() {
        *picked = None;
    }
    if macos_major() < 14 {
        return;
    }
    autorelease::pool(|| {
        let Ok(mut observer) = OBSERVER.lock() else {
            return;
        };
        if observer.is_some() {
            return;
        }
        let made = PickerObserver::new();
        // SAFETY: the shared picker exists from macOS 14 (checked); the
        // observer is kept for good in OBSERVER.
        unsafe {
            SCContentSharingPicker::sharedPicker().addObserver(ProtocolObject::from_ref(&*made));
        }
        *observer = Some(Shared(made));
    });
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop.
    #[unsafe(super(NSObject))]
    #[name = "MorpheusScreenAudioPickerObserver"]
    struct PickerObserver;

    unsafe impl NSObjectProtocol for PickerObserver {}

    unsafe impl SCContentSharingPickerObserver for PickerObserver {
        #[unsafe(method(contentSharingPicker:didCancelForStream:))]
        fn did_cancel(&self, _picker: &SCContentSharingPicker, _stream: Option<&SCStream>) {}

        #[unsafe(method(contentSharingPicker:didUpdateWithFilter:forStream:))]
        fn did_update(
            &self,
            _picker: &SCContentSharingPicker,
            filter: &SCContentFilter,
            _stream: Option<&SCStream>,
        ) {
            autorelease::pool(|| {
                info!("screen audio: the picker chose what to share; its sound follows");
                if let Ok(live) = LIVE.lock()
                    && let Some(live) = live.as_ref()
                {
                    // SAFETY: a running stream and a filter from the picker.
                    unsafe { live.0.updateContentFilter_completionHandler(filter, None) };
                }
                if let Ok(mut picked) = PICKED.lock() {
                    *picked = Some(Shared(filter.retain()));
                }
            });
        }

        #[unsafe(method(contentSharingPickerStartDidFailWithError:))]
        fn start_did_fail(&self, _error: &NSError) {}
    }
);

impl PickerObserver {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        // SAFETY: NSObject's init.
        unsafe { msg_send![super(this), init] }
    }
}

/// What the sound stream's callback keeps.
struct OutputState {
    framer: Framer,
    /// Buffers that could not be read (logged once).
    unreadable: u64,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop.
    #[unsafe(super(NSObject))]
    #[name = "MorpheusScreenAudioOutput"]
    #[ivars = Mutex<OutputState>]
    struct Output;

    unsafe impl NSObjectProtocol for Output {}

    unsafe impl SCStreamOutput for Output {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn did_output(
            &self,
            _stream: &SCStream,
            sample: &CMSampleBuffer,
            kind: SCStreamOutputType,
        ) {
            if kind != SCStreamOutputType::Audio {
                return;
            }
            autorelease::pool(|| {
                let Ok(mut state) = self.ivars().lock() else {
                    return;
                };
                // SAFETY: a sample buffer ScreenCaptureKit handed over, used
                // only during this call.
                if let Err(why) = unsafe { read(sample, &mut state.framer) } {
                    state.unreadable += 1;
                    if state.unreadable == 1 {
                        warn!(%why, "screen audio: a buffer could not be read");
                    }
                }
            });
        }
    }

    unsafe impl SCStreamDelegate for Output {
        #[unsafe(method(stream:didStopWithError:))]
        fn did_stop(&self, _stream: &SCStream, error: &NSError) {
            warn!(error = %describe(error), "screen audio: ScreenCaptureKit stopped the sound");
        }
    }
);

impl Output {
    fn new(out: Out) -> Retained<Self> {
        let this = Self::alloc().set_ivars(Mutex::new(OutputState {
            framer: Framer::new(out),
            unreadable: 0,
        }));
        // SAFETY: NSObject's init.
        unsafe { msg_send![super(this), init] }
    }
}

/// One audio sample buffer into `framer`: its format, then its buffers.
///
/// # Safety
/// `sample` is a live CMSampleBuffer.
unsafe fn read(sample: &CMSampleBuffer, framer: &mut Framer) -> Result<usize, String> {
    // SAFETY: plain CoreMedia getters on a live buffer; the buffer list is
    // ours, large enough (8 channels), and its data is held by the retained
    // block buffer until the end of this function.
    unsafe {
        let frames = usize::try_from(sample.num_samples()).unwrap_or(0);
        let format = sample.format_description().ok_or("no format")?;
        let basic = CMAudioFormatDescriptionGetStreamBasicDescription(&format);
        let Some(basic) = basic.as_ref() else {
            return Err("not sound".into());
        };
        let layout = Layout::from_core_audio(
            basic.mSampleRate,
            basic.mFormatID,
            basic.mFormatFlags,
            basic.mBitsPerChannel,
            basic.mChannelsPerFrame,
        )?;
        #[repr(C)]
        struct List {
            count: u32,
            buffers: [AudioBuffer; 8],
        }
        let empty = AudioBuffer {
            mNumberChannels: 0,
            mDataByteSize: 0,
            mData: std::ptr::null_mut(),
        };
        let mut list = List {
            count: 0,
            buffers: [empty; 8],
        };
        let mut block: *mut CMBlockBuffer = std::ptr::null_mut();
        let status = sample.audio_buffer_list_with_retained_block_buffer(
            std::ptr::null_mut(),
            (&raw mut list).cast(),
            size_of::<List>(),
            None,
            None,
            kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment,
            &mut block,
        );
        // Released when this returns.
        let _block = NonNull::new(block).map(|b| CFRetained::from_raw(b));
        if status != 0 {
            return Err(format!("its buffers: OSStatus {status}"));
        }
        let count = (list.count as usize).min(list.buffers.len());
        let buffers: Vec<&[u8]> = list.buffers[..count]
            .iter()
            .map(|b| {
                if b.mData.is_null() {
                    &[][..]
                } else {
                    std::slice::from_raw_parts(b.mData.cast::<u8>(), b.mDataByteSize as usize)
                }
            })
            .collect();
        Ok(framer.push(&layout, &buffers, frames))
    }
}

/// Capture until `stop`; `started` hears once whether it could.
pub(crate) fn run(stop: &AtomicBool, out: Out, started: Sender<Result<(), String>>) {
    let opened = autorelease::pool(|| open(out));
    let (stream, _output, _queue) = match opened {
        Ok(running) => running,
        Err(why) => {
            warn!(%why, "screen audio: no ScreenCaptureKit sound");
            let _ = started.send(Err(why));
            return;
        }
    };
    if let Ok(mut live) = LIVE.lock() {
        *live = Some(Shared(stream.clone()));
    }
    let _ = started.send(Ok(()));
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
    }
    if let Ok(mut live) = LIVE.lock() {
        *live = None;
    }
    autorelease::pool(|| {
        let (done, finished) = mpsc::channel();
        let block = RcBlock::new(move |_: *mut NSError| {
            let _ = done.send(());
        });
        // SAFETY: our running stream; the block is copied by the call.
        unsafe { stream.stopCaptureWithCompletionHandler(Some(&block)) };
        let _ = finished.recv_timeout(Duration::from_secs(2));
    });
    info!("screen audio: ScreenCaptureKit sound stopped");
}

type Running = (Retained<SCStream>, Retained<Output>, DispatchRetained<DispatchQueue>);

/// A started sound stream of the picked content (or the main display) less
/// this process, at 48 kHz stereo.
fn open(out: Out) -> Result<Running, String> {
    let major = macos_major();
    if let Some(why) = super::system_audio::macos_unavailable(major) {
        return Err(why.into());
    }
    let picked = PICKED.lock().ok().and_then(|p| p.as_ref().map(|f| f.0.clone()));
    let (filter, what) = match picked {
        Some(filter) => (filter, "what was picked"),
        None => (main_display()?, "the main display"),
    };
    // SAFETY: plain ScreenCaptureKit calls with objects made here; the
    // output and queue outlive the stream (they are returned with it).
    unsafe {
        let config = SCStreamConfiguration::new();
        config.setCapturesAudio(true);
        config.setExcludesCurrentProcessAudio(true);
        config.setSampleRate(RATE as isize);
        config.setChannelCount(CHANNELS as isize);
        // Pictures as small and as few as it allows: they are dropped.
        config.setWidth(2);
        config.setHeight(2);
        config.setMinimumFrameInterval(CMTime::new(1, 1));
        config.setShowsCursor(false);
        let output = Output::new(out);
        let stream = SCStream::initWithFilter_configuration_delegate(
            SCStream::alloc(),
            &filter,
            &config,
            Some(ProtocolObject::from_ref(&*output)),
        );
        let queue = DispatchQueue::new("morpheus.screen-audio", None);
        for kind in [SCStreamOutputType::Audio, SCStreamOutputType::Screen] {
            stream
                .addStreamOutput_type_sampleHandlerQueue_error(
                    ProtocolObject::from_ref(&*output),
                    kind,
                    Some(&queue),
                )
                .map_err(|err| format!("its output: {}", describe(&err)))?;
        }
        let (done, finished) = mpsc::channel::<Option<String>>();
        let block = RcBlock::new(move |err: *mut NSError| {
            let _ = done.send(err.as_ref().map(describe));
        });
        stream.startCaptureWithCompletionHandler(Some(&block));
        match finished.recv_timeout(Duration::from_secs(10)) {
            Ok(None) => {}
            Ok(Some(why)) => return Err(format!("starting it: {why}")),
            Err(_) => return Err("starting it: no answer".into()),
        }
        info!(source = what, macos = major, "screen audio: capturing the system's sound less this app (ScreenCaptureKit)");
        Ok((stream, output, queue))
    }
}

/// The main display's content filter (needs the Screen Recording permission).
fn main_display() -> Result<Retained<SCContentFilter>, String> {
    let (done, finished) = mpsc::channel::<Result<Shared<SCShareableContent>, String>>();
    let block = RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
        // SAFETY: the objects ScreenCaptureKit passes, retained for the
        // waiting thread.
        let got = unsafe {
            match (Retained::retain(content), err.as_ref()) {
                (Some(content), None) => Ok(Shared(content)),
                (_, Some(err)) => Err(describe(err)),
                (None, None) => Err("nothing to share".into()),
            }
        };
        let _ = done.send(got);
    });
    // SAFETY: the block is copied by the call.
    unsafe {
        SCShareableContent::getShareableContentExcludingDesktopWindows_onScreenWindowsOnly_completionHandler(
            true, true, &block,
        );
    }
    let content = finished
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "the screen's content: no answer".to_owned())?
        .map_err(|why| format!("the screen's content: {why}"))?
        .0;
    // SAFETY: getters and an init on objects ScreenCaptureKit gave.
    unsafe {
        let display = content
            .displays()
            .firstObject()
            .ok_or("no display to take the sound of")?;
        Ok(SCContentFilter::initWithDisplay_excludingWindows(
            SCContentFilter::alloc(),
            &display,
            &NSArray::new(),
        ))
    }
}

/// An error, with what to do about the usual one.
fn describe(err: &NSError) -> String {
    if err.code() == SCStreamErrorCode::UserDeclined.0 {
        return "Morpheus may not record the screen: allow it in System Settings, \
                Privacy & Security, Screen & System Audio Recording"
            .into();
    }
    format!("{} ({})", err.localizedDescription(), err.code())
}
