//! Small presentation helpers shared by components.

use std::time::Duration;

use jiff::{Timestamp, tz::TimeZone};

/// A byte count as `12.3 MB`, `850 KB`, `40 B`, ...
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

/// A duration as `M:SS`, for audio/video playback lengths.
pub fn human_duration(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    format!("{}:{:02}", total_secs / 60, total_secs % 60)
}

/// `HH:MM` in the device's time zone for a Unix timestamp in milliseconds.
pub fn clock_time(timestamp_ms: u64) -> String {
    clock_time_in(timestamp_ms, &TimeZone::system())
}

/// `YYYY-MM-DD` in the device's time zone.
pub fn calendar_date(timestamp_ms: u64) -> String {
    calendar_date_in(timestamp_ms, &TimeZone::system())
}

fn zoned(timestamp_ms: u64, zone: &TimeZone) -> Option<jiff::Zoned> {
    let ms = i64::try_from(timestamp_ms).ok()?;
    Some(Timestamp::from_millisecond(ms).ok()?.to_zoned(zone.clone()))
}

pub(crate) fn clock_time_in(timestamp_ms: u64, zone: &TimeZone) -> String {
    zoned(timestamp_ms, zone).map_or_else(String::new, |at| at.strftime("%H:%M").to_string())
}

pub(crate) fn calendar_date_in(timestamp_ms: u64, zone: &TimeZone) -> String {
    zoned(timestamp_ms, zone).map_or_else(String::new, |at| at.strftime("%Y-%m-%d").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip_known_values() {
        let utc = &jiff::tz::TimeZone::UTC;
        assert_eq!(calendar_date_in(0, utc), "1970-01-01");
        assert_eq!(calendar_date_in(1_700_000_000_000, utc), "2023-11-14");
        assert_eq!(clock_time_in(1_700_000_000_000, utc), "22:13");
    }

    #[test]
    fn sizes_pick_the_largest_unit_under_1024() {
        assert_eq!(human_size(40), "40 B");
        assert_eq!(human_size(850 * 1024), "850.0 KB");
        assert_eq!(human_size(12 * 1024 * 1024 + 300 * 1024), "12.3 MB");
    }

    #[test]
    fn durations_render_as_minutes_seconds() {
        assert_eq!(human_duration(Duration::from_secs(5)), "0:05");
        assert_eq!(human_duration(Duration::from_secs(125)), "2:05");
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod zone_tests {
    use super::{calendar_date_in, clock_time_in};
    use jiff::tz::TimeZone;

    const MS_2026_10_19_08_15_UTC: u64 = 1_792_397_700_000;

    #[test]
    fn times_are_shown_in_the_local_zone() {
        let new_york = TimeZone::get("America/New_York").expect("zone");
        assert_eq!(clock_time_in(MS_2026_10_19_08_15_UTC, &new_york), "04:15");
        assert_eq!(
            clock_time_in(MS_2026_10_19_08_15_UTC, &TimeZone::UTC),
            "08:15"
        );
    }

    #[test]
    fn dates_follow_the_local_zone_across_midnight() {
        let new_york = TimeZone::get("America/New_York").expect("zone");
        // 02:00 UTC on the 19th is still the evening of the 18th in New York.
        let early = MS_2026_10_19_08_15_UTC - 6 * 3_600_000 - 15 * 60_000;
        assert_eq!(calendar_date_in(early, &new_york), "2026-10-18");
        assert_eq!(calendar_date_in(early, &TimeZone::UTC), "2026-10-19");
    }
}
