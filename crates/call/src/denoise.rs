//! Strong noise suppression: DeepFilterNet 3 (libDF, run with tract), at
//! 48 kHz in 10 ms frames, on this device's microphone before it is sent.
//!
//! Tuned so a voice trailing off fades instead of chopping. Out of the box
//! the model takes a frame down 100 dB (a zero mask) whenever it guesses
//! the frame's speech-to-noise ratio is under -10 dB, and the quiet end of
//! a sentence 6-12 dB above the noise was guessed so, frame by frame: it
//! came through in bits. Here no frame goes down more than
//! the attenuation limit (40 dB by default); a frame clearly louder than the noise floor keeps at
//! least part of itself ([`SPEECH_OVER`], the floor margin); and the
//! frame's overall gain falls at most the fall rate (2 dB) per frame. What is
//! kept beyond the model's output is mixed back from the frame as it came
//! in.

/// How late the cleaned voice comes out, in samples (30 ms).
pub const DELAY: usize = crate::noise::DFN_DELAY;

use crate::noise::NoiseTuning;

/// A frame with more than this times the noise floor's energy is speech...
const SPEECH_OVER: f32 = 4.0;
// ...and keeps at least `tuning.floor_margin_db` (-3 dB by default) of what
// is above it (`(1 - SPEECH_OVER * floor / energy).sqrt()`).
/// How fast the noise floor (the quietest frame lately) rises: 3 dB/s.
const FLOOR_RISE_DB: f32 = 0.03;

/// 10 ms at 48 kHz.
pub const FRAME: usize = 480;

/// After frames it missed, the model's next [`DELAY`] of output is of the
/// frames before them (its look-ahead): not used.
const WARM_UP: usize = DELAY / FRAME;

/// The suppressor, with its state (it follows the voice over time).
pub struct Denoiser {
    model: df::tract::DfTract,
    input: ndarray::Array2<f32>,
    output: ndarray::Array2<f32>,
    /// The frames that went in, [`DELAY`] long: the one the output matches
    /// is the oldest.
    history: std::collections::VecDeque<f32>,
    /// What the model and the smoothing are tuned to now.
    tuning: NoiseTuning,
    /// The last frame's gain (amplitude, as sent).
    gain: f32,
    /// The noise floor: the quietest frame energy lately (rises 3 dB/s).
    noise: f32,
    /// Frames the model has cleaned in a row (since [`Denoiser::pass`]).
    run: usize,
}

impl Denoiser {
    /// Loads the model (built in; about 0.2 s).
    pub fn new(tuning: &NoiseTuning) -> Result<Self, String> {
        let params = df::tract::DfParams::default();
        let runtime = df::tract::RuntimeParams::default_with_ch(1);
        let model = df::tract::DfTract::new(params, &runtime).map_err(|err| format!("{err:#}"))?;
        if model.hop_size != FRAME || model.sr != 48_000 {
            return Err(format!(
                "the model works in {} samples at {} Hz",
                model.hop_size, model.sr
            ));
        }
        let mut denoiser = Self {
            model,
            input: ndarray::Array2::zeros((1, FRAME)),
            output: ndarray::Array2::zeros((1, FRAME)),
            history: std::iter::repeat_n(0.0, DELAY).collect(),
            gain: 1.0,
            noise: 0.0,
            run: WARM_UP + 1,
            tuning: *tuning,
        };
        denoiser.tune(tuning);
        Ok(denoiser)
    }

    /// Tune the model and the smoothing from the next frame (the model keeps
    /// its state, so a voice in the middle of a word is not cut).
    pub fn tune(&mut self, tuning: &NoiseTuning) {
        let tuning = tuning.clamped();
        self.model.set_atten_lim(tuning.atten_limit_db);
        self.model
            .set_pf_beta(if tuning.post_filter { tuning.post_filter_beta } else { 0.0 });
        self.model.min_db_thresh = tuning.min_snr_db;
        self.model.max_db_erb_thresh = tuning.max_erb_snr_db;
        self.model.max_db_df_thresh = tuning.max_df_snr_db;
        self.tuning = tuning;
    }

    /// What the model itself is set to, read back from it (for tests).
    #[cfg(test)]
    pub fn model_settings(&self) -> (Option<f32>, bool, f32, f32, f32, f32) {
        let m = &self.model;
        (m.atten_lim, m.post_filter, m.post_filter_beta, m.min_db_thresh, m.max_db_erb_thresh, m.max_db_df_thresh)
    }

    /// The smoothing's settings: (floor margin as amplitude, fall per frame, dB).
    #[cfg(test)]
    pub fn smoothing(&self) -> (f32, f32) {
        (self.keep(), self.tuning.max_fall_db)
    }

    /// What a frame over the floor keeps, as amplitude.
    fn keep(&self) -> f32 {
        10f32.powf(self.tuning.floor_margin_db / 20.0)
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
        self.run += 1;
        let before = energy(&came);
        self.follow_noise(before);
        if self.run <= WARM_UP {
            // The model's output is still of frames from before the gap.
            self.hold(frame, &came, before);
            return;
        }
        // The model's gain for the frame, then the least it may have.
        let after = energy(self.output.as_slice().unwrap_or_default());
        let gain = if before > 1e-9 {
            (after / before).sqrt().min(1.0)
        } else {
            1.0
        };
        let least = (self.gain * self.fall()).max(self.speech(before) * self.keep());
        let mix = (least - gain).max(0.0);
        self.gain = gain.max(least);
        frame
            .iter_mut()
            .zip(self.output.iter().zip(&came))
            .for_each(|(s, (o, c))| *s = (o + mix * c).clamp(-1.0, 1.0));
    }

    /// One frame without the model, when the capture is behind and must
    /// catch up (#101): as late as the model's frames, so nothing repeats
    /// or jumps, and kept as speech or lowered as noise by its energy over
    /// the noise floor alone. The model misses the frame, so the next
    /// [`WARM_UP`] frames it cleans (its output is still of frames before
    /// the gap) go out this way too.
    pub fn pass(&mut self, frame: &mut [f32]) {
        if frame.len() != FRAME {
            return;
        }
        self.history.extend(frame.iter());
        let came: Vec<f32> = self.history.drain(..FRAME).collect();
        let before = energy(&came);
        self.follow_noise(before);
        self.run = 0;
        self.hold(frame, &came, before);
    }

    /// `came` out as `frame`, at the gain its energy over the noise floor
    /// gives (a voice keeps all of itself, noise goes down to the limit),
    /// falling no faster than the fall rate.
    fn hold(&mut self, frame: &mut [f32], came: &[f32], before: f32) {
        let limit = 10f32.powf(-self.tuning.atten_limit_db / 20.0);
        self.gain = (self.gain * self.fall()).max(self.speech(before)).max(limit).min(1.0);
        frame
            .iter_mut()
            .zip(came)
            .for_each(|(s, c)| *s = (c * self.gain).clamp(-1.0, 1.0));
    }

    /// The noise floor follows the quietest frame lately.
    fn follow_noise(&mut self, before: f32) {
        if before > 1e-9 && (self.noise == 0.0 || before < self.noise) {
            self.noise = before;
        } else {
            self.noise *= 10f32.powf(FLOOR_RISE_DB / 10.0);
        }
    }

    /// How much of a frame of energy `before` is above the noise floor
    /// (amplitude; 0 for noise).
    fn speech(&self, before: f32) -> f32 {
        if before > 1e-9 {
            (1.0 - SPEECH_OVER * self.noise / before).max(0.0).sqrt()
        } else {
            0.0
        }
    }

    /// The most the gain falls per frame (amplitude).
    fn fall(&self) -> f32 {
        10f32.powf(-self.tuning.max_fall_db / 20.0)
    }
}

fn energy(x: &[f32]) -> f32 {
    x.iter().map(|s| s * s).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imp::speech_fixture::{change_db, noisy_speech};

    #[test]
    fn strong_suppression_takes_noise_out_of_pauses_and_keeps_the_voice() {
        let speech = noisy_speech(10, 0.05, 0.03);
        let mix = speech.mix();
        let mut denoiser = Denoiser::new(&NoiseTuning::default()).expect("model");
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

    /// Frames passed without the model (the capture catching up, #101) keep
    /// the voice, as late as the model's frames, and the frames after them
    /// do not replay audio from before the gap: every frame of speech out
    /// matches the speech that went in [`DELAY`] earlier.
    #[test]
    fn frames_passed_without_the_model_keep_the_voice_in_time() {
        let speech = noisy_speech(10, 0.05, 0.03);
        let mix = speech.mix();
        let mut denoiser = Denoiser::new(&NoiseTuning::default()).expect("model");
        let mut out = mix.clone();
        for (i, frame) in out.chunks_exact_mut(FRAME).enumerate() {
            // 5 of every 40 frames passed, in a row.
            if i % 40 >= 35 {
                denoiser.pass(frame);
            } else {
                denoiser.process(frame);
            }
        }
        let voice = change_db(&mix, &out, &speech.speech, DELAY);
        let pauses = change_db(&mix, &out, &speech.pause, DELAY);
        eprintln!("with passes: pauses {pauses:.1} dB, voice {voice:.1} dB");
        assert!(voice >= -3.0, "the voice lost {voice:.1} dB");
        assert!(pauses <= -6.0, "noise between phrases only {pauses:.1} dB down");
        // Each frame out is the frame that came DELAY before it, the ones
        // just after the passes (the model starting again) too: compared
        // where the voice is loud.
        let loud = 4.0 * energy(&speech.noise[..FRAME]) + 0.1;
        let mut worst = (1.0f32, 0);
        for f in DELAY / FRAME..out.len() / FRAME {
            let start = f * FRAME;
            let o = &out[start..start + FRAME];
            let i = &mix[start - DELAY..start - DELAY + FRAME];
            let dot: f32 = o.iter().zip(i).map(|(a, b)| a * b).sum();
            let norm = (energy(o) * energy(i)).sqrt();
            if energy(i) > loud && norm > 0.0 && dot / norm < worst.0 {
                worst = (dot / norm, f);
            }
        }
        eprintln!("worst {worst:?}");
        assert!(worst.0 > 0.6, "frame {} out matches what came only {:.2}", worst.1, worst.0);
    }
}
