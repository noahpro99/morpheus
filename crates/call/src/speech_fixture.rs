//! Noisy speech made in code, for testing the noise suppression: a small
//! formant synthesizer (an LF-like glottal pulse with jitter and shimmer,
//! five formants gliding between six vowels, fricative consonants, falling
//! intonation) speaking 0.2 s syllables in 1.4 s phrases with 0.6 s pauses,
//! over white and pink noise. No recording is needed (none with a free
//! licence is in the repository), and it is the same on every run.

pub const SR: usize = 48_000;
/// 10 ms.
pub const HOP: usize = 480;

#[derive(Clone, Copy, Default)]
struct Resonator {
    a1: f32,
    a2: f32,
    g: f32,
    y1: f32,
    y2: f32,
}

impl Resonator {
    /// A formant at `f` Hz, `bw` Hz wide (Klatt's, unity gain at 0 Hz).
    fn set(&mut self, f: f32, bw: f32) {
        let r = (-std::f32::consts::PI * bw / SR as f32).exp();
        let theta = std::f32::consts::TAU * f / SR as f32;
        self.a1 = 2.0 * r * theta.cos();
        self.a2 = -r * r;
        self.g = 1.0 - self.a1 - self.a2;
    }

    fn run(&mut self, x: f32) -> f32 {
        let y = self.g * x + self.a1 * self.y1 + self.a2 * self.y2;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

pub struct NoisySpeech {
    pub voice: Vec<f32>,
    pub noise: Vec<f32>,
    /// Per 10 ms frame: the middle of a vowel.
    pub speech: Vec<bool>,
    /// Per 10 ms frame: a pause between phrases (only noise).
    pub pause: Vec<bool>,
}

impl NoisySpeech {
    /// What a microphone would hear: the voice over the noise.
    pub fn mix(&self) -> Vec<f32> {
        self.voice
            .iter()
            .zip(&self.noise)
            .map(|(v, n)| v + n)
            .collect()
    }
}

/// F1-F3 of /a/ /i/ /u/ /e/ /o/ /æ/ (Peterson and Barney).
const VOWELS: [[f32; 3]; 6] = [
    [730.0, 1090.0, 2440.0],
    [270.0, 2290.0, 3010.0],
    [300.0, 870.0, 2240.0],
    [530.0, 1840.0, 2480.0],
    [570.0, 840.0, 2410.0],
    [660.0, 1720.0, 2410.0],
];

/// `secs` of speech at `voice_gain`, over noise at `noise_gain`.
pub fn noisy_speech(secs: usize, voice_gain: f32, noise_gain: f32) -> NoisySpeech {
    let n = secs * SR;
    let mut state = 0x1234_5678u32;
    let mut random = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        (state as f32 / u32::MAX as f32) * 2.0 - 1.0
    };
    let mut formants = [Resonator::default(); 5];
    let mut hiss = Resonator::default();
    let (mut b0, mut b1, mut b2) = (0f32, 0f32, 0f32);
    let (mut voice, mut noise) = (vec![0f32; n], vec![0f32; n]);
    let (mut t0, mut period, mut amp, mut smooth) = (0f32, 400f32, 1f32, 0f32);
    let mut now = VOWELS[0];
    for i in 0..n {
        let t = i as f32 / SR as f32;
        // Pink noise (Paul Kellet's filter) and some white.
        let white = random();
        b0 = 0.99765 * b0 + white * 0.099_046;
        b1 = 0.96300 * b1 + white * 0.296_516_4;
        b2 = 0.57000 * b2 + white * 1.052_691_3;
        let pink = (b0 + b1 + b2 + white * 0.1848) * 0.2;
        noise[i] = noise_gain * (0.3 * random() + pink);

        let in_phrase = t % 2.0;
        let phrase = (t / 2.0) as usize;
        let syllable = (in_phrase / 0.2) as usize;
        let in_syllable = in_phrase % 0.2;
        let talking = in_phrase < 1.4;
        // Odd syllables start with 40 ms of a fricative.
        let onset = if syllable % 2 == 1 { 0.04 } else { 0.0 };
        let consonant = talking && in_syllable < onset;
        let envelope = if talking && !consonant {
            let x = (in_syllable - onset) / (0.2 - onset);
            (std::f32::consts::PI * x).sin().max(0.0).powf(0.4)
        } else {
            0.0
        };
        let target = VOWELS[(syllable * 7 + phrase * 3) % VOWELS.len()];
        if i % 48 == 0 {
            for (f, goal) in now.iter_mut().zip(target) {
                *f += (goal - *f) * 0.04;
            }
            let centres = [now[0], now[1], now[2], 3300.0, 3750.0];
            let widths = [60.0, 90.0, 120.0, 200.0, 250.0];
            for ((formant, f), bw) in formants.iter_mut().zip(centres).zip(widths) {
                formant.set(f, bw);
            }
            hiss.set(4500.0 + 1500.0 * ((syllable / 2) % 2) as f32, 1500.0);
        }
        // Falling over the phrase, rising or falling over each syllable.
        let lilt = if syllable % 3 == 0 { -1.0 } else { 1.0 };
        let f0 = 125.0
            + 35.0 * (1.0 - in_phrase / 1.4)
            + 12.0 * lilt * (std::f32::consts::PI * in_syllable / 0.2).sin();
        t0 += 1.0;
        if t0 >= period {
            t0 -= period;
            period = SR as f32 / f0 * (1.0 + 0.01 * random());
            amp = 1.0 + 0.05 * random();
        }
        // Glottal flow derivative: an opening phase, then a sharp closure.
        let x = t0 / period;
        let glottal = if x < 0.6 {
            (std::f32::consts::PI * x / 0.6).sin() * (x / 0.6)
        } else if x < 0.66 {
            -((x - 0.6) / 0.06) * 3.0
        } else {
            0.0
        };
        smooth = 0.7 * smooth + 0.3 * (amp * glottal + 0.03 * random());
        let mut vowel = 0.0;
        for (formant, gain) in formants.iter_mut().zip([1.0, 0.7, 0.4, 0.2, 0.1]) {
            vowel += formant.run(smooth) * gain;
        }
        let fricative = if consonant {
            hiss.run(random()) * 0.6 * (std::f32::consts::PI * in_syllable / 0.04).sin()
        } else {
            hiss.run(0.0)
        };
        voice[i] = voice_gain * (envelope * vowel + fricative);
    }
    let frames = n / HOP;
    let at = |f: usize| (f * HOP) as f32 / SR as f32 % 2.0;
    let speech = (0..frames)
        .map(|f| {
            let in_syllable = at(f) % 0.2;
            at(f) < 1.4 && in_syllable > 0.07 && in_syllable < 0.16
        })
        .collect();
    let pause = (0..frames).map(|f| at(f) > 1.5).collect();
    NoisySpeech {
        voice,
        noise,
        speech,
        pause,
    }
}

fn energy(x: &[f32]) -> f64 {
    x.iter().map(|v| f64::from(*v).powi(2)).sum()
}

/// How much louder (dB) `out` is than `input` over the frames marked in
/// `frames`, `out` read `delay` samples later. The first second is left out
/// (adaptive processing settles).
pub fn change_db(input: &[f32], out: &[f32], frames: &[bool], delay: usize) -> f64 {
    let (mut before, mut after) = (0.0, 0.0);
    for (f, marked) in frames.iter().enumerate().skip(100) {
        let a = f * HOP;
        if !marked || a + HOP + delay > out.len() {
            continue;
        }
        before += energy(&input[a..a + HOP]);
        after += energy(&out[a + delay..a + delay + HOP]);
    }
    10.0 * (after / before).log10()
}

/// Speech that trails off, as the quiet end of a sentence: [`noisy_speech`]
/// whose phrases keep `voice_gain` for 0.5 s, then fall 30 dB over the
/// next 0.9 s (to their end).
pub struct FadingSpeech {
    pub voice: Vec<f32>,
    pub noise: Vec<f32>,
    /// Per 10 ms frame: the middle of a vowel in a fade.
    pub tail: Vec<bool>,
}

impl FadingSpeech {
    pub fn mix(&self) -> Vec<f32> {
        self.voice
            .iter()
            .zip(&self.noise)
            .map(|(v, n)| v + n)
            .collect()
    }
}

pub fn fading_speech(secs: usize, voice_gain: f32, noise_gain: f32) -> FadingSpeech {
    const HOLD: f32 = 0.5;
    const FADE: f32 = 0.9;
    const FADE_DB: f32 = 30.0;
    let speech = noisy_speech(secs, voice_gain, noise_gain);
    let at = |i: usize| i as f32 / SR as f32 % 2.0;
    let voice = speech
        .voice
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let into = (at(i) - HOLD).clamp(0.0, FADE);
            v * 10f32.powf(-FADE_DB * into / FADE / 20.0)
        })
        .collect();
    let tail = speech
        .speech
        .iter()
        .enumerate()
        .map(|(f, vowel)| *vowel && at(f * HOP) >= HOLD)
        .collect();
    FadingSpeech {
        voice,
        noise: speech.noise,
        tail,
    }
}

/// Energy of `x`'s 10 ms frame `f`, `delay` samples later.
pub fn frame_energy(x: &[f32], f: usize, delay: usize) -> f64 {
    let a = f * HOP + delay;
    x.get(a..a + HOP).map_or(0.0, energy)
}

/// Keyboard typing: a click at irregular gaps (80-260 ms), each a key going
/// down (a 3 ms broadband burst over a short 300 Hz thump) and, 90 ms
/// later, the key coming up (a quieter, shorter one). `peak` is the loudest
/// sample of a key-down.
pub struct Typing {
    pub clicks: Vec<f32>,
    /// Per 10 ms frame: a click (or its tail) is sounding.
    pub sounding: Vec<bool>,
}

pub fn typing(secs: usize, peak: f32, seed: u32) -> Typing {
    let n = secs * SR;
    let mut state = seed | 1;
    fn random(state: &mut u32) -> f32 {
        *state ^= *state << 13;
        *state ^= *state >> 17;
        *state ^= *state << 5;
        (*state as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
    let mut clicks = vec![0f32; n];
    let burst = |state: &mut u32, out: &mut [f32], at: usize, amp: f32, tau_ms: f32| {
        let tau = tau_ms * SR as f32 / 1000.0;
        let (mut lp, mut prev) = (0f32, 0f32);
        for k in 0..(tau * 6.0) as usize {
            let Some(slot) = out.get_mut(at + k) else { break };
            let env = (-(k as f32) / tau).exp();
            let white = random(state);
            // High-passed noise (keys are bright) over a low thump.
            let hp = white - prev;
            prev = white;
            lp += 0.03 * (white - lp);
            let thump = (std::f32::consts::TAU * 300.0 * k as f32 / SR as f32).sin() * (-(k as f32) / (tau * 2.0)).exp();
            *slot += amp * (0.7 * hp * env + 0.5 * thump + 2.0 * lp * env * 0.2);
        }
    };
    let mut at = SR / 5;
    while at < n {
        let amp = peak * (0.6 + 0.4 * random(&mut state).abs());
        burst(&mut state, &mut clicks, at, amp, 1.0);
        burst(&mut state, &mut clicks, at + SR * 9 / 100, amp * 0.4, 0.7);
        at += SR * 80 / 1000 + ((random(&mut state).abs()) * SR as f32 * 0.18) as usize;
    }
    let sounding = (0..n / HOP)
        .map(|f| clicks[f * HOP..(f + 1) * HOP].iter().any(|v| v.abs() > peak * 0.02))
        .collect();
    Typing { clicks, sounding }
}
