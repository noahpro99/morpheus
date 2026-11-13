//! Strong noise suppression: DeepFilterNet 3 (libDF, run with tract), at
//! 48 kHz in 10 ms frames, on this device's microphone before it is sent.

/// How late the cleaned voice comes out, in samples (30 ms).
#[cfg_attr(not(test), allow(dead_code))]
pub const DELAY: usize = 1440;

/// 10 ms at 48 kHz.
pub const FRAME: usize = 480;

/// The suppressor, with its state (it follows the voice over time).
pub struct Denoiser {
    model: df::tract::DfTract,
    input: ndarray::Array2<f32>,
    output: ndarray::Array2<f32>,
}

impl Denoiser {
    /// Loads the model (built in; about 0.2 s).
    pub fn new() -> Result<Self, String> {
        let params = df::tract::DfParams::default();
        let runtime = df::tract::RuntimeParams::default_with_ch(1);
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
        if self
            .model
            .process(self.input.view(), self.output.view_mut())
            .is_ok()
        {
            frame
                .iter_mut()
                .zip(self.output.iter())
                .for_each(|(s, o)| *s = o.clamp(-1.0, 1.0));
        }
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
