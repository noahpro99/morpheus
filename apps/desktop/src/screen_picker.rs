//! Share screen where the desktop has no picker of its own (Windows; Linux
//! has the portal's, macOS ScreenCaptureKit's): which screen or window to
//! share, opened at once by the button, with no settings window before. A dialog with the screens and the windows on tabs of
//! their own, each a card with a picture of it now (taken again about once a
//! second) and its name, the share settings at the bottom (resolution, frame rate, the
//! computer's sound, at their remembered values) and Cancel. One click on a
//! card shares it; Cancel shares nothing.
//!
//! The sources come from a `morpheus_call::ScreenSourceProvider`: Windows'
//! own, or in e2e builds a test's (MORPHEUS_E2E_SCREEN_SOURCES, see
//! [`TestSources`]), so the dialog can be tested anywhere.

use crate::view_model::i18n::t;
use std::{collections::HashMap, sync::Arc, time::Duration};

use gpui::{
    AnyElement, Context, EventEmitter, FocusHandle, IntoElement, ObjectFit, Render, RenderImage,
    Task, Window, div, prelude::*, px, rgb, rgba,
};
use morpheus_call::{SCREEN_FPS, ScreenQuality, ScreenResolution, ScreenSource, ScreenSourceKind, ScreenSourceProvider};

use crate::{
    dialogs::{dialog_box, heading, muted, overlay},
    e2e::E2eExt as _,
    pictures::Pictures,
    shell::Shell,
    theme,
};

/// The size pictures are taken at (each card shows one at 16:9).
const THUMB_W: u32 = 320;
const THUMB_H: u32 = 180;
/// How often the list and the pictures are taken again.
const REFRESH: Duration = Duration::from_secs(1);

/// The sources this app picks from itself, if it does here: a test's, or
/// Windows'. Made once (Windows' keeps libwebrtc's listers).
pub fn provider() -> Option<Arc<dyn ScreenSourceProvider>> {
    static PROVIDER: std::sync::OnceLock<Option<Arc<dyn ScreenSourceProvider>>> =
        std::sync::OnceLock::new();
    PROVIDER
        .get_or_init(|| {
            #[cfg(feature = "e2e")]
            if let Some(test) = crate::e2e::sources::TestSources::from_env() {
                return Some(Arc::new(test) as Arc<dyn ScreenSourceProvider>);
            }
            morpheus_call::platform_source_provider()
        })
        .clone()
}

/// What the picker ends with.
pub enum Picked {
    /// Share this, at these settings (resolution, frame rate, sound).
    Share(ScreenSource, ScreenQuality),
    Cancel,
}

/// One source's picture: the latest, and how many have come.
#[derive(Default)]
struct Thumb {
    pictures: Pictures,
    seq: u64,
}

pub struct ScreenPicker {
    provider: Arc<dyn ScreenSourceProvider>,
    tab: ScreenSourceKind,
    sources: Vec<ScreenSource>,
    /// Whether the first list has come.
    listed: bool,
    thumbs: HashMap<String, Thumb>,
    quality: ScreenQuality,
    focus: FocusHandle,
    take_focus: bool,
    _refresh: Task<()>,
}

impl EventEmitter<Picked> for ScreenPicker {}

impl ScreenPicker {
    pub fn new(
        provider: Arc<dyn ScreenSourceProvider>,
        quality: ScreenQuality,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            provider,
            tab: ScreenSourceKind::Screen,
            sources: Vec::new(),
            listed: false,
            thumbs: HashMap::new(),
            quality,
            focus: cx.focus_handle(),
            take_focus: true,
            _refresh: Self::refresh(cx),
        }
    }

    /// Lists the sources and pictures the tab's, now and every second, off
    /// the interface's thread.
    fn refresh(cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| {
            loop {
                let Ok((provider, tab)) = this.read_with(cx, |p, _| (p.provider.clone(), p.tab))
                else {
                    break;
                };
                let got = cx
                    .background_executor()
                    .spawn(async move {
                        let sources = provider.sources();
                        let thumbs: Vec<_> = sources
                            .iter()
                            .filter(|s| s.kind == tab)
                            .map(|s| (s.key(), provider.thumbnail(s, THUMB_W, THUMB_H)))
                            .collect();
                        (sources, thumbs)
                    })
                    .await;
                if this.update(cx, |p, cx| p.apply(got, cx)).is_err() {
                    break;
                }
                cx.background_executor().timer(REFRESH).await;
            }
        })
    }

    fn apply(
        &mut self,
        (sources, thumbs): (
            Vec<ScreenSource>,
            Vec<(String, Option<morpheus_call::Thumbnail>)>,
        ),
        cx: &mut Context<Self>,
    ) {
        self.thumbs
            .retain(|key, _| sources.iter().any(|s| &s.key() == key));
        for (key, thumb) in thumbs {
            let Some(thumb) = thumb else { continue };
            // B, G, R, A: what GPUI's images are made of.
            let Some(buffer) = image::RgbaImage::from_raw(thumb.width, thumb.height, thumb.bgra)
            else {
                continue;
            };
            let image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
            let entry = self.thumbs.entry(key).or_default();
            entry.pictures.show(image);
            entry.seq += 1;
        }
        self.sources = sources;
        self.listed = true;
        cx.notify();
    }

    fn set_tab(&mut self, tab: ScreenSourceKind, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            // The new tab's pictures now, not in a second.
            self._refresh = Self::refresh(cx);
            cx.notify();
        }
    }

    /// One click on a source shares it, at the settings below.
    fn share(&mut self, source: ScreenSource, cx: &mut Context<Self>) {
        cx.emit(Picked::Share(source, self.quality));
    }

    fn tab_el(&self, tab: ScreenSourceKind, label: &str, cx: &mut Context<Self>) -> AnyElement {
        let active = self.tab == tab;
        let value = match tab {
            ScreenSourceKind::Screen => "screens",
            ScreenSourceKind::Window => "windows",
        };
        div()
            .id(gpui::SharedString::from(format!(
                "screen-picker-tab-{value}"
            )))
            .px(px(12.0))
            .py(px(6.0))
            .rounded(px(theme::RADIUS_SM))
            .cursor_pointer()
            .when(active, |d| {
                d.bg(rgb(theme::accent_solid())).text_color(rgb(theme::on_accent_solid()))
            })
            .when(!active, |d| {
                d.text_color(rgb(theme::text_muted())).hover(|s| {
                    s.bg(rgba(theme::bg_selected()))
                        .text_color(rgb(theme::text()))
                })
            })
            .child(label.to_owned())
            .on_click(cx.listener(move |this, _, _, cx| this.set_tab(tab, cx)))
            .e2e(
                "button",
                if active {
                    "screen-picker-tab active"
                } else {
                    "screen-picker-tab"
                },
            )
            .attr("data-value", value)
            .text(label)
            .into_any_element()
    }

    fn card(&self, source: &ScreenSource, cx: &mut Context<Self>) -> AnyElement {
        let key = source.key();
        let thumb = self.thumbs.get(&key);
        let painting = thumb.and_then(|t| t.pictures.to_paint());
        let shown = painting.is_some();
        let picture = div()
            .relative()
            .w_full()
            .h(px(101.0))
            .rounded(px(theme::RADIUS_SM))
            .bg(theme::solid_chat())
            .overflow_hidden()
            .when_some(painting, |d, (image, painted)| {
                d.child(
                    gpui::canvas(
                        |_, _, _| (),
                        move |bounds, (), window, _| {
                            let fitted = ObjectFit::Contain.get_bounds(bounds, image.size(0));
                            painted.set(true);
                            let _ = window.paint_image(
                                fitted,
                                gpui::Corners::default(),
                                image.clone(),
                                0,
                                false,
                            );
                        },
                    )
                    .absolute()
                    .inset_0(),
                )
            })
            .e2e("div", "screen-thumb")
            .attr("data-shown", shown.to_string())
            .attr("data-seq", thumb.map_or(0, |t| t.seq).to_string());
        let pick = source.clone();
        div()
            .id(gpui::SharedString::from(format!("screen-source-{key}")))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .w(px(188.0))
            .p(px(4.0))
            .rounded(px(theme::RADIUS))
            .border_2()
            .border_color(rgba(0x00000000))
            .cursor_pointer()
            .hover(|s| s.bg(rgba(theme::bg_selected())).border_color(rgb(theme::accent())))
            .child(picture)
            .child(
                div()
                    .text_size(px(0.85 * theme::REM))
                    .truncate()
                    .child(source.name.clone())
                    .e2e("span", "screen-source-name")
                    .text(source.name.clone()),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.share(pick.clone(), cx)))
            .e2e("div", "screen-source")
            .attr("data-source", key)
            .text(source.name.clone())
            .into_any_element()
    }
}

impl Render for ScreenPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.take_focus) {
            window.focus(&self.focus);
        }
        for thumb in self.thumbs.values_mut() {
            thumb.pictures.drop_stale(window);
        }
        let shown: Vec<ScreenSource> = self
            .sources
            .iter()
            .filter(|s| s.kind == self.tab)
            .cloned()
            .collect();
        let cards: Vec<AnyElement> = shown.iter().map(|s| self.card(s, cx)).collect();
        let empty = match self.tab {
            _ if !self.listed => "Looking for what you can share…",
            ScreenSourceKind::Screen => "No screens to share",
            ScreenSourceKind::Window => "No windows to share",
        };
        let quality = self.quality;
        let sound = crate::screen_quality::sound_switch(
            quality.audio,
            |this: &mut Self, cx| {
                this.quality.audio = !this.quality.audio;
                cx.notify();
            },
            cx,
        );
        let resolutions = ScreenResolution::ALL
            .into_iter()
            .map(|r| (r, r.key().to_owned(), r.label().to_owned(), r == quality.resolution))
            .collect();
        let rates = SCREEN_FPS
            .into_iter()
            .map(|fps| (fps, fps.to_string(), fps.to_string(), fps == quality.fps))
            .collect();
        let resolution_row = crate::screen_quality::row(
            cx,
            "Resolution",
            "screen-res",
            resolutions,
            |this: &mut Self, resolution, cx| {
                this.quality.resolution = resolution;
                cx.notify();
            },
        );
        let fps_row = crate::screen_quality::row(
            cx,
            "Frame rate",
            "screen-fps",
            rates,
            |this: &mut Self, fps, cx| {
                this.quality.fps = fps;
                cx.notify();
            },
        );
        let cancel = div()
            .id("screen-picker-cancel")
            .px(px(12.0))
            .py(px(8.0))
            .rounded(px(theme::RADIUS_SM))
            .cursor_pointer()
            .hover(|s| s.bg(rgba(theme::bg_selected())))
            .child(t("Cancel"))
            .on_click(cx.listener(|_, _, _, cx| cx.emit(Picked::Cancel)))
            .e2e("button", "secondary screen-picker-cancel")
            .text(t("Cancel"));
        let boxed = dialog_box()
            .id("screen-picker")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, _, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => cx.emit(Picked::Cancel),
                    _ => {}
                }
            }))
            .w(px(640.0))
            .child(heading("Choose what to share"))
            .child(
                div()
                    .flex()
                    .gap(px(4.0))
                    .child(self.tab_el(ScreenSourceKind::Screen, "Screens", cx))
                    .child(self.tab_el(ScreenSourceKind::Window, "Windows", cx)),
            )
            .child(
                div()
                    .id("screen-picker-sources")
                    .h(px(330.0))
                    .overflow_y_scroll()
                    .child(if cards.is_empty() {
                        div()
                            .p(px(12.0))
                            .child(muted(empty, "muted screen-picker-empty"))
                            .into_any_element()
                    } else {
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(8.0))
                            .children(cards)
                            .into_any_element()
                    }),
            )
            .children(
                crate::view_model::calls::screen_picker_hint(cfg!(target_os = "windows"))
                    .map(|hint| crate::dialogs::muted(hint, "muted screen-picker-hint")),
            )
            // The share settings, at their remembered values: a click on a
            // source above shares with them.
            .child(
                div()
                    .id("screen-picker-settings")
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex()
                            .gap(px(12.0))
                            .child(div().flex_1().child(resolution_row))
                            .child(div().flex_1().child(fps_row)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.0))
                            .child(div().flex_1().child(sound))
                            .child(cancel),
                    )
                    .e2e("div", "screen-picker-settings"),
            )
            .e2e("div", "message-dialog screen-picker")
            .focusable(&self.focus);
        let this = cx.weak_entity();
        overlay(window, cx, boxed.into_any_element(), move |_, cx| {
            this.update(cx, |_, cx| cx.emit(Picked::Cancel)).ok();
        })
    }
}

impl Shell {
    /// Go Live where the app picks the source: the picker, sharing when it
    /// says Share. Returns false where the system picks instead.
    pub fn ask_screen_source(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(provider) = provider() else {
            return false;
        };
        let quality = self.calls.screen_quality;
        let picker = cx.new(|cx| ScreenPicker::new(provider, quality, cx));
        cx.subscribe(&picker, |this, _, picked: &Picked, cx| {
            this.dialog_close(cx);
            if let Picked::Share(source, quality) = picked {
                if this.calls.screen_quality != *quality {
                    this.choose_screen_quality(*quality, cx);
                }
                this.share_screen_from(Some(source.clone()), cx);
            }
        })
        .detach();
        self.dialog_open(crate::dialogs::Dialog::ScreenPicker(picker), cx);
        true
    }
}
