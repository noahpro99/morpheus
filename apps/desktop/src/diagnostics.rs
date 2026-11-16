//! What the app can tell about itself (issue #85): Settings > Help &
//! Feedback > Diagnostics, the F3 overlay and bug reports all read this.
//! Lines are plain data (`view_model::diagnostics`); what is not known says
//! "unknown". Streamer mode hides the homeserver, user ID and paths; tokens
//! are never read.

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use gpui::{App, Window};
use morpheus_core::{EncryptionStatus, RecoveryAvailability, Session};

use crate::view_model::diagnostics::{self as vm, Report, UNKNOWN};

/// What the shell and settings know that the report needs.
#[derive(Default)]
pub struct Inputs<'a> {
    pub session: Option<&'a Session>,
    pub encryption: Option<&'a EncryptionStatus>,
}

/// How many rooms the shell lists (kept by its render).
static ROOMS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn note_rooms(count: usize) {
    ROOMS.store(count, std::sync::atomic::Ordering::Relaxed);
}

const HIDDEN: &str = "hidden (streamer mode)";

/// The call service (JWT service) of the call this device joined last.
static CALL_SERVICE: Mutex<String> = Mutex::new(String::new());

pub fn set_call_service(url: &str) {
    if let Ok(mut service) = CALL_SERVICE.lock() {
        *service = url.to_owned();
    }
}

/// Frames drawn per second, measured over the last half second or more.
static FPS: Mutex<Option<(Instant, u64, f32)>> = Mutex::new(None);

fn fps() -> f32 {
    let (count, _) = gpui::frame_counter();
    let now = Instant::now();
    let Ok(mut sample) = FPS.lock() else {
        return 0.0;
    };
    match *sample {
        Some((at, before, rate)) => {
            let secs = now.duration_since(at).as_secs_f32();
            if secs >= 0.5 {
                let rate = (count - before) as f32 / secs;
                *sample = Some((now, count, rate));
                rate
            } else {
                rate
            }
        }
        None => {
            *sample = Some((now, count, 0.0));
            0.0
        }
    }
}

fn yes_no(on: bool) -> String {
    if on { "on" } else { "off" }.to_owned()
}

fn app_section(report: &mut Report, hidden: bool) {
    let section = report.section("App");
    section
        .line("Version", env!("CARGO_PKG_VERSION"))
        .line("Commit", env!("MORPHEUS_COMMIT"))
        .line("Build", if cfg!(debug_assertions) { "debug" } else { "release" });
    let dir = crate::data_dir();
    section
        .line("Data folder", if hidden { HIDDEN.to_owned() } else { dir.display().to_string() })
        .line(
            "Log file",
            if hidden {
                HIDDEN.to_owned()
            } else {
                crate::log_file::path(&dir, 0).display().to_string()
            },
        );
}

fn system_section(window: &Window, cx: &App, report: &mut Report) {
    let section = report.section("System");
    section.line("OS", morpheus_core::device());
    let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease").ok();
    section.line("Kernel", kernel.map(|k| k.trim().to_owned()).unwrap_or_default());
    let env = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    let session = if env("WAYLAND_DISPLAY").is_some() {
        "Wayland"
    } else if env("DISPLAY").is_some() {
        "X11"
    } else {
        UNKNOWN
    };
    let hyprland = env("HYPRLAND_INSTANCE_SIGNATURE").is_some();
    let desktop = env("XDG_CURRENT_DESKTOP").or_else(|| hyprland.then(|| "Hyprland".to_owned()));
    section.line("Window system", session);
    section.line("Desktop", desktop.unwrap_or_default());
    let scale = window.scale_factor();
    let displays = cx.displays();
    if displays.is_empty() {
        section.line("Displays", UNKNOWN);
    }
    for (n, display) in displays.iter().enumerate() {
        let bounds = display.bounds();
        section.line(
            &format!("Display {}", n + 1),
            format!(
                "{}x{} logical, scale {scale:.2}, refresh rate {UNKNOWN}",
                f32::from(bounds.size.width) as i64,
                f32::from(bounds.size.height) as i64
            ),
        );
    }
}

fn gpu_lines(section: &mut vm::Section, compact: bool) {
    let info = gpui::gpu_info();
    let drawing = info.as_ref().and_then(|i| i.drawing.clone());
    match (&info, &drawing) {
        (Some(info), Some((name, driver, driver_info))) => {
            let device = info.devices.iter().find(|d| &d.name == name);
            let kind = device.map(|d| d.kind.as_str()).unwrap_or(UNKNOWN);
            let vendor = device.map(|d| format!("vendor 0x{:04x}", d.vendor_id)).unwrap_or_default();
            if compact {
                section.line("GPU", format!("{name} ({kind})"));
            } else {
                section.line("GPU", name.clone());
                section.line("GPU type", kind);
                section.line("GPU vendor", vendor);
                section.line("Driver", format!("{driver} {driver_info}").trim().to_owned());
            }
        }
        _ => {
            section.line("GPU", UNKNOWN);
        }
    }
    if !compact && let Some(info) = &info {
        let found = info
            .devices
            .iter()
            .map(|d| format!("{} ({})", d.name, d.kind))
            .collect::<Vec<_>>()
            .join("; ");
        section.line("GPUs found", found);
    }
}

fn graphics_section(window: &Window, report: &mut Report) {
    let section = report.section("Graphics");
    section.line(
        "Backend",
        if cfg!(target_os = "macos") {
            "Metal"
        } else if cfg!(target_os = "windows") {
            "DirectX"
        } else {
            "Vulkan (Blade)"
        },
    );
    gpu_lines(section, false);
    section.line("Dedicated GPU setting", yes_no(crate::graphics::dedicated()));
    section.line(
        "Validation",
        gpui::gpu_info().map(|i| yes_no(i.validation)).unwrap_or_default(),
    );
    section.line("FPS", format!("{:.1}", fps()));
    let (_, last) = gpui::frame_counter();
    section.line("Frame time", vm::millis(f64::from(last)));
    let atlas = window.sprite_atlas_stats();
    section.line(
        "Atlas",
        format!(
            "{} textures, {} ({} tiles; {} retired)",
            atlas.textures,
            vm::bytes(atlas.bytes as u64),
            atlas.tiles,
            atlas.retired
        ),
    );
}

fn memory_section(window: &Window, cx: &App, report: &mut Report) {
    let m = crate::memory::report(window, cx);
    let kb = |kb: u64| vm::bytes(kb * 1024);
    let held = |h: crate::memory::Held| format!("{} ({})", h.entries, vm::bytes(h.bytes as u64));
    let section = report.section("Memory");
    section
        .line("Memory (RSS)", kb(m.rss_kb))
        .line("Anonymous (heap, GPU buffers)", kb(m.anon_kb))
        .line("Lazily freed", kb(m.lazy_free_kb))
        .line("Decoded pictures (GPUI)", held(m.gpui_decoded))
        .line("Pictures drawn", held(m.drawn))
        .line("Pictures, biggest copies", held(m.any_size))
        .line("Pictures, cut to shape", held(m.squared))
        .line("Timelines kept", m.timelines.to_string())
        .line("Views", format!("{} timelines, {} member lists", m.timeline_views, m.member_views));
}

fn matrix_section(report: &mut Report, hidden: bool, inputs: &Inputs) {
    let section = report.section("Matrix");
    let (homeserver, user) = match inputs.session {
        Some(s) => (
            if hidden { HIDDEN.to_owned() } else { s.homeserver() },
            if hidden {
                HIDDEN.to_owned()
            } else {
                s.user_id().map(|u| u.to_string()).unwrap_or_default()
            },
        ),
        None => (String::new(), String::new()),
    };
    section
        .line("Homeserver", homeserver)
        .line("User", user)
        .line("matrix-sdk", env!("MORPHEUS_MATRIX_SDK"))
        .line("Sync", inputs.session.map(Session::sync_state).unwrap_or_default())
        .line("Rooms", ROOMS.load(std::sync::atomic::Ordering::Relaxed).to_string());
    match inputs.encryption {
        Some(e) => {
            section.line("Cross-signing", if e.device_verified { "this device is verified" } else { "this device is not verified" });
            section.line(
                "Key backup",
                match e.recovery {
                    RecoveryAvailability::Unknown => UNKNOWN,
                    RecoveryAvailability::Enabled => "on, this device has the keys",
                    RecoveryAvailability::NeedsKey => "on, this device needs the recovery key",
                    RecoveryAvailability::Disabled => "not set up",
                },
            );
        }
        None => {
            section.line("Cross-signing", UNKNOWN).line("Key backup", UNKNOWN);
        }
    }
}

/// "sending 1280x720@30 fps, vaapi (VAAPI H264 Encoder), 2.40 Mbps, loss 0.0%".
fn track_line(t: &morpheus_call::TrackDiag) -> String {
    let mut parts = vec![format!("{} {}x{}@{:.0}", t.direction, t.width, t.height, t.fps)];
    parts.push(format!("{} ({})", t.backend.unwrap_or(UNKNOWN), t.implementation));
    parts.push(t.bitrate.map(vm::bitrate).unwrap_or_else(|| format!("bitrate {UNKNOWN}")));
    if let Some(loss) = t.loss_percent {
        parts.push(format!("loss {loss:.1}%"));
    }
    if let Some(jitter) = t.jitter_ms {
        parts.push(format!("jitter {}", vm::millis(jitter)));
    }
    parts.join(", ")
}

fn opt<T>(value: Option<T>, show: impl FnOnce(T) -> String) -> String {
    value.map(show).unwrap_or_else(|| UNKNOWN.to_owned())
}

/// The sending and receiving codec lines, shared with the overlay.
fn codec_lines(section: &mut vm::Section, call: &morpheus_call::CallDiag) {
    let encoder = call.tracks.iter().find(|t| t.direction == "sending");
    section.line(
        "Encoder",
        match encoder {
            Some(t) => format!("{} ({})", t.backend.unwrap_or(UNKNOWN), t.implementation),
            None => "none (no video is being sent)".to_owned(),
        },
    );
    let decoder = call.tracks.iter().find(|t| t.direction == "receiving");
    section.line(
        "Decoder",
        match decoder {
            Some(t) => format!("{} ({})", t.backend.unwrap_or(UNKNOWN), t.implementation),
            None => "none (no video is being received)".to_owned(),
        },
    );
}

fn link_lines(section: &mut vm::Section, call: &morpheus_call::CallDiag) {
    section
        .line("RTT", opt(call.rtt_ms, vm::millis))
        .line("Packet loss", opt(call.loss_percent, |l| format!("{l:.1}%")))
        .line("Jitter", opt(call.jitter_ms, vm::millis))
        .line("Voice bitrate", opt(call.audio_bitrate, vm::bitrate));
}

fn call_sections(report: &mut Report, hidden: bool) {
    let Some(call) = morpheus_call::call_diag() else {
        return;
    };
    let section = report.section("Call");
    section.line("SFU", if hidden { HIDDEN.to_owned() } else { call.url.clone() });
    let service = CALL_SERVICE.lock().map(|s| s.clone()).unwrap_or_default();
    section.line("JWT service", if hidden { HIDDEN.to_owned() } else { service });
    section.line("Connection", call.state.clone());
    // The SFUs of people on other homeservers, listened on (#91).
    for other in &call.others {
        section.line(
            "Other SFU",
            if hidden {
                HIDDEN.to_owned()
            } else {
                format!("{} ({}, via {})", other.url, other.state, other.service)
            },
        );
    }
    link_lines(section, &call);
    codec_lines(section, &call);
    for track in &call.tracks {
        section.line(&format!("Video {}", track.label), track_line(track));
    }
    let audio = morpheus_call::audio_stats();
    let devices = morpheus_call::chosen_audio_devices();
    section
        .line("Microphone", devices.microphone.unwrap_or_else(|| "system default".to_owned()))
        .line("Speaker", devices.speaker.unwrap_or_else(|| "system default".to_owned()))
        .line("Sample rate", "48000 Hz (10 ms frames)")
        .line("Noise suppression", format!("{:?}", morpheus_call::noise_suppression()))
        .line(
            "DeepFilterNet latency",
            if audio.algorithmic.is_zero() {
                UNKNOWN.to_owned()
            } else {
                vm::millis(audio.algorithmic.as_secs_f64() * 1000.0)
            },
        );
    let (echo, gain) = morpheus_call::processing();
    section
        .line("Echo cancellation", opt(echo, yes_no))
        .line("Auto gain", opt(gain, yes_no))
        .line("Dropped as late", audio.stale.to_string());
    if crate::streamer::sharing() {
        let section = report.section("Screen share");
        let (video, audio) = if cfg!(target_os = "linux") {
            ("desktop portal (PipeWire)", "PulseAudio / PipeWire, other apps' playback")
        } else if cfg!(target_os = "macos") {
            ("ScreenCaptureKit", "ScreenCaptureKit")
        } else if cfg!(target_os = "windows") {
            (UNKNOWN, "WASAPI process loopback")
        } else {
            (UNKNOWN, UNKNOWN)
        };
        section.line("Video capture", video).line("Audio capture", audio);
    }
}

/// The whole report, for Settings and bug reports.
pub fn full(window: &Window, cx: &App, inputs: &Inputs) -> Report {
    let hidden = crate::streamer::active();
    let mut report = Report::default();
    app_section(&mut report, hidden);
    system_section(window, cx, &mut report);
    graphics_section(window, &mut report);
    memory_section(window, cx, &mut report);
    matrix_section(&mut report, hidden, inputs);
    call_sections(&mut report, hidden);
    report
}

/// The few lines of the F3 overlay.
pub fn overlay(window: &Window, cx: &App) -> Report {
    let _ = cx;
    let mut report = Report::default();
    let section = report.section("Stats");
    section.line("FPS", format!("{:.1}", fps()));
    let (_, last) = gpui::frame_counter();
    section.line("Frame time", vm::millis(f64::from(last)));
    gpu_lines(section, true);
    let atlas = window.sprite_atlas_stats();
    section.line("Atlas", vm::bytes(atlas.bytes as u64));
    if let Some(call) = morpheus_call::call_diag() {
        section.line("SFUs", (1 + call.others.len()).to_string());
        codec_lines(section, &call);
        link_lines(section, &call);
        for track in &call.tracks {
            section.line(&format!("Video {}", track.label), track_line(track));
        }
    }
    report
}

/// How often the live views redraw.
pub const REFRESH: Duration = Duration::from_secs(1);

use gpui::{AnyElement, Context, IntoElement as _, div, prelude::*, px, rgb, rgba};

use crate::{e2e::E2eExt as _, shell::Shell, theme};

/// One "Label: value" line, as the settings page and the overlay draw it.
pub fn line_element(label: &str, value: &str, color: u32) -> AnyElement {
    let text = format!("{label}: {value}");
    div()
        .text_color(rgb(color))
        .child(text.clone())
        .e2e("div", "diag-line")
        .text(text)
        .into_any_element()
}

impl Shell {
    /// F3: show or hide the stats overlay.
    pub fn toggle_diagnostics(&mut self, cx: &mut Context<Self>) {
        self.diag_overlay = !self.diag_overlay;
        cx.notify();
    }

    /// The overlay, top right of the window, drawn over everything and
    /// letting clicks through (it listens for nothing). It redraws once a
    /// second for as long as it is shown.
    pub fn diagnostics_overlay(&mut self, window: &gpui::Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.diag_overlay {
            return None;
        }
        if !self.diag_timer {
            self.diag_timer = true;
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(REFRESH).await;
                    let keep = this
                        .update(cx, |this, cx| {
                            if this.diag_overlay {
                                cx.notify();
                            } else {
                                this.diag_timer = false;
                            }
                            this.diag_overlay
                        })
                        .unwrap_or(false);
                    if !keep {
                        break;
                    }
                }
            })
            .detach();
        }
        let report = overlay(window, cx);
        let lines = report
            .sections
            .iter()
            .flat_map(|s| s.lines.iter())
            .map(|(label, value)| line_element(label, value, 0xf2f3f5))
            .collect::<Vec<_>>();
        Some(
            div()
                .absolute()
                .top(px(40.0))
                .right(px(12.0))
                .max_w(px(460.0))
                .px(px(10.0))
                .py(px(8.0))
                .rounded(px(6.0))
                .bg(rgba(0x000000b8))
                .text_size(px(12.0))
                .font_family(theme::MONO)
                .flex()
                .flex_col()
                .children(lines)
                .e2e("div", "diag-overlay")
                .into_any_element(),
        )
    }
}
