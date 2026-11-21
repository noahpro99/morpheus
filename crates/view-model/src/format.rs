//! Small presentation helpers shared by components.

use std::time::Duration;

use jiff::{Timestamp, tz::TimeZone};

/// A byte count as `12.3 MB`, `850 KB`, `40 B`, ... (`12,3 MB` in Spanish).
pub fn human_size(bytes: u64) -> String {
    human_size_in(crate::i18n::language(), bytes)
}

pub fn human_size_in(code: &str, bytes: u64) -> String {
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
        let number = format!("{size:.1}");
        let number = if code == "es" { number.replace('.', ",") } else { number };
        format!("{number} {}", UNITS[unit])
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

pub(crate) fn zoned(timestamp_ms: u64, zone: &TimeZone) -> Option<jiff::Zoned> {
    let ms = i64::try_from(timestamp_ms).ok()?;
    Some(Timestamp::from_millisecond(ms).ok()?.to_zoned(zone.clone()))
}

pub(crate) fn clock_time_in(timestamp_ms: u64, zone: &TimeZone) -> String {
    zoned(timestamp_ms, zone).map_or_else(String::new, |at| at.strftime("%H:%M").to_string())
}

pub(crate) fn calendar_date_in(timestamp_ms: u64, zone: &TimeZone) -> String {
    zoned(timestamp_ms, zone).map_or_else(String::new, |at| at.strftime("%Y-%m-%d").to_string())
}

const MONTHS_ES: [&str; 12] = [
    "enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre",
    "diciembre",
];
const WEEKDAYS_ES: [&str; 7] = ["lunes", "martes", "miércoles", "jueves", "viernes", "sábado", "domingo"];

/// A month's name (short or long) in `code`: the words of a date are the language's.
pub(crate) fn month_name(code: &str, at: &jiff::Zoned, long: bool) -> String {
    if code == "es" {
        let name = MONTHS_ES[usize::try_from(at.month() - 1).unwrap_or(0)];
        return if long { name.to_owned() } else { name.chars().take(3).collect() };
    }
    at.strftime(if long { "%B" } else { "%b" }).to_string()
}

pub(crate) fn weekday_name(code: &str, at: &jiff::Zoned) -> String {
    if code == "es" {
        let from_monday = at.weekday().to_monday_zero_offset();
        return WEEKDAYS_ES[usize::try_from(from_monday).unwrap_or(0)].to_owned();
    }
    at.strftime("%A").to_string()
}

/// A day as people say it: "Today", "Yesterday", else "Nov 8" (this year) or
/// "Nov 8, 2025" (in Spanish "8 nov" and "8 nov 2025"): a message's time and the
/// date dividers share it.
fn day_phrase(code: &str, at: &jiff::Zoned, now: &jiff::Zoned) -> String {
    let tf = |key: &str, args: &[(&str, &str)]| crate::i18n::tf_in(code, key, args);
    let t = |key: &str| crate::i18n::t_in(code, key);
    if at.date() == now.date() {
        return t("Today");
    }
    if now.date().yesterday().is_ok_and(|y| y == at.date()) {
        return t("Yesterday");
    }
    let day = at.day().to_string();
    let month = month_name(code, at, false);
    if at.year() == now.year() {
        tf("%{month} %{day}", &[("month", &month), ("day", &day)])
    } else {
        tf("%{month} %{day}, %{year}", &[("month", &month), ("day", &day), ("year", &at.year().to_string())])
    }
}

pub(crate) fn message_time_lang(code: &str, timestamp_ms: u64, now_ms: u64, zone: &TimeZone) -> String {
    let tf = |key: &str, args: &[(&str, &str)]| crate::i18n::tf_in(code, key, args);
    let t = |key: &str| crate::i18n::t_in(code, key);
    let age = now_ms.saturating_sub(timestamp_ms);
    if age < MINUTE_MS {
        return t("just now");
    }
    if age < HOUR_MS {
        return tf("%{n} min ago", &[("n", &(age / MINUTE_MS).to_string())]);
    }
    if age < RELATIVE_LIMIT_MS {
        return tf("%{n} hr ago", &[("n", &(age / HOUR_MS).to_string())]);
    }
    let (Some(at), Some(now)) = (zoned(timestamp_ms, zone), zoned(now_ms, zone)) else {
        return String::new();
    };
    let clock = at.strftime("%H:%M").to_string();
    tf("%{day} at %{clock}", &[("day", &day_phrase(code, &at, &now)), ("clock", &clock)])
}

pub(crate) fn full_time_lang(code: &str, timestamp_ms: u64, zone: &TimeZone) -> String {
    let tf = |key: &str, args: &[(&str, &str)]| crate::i18n::tf_in(code, key, args);
    zoned(timestamp_ms, zone).map_or_else(String::new, |at| {
        tf(
            "%{weekday}, %{month} %{day}, %{year} at %{clock}",
            &[
                ("weekday", &weekday_name(code, &at)),
                ("month", &month_name(code, &at, true)),
                ("day", &at.day().to_string()),
                ("year", &at.year().to_string()),
                ("clock", &at.strftime("%H:%M").to_string()),
            ],
        )
    })
}

pub(crate) fn day_label_lang(code: &str, timestamp_ms: u64, now_ms: u64, zone: &TimeZone) -> String {
    let (Some(at), Some(now)) = (zoned(timestamp_ms, zone), zoned(now_ms, zone)) else {
        return String::new();
    };
    day_phrase(code, &at, &now)
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

/// Now, as Unix milliseconds.
pub fn now_ms() -> u64 {
    u64::try_from(Timestamp::now().as_millisecond()).unwrap_or(0)
}

/// When a message was sent, for beside its name. Under a minute "just now",
/// under an hour "N min ago", under six hours "N hr ago", otherwise the
/// clock time on a day word: "Today at 14:05", "Yesterday at 14:05", "Mar 3
/// at 14:05" (with the year when it is not this year's).
pub fn message_time(timestamp_ms: u64, now_ms: u64) -> String {
    message_time_lang(crate::i18n::language(), timestamp_ms, now_ms, &TimeZone::system())
}

/// The full date and time, for a hover hint: "Monday, March 3, 2026 at 14:05".
pub fn full_time(timestamp_ms: u64) -> String {
    full_time_lang(crate::i18n::language(), timestamp_ms, &TimeZone::system())
}

/// A date divider's label: "Today", "Yesterday", else "Nov 8" or "Nov 8, 2025".
pub fn day_label(timestamp_ms: u64, now_ms: u64) -> String {
    day_label_lang(crate::i18n::language(), timestamp_ms, now_ms, &TimeZone::system())
}

const MINUTE_MS: u64 = 60_000;
const HOUR_MS: u64 = 3_600_000;
/// Relative times ("3 hr ago") are used up to this age.
const RELATIVE_LIMIT_MS: u64 = 6 * HOUR_MS;

#[cfg(test)]
pub(crate) fn message_time_in(timestamp_ms: u64, now_ms: u64, zone: &TimeZone) -> String {
    message_time_lang("en", timestamp_ms, now_ms, zone)
}

#[cfg(test)]
pub(crate) fn full_time_in(timestamp_ms: u64, zone: &TimeZone) -> String {
    full_time_lang("en", timestamp_ms, zone)
}

#[cfg(test)]
pub(crate) fn day_label_in(timestamp_ms: u64, now_ms: u64, zone: &TimeZone) -> String {
    day_label_lang("en", timestamp_ms, now_ms, zone)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod relative_tests {
    use super::{day_label_in, full_time_in, message_time_in};
    use jiff::{civil::date, tz::TimeZone};

    const UTC: &TimeZone = &TimeZone::UTC;

    fn ms(y: i16, mo: i8, d: i8, h: i8, mi: i8, s: i8) -> u64 {
        let at = date(y, mo, d).at(h, mi, s, 0).to_zoned(TimeZone::UTC).expect("zoned");
        u64::try_from(at.timestamp().as_millisecond()).expect("positive")
    }

    // "Now" is Tuesday 2026-03-10, 15:30:00 UTC.
    fn now() -> u64 {
        ms(2026, 3, 10, 15, 30, 0)
    }

    fn at(then: u64) -> String {
        message_time_in(then, now(), UTC)
    }

    #[test]
    fn under_a_minute_is_just_now() {
        assert_eq!(at(now()), "just now");
        assert_eq!(at(now() - 59_000), "just now");
        // A clock a little ahead of ours is still "just now".
        assert_eq!(at(now() + 5_000), "just now");
    }

    #[test]
    fn under_an_hour_counts_minutes() {
        assert_eq!(at(now() - 60_000), "1 min ago");
        assert_eq!(at(now() - 5 * 60_000 - 30_000), "5 min ago");
        assert_eq!(at(now() - 59 * 60_000 - 59_000), "59 min ago");
    }

    #[test]
    fn under_six_hours_counts_hours() {
        assert_eq!(at(now() - 3_600_000), "1 hr ago");
        assert_eq!(at(now() - 2 * 3_600_000 - 59 * 60_000), "2 hr ago");
        assert_eq!(at(now() - 6 * 3_600_000 + 1), "5 hr ago");
    }

    #[test]
    fn relative_times_run_across_midnight() {
        // 01:00 now, a message at 22:00 yesterday: 3 hours, not "Yesterday".
        let now = ms(2026, 3, 10, 1, 0, 0);
        assert_eq!(message_time_in(ms(2026, 3, 9, 22, 0, 0), now, UTC), "3 hr ago");
    }

    #[test]
    fn older_today_names_the_day_and_clock() {
        assert_eq!(at(ms(2026, 3, 10, 9, 5, 0)), "Today at 09:05");
        assert_eq!(at(now() - 6 * 3_600_000), "Today at 09:30");
    }

    #[test]
    fn yesterday_and_older_days() {
        assert_eq!(at(ms(2026, 3, 9, 14, 5, 0)), "Yesterday at 14:05");
        assert_eq!(at(ms(2026, 3, 3, 14, 5, 0)), "Mar 3 at 14:05");
        assert_eq!(at(ms(2026, 1, 20, 8, 0, 0)), "Jan 20 at 08:00");
        assert_eq!(at(ms(2025, 12, 31, 23, 59, 0)), "Dec 31, 2025 at 23:59");
    }

    #[test]
    fn days_follow_the_local_zone() {
        let ny = TimeZone::get("America/New_York").expect("zone");
        // 02:00 UTC on the 10th is the evening of the 9th in New York (EDT, UTC-4).
        let then = ms(2026, 3, 10, 2, 0, 0);
        let later = ms(2026, 3, 10, 20, 0, 0);
        assert_eq!(message_time_in(then, later, UTC), "Today at 02:00");
        assert_eq!(message_time_in(then, later, &ny), "Yesterday at 22:00");
    }

    #[test]
    fn the_hover_text_is_the_full_date() {
        assert_eq!(full_time_in(ms(2026, 3, 3, 14, 5, 0), UTC), "Tuesday, March 3, 2026 at 14:05");
    }

    #[test]
    fn dividers_say_today_and_yesterday() {
        let label = |then| day_label_in(then, now(), UTC);
        assert_eq!(label(ms(2026, 3, 10, 0, 1, 0)), "Today");
        assert_eq!(label(ms(2026, 3, 9, 23, 0, 0)), "Yesterday");
        assert_eq!(label(ms(2026, 3, 8, 12, 0, 0)), "Mar 8");
        assert_eq!(label(ms(2026, 1, 1, 12, 0, 0)), "Jan 1");
        assert_eq!(label(ms(2025, 11, 8, 12, 0, 0)), "Nov 8, 2025");
        // The same wording as a message's time, minus "at 12:00".
        assert_eq!(message_time_in(ms(2025, 11, 8, 12, 0, 0), now(), UTC), "Nov 8, 2025 at 12:00");
    }
}

#[cfg(test)]
mod language_tests {
    use super::*;

    #[test]
    fn dates_and_numbers_are_written_the_way_the_language_does() {
        let utc = &TimeZone::UTC;
        let at = |y: i16, mo: i8, d: i8, h: i8| {
            let z = jiff::civil::date(y, mo, d).at(h, 5, 0, 0).to_zoned(TimeZone::UTC).expect("zoned");
            u64::try_from(z.timestamp().as_millisecond()).expect("positive")
        };
        let now = at(2026, 3, 10, 15);
        assert_eq!(message_time_lang("es", at(2026, 3, 3, 14), now, utc), "3 mar a las 14:05");
        assert_eq!(message_time_lang("es", at(2025, 12, 31, 23), now, utc), "31 dic 2025 a las 23:05");
        assert_eq!(message_time_lang("es", at(2026, 3, 9, 14), now, utc), "Ayer a las 14:05");
        assert_eq!(message_time_lang("es", now - 5 * 60_000, now, utc), "hace 5 min");
        assert_eq!(full_time_lang("es", at(2026, 3, 3, 14), utc), "martes, 3 de marzo de 2026, 14:05");
        assert_eq!(day_label_lang("es", at(2026, 3, 10, 1), now, utc), "Hoy");
        assert_eq!(human_size_in("es", 12 * 1024 * 1024 + 300 * 1024), "12,3 MB");
        assert_eq!(crate::i18n::number_in("es", 1_234_567), "1.234.567");
        assert_eq!(crate::i18n::number_in("en", 1_234_567), "1,234,567");
        assert_eq!(crate::i18n::number_in("en", 999), "999");
        assert_eq!(crate::i18n::tn_in("es", "1 member", "%{count} members", 1), "1 miembro");
        assert_eq!(crate::i18n::tn_in("es", "1 member", "%{count} members", 2500), "2.500 miembros");
    }
}
