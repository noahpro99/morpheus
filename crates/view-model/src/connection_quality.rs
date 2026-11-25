//! How good the call's connection is, from its round-trip delay and packet loss: the signal
//! icon beside "Voice connected" and the tooltip on it (#403).

use crate::i18n::{t, tf};

/// Under this round trip (ms) and [`GOOD_LOSS`] the connection is good.
pub const GOOD_RTT_MS: f64 = 100.0;
/// Under this loss (percent) it can still be good.
pub const GOOD_LOSS: f64 = 2.0;
/// Under this round trip (ms) and [`FAIR_LOSS`] it is fair; past either, poor.
pub const FAIR_RTT_MS: f64 = 250.0;
/// Under this loss (percent) it can still be fair.
pub const FAIR_LOSS: f64 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    /// Nothing measured yet.
    Unknown,
    Good,
    Fair,
    Poor,
}

impl Quality {
    /// Lit bars of the three the icon has.
    pub fn bars(self) -> usize {
        match self {
            Quality::Unknown => 0,
            Quality::Poor => 1,
            Quality::Fair => 2,
            Quality::Good => 3,
        }
    }
}

/// The quality for a measured round trip (ms) and loss (percent); either may be missing (loss is
/// taken as none until measured), but with no round trip nothing is known.
pub fn quality(rtt_ms: Option<f64>, loss_percent: Option<f64>) -> Quality {
    let Some(rtt) = rtt_ms.filter(|r| r.is_finite()) else {
        return Quality::Unknown;
    };
    let loss = loss_percent.filter(|l| l.is_finite()).unwrap_or(0.0);
    if rtt < GOOD_RTT_MS && loss < GOOD_LOSS {
        Quality::Good
    } else if rtt < FAIR_RTT_MS && loss < FAIR_LOSS {
        Quality::Fair
    } else {
        Quality::Poor
    }
}

/// The icon's tooltip: the round trip, e.g. "42 ms", with the loss when there is some, or
/// "Measuring…" before the first measurement.
pub fn tooltip(rtt_ms: Option<f64>, loss_percent: Option<f64>) -> String {
    let Some(rtt) = rtt_ms.filter(|r| r.is_finite()) else {
        return t("Measuring…");
    };
    let ms = format!("{}", rtt.round().max(0.0) as u64);
    match loss_percent.filter(|l| l.is_finite() && *l >= 0.5) {
        Some(loss) => tf("%{ms} ms, %{loss}% lost", &[("ms", &ms), ("loss", &format!("{}", loss.round() as u64))]),
        None => tf("%{ms} ms", &[("ms", &ms)]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_follows_the_round_trip_and_the_loss() {
        assert_eq!(quality(None, None), Quality::Unknown);
        assert_eq!(quality(None, Some(0.0)), Quality::Unknown);
        assert_eq!(quality(Some(42.0), None), Quality::Good);
        assert_eq!(quality(Some(42.0), Some(0.5)), Quality::Good);
        assert_eq!(quality(Some(99.9), Some(1.9)), Quality::Good);
        assert_eq!(quality(Some(100.0), Some(0.0)), Quality::Fair);
        assert_eq!(quality(Some(42.0), Some(2.0)), Quality::Fair);
        assert_eq!(quality(Some(249.0), Some(7.0)), Quality::Fair);
        assert_eq!(quality(Some(250.0), Some(0.0)), Quality::Poor);
        assert_eq!(quality(Some(40.0), Some(8.0)), Quality::Poor);
        assert_eq!(quality(Some(f64::NAN), None), Quality::Unknown);
    }

    #[test]
    fn the_tooltip_says_the_round_trip_or_that_it_is_measuring() {
        assert_eq!(tooltip(None, None), "Measuring…");
        assert_eq!(tooltip(Some(41.6), None), "42 ms");
        assert_eq!(tooltip(Some(41.6), Some(0.1)), "42 ms");
        assert_eq!(tooltip(Some(180.0), Some(4.2)), "180 ms, 4% lost");
    }

    #[test]
    fn better_quality_lights_more_bars() {
        assert!(Quality::Good.bars() > Quality::Fair.bars());
        assert!(Quality::Fair.bars() > Quality::Poor.bars());
        assert!(Quality::Poor.bars() > Quality::Unknown.bars());
    }
}
