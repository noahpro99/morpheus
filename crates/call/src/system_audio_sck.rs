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
//! The sound comes as 32-bit float, one buffer per channel (read through
//! CoreMedia's buffer list of the size it asks for, or else its data buffer;
//! a buffer that cannot be read is counted and skipped), converted by
//! `system_audio_frames.rs`. Objective-C through raw calls: this module alone
//! may use `unsafe`.
#![allow(unsafe_code)]

use std::{
    mem::offset_of,
    ptr::NonNull,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
    },
    time::{Duration, Instant},
};

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::{
    AnyThread, DefinedClass, Message, define_class, msg_send, rc::Retained, runtime::ProtocolObject,
};
use objc2_core_audio_types::{AudioBuffer, AudioBufferList};
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
    system_audio::{CHANNELS, Out, RATE, UNREADABLE},
    system_audio_frames::{
        Failures, Framer, LIST_ENTRY, LIST_HEADER, Layout, list_room, list_size,
    },
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
    /// Buffers that could not be read (all counted, logged now and then).
    unreadable: Failures,
    /// Buffers read from the data buffer as the list failed (the same).
    fell_back: Failures,
    /// Whether the first readable buffer was logged.
    told: bool,
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
                match unsafe { read(sample, &mut state.framer) } {
                    Ok(read) => {
                        if !state.told {
                            state.told = true;
                            info!(layout = ?read.layout, via = ?read.via, "screen audio: the first sound arrived");
                        }
                        if let Some(why) = read.list_failed
                            && let Some(since) = state.fell_back.note(Instant::now())
                        {
                            let total = state.fell_back.count;
                            warn!(%why, via = ?read.via, since, total, "screen audio: CoreMedia's buffer list needed another try (read anyway)");
                        }
                    }
                    // Counted, logged now and then, and on to the next.
                    Err(why) => {
                        UNREADABLE.fetch_add(1, Ordering::Relaxed);
                        if let Some(since) = state.unreadable.note(Instant::now()) {
                            let total = state.unreadable.count;
                            warn!(%why, since, total, "screen audio: a buffer could not be read");
                        }
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
            unreadable: Failures::default(),
            fell_back: Failures::default(),
            told: false,
        }));
        // SAFETY: NSObject's init.
        unsafe { msg_send![super(this), init] }
    }
}

/// `OSStatus` kCMSampleBufferError_ArrayTooSmall: the list handed over has
/// too few buffers.
const ARRAY_TOO_SMALL: i32 = -12737;

// The list's layout as `system_audio_frames.rs` counts it.
const _: () = assert!(offset_of!(AudioBufferList, mBuffers) == LIST_HEADER);
const _: () = assert!(size_of::<AudioBuffer>() == LIST_ENTRY);

/// 16 bytes, 16-aligned: what a buffer list is allocated in.
#[derive(Clone, Copy)]
#[repr(C, align(16))]
struct Chunk([u8; 16]);

/// How a sample buffer was read, for the log.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Via {
    /// CoreMedia's buffer list, of this many bytes.
    List(usize),
    /// Its data buffer, split by the layout (the list could not be had).
    Block,
}

/// A sample buffer read: its layout, how, and why the list failed if it did.
struct Read {
    layout: Layout,
    via: Via,
    list_failed: Option<String>,
}

/// One audio sample buffer into `framer`: its format, then its buffers, by
/// CoreMedia's buffer list (its size asked first, as Apple documents and
/// screencapturekit-rs does), or else its data buffer split by the format
/// (as OBS's mac-sck-audio reads it).
///
/// # Safety
/// `sample` is a live CMSampleBuffer.
unsafe fn read(sample: &CMSampleBuffer, framer: &mut Framer) -> Result<Read, String> {
    // SAFETY: plain CoreMedia getters on a live buffer; the buffer list is
    // ours, at least of the size CoreMedia asked for, and the data it points
    // at is held by the retained block buffer until the end of this function.
    unsafe {
        if !sample.is_valid() {
            return Err("an invalid sample buffer".into());
        }
        if !sample.data_is_ready() {
            let status = sample.make_data_ready();
            if status != 0 {
                return Err(format!("its data was not ready: OSStatus {status}"));
            }
        }
        let reported = usize::try_from(sample.num_samples()).unwrap_or(0);
        let format = sample.format_description().ok_or("no format")?;
        let basic = CMAudioFormatDescriptionGetStreamBasicDescription(&format);
        let Some(basic) = basic.as_ref() else {
            return Err("not sound".into());
        };
        let what = || {
            format!(
                "{reported} frames, {} Hz, {} channels, {} bits, flags {:#x}",
                basic.mSampleRate,
                basic.mChannelsPerFrame,
                basic.mBitsPerChannel,
                basic.mFormatFlags
            )
        };
        let layout = Layout::from_core_audio(
            basic.mSampleRate,
            basic.mFormatID,
            basic.mFormatFlags,
            basic.mBitsPerChannel,
            basic.mChannelsPerFrame,
        )?;
        let aligned = kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment;
        // The list's size, asked first (it depends on the buffers and flags).
        let mut asked = 0usize;
        let status = sample.audio_buffer_list_with_retained_block_buffer(
            &mut asked,
            std::ptr::null_mut(),
            0,
            None,
            None,
            aligned,
            std::ptr::null_mut(),
        );
        let asked = (status == 0 && asked >= LIST_HEADER + LIST_ENTRY).then_some(asked);
        let first = list_size(asked, &layout);
        // That size; four times it if CoreMedia still says too small; then
        // without asking for 16-byte aligned data.
        let mut why = match asked {
            Some(_) => None,
            None => Some(format!("its list's size: OSStatus {status}")),
        };
        for (size, flags) in [(first, aligned), (first * 4, aligned), (first * 4, 0)] {
            let mut chunks = vec![Chunk([0; 16]); size / 16];
            let list = chunks.as_mut_ptr().cast::<AudioBufferList>();
            let mut block: *mut CMBlockBuffer = std::ptr::null_mut();
            let status = sample.audio_buffer_list_with_retained_block_buffer(
                std::ptr::null_mut(),
                list,
                size,
                None,
                None,
                flags,
                &mut block,
            );
            // Released when this iteration ends.
            let _block = NonNull::new(block).map(|b| CFRetained::from_raw(b));
            if status != 0 {
                why = Some(format!(
                    "its list ({size} bytes, flags {flags}): OSStatus {status}"
                ));
                if status == ARRAY_TOO_SMALL {
                    continue;
                }
                break;
            }
            let count = ((*list).mNumberBuffers as usize).min(list_room(size));
            let entries = (&raw const (*list).mBuffers).cast::<AudioBuffer>();
            let buffers: Vec<&[u8]> = (0..count)
                .map(|i| {
                    let b = *entries.add(i);
                    if b.mData.is_null() {
                        &[][..]
                    } else {
                        std::slice::from_raw_parts(b.mData.cast::<u8>(), b.mDataByteSize as usize)
                    }
                })
                .collect();
            if buffers.len() < layout.buffers() {
                why = Some(format!(
                    "its list had {} buffers for {} channels ({})",
                    buffers.len(),
                    layout.channels,
                    if layout.interleaved {
                        "interleaved"
                    } else {
                        "planar"
                    }
                ));
                break;
            }
            let lens: Vec<usize> = buffers.iter().map(|b| b.len()).collect();
            framer.push(&layout, &buffers, layout.frames_in(reported, &lens));
            return Ok(Read {
                layout,
                via: Via::List(size),
                list_failed: why,
            });
        }
        // The data buffer, copied out whole (it may be in pieces) and split.
        let why = why.unwrap_or_default();
        let data = sample
            .data_buffer()
            .ok_or_else(|| format!("{why}; and no data buffer ({})", what()))?;
        let len = data.data_length();
        if len == 0 {
            return Err(format!("{why}; and an empty data buffer ({})", what()));
        }
        let mut bytes = vec![0u8; len];
        let status = data.copy_data_bytes(0, len, NonNull::from(&mut bytes[..]).cast());
        if status != 0 {
            return Err(format!(
                "{why}; and its data: OSStatus {status} ({})",
                what()
            ));
        }
        let ranges = layout.split_block(reported, len);
        let buffers: Vec<&[u8]> = ranges.iter().map(|r| &bytes[r.clone()]).collect();
        let lens: Vec<usize> = buffers.iter().map(|b| b.len()).collect();
        framer.push(&layout, &buffers, layout.frames_in(reported, &lens));
        Ok(Read {
            layout,
            via: Via::Block,
            list_failed: Some(format!("{why} ({})", what())),
        })
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
