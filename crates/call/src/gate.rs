//! Input sensitivity, as in Discord: a gate on the microphone that lets the
//! voice through and keeps out what is not speech between words, such as a
//! keyboard (#332). The noise models take out steady noise and some clicks;
//! a loud click is still louder than the noise floor, so the gate tells it
//! from a voice by how long it lasts: a key is over in 10-20 ms, a voice
//! stays over the threshold for at least [`MIN_RUN`] frames (30 ms).
//!
//! It looks [`LOOKAHEAD`] frames ahead, so the gate is already open when a
//! word starts (one frame before), and stays open for [`HOLD`] after the last
//! loud frame of a voice so a trailing syllable or a breath is not cut. A
//! click is let through while it is within a word's hold, and not between
//! words. Closed, it lowers the signal by [`CLOSED_DB`] (not to silence: the
//! room's own sound keeps going, so there is no pumping in and out of dead
//! air, and Opus's DTX has nothing to flip on).
//!
//! Automatic (the default): the threshold follows the quietest the input has
//! been lately, [`AUTO_MARGIN_DB`] over it. Manual: a threshold in dBFS, set
//! over the level meter.

use std::collections::VecDeque;

/// Frames of 10 ms the gate looks ahead, and so delays the voice by.
pub const LOOKAHEAD: usize = 3;
/// Frames over the threshold in a row that make a voice (a key is under it).
pub const MIN_RUN: usize = 3;
/// Frames the gate stays open after the voice's last loud one (150 ms).
pub const HOLD: usize = 15;
/// How far down a closed gate takes the signal.
pub const CLOSED_DB: f32 = -30.0;
/// Automatic: the threshold over the floor of the input's level.
pub const AUTO_MARGIN_DB: f32 = 6.0;
/// Automatic: the threshold never goes under this.
pub const AUTO_MIN_DB: f32 = -58.0;
/// A run without a pitch must be this far over the threshold to be a voice.
pub const UNVOICED_OVER_DB: f32 = 12.0;
const FLOOR_WINDOW: usize = 300;
/// The manual threshold's range, dBFS; at the bottom the gate is off.
pub const MIN_DB: f32 = -100.0;
pub const MAX_DB: f32 = 0.0;

/// A frame is sharp (a key, not a voice) when its peak is this many times its RMS.
pub const IMPULSIVE_CREST: f32 = 6.0;

/// Whether the frame is sharp: a click has a peak far over its RMS.
pub fn impulsive(frame: &[i16]) -> bool {
    let peak = frame.iter().map(|s| f32::from(*s).abs()).fold(0.0, f32::max);
    let rms = 10f32.powf(level_db(frame) / 20.0) * 32768.0;
    peak > IMPULSIVE_CREST * rms
}

/// A 10 ms frame's level, dBFS (RMS, full scale 32768).
pub fn level_db(frame: &[i16]) -> f32 {
    if frame.is_empty() {
        return MIN_DB;
    }
    let sum: f64 = frame.iter().map(|s| f64::from(*s).powi(2)).sum();
    let rms = (sum / frame.len() as f64).sqrt() / 32768.0;
    (20.0 * rms.max(1e-6).log10()) as f32
}

/// What the gate does with a frame: let it through (`open`) or not, and whether
/// it is a key (a sharp sound that is not in a voice).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verdict {
    pub open: bool,
    pub key: bool,
}

pub struct Gate {
    auto: bool,
    manual_db: f32,
    /// The levels of the frames not decided yet, the oldest first.
    ahead: VecDeque<(f32, bool, bool)>,
    /// The two frames before, for pitch (a voice is periodic over 30 ms).
    prev: Vec<i16>,
    /// The floor of the input's level, dBFS.
    floor_db: f32,
    recent: VecDeque<f32>,
    lows: VecDeque<f32>,
    /// In a run of loud frames that made a voice.
    in_run: bool,
    hold: usize,
    /// What the last frame was let through at (linear), for the ramps.
    gain: f32,
}

impl Default for Gate {
    fn default() -> Self {
        Self::new()
    }
}

impl Gate {
    pub fn new() -> Self {
        Self {
            auto: true,
            manual_db: -50.0,
            ahead: VecDeque::new(),
            prev: Vec::new(),
            floor_db: 0.0,
            recent: VecDeque::new(),
            lows: VecDeque::new(),
            in_run: false,
            hold: 0,
            gain: 1.0,
        }
    }

    pub fn set(&mut self, auto: bool, manual_db: f32) {
        self.auto = auto;
        self.manual_db = manual_db.clamp(MIN_DB, MAX_DB);
    }

    /// Whether the gate does anything: not with a manual threshold at the
    /// bottom of the meter (Discord's "off").
    pub fn active(auto: bool, manual_db: f32) -> bool {
        !legacy() && (auto || manual_db > MIN_DB + 0.5)
    }

    /// The threshold in use, dBFS.
    pub fn threshold_db(&self) -> f32 {
        if self.auto {
            (self.floor_db + AUTO_MARGIN_DB).max(AUTO_MIN_DB)
        } else {
            self.manual_db
        }
    }

    /// The level of the frame just captured. Returns whether the gate is
    /// open for the frame [`LOOKAHEAD`] frames before it (everything is open
    /// until there are that many). The voice must be delayed by as much.
    pub fn push(&mut self, level: f32, impulsive: bool) -> Verdict {
        self.push_features(level, impulsive, true)
    }

    /// [`Self::push`] for a frame: its level, sharpness and pitch measured here.
    pub fn push_frame(&mut self, frame: &[i16]) -> Verdict {
        let level = level_db(frame);
        let voiced = level > self.threshold_db() && voiced(&self.prev, frame);
        self.prev.extend_from_slice(frame);
        let extra = self.prev.len().saturating_sub(2 * frame.len());
        self.prev.drain(..extra);
        self.push_features(level, impulsive(frame), voiced)
    }

    /// The features of the frame just captured: its level (dBFS), whether it
    /// is sharp, whether it has a pitch.
    pub fn push_features(&mut self, level: f32, impulsive: bool, voiced: bool) -> Verdict {
        // The floor: the quietest the input has been over the last 3 s, each
        // reading averaged over 3 frames (one odd quiet frame, or a stall, must
        // not pull it down and leave the room's own sound over the threshold).
        self.recent.push_back(level.max(-90.0));
        if self.recent.len() > 3 {
            self.recent.pop_front();
        }
        let smooth = 10.0 * (self.recent.iter().map(|l| 10f32.powf(l / 10.0)).sum::<f32>() / self.recent.len() as f32).log10();
        self.lows.push_back(smooth);
        if self.lows.len() > FLOOR_WINDOW {
            self.lows.pop_front();
        }
        self.floor_db = self.lows.iter().copied().fold(f32::MAX, f32::min);
        self.ahead.push_back((level, impulsive, voiced));
        if self.ahead.len() <= LOOKAHEAD {
            return Verdict { open: true, key: false };
        }
        let threshold = self.threshold_db();
        let loud = |i: usize| self.ahead.get(i).is_some_and(|l| l.0 > threshold);
        // A voice starts at `s` when MIN_RUN frames from it are loud.
        // ...with a pitch in it, or all of them well over the threshold (an
        // s or f has none); a long steady noise or a slam has neither.
        let strong = |i: usize| self.ahead.get(i).is_some_and(|l| l.0 > threshold + UNVOICED_OVER_DB);
        let voiced_at = |i: usize| self.ahead.get(i).is_some_and(|l| l.2);
        let starts = |s: usize| {
            (s..s + MIN_RUN).all(&loud)
                && ((s..s + MIN_RUN).any(voiced_at) || (s..s + MIN_RUN).all(strong))
        };
        let now = loud(0);
        self.in_run = starts(0) || (self.in_run && now);
        if self.in_run || starts(1) {
            self.hold = HOLD;
        } else {
            self.hold = self.hold.saturating_sub(1);
        }
        // A sharp, short sound alone (not in a voice) is a key, even in a word's
        // hold: that frame goes down, the hold goes on.
        let key = now && !self.in_run && self.ahead[0].1;
        self.ahead.pop_front();
        Verdict { open: self.hold > 0 && !key, key }
    }

    /// `frame` let through (`open`) or lowered, ramped over the frame so
    /// there is no step: up in 1 ms, down in 40 ms.
    pub fn apply(&mut self, frame: &mut [i16], verdict: Verdict) {
        let open = verdict.open;
        let closed = 10f32.powf(CLOSED_DB / 20.0);
        let target = if open { 1.0 } else { closed };
        let up = 1.0 / 48.0;
        // A key is taken down at once; a voice ending fades over 40 ms (no step a
        // listener hears as chopping).
        let down = (1.0 - closed) / if verdict.key { 48.0 } else { 1920.0 };
        for s in frame {
            if self.gain < target {
                self.gain = (self.gain + up).min(target);
            } else if self.gain > target {
                self.gain = (self.gain - down).max(target);
            }
            *s = (f32::from(*s) * self.gain) as i16;
        }
    }

    /// Back to the start (a call begins, or the gate was off).
    pub fn reset(&mut self) {
        self.ahead.clear();
        self.recent.clear();
        self.lows.clear();
        self.in_run = false;
        self.hold = 0;
        self.gain = 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `levels` through; what each frame's gate was, aligned to the
    /// frames (the first LOOKAHEAD are for frames before the signal).
    fn run(gate: &mut Gate, levels: &[f32]) -> Vec<bool> {
        let mut out: Vec<bool> = levels.iter().map(|l| gate.push(*l, false).open).collect();
        // Flush the last frames with silence.
        for _ in 0..LOOKAHEAD {
            out.push(gate.push(-90.0, false).open);
        }
        out.split_off(LOOKAHEAD)
    }

    fn frames(parts: &[(f32, usize)]) -> Vec<f32> {
        parts.iter().flat_map(|(l, n)| std::iter::repeat_n(*l, *n)).collect()
    }

    #[test]
    fn a_click_between_words_is_kept_out_and_a_word_opens_the_gate_in_time() {
        let mut gate = Gate::new();
        // 1 s of room at -60, a word (25 frames at -25), 0.6 s of room with a
        // two-frame key in it, a second word.
        let mut levels = frames(&[(-60.0, 100), (-25.0, 25), (-60.0, 20), (-15.0, 2)]);
        let key_at = levels.len() - 2;
        levels.extend(frames(&[(-60.0, 40), (-25.0, 25), (-60.0, 50)]));
        let open = run(&mut gate, &levels);
        assert_eq!(open.len(), levels.len());
        assert!(!open[90], "closed in the room's sound");
        assert!(open[99], "open one frame before the word");
        assert!(open[100..125].iter().all(|o| *o), "open through the word");
        assert!(open[125..125 + HOLD - 1].iter().all(|o| *o), "held after it");
        assert!(!open[key_at], "the key is kept out");
        assert!(!open[key_at + 1], "also its second frame");
        let second = key_at + 2 + 40;
        assert!(open[second - 1], "the second word opens in time");
        assert!(open[second..second + 25].iter().all(|o| *o));
        assert!(!open[second + 25 + HOLD + 2], "closed again after the hold");
    }

    #[test]
    fn a_key_during_a_words_hold_goes_through_and_does_not_extend_it() {
        let mut gate = Gate::new();
        let levels = frames(&[(-60.0, 100), (-25.0, 20), (-60.0, 5), (-15.0, 1), (-60.0, 60)]);
        let open = run(&mut gate, &levels);
        // Open from the word (frame 99) until HOLD after its last frame (119).
        assert!(open[119 + HOLD - 1]);
        assert!(!open[119 + HOLD + 2], "the key at 125 did not hold it open");
    }

    #[test]
    fn a_manual_threshold_over_the_voice_keeps_it_out_and_under_it_lets_it_in() {
        let levels = frames(&[(-60.0, 60), (-30.0, 30), (-60.0, 60)]);
        let mut high = Gate::new();
        high.set(false, -20.0);
        assert!(run(&mut high, &levels).iter().all(|o| !o));
        let mut low = Gate::new();
        low.set(false, -40.0);
        let open = run(&mut low, &levels);
        assert!(open[60..90].iter().all(|o| *o) && !open[10]);
    }

    #[test]
    fn automatic_follows_the_room() {
        // A noisy room (-45): the same voice at -25 still opens it, and the
        // room alone does not.
        let mut gate = Gate::new();
        let levels = frames(&[(-45.0, 200), (-25.0, 25), (-45.0, 100)]);
        let open = run(&mut gate, &levels);
        assert!(!open[150]);
        assert!(open[210]);
        let mut room = Gate::new();
        for _ in 0..50 {
            room.push(-45.0, false);
        }
        assert!(room.threshold_db() > -45.0 && room.threshold_db() < -25.0);
    }

    #[test]
    fn a_closed_gate_lowers_the_signal_smoothly_and_an_open_one_leaves_it() {
        let mut gate = Gate::new();
        let mut frame = [10_000i16; 480];
        gate.apply(&mut frame, Verdict { open: true, key: false });
        assert_eq!(frame[479], 10_000);
        gate.apply(&mut frame, Verdict { open: false, key: false });
        let biggest_step = frame.windows(2).map(|w| (w[0] - w[1]).abs()).max().unwrap_or(0);
        assert!(biggest_step < 10, "no step: {biggest_step}");
        let mut more = [10_000i16; 480];
        for _ in 0..4 {
            more = [10_000i16; 480];
            gate.apply(&mut more, Verdict { open: false, key: false });
        }
        assert!(more[479] < 400, "{}", more[479]);
    }

    #[test]
    fn off_at_the_bottom_of_the_meter() {
        assert!(!Gate::active(false, MIN_DB));
        assert!(Gate::active(false, -50.0));
        assert!(Gate::active(true, MIN_DB));
    }
}


/// Whether `cur` (with the frame before it) has a pitch: its normalized
/// autocorrelation, at the lags of 80-400 Hz, peaks over [`VOICED_AT`]. Done
/// at 12 kHz (four samples averaged).
pub fn voiced(history: &[i16], cur: &[i16]) -> bool {
    let down = |x: &[i16]| -> Vec<f32> {
        x.chunks_exact(4)
            .map(|c| c.iter().map(|s| f32::from(*s)).sum::<f32>() / 4.0)
            .collect()
    };
    let mut x = down(history);
    x.extend(down(cur));
    // DC blocked (a constant correlates with itself).
    let (mut px, mut py) = (0f32, 0f32);
    for v in x.iter_mut() {
        let y = *v - px + 0.97 * py;
        px = *v;
        py = y;
        *v = y;
    }
    let (min_lag, max_lag) = (30usize, 150usize);
    if x.len() < max_lag * 2 {
        return false;
    }
    let n = x.len() - max_lag;
    let e0: f32 = x[..n].iter().map(|v| v * v).sum();
    if e0 < 1e-3 {
        return false;
    }
    (min_lag..=max_lag).any(|lag| {
        let (mut c, mut el) = (0f32, 0f32);
        for i in 0..n {
            c += x[i] * x[i + lag];
            el += x[i + lag] * x[i + lag];
        }
        // A voice is as strong a lag later; a lone click is not.
        if el < 0.25 * e0 {
            return false;
        }
        c / (e0 * el).sqrt().max(1e-6) > VOICED_AT
    })
}

/// The autocorrelation over which a frame counts as having a pitch.
pub const VOICED_AT: f32 = 0.6;

#[cfg(test)]
mod voicing_tests {
    use super::*;

    fn tone(hz: f32, from: usize, n: usize) -> Vec<i16> {
        (from..from + n)
            .map(|i| ((i as f32 * hz * std::f32::consts::TAU / 48_000.0).sin() * 8000.0) as i16)
            .collect()
    }

    #[test]
    fn a_voice_has_a_pitch_and_noise_and_a_click_have_none() {
        // 150 Hz with overtones, like a vowel.
        let vowel = |from: usize, n: usize| -> Vec<i16> {
            (from..from + n)
                .map(|i| {
                    let t = i as f32 * std::f32::consts::TAU / 48_000.0;
                    ((t * 150.0).sin() * 5000.0 + (t * 450.0).sin() * 3000.0 + (t * 900.0).sin() * 1500.0) as i16
                })
                .collect()
        };
        assert!(voiced(&vowel(0, 960), &vowel(960, 480)));
        assert!(voiced(&tone(220.0, 0, 960), &tone(220.0, 960, 480)));
        let mut state = 12345u32;
        let mut noise = |n: usize| -> Vec<i16> {
            (0..n)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    ((state as i32) >> 18) as i16
                })
                .collect()
        };
        let (a, b) = (noise(960), noise(480));
        assert!(!voiced(&a, &b), "noise has no pitch");
        let mut click = vec![0i16; 480];
        click[100] = 20_000;
        click[101] = -15_000;
        assert!(!voiced(&vec![0; 960], &click), "a click has no pitch");
    }

    #[test]
    fn a_long_run_without_a_pitch_is_a_voice_only_when_well_over_the_floor() {
        // A steady noise 14 dB over the floor, unvoiced: not a voice. 25 dB
        // over: an s, a voice.
        let run = |level: f32| {
            let mut gate = Gate::new();
            let mut open = Vec::new();
            for _ in 0..100 {
                open.push(gate.push_features(-60.0, false, false).open);
            }
            for _ in 0..20 {
                open.push(gate.push_features(level, false, false).open);
            }
            for _ in 0..30 {
                open.push(gate.push_features(-60.0, false, false).open);
            }
            open[103 + 8..103 + 15].iter().any(|o| *o)
        };
        assert!(!run(-45.0), "a steady unvoiced bump is not speech");
        assert!(run(-30.0), "a loud unvoiced run (an s) is");
    }
}

/// Before #332 (`MORPHEUS_VOICE_LEGACY=1`, for measuring against the old
/// behaviour): no gate, and the speaking ring from WebRTC's level.
pub fn legacy() -> bool {
    static LEGACY: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *LEGACY.get_or_init(|| std::env::var_os("MORPHEUS_VOICE_LEGACY").is_some())
}
