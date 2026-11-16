//! Keeping the microphone's capture up with real time (#101).
//!
//! Each 10 ms frame has 10 ms to be cleaned. DeepFilterNet (Strong) takes
//! 0.2 ms of it on a fast desktop, several ms on a slow laptop, and now and
//! then the capture thread is not run for a while (a busy CPU) or the
//! device holds back its samples and then brings them all at once (WASAPI
//! after a glitch). What waits is worked off, not thrown away: while the
//! frames are well behind and DeepFilterNet would take long to catch up,
//! frames go without it ([`Pace::neural`]); a CPU on which it cannot keep
//! up at all gets Standard instead of Strong ([`Pace::too_slow`]), with a
//! notice. Only a stall far beyond anything a device delivers (1 s) drops
//! the backlog (`Chunker::drop_stale`).

use std::{collections::VecDeque, time::Duration};

/// A frame's time.
const FRAME_MS: f64 = 10.0;
/// Frames behind by up to this are not late enough to skip DeepFilterNet.
const LAG_MS: f64 = 20.0;
/// Behind by more: DeepFilterNet goes on only if it would work it off
/// within this long.
const CATCH_UP_MS: f64 = 50.0;
/// DeepFilterNet taking this much of a frame's time (the median of the
/// last [`SLOW_WINDOW`] frames) cannot keep up with real time.
const SLOW_MS: f64 = 9.0;
const SLOW_WINDOW: usize = 300;
/// More than [`SKIPS_LIMIT`] of the last [`SKIPS_WINDOW`] frames without
/// DeepFilterNet: it is not keeping up either.
const SKIPS_WINDOW: usize = 1000;
const SKIPS_LIMIT: usize = 250;

/// Whether each frame gets DeepFilterNet, and whether Strong should give
/// way to Standard on this CPU.
#[derive(Default)]
pub(super) struct Pace {
    /// What a frame with DeepFilterNet takes, on average lately (ms).
    cost_ms: Option<f64>,
    /// Those frames' times, the newest last ([`SLOW_WINDOW`] of them).
    recent: VecDeque<f64>,
    /// The last frames: whether each went without DeepFilterNet.
    skips: VecDeque<bool>,
    skipped: usize,
}

impl Pace {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Whether the next frame gets DeepFilterNet, the frames being
    /// `behind_ms` behind the device: yes unless it is far behind and
    /// DeepFilterNet, at what it takes lately, would not soon catch up.
    pub(super) fn neural(&self, behind_ms: f64) -> bool {
        if behind_ms <= LAG_MS {
            return true;
        }
        let cost = self.cost_ms.unwrap_or(0.0);
        if cost >= FRAME_MS {
            return false;
        }
        // Each frame works off 10 ms less what it takes.
        behind_ms * cost / (FRAME_MS - cost) <= CATCH_UP_MS
    }

    /// A frame was processed in `took`, with DeepFilterNet if `neural`.
    pub(super) fn note(&mut self, neural: bool, took: Duration) {
        if neural {
            let ms = took.as_secs_f64() * 1000.0;
            self.cost_ms = Some(self.cost_ms.map_or(ms, |avg| avg * 0.95 + ms * 0.05));
            self.recent.push_back(ms);
            if self.recent.len() > SLOW_WINDOW {
                self.recent.pop_front();
            }
        }
        self.skips.push_back(!neural);
        self.skipped += usize::from(!neural);
        if self.skips.len() > SKIPS_WINDOW && self.skips.pop_front() == Some(true) {
            self.skipped -= 1;
        }
    }

    /// Why DeepFilterNet cannot keep up on this CPU, if it cannot.
    pub(super) fn too_slow(&self) -> Option<String> {
        if self.recent.len() >= SLOW_WINDOW {
            let median = percentile(self.recent.iter().copied(), 50);
            if median >= SLOW_MS {
                return Some(format!(
                    "DeepFilterNet takes {median:.1} ms of each 10 ms frame"
                ));
            }
        }
        (self.skipped > SKIPS_LIMIT).then(|| {
            format!(
                "{} of the last {} frames went without DeepFilterNet to keep up",
                self.skipped,
                self.skips.len()
            )
        })
    }

    /// Start over (another mode, the model loaded again).
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
}

/// The `p`th percentile of `values`.
pub(super) fn percentile(values: impl Iterator<Item = f64>, p: usize) -> f64 {
    let mut values: Vec<f64> = values.collect();
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    values[(values.len() * p / 100).min(values.len() - 1)]
}

/// How long frames take to process: the last few seconds (for Diagnostics)
/// and since the last report (for the log).
#[derive(Default)]
pub(super) struct Timing {
    recent: VecDeque<f64>,
    window: Vec<f64>,
    /// Frames since the figures were last worked out.
    fresh: usize,
}

/// Frames the live figures cover (5 s).
const RECENT: usize = 500;

impl Timing {
    /// A frame took `took`. Every 100 frames, the last 5 s's median and
    /// 99th percentile (ms).
    pub(super) fn note(&mut self, took: Duration) -> Option<(f64, f64)> {
        let ms = took.as_secs_f64() * 1000.0;
        self.recent.push_back(ms);
        if self.recent.len() > RECENT {
            self.recent.pop_front();
        }
        self.window.push(ms);
        self.fresh += 1;
        (self.fresh >= 100).then(|| {
            self.fresh = 0;
            (
                percentile(self.recent.iter().copied(), 50),
                percentile(self.recent.iter().copied(), 99),
            )
        })
    }

    /// Since the last report: frames, the median, 99th percentile and most
    /// (ms); and start a new one.
    pub(super) fn report(&mut self) -> (usize, f64, f64, f64) {
        let window = std::mem::take(&mut self.window);
        let most = window.iter().copied().fold(0.0, f64::max);
        let it = || window.iter().copied();
        (window.len(), percentile(it(), 50), percentile(it(), 99), most)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{denoise::FRAME, mic::{Chunker, STALL_CHUNKS}};
    use super::*;

    /// Small reproducible randomness.
    struct Rng(u32);

    impl Rng {
        /// Uniform in [0, 1).
        fn next(&mut self) -> f64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 17;
            self.0 ^= self.0 << 5;
            f64::from(self.0) / f64::from(u32::MAX)
        }

        fn between(&mut self, low: f64, high: f64) -> f64 {
            low + (high - low) * self.next()
        }
    }

    /// How a device calls back: `size` samples (48 kHz mono) every
    /// `period_ms`, each callback up to `jitter_ms` late, and the device's
    /// own stalls (nothing for `.1` ms from `.0` ms, then all of it at once).
    struct Device {
        name: &'static str,
        size: usize,
        period_ms: f64,
        jitter_ms: f64,
        /// Callbacks of 0.5x-1.5x `size` instead of `size` exactly.
        irregular: bool,
        stalls: Vec<(f64, f64)>,
    }

    /// When each callback comes and how many samples it brings.
    fn callbacks(device: &Device, seconds: f64, rng: &mut Rng) -> Vec<(f64, usize)> {
        let mut out = Vec::new();
        let (mut due, mut sent) = (0.0, 0usize);
        while due < seconds * 1000.0 {
            due += device.period_ms;
            // As many samples as real time has brought by now, in callbacks
            // of about `size`.
            let owed = (due * 48.0) as usize - sent;
            let size = if device.irregular {
                ((device.size as f64 * rng.between(0.5, 1.5)) as usize).min(owed)
            } else {
                owed
            };
            sent += size;
            let mut at = due + rng.between(0.0, device.jitter_ms);
            if let Some((start, len)) = device.stalls.iter().find(|(s, l)| at >= *s && at < s + l) {
                at = start + len;
            }
            out.push((at, size));
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        out
    }

    /// What the capture thread's frames cost: DeepFilterNet's frame (with
    /// WebRTC's processing), uniform in `dfn_ms`, a spike of `spike_ms`
    /// once in `spike_every` frames; and the capture thread's own stalls.
    /// A frame without DeepFilterNet (or in Standard) takes 0.15 ms.
    struct Cost {
        dfn_ms: (f64, f64),
        spike_ms: f64,
        spike_every: usize,
        thread_stalls: Vec<(f64, f64)>,
    }

    const WITHOUT_MS: f64 = 0.15;

    #[derive(Debug, Default)]
    struct Outcome {
        pushed: usize,
        out: usize,
        dropped: usize,
        /// Frames that went without DeepFilterNet.
        skipped: usize,
        /// When Strong gave way to Standard (ms), and why.
        degraded: Option<(f64, String)>,
        /// The most the frames were behind the device, ms.
        most_behind_ms: f64,
        /// Behind at the end, ms.
        behind_ms: f64,
    }

    /// The capture loop (`Device::next`, [`Pace`] and the processing, as
    /// `capture` runs them) in simulated time over `seconds` of the
    /// device's callbacks.
    fn run(device: &Device, cost: &Cost, seconds: f64) -> Outcome {
        let mut rng = Rng(0x2545_f491);
        let events = callbacks(device, seconds, &mut rng);
        let mut chunker = Chunker::new(FRAME);
        let mut pace = Pace::new();
        let mut outcome = Outcome::default();
        let (mut now, mut next, mut frames) = (0.0f64, 0usize, 0usize);
        let mut stalls = cost.thread_stalls.iter().peekable();
        loop {
            if let Some((start, len)) = stalls.peek().copied().copied()
                && now >= start
            {
                now = now.max(start + len);
                stalls.next();
            }
            while next < events.len() && events[next].0 <= now {
                chunker.push(&vec![1; events[next].1]);
                outcome.pushed += events[next].1;
                next += 1;
            }
            outcome.dropped += chunker.drop_stale();
            let Some(_frame) = chunker.next() else {
                if next == events.len() {
                    break;
                }
                now = now.max(events[next].0);
                continue;
            };
            outcome.out += FRAME;
            frames += 1;
            // Behind: what has come and is not out yet, beyond a callback.
            let behind = (outcome.pushed - outcome.out - outcome.dropped) as f64 / 48.0;
            let behind = (behind - device.size as f64 * 1.5 / 48.0).max(0.0);
            outcome.most_behind_ms = outcome.most_behind_ms.max(behind);
            outcome.behind_ms = behind;
            let took = if outcome.degraded.is_some() {
                WITHOUT_MS
            } else {
                let neural = pace.neural(chunker.behind() as f64 / 48.0);
                let took = if !neural {
                    outcome.skipped += 1;
                    WITHOUT_MS
                } else if cost.spike_every > 0 && frames % cost.spike_every == 0 {
                    cost.spike_ms
                } else {
                    rng.between(cost.dfn_ms.0, cost.dfn_ms.1)
                };
                pace.note(neural, Duration::from_secs_f64(took / 1000.0));
                if let Some(why) = pace.too_slow() {
                    outcome.degraded = Some((now, why));
                }
                took
            };
            now += took;
        }
        outcome
    }

    /// The device patterns a Windows microphone shows: WASAPI's shared-mode
    /// period is usually 10 ms, some drivers 20 or 40 ms, some deliver
    /// uneven packets, and after a glitch a burst of what was held back.
    fn windows_devices() -> Vec<Device> {
        let stalls = vec![(5_000.0, 50.0), (12_000.0, 120.0), (20_000.0, 200.0), (27_000.0, 80.0)];
        vec![
            Device { name: "10 ms", size: 480, period_ms: 10.0, jitter_ms: 3.0, irregular: false, stalls: stalls.clone() },
            Device { name: "20 ms", size: 960, period_ms: 20.0, jitter_ms: 5.0, irregular: false, stalls: stalls.clone() },
            Device { name: "40 ms", size: 1920, period_ms: 40.0, jitter_ms: 8.0, irregular: false, stalls: stalls.clone() },
            Device { name: "uneven", size: 960, period_ms: 20.0, jitter_ms: 10.0, irregular: true, stalls },
        ]
    }

    /// A slower CPU, but one that keeps up on average (DeepFilterNet 6.5-9.5
    /// ms per 10 ms frame, a 25 ms frame now and then) and a capture thread
    /// that is not run for 100-180 ms now and then: nothing is dropped,
    /// Strong stays, few frames go without DeepFilterNet (only to work off
    /// the bursts), and the call is not left behind.
    #[test]
    fn a_slow_cpu_that_keeps_up_on_average_drops_nothing() {
        for device in windows_devices() {
            let cost = Cost {
                dfn_ms: (6.5, 9.5),
                spike_ms: 25.0,
                spike_every: 97,
                thread_stalls: vec![(8_000.0, 100.0), (16_000.0, 180.0), (24_000.0, 150.0)],
            };
            let outcome = run(&device, &cost, 30.0);
            eprintln!("{}: {outcome:?}", device.name);
            assert_eq!(outcome.dropped, 0, "{}: {outcome:?}", device.name);
            assert!(outcome.degraded.is_none(), "{}: {outcome:?}", device.name);
            assert!(outcome.skipped < 3000 / 20, "{}: {outcome:?}", device.name);
            assert!(outcome.most_behind_ms < 300.0, "{}: {outcome:?}", device.name);
            assert!(outcome.behind_ms < 30.0, "{}: still behind at the end: {outcome:?}", device.name);
        }
    }

    /// A fast CPU (DeepFilterNet 0.3 ms) works off even a 200 ms burst with
    /// DeepFilterNet: no frame goes without it.
    #[test]
    fn a_fast_cpu_cleans_every_frame_even_after_a_burst() {
        for device in windows_devices() {
            let cost = Cost { dfn_ms: (0.2, 0.4), spike_ms: 0.0, spike_every: 0, thread_stalls: vec![(9_000.0, 150.0)] };
            let outcome = run(&device, &cost, 30.0);
            assert_eq!((outcome.dropped, outcome.skipped), (0, 0), "{}: {outcome:?}", device.name);
            assert!(outcome.degraded.is_none(), "{}: {outcome:?}", device.name);
        }
    }

    /// A CPU on which DeepFilterNet takes 8-12 ms per 10 ms frame (more than
    /// real time on average): nothing of the voice is thrown away, Strong
    /// gives way to Standard within a few seconds (saying why), and the
    /// capture does not stay behind.
    #[test]
    fn a_cpu_too_slow_for_strong_degrades_instead_of_dropping() {
        for dfn_ms in [(8.0, 12.0), (12.0, 12.0), (9.6, 10.4)] {
            for device in windows_devices() {
                let cost = Cost { dfn_ms, spike_ms: 0.0, spike_every: 0, thread_stalls: vec![] };
                let outcome = run(&device, &cost, 30.0);
                eprintln!("{} {dfn_ms:?}: {outcome:?}", device.name);
                assert_eq!(outcome.dropped, 0, "{}: {outcome:?}", device.name);
                let (at, why) = outcome.degraded.clone().expect("Strong gives way");
                assert!(at < 6_000.0, "{}: degraded only at {at} ms", device.name);
                assert!(why.contains("DeepFilterNet"), "{why}");
                assert!(outcome.most_behind_ms < 300.0, "{}: {outcome:?}", device.name);
                assert!(outcome.behind_ms < 30.0, "{}: still behind at the end: {outcome:?}", device.name);
            }
        }
    }

    /// A stall far beyond anything a device delivers (the capture thread
    /// not run for 1.5 s) is still cut, to the newest 10 ms, so the call
    /// does not stay 1.5 s late; it is counted.
    #[test]
    fn a_stall_of_seconds_is_cut_and_counted() {
        let device = &windows_devices()[0];
        let cost = Cost { dfn_ms: (0.2, 0.4), spike_ms: 0.0, spike_every: 0, thread_stalls: vec![(5_000.0, 1_500.0)] };
        let outcome = run(device, &cost, 10.0);
        assert!(outcome.dropped > STALL_CHUNKS * FRAME, "{outcome:?}");
        assert!(outcome.dropped < 1_600 * 48, "{outcome:?}");
        assert!(outcome.behind_ms < 30.0, "{outcome:?}");
    }

    /// The live figures and the report's.
    #[test]
    fn timing_reports_the_median_and_99th_percentile() {
        let mut timing = Timing::default();
        let mut live = None;
        for i in 0..1000u64 {
            // 1 ms, and 9 ms one frame in 50.
            let ms = if i % 50 == 0 { 9 } else { 1 };
            live = timing.note(Duration::from_millis(ms)).or(live);
        }
        let (p50, p99) = live.expect("figures every 100 frames");
        assert_eq!((p50, p99), (1.0, 9.0));
        assert_eq!(timing.report(), (1000, 1.0, 9.0, 9.0));
        assert_eq!(timing.report().0, 0, "a new window");
    }
}
