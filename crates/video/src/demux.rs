//! MP4 (ISO BMFF) demuxing with the `mp4` crate: the H.264 and AAC tracks,
//! each sample's display time worked out from the sample tables (decode
//! times, composition offsets and the edit list), and samples read by index.

use std::{io::Cursor, sync::Arc, time::Duration};

use mp4::{MediaType, Mp4Reader, Mp4Track, TrackType};

use crate::{Info, Unsupported};

/// A track's samples in decode order: display time in the track's timescale
/// (edit list applied; may be below 0 for the priming before the start).
pub(crate) struct Samples {
    pub id: u32,
    pub timescale: u32,
    pub pts: Vec<i64>,
    /// 1-based sample numbers that start a group of pictures (all, if the
    /// track has no sync table).
    pub sync: Option<Vec<u32>>,
}

impl Samples {
    fn from_track(track: &Mp4Track) -> Self {
        let stbl = &track.trak.mdia.minf.stbl;
        let mut dts = Vec::new();
        let mut at: i64 = 0;
        for entry in &stbl.stts.entries {
            for _ in 0..entry.sample_count {
                dts.push(at);
                at += i64::from(entry.sample_delta);
            }
        }
        let mut offsets = vec![0i64; dts.len()];
        if let Some(ctts) = &stbl.ctts {
            let mut i = 0;
            for entry in &ctts.entries {
                for _ in 0..entry.sample_count {
                    if let Some(slot) = offsets.get_mut(i) {
                        *slot = i64::from(entry.sample_offset);
                    }
                    i += 1;
                }
            }
        }
        // The edit list's first real entry says which media time is shown
        // first (an empty edit, media_time -1, only delays the track).
        let shift = track
            .trak
            .edts
            .as_ref()
            .and_then(|edts| edts.elst.as_ref())
            .and_then(|elst| {
                elst.entries
                    .iter()
                    .map(|e| e.media_time)
                    .find(|&t| t != u64::MAX && t != u64::from(u32::MAX))
            })
            .and_then(|t| i64::try_from(t).ok())
            .unwrap_or(0);
        let pts = dts.iter().zip(&offsets).map(|(d, o)| d + o - shift).collect();
        let sync = stbl.stss.as_ref().map(|stss| stss.entries.clone());
        Self {
            id: track.track_id(),
            timescale: track.timescale().max(1),
            pts,
            sync,
        }
    }

    pub fn len(&self) -> u32 {
        u32::try_from(self.pts.len()).unwrap_or(u32::MAX)
    }

    /// Display time of a 0-based sample, as a signed number of seconds.
    pub fn seconds(&self, index: u32) -> f64 {
        self.pts.get(index as usize).copied().unwrap_or(0) as f64 / f64::from(self.timescale)
    }

    fn is_sync(&self, index: u32) -> bool {
        self.sync
            .as_ref()
            .is_none_or(|sync| sync.binary_search(&(index + 1)).is_ok())
    }

    /// The 0-based sample to start decoding at to show `to`: the last sync
    /// sample shown at or before it (decode order).
    pub fn sync_before(&self, to: f64) -> u32 {
        let mut best = 0;
        for index in 0..self.len() {
            if self.is_sync(index) && self.seconds(index) <= to + 1e-6 {
                best = index;
            }
        }
        best
    }

    /// The 0-based sample (audio: no reordering) shown at or before `to`.
    pub fn at_or_before(&self, to: f64) -> u32 {
        let after = self.pts.partition_point(|&p| (p as f64 / f64::from(self.timescale)) <= to + 1e-6);
        u32::try_from(after.saturating_sub(1)).unwrap_or(0)
    }

    /// When the track's last sample ends.
    pub fn end(&self) -> f64 {
        self.pts.iter().copied().max().map_or(0.0, |p| p as f64 / f64::from(self.timescale))
    }
}

pub(crate) struct VideoTrack {
    pub samples: Samples,
    /// Bytes in each NAL unit's length prefix.
    pub nal_len: usize,
    /// SPS and PPS as Annex B (start codes), fed before every sync sample.
    pub header: Vec<u8>,
    pub frame_rate: f64,
}

pub(crate) struct AudioTrack {
    pub samples: Samples,
    pub rate: u32,
    pub channels: u16,
    /// The AudioSpecificConfig (AAC object type, rate index, channels).
    pub config: Vec<u8>,
}

pub(crate) struct Demux {
    reader: Mp4Reader<Cursor<Arc<[u8]>>>,
    pub video: VideoTrack,
    pub audio: Option<AudioTrack>,
    pub info: Info,
}

/// AAC's sampling frequency table (ISO 14496-3).
const RATES: [u32; 13] = [96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350];

pub(crate) const START_CODE: [u8; 4] = [0, 0, 0, 1];

/// What the first bytes say the file is, when it is not ISO media.
pub(crate) fn sniff(bytes: &[u8]) -> Result<(), Unsupported> {
    match bytes {
        [0x1A, 0x45, 0xDF, 0xA3, ..] => Err(Unsupported::Format("WebM".into())),
        [_, _, _, _, b'f', b't', b'y', b'p', ..] => Ok(()),
        // Some MP4 writers start with another box (moov, free, mdat, wide).
        [_, _, _, _, b'm' | b'f' | b'w', b'o' | b'r' | b'd' | b'i', ..] => Ok(()),
        _ => Err(Unsupported::Format("not a video file".into())),
    }
}

impl Demux {
    pub fn open(bytes: Arc<[u8]>) -> Result<Self, Unsupported> {
        sniff(&bytes)?;
        let size = bytes.len() as u64;
        // The mp4 crate trusts box sizes; a broken file must not take the app down.
        let reader = std::panic::catch_unwind(move || Mp4Reader::read_header(Cursor::new(bytes), size))
            .map_err(|_| Unsupported::Broken("unreadable MP4".into()))?
            .map_err(|err| Unsupported::Broken(err.to_string()))?;
        if reader.is_fragmented() {
            return Err(Unsupported::Format("fragmented MP4".into()));
        }
        let mut video = None;
        let mut audio = None;
        let mut other_video = None;
        let mut ids: Vec<u32> = reader.tracks().keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let Some(track) = reader.tracks().get(&id) else { continue };
            match (track.track_type(), track.media_type()) {
                (Ok(TrackType::Video), Ok(MediaType::H264)) if video.is_none() => {
                    let Some(avc1) = &track.trak.mdia.minf.stbl.stsd.avc1 else { continue };
                    let mut header = Vec::new();
                    for nal in avc1.avcc.sequence_parameter_sets.iter().chain(&avc1.avcc.picture_parameter_sets) {
                        header.extend_from_slice(&START_CODE);
                        header.extend_from_slice(&nal.bytes);
                    }
                    video = Some(VideoTrack {
                        samples: Samples::from_track(track),
                        nal_len: usize::from(avc1.avcc.length_size_minus_one & 3) + 1,
                        header,
                        frame_rate: track.frame_rate(),
                    });
                }
                (Ok(TrackType::Video), media) => {
                    other_video.get_or_insert(match media {
                        Ok(media) => media.to_string(),
                        Err(_) => "an unknown video codec".to_owned(),
                    });
                }
                (Ok(TrackType::Audio), Ok(MediaType::AAC)) if audio.is_none() => {
                    let Some(mp4a) = &track.trak.mdia.minf.stbl.stsd.mp4a else { continue };
                    let Some(esds) = &mp4a.esds else { continue };
                    let spec = &esds.es_desc.dec_config.dec_specific;
                    // Plain AAC (LC) and its relatives; SBR/PS files decode
                    // their core, at the core's rate.
                    let object = match spec.profile {
                        0 => 2,
                        p => p.min(31),
                    };
                    let rate_index = spec.freq_index.min(12);
                    let channels = match spec.chan_conf {
                        0 => mp4a.channelcount.clamp(1, 2),
                        7 => 8,
                        c => u16::from(c),
                    };
                    let config = vec![
                        (object << 3) | (rate_index >> 1),
                        ((rate_index & 1) << 7) | ((spec.chan_conf & 0x0f) << 3),
                    ];
                    audio = Some(AudioTrack {
                        samples: Samples::from_track(track),
                        rate: RATES[usize::from(rate_index)],
                        channels,
                        config,
                    });
                }
                _ => {}
            }
        }
        let Some(video) = video else {
            return Err(Unsupported::Format(match other_video {
                Some(codec) => codec,
                // hvc1 (HEVC as phones record it) is not even listed by the crate.
                None => "HEVC or another codec".to_owned(),
            }));
        };
        if video.samples.len() == 0 {
            return Err(Unsupported::Broken("no pictures".into()));
        }
        let (width, height) = reader
            .tracks()
            .get(&video.samples.id)
            .map_or((0, 0), |t| (u32::from(t.width()), u32::from(t.height())));
        let frame = if video.frame_rate > 0.0 { 1.0 / video.frame_rate } else { 0.0 };
        let mut end = video.samples.end() + frame;
        if let Some(audio) = &audio {
            end = end.max(audio.samples.end() + 1024.0 / f64::from(audio.rate.max(1)));
        }
        let movie = reader.duration();
        let duration = if movie > Duration::ZERO {
            movie.min(Duration::from_secs_f64(end.max(0.0)))
        } else {
            Duration::from_secs_f64(end.max(0.0))
        };
        let info = Info {
            width,
            height,
            duration,
            has_audio: audio.is_some(),
        };
        Ok(Self {
            reader,
            video,
            audio,
            info,
        })
    }

    /// A video sample (0-based) as Annex B, with SPS/PPS in front of a sync
    /// sample.
    pub fn video_sample(&mut self, index: u32) -> Option<Vec<u8>> {
        let id = self.video.samples.id;
        let sample = self.reader.read_sample(id, index + 1).ok().flatten()?;
        let mut out = Vec::with_capacity(sample.bytes.len() + self.video.header.len() + 16);
        if sample.is_sync || index == 0 {
            out.extend_from_slice(&self.video.header);
        }
        let mut rest: &[u8] = &sample.bytes;
        let n = self.video.nal_len;
        while rest.len() > n {
            let len = rest[..n].iter().fold(0usize, |acc, b| (acc << 8) | usize::from(*b));
            let Some(nal) = rest.get(n..n + len) else { break };
            out.extend_from_slice(&START_CODE);
            out.extend_from_slice(nal);
            rest = &rest[n + len..];
        }
        Some(out)
    }

    /// An audio sample (0-based): one AAC frame.
    pub fn audio_sample(&mut self, index: u32) -> Option<Vec<u8>> {
        let id = self.audio.as_ref()?.samples.id;
        let sample = self.reader.read_sample(id, index + 1).ok().flatten()?;
        Some(sample.bytes.to_vec())
    }
}
