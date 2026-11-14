// Copyright 2025 LiveKit, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::fmt::Debug;

use crate::{
    imp::audio_track as imp_at,
    media_stream_track::{media_stream_track, RtcTrackState},
};

#[derive(Clone)]
pub struct RtcAudioTrack {
    pub(crate) handle: imp_at::RtcAudioTrack,
}

impl RtcAudioTrack {
    media_stream_track!();

    /// Morpheus patch: how loud a remote track plays (0 to 10, 1 as sent;
    /// WebRTC applies it after the track's sinks).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_volume(&self, volume: f64) {
        self.handle.set_volume(volume.clamp(0.0, 10.0));
    }
}

impl Debug for RtcAudioTrack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RtcAudioTrack")
            .field("id", &self.id())
            .field("enabled", &self.enabled())
            .field("state", &self.state())
            .finish()
    }
}
