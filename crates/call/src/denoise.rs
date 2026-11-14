//! Strong noise suppression: DeepFilterNet 3 (libDF, run with tract), at
//! 48 kHz in 10 ms frames, on this device's microphone before it is sent.
//!
//! Tuned so a voice trailing off fades instead of chopping. Out of the box
//! the model takes a frame down 100 dB (a zero mask) whenever it guesses
//! the frame's speech-to-noise ratio is under -10 dB, and the quiet end of
//! a sentence 6-12 dB above the noise was guessed so, frame by frame: it
//! came through in bits. Here no frame goes down more than
//! [`ATTEN_LIM_DB`]; a frame clearly louder than the noise floor keeps at
//! least part of itself ([`SPEECH_OVER`], [`SPEECH_KEEP`]); and the
//! frame's overall gain falls at most [`RELEASE_DB`] per frame. What is
//! kept beyond the model's output is mixed back from the frame as it came
//! in.

/// How late the cleaned voice comes out, in samples (30 ms).
pub const DELAY: usize = 1440;

/// The most a frame is taken down by (the model alone: 100 dB).
const ATTEN_LIM_DB: f32 = 40.0;
/// How fast a frame's gain may fall from one frame to the next.
const RELEASE_DB: f32 = 2.0;
/// A frame with more than this times the noise floor's energy is speech...
const SPEECH_OVER: f32 = 4.0;
/// ...and keeps at least this much (amplitude, -3 dB) of what is above it
/// (`(1 - SPEECH_OVER * floor / energy).sqrt()`).
const SPEECH_KEEP: f32 = 0.7;
/// How fast the noise floor (the quietest frame lately) rises: 3 dB/s.
const FLOOR_RISE_DB: f32 = 0.03;

/// 10 ms at 48 kHz.
pub const FRAME: usize = 480;

/// The suppressor, with its state (it follows the voice over time).
pub struct Denoiser {
    model: df::tract::DfTract,
    input: ndarray::Array2<f32>,
    output: ndarray::Array2<f32>,
    /// The frames that went in, [`DELAY`] long: the one the output matches
    /// is the oldest.
    history: std::collections::VecDeque<f32>,
    /// The last frame's gain (amplitude, as sent).
    gain: f32,
    /// The noise floor: the quietest frame energy lately (rises 3 dB/s).
    noise: f32,
}

impl Denoiser {
    /// Loads the model (built in; about 0.2 s).
    pub fn new() -> Result<Self, String> {
        let params = df::tract::DfParams::default();
        let runtime = df::tract::RuntimeParams::default_with_ch(1).with_atten_lim(ATTEN_LIM_DB);
        let model = df::tract::DfTract::new(params, &runtime).map_err(|err| format!("{err:#}"))?;
        if model.hop_size != FRAME || model.sr != 48_000 {
            return Err(format!(
                "the model works in {} samples at {} Hz",
                model.hop_size, model.sr
            ));
        }
        Ok(Self {
            model,
            input: ndarray::Array2::zeros((1, FRAME)),
            output: ndarray::Array2::zeros((1, FRAME)),
            history: std::iter::repeat_n(0.0, DELAY).collect(),
            gain: 1.0,
            noise: 0.0,
        })
    }

    /// One 10 ms frame (480 samples, full scale ±1), cleaned in place; what
    /// comes out is [`DELAY`] behind what went in. A frame the model fails
    /// on goes out as it came.
    pub fn process(&mut self, frame: &mut [f32]) {
        if frame.len() != FRAME {
            return;
        }
        self.input
            .iter_mut()
            .zip(frame.iter())
            .for_each(|(i, s)| *i = *s);
        self.history.extend(frame.iter());
        let came: Vec<f32> = self.history.drain(..FRAME).collect();
        if self
            .model
            .process(self.input.view(), self.output.view_mut())
            .is_err()
        {
            return;
        }
        // The model's gain for the frame, then the least it may have.
        let energy = |x: &mut dyn Iterator<Item = f32>| x.map(|s| s * s).sum::<f32>();
        let before = energy(&mut came.iter().copied());
        let after = energy(&mut self.output.iter().copied());
        let gain = if before > 1e-9 {
            (after / before).sqrt().min(1.0)
        } else {
            1.0
        };
        if before > 1e-9 && (self.noise == 0.0 || before < self.noise) {
            self.noise = before;
        } else {
            self.noise *= 10f32.powf(FLOOR_RISE_DB / 10.0);
        }
        let speech = if before > 1e-9 {
            (1.0 - SPEECH_OVER * self.noise / before).max(0.0).sqrt() * SPEECH_KEEP
        } else {
            0.0
        };
        let least = (self.gain * 10f32.powf(-RELEASE_DB / 20.0)).max(speech);
        let mix = (least - gain).max(0.0);
        self.gain = gain.max(least);
        frame
            .iter_mut()
            .zip(self.output.iter().zip(&came))
            .for_each(|(s, (o, c))| *s = (o + mix * c).clamp(-1.0, 1.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imp::speech_fixture::{change_db, noisy_speech};

    #[test]
    fn strong_suppression_takes_noise_out_of_pauses_and_keeps_the_voice() {
        let speech = noisy_speech(10, 0.05, 0.03);
        let mix = speech.mix();
        let mut denoiser = Denoiser::new().expect("model");
        let mut out = mix.clone();
        for frame in out.chunks_exact_mut(FRAME) {
            denoiser.process(frame);
        }
        let pauses = change_db(&mix, &out, &speech.pause, DELAY);
        let voice = change_db(&mix, &out, &speech.speech, DELAY);
        eprintln!("strong: pauses {pauses:.1} dB, voice {voice:.1} dB");
        assert!(
            pauses <= -10.0,
            "noise between phrases only {pauses:.1} dB down"
        );
        assert!(voice >= -3.0, "the voice lost {voice:.1} dB (over half)");
    }
}
