//! Settings → Voice & Video → Video: the camera preview, as Discord's
//! "Video Settings". Test Video opens the chosen camera and shows it
//! mirrored, in its own shape; Stop, leaving the page and closing Settings
//! let the camera go. With a call's camera on it shows that feed instead
//! (the call crate sees to the device: see `morpheus_call::preview`).

use std::time::Duration;

use gpui::{AnyElement, Context, MouseButton, div, prelude::*, px, rgb};

use super::{SettingsView, section_classed};
use crate::{call_ui::VideoFrame, e2e::E2eExt as _, theme};

/// How wide the preview is drawn.
const PREVIEW_WIDTH: f32 = 360.0;

impl SettingsView {
    pub(super) fn video_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let (open, opened) = morpheus_call::camera_opens();
        let device = morpheus_call::camera_preview_device().unwrap_or_default();
        let running = self.preview.is_some();
        let busy = self.preview_starting;
        let (label, class, bg) = if running {
            ("Stop", "camera-stop", theme::danger())
        } else {
            ("Test Video", "camera-test", theme::accent_solid())
        };
        let button = div()
            .id(class)
            .flex_none()
            .px(px(14.0))
            .py(px(7.0))
            .rounded(px(theme::RADIUS_SM))
            .cursor_pointer()
            .bg(rgb(bg))
            .text_color(rgb(0xffffff))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .hover(|s| s.opacity(0.9))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                if running {
                    this.stop_video(cx);
                } else if !busy {
                    this.test_video(cx);
                }
            }))
            .child(label)
            .e2e("button", class)
            .text(label);
        let mut preview = div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .items_start()
            .child(button);
        if let Some((_, frame)) = &self.preview {
            let (w, h) = frame.read(cx).dim;
            let ratio = if w > 0 && h > 0 {
                h as f32 / w as f32
            } else {
                9.0 / 16.0
            };
            preview = preview.child(
                div()
                    .relative()
                    .w(px(PREVIEW_WIDTH))
                    .h(px(PREVIEW_WIDTH * ratio))
                    .rounded(px(theme::RADIUS_LG))
                    .bg(rgb(theme::bg_input()))
                    .child(frame.clone()),
            );
        }
        if let Some(message) = &self.preview_message {
            preview = preview.child(
                div()
                    .text_color(rgb(theme::danger()))
                    .max_w(px(PREVIEW_WIDTH))
                    .child(message.clone())
                    .e2e("p", "camera-message")
                    .text(message.clone()),
            );
        }
        let note = div()
            .text_size(px(0.75 * theme::REM))
            .text_color(rgb(theme::text_muted()))
            .child("Your picture is shown as a mirror; others see it the other way round.")
            .into_any_element();
        // The counters and the camera in use are for tests and support.
        let body = preview
            .e2e("div", "camera-preview")
            .attr("data-open", open.to_string())
            .attr("data-opened", opened.to_string())
            .attr("data-camera", device)
            .attr("data-state", if running { "on" } else { "off" })
            .into_any_element();
        let hardware = self.hardware_video_row(cx);
        section_classed(
            "Video",
            "video-preview",
            vec![body, note, hardware],
            self.hl("video-preview"),
            "voice-video",
        )
        .into_any_element()
    }

    /// Test Video: open the camera and show it.
    pub(super) fn test_video(&mut self, cx: &mut Context<Self>) {
        self.preview_message = None;
        self.preview_starting = true;
        cx.notify();
        let starting = morpheus_call::camera_preview_start();
        cx.spawn(async move |this, cx| {
            let result = starting.await;
            this.update(cx, |this, cx| {
                this.preview_starting = false;
                match result {
                    // Closed (or left) while the camera was opening.
                    Ok(_) if !this.open || this.page != super::SettingsPage::VoiceVideo => {
                        this.stop_video(cx);
                    }
                    Ok(key) => {
                        this.show_preview(key, cx);
                        this.watch_preview(cx);
                    }
                    Err(why) => this.preview_message = Some(why.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn show_preview(&mut self, key: &'static str, cx: &mut Context<Self>) {
        let me = cx.weak_entity();
        let frame = cx.new(|cx| {
            VideoFrame::with(
                key.to_owned(),
                false,
                true,
                Box::new(move |cx| {
                    let _ = me.update(cx, |_, cx| cx.notify());
                }),
                cx,
            )
        });
        self.preview = Some((key, frame));
    }

    /// Follows what the preview shows: a call turning its camera on or off
    /// moves it to the call's feed or ends it.
    fn watch_preview(&mut self, cx: &mut Context<Self>) {
        self.preview_watch += 1;
        let watch = self.preview_watch;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                let Ok(going) = this.update(cx, |this, cx| {
                    if this.preview_watch != watch {
                        return false;
                    }
                    let now = morpheus_call::camera_preview_key();
                    match (now, this.preview.as_ref().map(|(key, _)| *key)) {
                        (Some(now), Some(shown)) if now != shown => this.show_preview(now, cx),
                        (None, Some(_)) => {
                            this.preview = None;
                            this.preview_message =
                                Some("The camera was turned off in the call.".to_owned());
                        }
                        _ => return this.preview.is_some(),
                    }
                    cx.notify();
                    this.preview.is_some()
                }) else {
                    break;
                };
                if !going {
                    break;
                }
            }
        })
        .detach();
    }

    /// Stop: the picture goes and the camera is let go.
    pub fn stop_video(&mut self, cx: &mut Context<Self>) {
        self.preview_watch += 1;
        self.preview = None;
        self.preview_message = None;
        let stopping = morpheus_call::camera_preview_stop();
        cx.spawn(async move |this, cx| {
            stopping.await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
        cx.notify();
    }
}

impl SettingsView {
    /// "Hardware video acceleration": the graphics card encodes the camera
    /// and screen (H.264) where it can; off, VP8 in software. Kept with the
    /// devices; a camera or screen already sent keeps its encoder.
    fn hardware_video_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let on = self.voice_prefs.hardware_video;
        let switch = div()
            .id("hw-video-switch")
            .flex_none()
            .w(px(38.0))
            .h(px(20.0))
            .rounded_full()
            .p(px(2.0))
            .flex()
            .when(on, |d| d.justify_end())
            .bg(if on {
                rgb(theme::accent_solid())
            } else {
                rgb(theme::bg_input())
            })
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| this.set_hardware_video(!on, cx)))
            .child(div().size(px(16.0)).rounded_full().bg(rgb(0xffffff)))
            .e2e("button", "hw-video-switch")
            .attr("data-on", on.to_string());
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.0))
            .mt(px(6.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .min_w_0()
                    .child(div().child(crate::view_model::calls::HARDWARE_VIDEO_LABEL))
                    .child(
                        div()
                            .text_size(px(0.75 * theme::REM))
                            .text_color(rgb(theme::text_muted()))
                            .child(crate::view_model::calls::HARDWARE_VIDEO_HINT),
                    ),
            )
            .child(switch)
            .e2e("div", "hw-video")
            .text(format!(
                "{} {}",
                crate::view_model::calls::HARDWARE_VIDEO_LABEL,
                crate::view_model::calls::HARDWARE_VIDEO_HINT
            ))
            .into_any_element()
    }

    fn set_hardware_video(&mut self, on: bool, cx: &mut Context<Self>) {
        self.voice_prefs.hardware_video = on;
        morpheus_call::set_hardware_video(on);
        if let Err(err) =
            crate::view_model::calls::write_device_prefs(&self.data_dir, &self.voice_prefs)
        {
            eprintln!("morpheus-desktop: saving the video setting: {err}");
        }
        // The shell keeps its own copy of the devices file: have it read
        // this one (a camera choice, so no audio device is switched).
        cx.emit(super::DevicesChosen(crate::view_model::menu::DeviceKind::Camera));
        cx.notify();
    }
}
