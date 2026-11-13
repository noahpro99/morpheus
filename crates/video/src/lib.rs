//! Video playback for the desktop app: MP4 files with H.264 pictures and
//! AAC sound (what phones and most Matrix clients send), demuxed by the
//! `mp4` crate, pictures decoded by OpenH264 (built from source, no system
//! library), sound by Symphonia's AAC decoder and played through rodio;
//! each picture is shown when the sound's clock reaches it.
//!
//! Other formats (WebM with VP8/VP9 and Opus, HEVC, fragmented MP4) are
//! [`Unsupported`]: the app offers the file for download instead.

use std::time::Duration;

mod decode;
mod demux;
mod player;

pub use decode::Decoder;
pub use player::{Options, Player, Status};

/// What a playable file holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub width: u32,
    pub height: u32,
    pub duration: Duration,
    /// Whether it has sound this can play.
    pub has_audio: bool,
}

/// Why a file cannot be played.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unsupported {
    /// A format or codec not built in (WebM, HEVC, ...): which.
    Format(String),
    /// Not readable as what it claims to be.
    Broken(String),
}

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Format(what) => write!(f, "can't play this format ({what})"),
            Self::Broken(why) => write!(f, "can't play this video ({why})"),
        }
    }
}

/// Reads what a file holds without decoding it.
pub fn probe(bytes: &[u8]) -> Result<Info, Unsupported> {
    demux::sniff(bytes)?;
    demux::Demux::open(std::sync::Arc::from(bytes)).map(|d| d.info)
}

/// A decoded picture: B, G, R, A bytes, `width * height * 4` of them.
#[derive(Clone, Debug)]
pub struct Frame {
    /// When it is shown, from the start of the video.
    pub pts: Duration,
    /// Pictures shown so far by a [`Player`], this one included (0 straight
    /// from a [`Decoder`]).
    pub index: u64,
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// Decoded sound: interleaved samples from `pts` on.
#[derive(Clone, Debug)]
pub struct AudioChunk {
    pub pts: Duration,
    pub rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}
