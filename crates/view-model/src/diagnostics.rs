//! The diagnostics report (Settings > Help & Feedback, the F3 overlay, bug
//! reports): sections of "label: value" lines and how they read as the text
//! that "Copy all" puts on the clipboard. What is not known says `unknown`
//! rather than being left out or guessed.

use std::fmt::Display;

/// What a value that cannot be found says.
pub const UNKNOWN: &str = "unknown";

/// A titled group of lines.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Section {
    pub title: String,
    pub lines: Vec<(String, String)>,
}

impl Section {
    /// A line; an empty value reads as unknown.
    pub fn line(&mut self, label: &str, value: impl Into<String>) -> &mut Self {
        let value = value.into();
        let value = if value.trim().is_empty() { UNKNOWN.to_owned() } else { value };
        self.lines.push((label.to_owned(), value));
        self
    }

    /// A line whose value may not be known.
    pub fn maybe(&mut self, label: &str, value: Option<impl Display>) -> &mut Self {
        self.line(label, value.map(|v| v.to_string()).unwrap_or_default())
    }
}

/// All the sections.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub sections: Vec<Section>,
}

impl Report {
    /// Start a section and return it to fill.
    pub fn section(&mut self, title: &str) -> &mut Section {
        self.sections.push(Section { title: title.to_owned(), lines: Vec::new() });
        self.sections.last_mut().unwrap_or_else(|| unreachable!("just pushed"))
    }

    /// The value of the first line called `label`.
    pub fn find(&self, label: &str) -> Option<&str> {
        self.sections
            .iter()
            .flat_map(|s| &s.lines)
            .find(|(l, _)| l == label)
            .map(|(_, v)| v.as_str())
    }

    /// The report as plain text, for the clipboard and bug reports.
    pub fn text(&self) -> String {
        let mut out = String::from("Morpheus diagnostics\n");
        for section in &self.sections {
            out.push_str(&format!("\n## {}\n", section.title));
            for (label, value) in &section.lines {
                out.push_str(&format!("{label}: {value}\n"));
            }
        }
        out
    }

    /// The text cut to at most `max` bytes at a line end (for a link).
    pub fn text_within(&self, max: usize) -> String {
        let text = self.text();
        if text.len() <= max {
            return text;
        }
        let mut cut = max;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        let kept = &text[..cut];
        let end = kept.rfind('\n').unwrap_or(0);
        format!("{}\n(cut)", &kept[..end])
    }
}

/// "12.3 MB", "512 KB", "0 B".
pub fn bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    let f = n as f64;
    if f >= KB * KB * KB {
        format!("{:.2} GB", f / (KB * KB * KB))
    } else if f >= KB * KB {
        format!("{:.1} MB", f / (KB * KB))
    } else if f >= KB {
        format!("{:.0} KB", f / KB)
    } else {
        format!("{n} B")
    }
}

/// A bit rate: "850 kbps", "2.40 Mbps".
pub fn bitrate(bps: f64) -> String {
    if bps >= 1_000_000.0 {
        format!("{:.2} Mbps", bps / 1_000_000.0)
    } else {
        format!("{:.0} kbps", bps / 1000.0)
    }
}

/// "42 ms".
pub fn millis(ms: f64) -> String {
    if ms < 10.0 { format!("{ms:.1} ms") } else { format!("{ms:.0} ms") }
}

/// "Windows 11 24H2 (build 26100)": Windows 11 is build 22000 and up (the
/// registry's product name still says Windows 10 there). `display` is the
/// registry's DisplayVersion ("24H2"), empty on old builds.
pub fn windows_version(build: u32, display: &str) -> String {
    if build == 0 {
        return UNKNOWN.to_owned();
    }
    let name = if build >= 22000 { "Windows 11" } else { "Windows 10" };
    let display = display.trim();
    if display.is_empty() {
        format!("{name} (build {build})")
    } else {
        format!("{name} {display} (build {build})")
    }
}

/// The matrix-sdk version out of a Cargo.lock's text (any line ending).
pub fn lock_version(lock: &str, package: &str) -> Option<String> {
    let name = format!("name = \"{package}\"");
    let mut lines = lock.lines().map(str::trim_end);
    while let Some(line) = lines.next() {
        if line == name {
            return lines
                .next()?
                .strip_prefix("version = ")
                .map(|v| v.trim_matches('"').to_owned());
        }
    }
    None
}

/// Diagnostics' "Hardware video" line: Settings' switch (`setting`),
/// whether `MORPHEUS_HW_VIDEO=0` turned it off, and whether libwebrtc found
/// a hardware encoder (`found`, e.g. "Media Foundation: Intel Quick Sync
/// Video H.264 Encoder MFT"; `None` when there is none).
pub fn hardware_video_line(setting: bool, env_off: bool, found: Option<&str>) -> String {
    let sent = match found {
        Some(found) => format!("hardware encoder: {found}"),
        None => "no hardware encoder found".to_owned(),
    };
    if !setting {
        format!("off in Settings (VP8 in software; {sent})")
    } else if env_off {
        format!("off (MORPHEUS_HW_VIDEO=0; VP8 in software; {sent})")
    } else if found.is_some() {
        format!("on (H.264, {sent})")
    } else {
        format!("on, {sent} (VP8 in software)")
    }
}

/// Diagnostics' "Hardware decoder" line: Settings' switch, the env
/// override and what libwebrtc found (`found`: "Media Foundation on <GPU>",
/// "NVDEC", or "none: <why>").
pub fn hardware_decoder_line(setting: bool, env_off: bool, found: &str) -> String {
    if !setting {
        "off in Settings (all video decoded in software)".to_owned()
    } else if env_off {
        "off (MORPHEUS_HW_VIDEO=0; all video decoded in software)".to_owned()
    } else if let Some(why) = found.strip_prefix("none: ") {
        format!("on, but no hardware decoder: {why} (software)")
    } else {
        format!("on (H.264 on {found}; VP8, VP9 and AV1 have no GPU decoder here and use software)")
    }
}

/// The labels and headings the screens draw (the copied and reported text stays English, for
/// whoever reads a bug report), and the fixed words their values are.
pub fn translatable_labels() -> Vec<&'static str> {
    let mut labels = vec![
        "App", "Atlas", "Audio capture", "Auto gain", "Backend", "Build", "Call", "Camera", "Camera health", "Commit", "Connection",
        "Cross-signing", "Data folder", "Decoded pictures (GPUI)", "Decoder", "DeepFilterNet latency", "Dedicated GPU setting",
        "Desktop", "Displays", "Driver", "Dropped as late", "Echo cancellation", "Encoder", "FPS (frames drawn)",
        "Frame time", "Frames without DeepFilterNet", "GPU", "GPU type", "GPU vendor", "GPUs found", "Graphics",
        "Hardware decoder", "Hardware video", "Homeserver", "Jitter", "JWT service", "Kernel", "Key backup",
        "Lazily freed", "Log file", "Matrix", "matrix-sdk", "Memory", "Memory (RSS)", "Memory (working set)",
        "Microphone", "Microphone callback", "Noise suppression", "Other SFU", "OS", "Packet loss",
        "Pictures, biggest copies", "Pictures, cut to shape", "Pictures drawn", "Private bytes (heap, GPU buffers)",
        "Anonymous (heap, GPU buffers)", "Processing per 10 ms frame", "Rooms", "RTT", "Sample rate", "Screen share", "SFU", "SFUs",
        "Speaker", "Stats", "Strong too slow, using Standard", "Sync", "System", "Timelines kept", "User",
        "Validation", "Version", "Video capture", "Views", "Voice bitrate", "Window system",
    ];
    labels.extend(FIXED_VALUES);
    labels
}

/// Values that are words rather than measurements: translated on the screen.
pub const FIXED_VALUES: &[&str] = &[
    "unknown",
    "on",
    "off",
    "yes",
    "no",
    "system default",
    "debug",
    "release",
    "hidden (streamer mode)",
    "this device is verified",
    "this device is not verified",
    "none (no video is being sent)",
    "none (no video is being received)",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hardware_decoder_line_says_what_decodes_and_why_not() {
        assert_eq!(
            hardware_decoder_line(true, false, "Media Foundation on NVIDIA GeForce RTX 3060"),
            "on (H.264 on Media Foundation on NVIDIA GeForce RTX 3060; VP8, VP9 and AV1 have no GPU decoder here and use software)"
        );
        assert_eq!(
            hardware_decoder_line(true, false, "none: no GPU (Microsoft Basic Render Driver)"),
            "on, but no hardware decoder: no GPU (Microsoft Basic Render Driver) (software)"
        );
        assert!(hardware_decoder_line(false, false, "NVDEC").starts_with("off in Settings"));
        assert!(hardware_decoder_line(true, true, "NVDEC").starts_with("off (MORPHEUS_HW_VIDEO=0"));
    }

    #[test]
    fn the_hardware_video_line_says_what_sends_video_and_why() {
        let mf = Some("Media Foundation: Intel Quick Sync Video H.264 Encoder MFT");
        assert_eq!(
            hardware_video_line(true, false, mf),
            "on (H.264, hardware encoder: Media Foundation: Intel Quick Sync Video H.264 Encoder MFT)"
        );
        assert_eq!(
            hardware_video_line(true, false, None),
            "on, no hardware encoder found (VP8 in software)"
        );
        assert!(hardware_video_line(false, false, mf).starts_with("off in Settings (VP8 in software"));
        assert!(hardware_video_line(true, true, mf).starts_with("off (MORPHEUS_HW_VIDEO=0"));
    }

    /// Every label and heading the desktop writes into a report has a translation entry.
    #[test]
    fn every_label_the_report_writes_is_listed_for_translation() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/src/diagnostics.rs");
        let source = std::fs::read_to_string(path).expect("the desktop diagnostics");
        let listed = translatable_labels();
        let mut missing = Vec::new();
        for call in [".line(", ".maybe(", ".section("] {
            for (at, _) in source.match_indices(call) {
                let rest = source[at + call.len()..].trim_start();
                let Some(rest) = rest.strip_prefix('"') else {
                    continue;
                };
                let label = rest.split('"').next().unwrap_or_default();
                if !listed.contains(&label) {
                    missing.push(label.to_owned());
                }
            }
        }
        for constant in ["RSS_LABEL: &str = \"", "ANON_LABEL: &str = \""] {
            for (at, _) in source.match_indices(constant) {
                let label = source[at + constant.len()..].split('"').next().unwrap_or_default();
                if !listed.contains(&label) {
                    missing.push(label.to_owned());
                }
            }
        }
        assert!(missing.is_empty(), "not in translatable_labels: {missing:?}");
    }

    fn sample() -> Report {
        let mut report = Report::default();
        report.section("App").line("Version", "0.3.0").line("Commit", "");
        report.section("Graphics").maybe("GPU", None::<String>).maybe("FPS", Some(59.9));
        report
    }

    #[test]
    fn the_text_has_a_heading_per_section_and_a_line_per_value() {
        assert_eq!(
            sample().text(),
            "Morpheus diagnostics\n\n## App\nVersion: 0.3.0\nCommit: unknown\n\n## Graphics\nGPU: unknown\nFPS: 59.9\n"
        );
    }

    #[test]
    fn lines_are_found_by_label() {
        assert_eq!(sample().find("Version"), Some("0.3.0"));
        assert_eq!(sample().find("Nothing"), None);
    }

    #[test]
    fn a_long_report_is_cut_at_a_line_end() {
        let text = sample().text_within(40);
        assert_eq!(text, "Morpheus diagnostics\n\n## App\n(cut)");
        assert_eq!(sample().text_within(10_000), sample().text());
    }

    #[test]
    fn sizes_and_rates_read_naturally() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(2048), "2 KB");
        assert_eq!(bytes(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(bitrate(850_000.0), "850 kbps");
        assert_eq!(bitrate(2_400_000.0), "2.40 Mbps");
        assert_eq!(millis(4.26), "4.3 ms");
        assert_eq!(millis(42.0), "42 ms");
    }

    #[test]
    fn windows_builds_are_named() {
        assert_eq!(windows_version(26100, "24H2"), "Windows 11 24H2 (build 26100)");
        assert_eq!(windows_version(19045, "22H2"), "Windows 10 22H2 (build 19045)");
        assert_eq!(windows_version(17763, ""), "Windows 10 (build 17763)");
        assert_eq!(windows_version(0, ""), "unknown");
    }

    #[test]
    fn a_lock_file_version_is_found_with_unix_or_windows_line_endings() {
        let lock = "[[package]]\nname = \"matrix-sdk-base\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"matrix-sdk\"\nversion = \"0.19.0\"\n";
        assert_eq!(lock_version(lock, "matrix-sdk").as_deref(), Some("0.19.0"));
        let crlf = lock.replace('\n', "\r\n");
        assert_eq!(lock_version(&crlf, "matrix-sdk").as_deref(), Some("0.19.0"));
        assert_eq!(lock_version(lock, "nothing"), None);
    }
}
