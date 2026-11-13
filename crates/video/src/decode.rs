//! The decoders: OpenH264 for the pictures (in display order, each given
//! its display time), Symphonia's AAC for the sound.

use std::{collections::BinaryHeap, cmp::Reverse, sync::Arc, time::Duration};

use openh264::{
    decoder::{DecodeOptions, Decoder as H264, DecoderConfig, Flush},
    formats::YUVSource as _,
    OpenH264API,
};
use symphonia_core::{
    audio::SampleBuffer,
    codecs::{CODEC_TYPE_AAC, CodecParameters, Decoder as _, DecoderOptions},
    formats::Packet,
};

use crate::{
    AudioChunk, Frame, Info, Unsupported,
    demux::Demux,
};

/// MP4 (H.264 + AAC) decoded on demand: pictures and sound each in their
/// own order, from the start or from where [`Decoder::seek`] put them.
pub struct Decoder {
    demux: Demux,
    h264: H264,
    /// Next video sample to feed (decode order).
    next_video: u32,
    /// Display times of samples fed but not yet out of the decoder.
    pending: BinaryHeap<Reverse<i64>>,
    /// Pictures shown before this (a seek's target) are decoded and dropped.
    skip_before: f64,
    drained: bool,
    flushed: Vec<Frame>,
    aac: Option<symphonia_codec_aac::AacDecoder>,
    next_audio: u32,
    audio_skip_before: f64,
}

fn new_h264() -> Result<H264, Unsupported> {
    let config = DecoderConfig::new().flush_after_decode(Flush::NoFlush);
    H264::with_api_config(OpenH264API::from_source(), config).map_err(|err| Unsupported::Broken(err.to_string()))
}

fn new_aac(demux: &Demux) -> Option<symphonia_codec_aac::AacDecoder> {
    let audio = demux.audio.as_ref()?;
    let mut params = CodecParameters::new();
    params
        .for_codec(CODEC_TYPE_AAC)
        .with_sample_rate(audio.rate)
        .with_extra_data(audio.config.clone().into_boxed_slice());
    symphonia_core::codecs::Decoder::try_new(&params, &DecoderOptions::default()).ok()
}

/// I420 to B, G, R, A (GPUI's pixel order).
fn to_bgra(yuv: &openh264::decoder::DecodedYUV<'_>) -> (u32, u32, Vec<u8>) {
    let (w, h) = yuv.dimensions();
    let mut out = vec![0u8; w * h * 4];
    yuv.write_rgba8(&mut out);
    for pixel in out.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    (u32::try_from(w).unwrap_or(0), u32::try_from(h).unwrap_or(0), out)
}

fn duration(seconds: f64) -> Duration {
    Duration::from_secs_f64(seconds.max(0.0))
}

impl Decoder {
    pub fn open(bytes: Arc<[u8]>) -> Result<Self, Unsupported> {
        let demux = Demux::open(bytes)?;
        let aac = new_aac(&demux);
        Ok(Self {
            h264: new_h264()?,
            demux,
            next_video: 0,
            pending: BinaryHeap::new(),
            skip_before: f64::NEG_INFINITY,
            drained: false,
            flushed: Vec::new(),
            aac,
            next_audio: 0,
            audio_skip_before: 0.0,
        })
    }

    pub fn info(&self) -> &Info {
        &self.demux.info
    }

    /// The sound's sample rate and channels, if it has sound this can decode.
    pub fn audio_format(&self) -> Option<(u32, u16)> {
        let audio = self.demux.audio.as_ref()?;
        self.aac.as_ref().map(|_| (audio.rate, audio.channels))
    }

    /// Whether there is sound this can decode.
    pub fn has_audio(&self) -> bool {
        self.aac.is_some()
    }

    /// The next picture in display order, `None` at the end.
    pub fn next_video(&mut self) -> Option<Frame> {
        loop {
            let frame = self.decode_one()?;
            if frame.pts.as_secs_f64() + 0.001 >= self.skip_before {
                return Some(frame);
            }
        }
    }

    fn decode_one(&mut self) -> Option<Frame> {
        let timescale = f64::from(self.demux.video.samples.timescale);
        loop {
            if let Some(frame) = self.flushed.pop() {
                return Some(frame);
            }
            if self.drained {
                return None;
            }
            if self.next_video >= self.demux.video.samples.len() {
                // Whatever the decoder still holds (B-frame reordering).
                self.drained = true;
                let mut frames = Vec::new();
                if let Ok(left) = self.h264.flush_remaining() {
                    for yuv in left {
                        let (width, height, bgra) = to_bgra(&yuv);
                        let pts = self.pending.pop().map_or(0, |Reverse(p)| p);
                        frames.push(Frame {
                            pts: duration(pts as f64 / timescale),
                            index: 0,
                            width,
                            height,
                            bgra,
                        });
                    }
                }
                frames.reverse();
                self.flushed = frames;
                continue;
            }
            let index = self.next_video;
            self.next_video += 1;
            let Some(packet) = self.demux.video_sample(index) else { continue };
            let pts = self.demux.video.samples.pts.get(index as usize).copied().unwrap_or(0);
            self.pending.push(Reverse(pts));
            let decoded = self
                .h264
                .decode_with_options(&packet, DecodeOptions::new().flush_after_decode(Flush::NoFlush));
            if let Ok(Some(yuv)) = decoded {
                let (width, height, bgra) = to_bgra(&yuv);
                let pts = self.pending.pop().map_or(0, |Reverse(p)| p);
                return Some(Frame {
                    pts: duration(pts as f64 / timescale),
                    index: 0,
                    width,
                    height,
                    bgra,
                });
            }
        }
    }

    /// The next stretch of sound (interleaved f32), `None` at the end.
    pub fn next_audio(&mut self) -> Option<AudioChunk> {
        let (rate, channels, count) = {
            let audio = self.demux.audio.as_ref()?;
            (audio.rate, audio.channels, audio.samples.len())
        };
        loop {
            if self.next_audio >= count {
                return None;
            }
            let index = self.next_audio;
            self.next_audio += 1;
            let Some(bytes) = self.demux.audio_sample(index) else { continue };
            let start = self.demux.audio.as_ref()?.samples.seconds(index);
            let aac = self.aac.as_mut()?;
            let Ok(buffer) = aac.decode(&Packet::new_from_slice(0, index.into(), 1024, &bytes)) else {
                continue;
            };
            let spec = *buffer.spec();
            let mut samples = SampleBuffer::<f32>::new(buffer.capacity() as u64, spec);
            samples.copy_interleaved_ref(buffer);
            let out_channels = u16::try_from(spec.channels.count()).unwrap_or(channels).max(1);
            let out_rate = if spec.rate > 0 { spec.rate } else { rate };
            let mut data = samples.samples().to_vec();
            // Drop what comes before the start (encoder priming) or before a
            // seek's target.
            let from = self.audio_skip_before.max(0.0);
            let mut pts = start;
            if start < from {
                let skip_frames = ((from - start) * f64::from(out_rate)).round() as usize;
                let skip = (skip_frames * usize::from(out_channels)).min(data.len());
                data.drain(..skip);
                pts = from;
            }
            if data.is_empty() {
                continue;
            }
            return Some(AudioChunk {
                pts: duration(pts),
                rate: out_rate,
                channels: out_channels,
                samples: data,
            });
        }
    }

    /// Puts both tracks at `to`: the next picture is the one shown at `to`,
    /// the next sound starts there.
    pub fn seek(&mut self, to: Duration) {
        let to = to.as_secs_f64();
        if let Ok(h264) = new_h264() {
            self.h264 = h264;
        }
        self.next_video = self.demux.video.samples.sync_before(to);
        self.pending.clear();
        self.flushed.clear();
        self.drained = false;
        // Half a frame early, so the frame shown at `to` is kept.
        self.skip_before = to - 0.5 / self.demux.video.frame_rate.max(1.0);
        if let Some(audio) = &self.demux.audio {
            self.next_audio = audio.samples.at_or_before(to);
            self.audio_skip_before = to;
            self.aac = new_aac(&self.demux);
        }
    }
}
