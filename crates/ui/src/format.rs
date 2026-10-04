//! Small presentation helpers shared by components.

use std::time::Duration;

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

/// `HH:MM` in the local timezone for a Unix timestamp in milliseconds.
pub fn clock_time(timestamp_ms: u64) -> String {
    let secs = timestamp_ms / 1000;
    let offset = local_utc_offset_secs();
    let local = secs as i64 + offset;
    let minutes = local.rem_euclid(86_400) / 60;
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// `YYYY-MM-DD` in the local timezone. Good enough for date dividers until we
/// pull in a proper date library.
pub fn calendar_date(timestamp_ms: u64) -> String {
    let secs = timestamp_ms / 1000;
    let days = (secs as i64 + local_utc_offset_secs()).div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn local_utc_offset_secs() -> i64 {
    // The webview runtime exposes no timezone API to Rust without extra
    // crates; honour an explicit override and otherwise assume UTC.
    std::env::var("MORPHEUS_UTC_OFFSET_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Howard Hinnant's days-from-civil inverse. Days since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip_known_values() {
        assert_eq!(calendar_date(0), "1970-01-01");
        assert_eq!(calendar_date(1_700_000_000_000), "2023-11-14");
        assert_eq!(clock_time(1_700_000_000_000), "22:13");
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
