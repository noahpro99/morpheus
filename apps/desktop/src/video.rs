//! Videos in messages, played in place as in Discord: the thumbnail with a
//! play button and the length; a click fetches the file (decrypted by the
//! core into memory, never written anywhere) and plays it in the card, with
//! controls while the pointer is over it (play/pause, a seek bar, time,
//! mute and volume, download, full window). A short silent video plays by
//! itself, muted and looping, like a GIF. One video plays with sound at a
//! time; one that leaves the view pauses (a GIF starts again on return).
//!
//! Decoding and timing are `morpheus_video`'s (its own thread per video);
//! this polls it for the newest picture and draws it as an image.

use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, Bounds, ClickEvent, Context, EventEmitter, FocusHandle, Image, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, Pixels, Render, RenderImage,
    SharedString, StyledImage as _, Window, canvas, deferred, div, img, prelude::*, px, relative,
    rgb, rgba,
};
use morpheus_core::{MediaRef, Session};
use morpheus_video::{Info, Options, Player, Status, Unsupported};

use crate::{
    dialogs::Closed,
    e2e::E2eExt as _,
    icons::icon,
    message::{Shape, image_size},
    theme,
    view_model::format::human_duration,
};

/// A silent video shorter than this plays by itself, looping (a GIF).
const GIF_MAX: Duration = Duration::from_secs(15);
/// What the arrow keys skip.
const SEEK_STEP: Duration = Duration::from_secs(5);
/// Not drawn this long after asking for a redraw: out of view.
const OFFSCREEN: Duration = Duration::from_millis(300);

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// The player last started with sound; any other one playing sound pauses.
static SOUNDING: AtomicU64 = AtomicU64::new(0);

/// What a player asks of the timeline.
pub enum VideoEvent {
    /// The full-window button.
    Fullscreen,
    /// The full-window player's button: back to the message.
    ExitFullscreen,
}

/// The file, as far as it has come.
enum Load {
    Idle,
    Fetching,
    Probed(Arc<[u8]>, Info),
    Unsupported(String),
    Failed(String),
}

#[derive(Clone, Copy, PartialEq)]
enum Drag {
    Seek,
    Volume,
}

pub struct VideoPlayer {
    id: u64,
    session: Option<Session>,
    media: MediaRef,
    body: String,
    width: Option<u32>,
    height: Option<u32>,
    duration: Option<Duration>,
    thumbnail: Option<Arc<Image>>,
    load: Load,
    /// Play as soon as the file is here and open.
    play_when_ready: bool,
    opening: bool,
    player: Option<Player>,
    /// A short silent clip: plays by itself, muted, looping.
    gif: bool,
    status: Status,
    image: Option<Arc<RenderImage>>,
    /// Pictures replaced, to take out of the atlas on the next frame.
    stale: Vec<Arc<RenderImage>>,
    frame_index: u64,
    frame_pts: Duration,
    focus: FocusHandle,
    hovered: bool,
    /// When the card was last drawn, and when it last asked to be.
    painted: Rc<Cell<Instant>>,
    notified: Option<Instant>,
    /// Paused because it left the view (a GIF plays again on return).
    paused_offscreen: bool,
    seek_bar: Rc<Cell<Bounds<Pixels>>>,
    volume_bar: Rc<Cell<Bounds<Pixels>>>,
    dragging: Option<Drag>,
    last_drag_seek: Option<Instant>,
    muted: bool,
    volume: f32,
    download: Option<String>,
    polling: bool,
}

impl EventEmitter<VideoEvent> for VideoPlayer {}

impl VideoPlayer {
    pub fn new(
        session: Option<Session>,
        media: MediaRef,
        body: String,
        width: Option<u32>,
        height: Option<u32>,
        duration: Option<Duration>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            session,
            media,
            body,
            width,
            height,
            duration,
            thumbnail: None,
            load: Load::Idle,
            play_when_ready: false,
            opening: false,
            player: None,
            gif: false,
            status: Status::default(),
            image: None,
            stale: Vec::new(),
            frame_index: 0,
            frame_pts: Duration::ZERO,
            focus: cx.focus_handle(),
            hovered: false,
            painted: Rc::new(Cell::new(Instant::now())),
            notified: None,
            paused_offscreen: false,
            seek_bar: Rc::new(Cell::new(Bounds::default())),
            volume_bar: Rc::new(Cell::new(Bounds::default())),
            dragging: None,
            last_drag_seek: None,
            muted: false,
            volume: 1.0,
            download: None,
            polling: false,
        }
    }

    pub fn set_thumbnail(&mut self, thumbnail: Option<Arc<Image>>, cx: &mut Context<Self>) {
        let same = match (&self.thumbnail, &thumbnail) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.thumbnail = thumbnail;
            cx.notify();
        }
    }

    /// Short (or of unknown length): looked at when first shown, to know
    /// whether it is a GIF-like clip and whether it can play at all.
    fn worth_probing(&self) -> bool {
        self.duration.is_none_or(|d| d < GIF_MAX)
    }

    fn info(&self) -> Option<&Info> {
        match &self.load {
            Load::Probed(_, info) => Some(info),
            _ => None,
        }
    }

    /// Fetches the file (once) and reads what it holds.
    fn fetch(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.load, Load::Idle) {
            return;
        }
        let Some(session) = self.session.clone() else {
            self.load = Load::Failed("not signed in".into());
            return;
        };
        self.load = Load::Fetching;
        let fetching = session.media_file(self.media.clone());
        cx.spawn(async move |this, cx| {
            let bytes = fetching.await.ok().flatten().map(|media| Arc::<[u8]>::from(media.bytes));
            let probed = match bytes {
                Some(bytes) => {
                    cx.background_executor().spawn(async move {
                        let info = morpheus_video::probe(&bytes);
                        info.map(|info| (bytes, info))
                    })
                    .await
                }
                None => Err(Unsupported::Broken("could not fetch it".into())),
            };
            this.update(cx, |this, cx| {
                this.load = match probed {
                    Ok((bytes, info)) => {
                        this.gif = !info.has_audio && info.duration < GIF_MAX;
                        Load::Probed(bytes, info)
                    }
                    Err(Unsupported::Format(_)) => Load::Unsupported("Can't play this format".into()),
                    Err(err) => Load::Failed(err.to_string()),
                };
                if this.gif {
                    this.muted = true;
                    this.play_when_ready = true;
                }
                if this.play_when_ready {
                    this.open(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Opens the player on the fetched file (on a background thread: it
    /// reads the file's tables) and plays it if asked to meanwhile.
    fn open(&mut self, cx: &mut Context<Self>) {
        if self.player.is_some() || self.opening {
            return;
        }
        let Load::Probed(bytes, _) = &self.load else {
            self.fetch(cx);
            return;
        };
        let bytes = bytes.clone();
        self.opening = true;
        let options = Options {
            audible: crate::sounds::audible(),
            looped: self.gif,
            muted: self.muted,
            volume: self.volume,
        };
        cx.spawn(async move |this, cx| {
            let opened = cx.background_executor().spawn(async move { Player::open(bytes, options) }).await;
            this.update(cx, |this, cx| {
                this.opening = false;
                match opened {
                    Ok(player) => {
                        this.player = Some(player);
                        if std::mem::take(&mut this.play_when_ready) {
                            this.play(cx);
                        }
                        this.start_polling(cx);
                    }
                    Err(err) => this.load = Load::Failed(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn start_polling(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.polling, true) {
            return;
        }
        cx.spawn(async move |this, cx| {
            loop {
                let Ok(fast) = this.update(cx, |this, cx| {
                    this.poll(cx);
                    this.status.playing || this.dragging.is_some()
                }) else {
                    break;
                };
                let wait = if fast { 16 } else { 120 };
                cx.background_executor().timer(Duration::from_millis(wait)).await;
            }
        })
        .detach();
    }

    fn play(&mut self, cx: &mut Context<Self>) {
        self.paused_offscreen = false;
        let Some(player) = &self.player else {
            self.play_when_ready = true;
            self.open(cx);
            cx.notify();
            return;
        };
        if self.has_sound() {
            SOUNDING.store(self.id, Ordering::Relaxed);
        }
        player.play();
        // Counts as asking to be drawn: if it is not, it is out of view.
        self.notified = Some(Instant::now());
        self.status.playing = true;
        cx.notify();
    }

    fn pause(&mut self, cx: &mut Context<Self>) {
        if let Some(player) = &self.player {
            player.pause();
        }
        self.play_when_ready = false;
        // `playing` follows the player's own word (next poll), so the time
        // shown with it is where it stopped.
        cx.notify();
    }

    fn toggle(&mut self, cx: &mut Context<Self>) {
        if self.status.playing || self.play_when_ready {
            self.pause(cx);
        } else {
            self.play(cx);
        }
    }

    /// Whether it would be heard: sound in the file, not muted.
    fn has_sound(&self) -> bool {
        let has_audio = self.status.has_audio || self.info().is_some_and(|info| info.has_audio);
        has_audio && !self.muted && self.volume > 0.0
    }

    fn seek(&mut self, to: Duration, cx: &mut Context<Self>) {
        let Some(player) = &self.player else { return };
        let to = to.min(self.status.duration);
        player.seek(to);
        self.status.position = to;
        self.status.ended = false;
        cx.notify();
    }

    fn skip(&mut self, forward: bool, cx: &mut Context<Self>) {
        let at = self.status.position;
        let to = if forward { at + SEEK_STEP } else { at.saturating_sub(SEEK_STEP) };
        self.seek(to, cx);
    }

    /// The seek bar or volume slider under the pointer at `x`.
    fn drag_to(&mut self, x: Pixels, cx: &mut Context<Self>) {
        let Some(drag) = self.dragging else { return };
        let bar = match drag {
            Drag::Seek => self.seek_bar.get(),
            Drag::Volume => self.volume_bar.get(),
        };
        let width = f32::from(bar.size.width);
        if width <= 0.0 {
            return;
        }
        let fraction = (f32::from(x - bar.origin.x) / width).clamp(0.0, 1.0);
        match drag {
            Drag::Seek => {
                // A seek decodes from the keyframe before: not on every move.
                let now = Instant::now();
                if self.last_drag_seek.is_some_and(|t| now - t < Duration::from_millis(80)) {
                    self.status.position = self.status.duration.mul_f32(fraction);
                    cx.notify();
                    return;
                }
                self.last_drag_seek = Some(now);
                let to = self.status.duration.mul_f32(fraction);
                self.seek(to, cx);
            }
            Drag::Volume => {
                self.volume = fraction;
                self.muted = fraction <= 0.0;
                if let Some(player) = &self.player {
                    player.set_volume(fraction);
                    player.set_muted(self.muted);
                }
                if self.has_sound() && self.status.playing {
                    SOUNDING.store(self.id, Ordering::Relaxed);
                }
                cx.notify();
            }
        }
    }

    fn end_drag(&mut self, x: Option<Pixels>, cx: &mut Context<Self>) {
        if self.dragging.is_none() {
            return;
        }
        if let Some(x) = x {
            self.last_drag_seek = None;
            self.drag_to(x, cx);
        }
        self.dragging = None;
        cx.notify();
    }

    fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        self.muted = !self.muted;
        if !self.muted && self.volume <= 0.0 {
            self.volume = 1.0;
            if let Some(player) = &self.player {
                player.set_volume(1.0);
            }
        }
        if let Some(player) = &self.player {
            player.set_muted(self.muted);
        }
        if self.has_sound() && self.status.playing {
            SOUNDING.store(self.id, Ordering::Relaxed);
        }
        cx.notify();
    }

    fn start_download(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else { return };
        self.download = Some("Downloading".into());
        let saving = session.download(self.media.clone(), self.body.clone());
        cx.spawn(async move |this, cx| {
            let state = match saving.await {
                Ok(path) => format!("Saved to {}", path.display()),
                Err(err) => format!("Download failed: {err}"),
            };
            this.update(cx, |this, cx| {
                this.download = Some(state);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The newest picture and where playback is; pauses when out of view or
    /// when another video starts with sound.
    fn poll(&mut self, cx: &mut Context<Self>) {
        let Some(player) = &self.player else { return };
        let status = player.status();
        let mut changed = status != self.status;
        let dragging_seek = self.dragging == Some(Drag::Seek);
        let position = self.status.position;
        self.status = status;
        if dragging_seek {
            self.status.position = position;
        }
        if let Some(frame) = player.take_frame() {
            self.frame_index = frame.index;
            self.frame_pts = frame.pts;
            // The pixels are B, G, R, A: what GPUI's images are made of.
            if let Some(buffer) = image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra) {
                let image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
                if let Some(old) = self.image.replace(image) {
                    self.stale.push(old);
                }
            }
            changed = true;
        }
        let now = Instant::now();
        if self.status.playing {
            // Asked to be drawn but not drawn since: out of view.
            if let Some(asked) = self.notified
                && now - asked > OFFSCREEN
                && self.painted.get() < asked
            {
                player.pause();
                self.status.playing = false;
                self.paused_offscreen = self.gif;
                self.notified = None;
                return;
            }
            // Someone else's video started with sound.
            if self.has_sound() && SOUNDING.load(Ordering::Relaxed) != self.id {
                player.pause();
                self.status.playing = false;
                changed = true;
            }
        } else if self.paused_offscreen && self.painted.get().elapsed() < OFFSCREEN {
            // A GIF back in view.
            self.paused_offscreen = false;
            player.play();
            self.notified = Some(now);
        }
        if changed {
            if self.notified.is_none_or(|t| self.painted.get() >= t) {
                self.notified = Some(now);
            }
            cx.notify();
        }
    }

    fn state(&self) -> &'static str {
        match &self.load {
            Load::Unsupported(_) => "unsupported",
            Load::Failed(_) => "failed",
            _ if self.status.ended => "ended",
            _ if self.status.playing => "playing",
            _ if self.player.is_some() => "paused",
            Load::Fetching => "loading",
            _ if self.opening || self.play_when_ready => "loading",
            _ => "idle",
        }
    }

    fn audio_attr(&self) -> &'static str {
        if !self.status.has_audio {
            "none"
        } else if !self.status.playing {
            "off"
        } else if self.muted || self.volume <= 0.0 {
            "muted"
        } else {
            "on"
        }
    }

    /// The card's size in the message: the video's shape, from its event or
    /// from the file.
    fn card_size(&self) -> (f32, f32) {
        let (w, h) = match (self.width, self.height, self.info()) {
            (Some(w), Some(h), _) => (Some(w), Some(h)),
            (_, _, Some(info)) => (Some(info.width), Some(info.height)),
            _ => match &self.image {
                Some(image) => {
                    let size = image.size(0);
                    (u32::try_from(size.width.0).ok(), u32::try_from(size.height.0).ok())
                }
                None => (None, None),
            },
        };
        image_size(w, h)
    }

    /// The picture, filling `big` or the card, with the attributes tests read.
    /// In the full-window player (`viewer`: its focus, which takes the keys) the
    /// picture leaves the focus where it is.
    fn surface(&mut self, viewer: Option<&FocusHandle>, cx: &mut Context<Self>) -> AnyElement {
        let big = viewer.is_some();
        let image = self.image.clone();
        let radius = if big { 6.0 } else { theme::RADIUS };
        let id = SharedString::from(format!("video-surface-{}-{big}", self.id));
        let mut surface = div()
            .id(id)
            .absolute()
            .inset_0()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                if !big {
                    window.focus(&this.focus);
                }
                this.toggle(cx);
            }));
        if !big {
            surface = surface
                .track_focus(&self.focus)
                .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                    if this.key(event.keystroke.key.as_str(), cx) {
                        cx.stop_propagation();
                    }
                }));
        }
        if let Some(image) = image {
            surface = surface.child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), window, _| {
                        let fitted = ObjectFit::Contain.get_bounds(bounds, image.size(0));
                        let _ = window.paint_image(fitted, gpui::Corners::all(px(radius)), image.clone(), 0, false);
                    },
                )
                .absolute()
                .inset_0(),
            );
        }
        let ms = |d: Duration| d.as_millis().to_string();
        surface
            .e2e("div", "video-player")
            .attr("data-time", ms(self.status.position))
            .attr("data-duration", ms(self.status.duration))
            .attr("data-frame", self.frame_index.to_string())
            .attr("data-frame-time", ms(self.frame_pts))
            .attr("data-playing", if self.status.playing { "1" } else { "0" })
            .attr("data-muted", if self.muted { "1" } else { "0" })
            .attr("data-volume", format!("{:.0}", self.volume * 100.0))
            .attr("data-audio", self.audio_attr())
            .attr("data-loop", if self.status.looped { "1" } else { "0" })
            .attr("data-loops", self.status.loops.to_string())
            .attr("data-autoplay", if self.gif { "1" } else { "0" })
            .focusable(viewer.unwrap_or(&self.focus))
            .into_any_element()
    }

    /// A key on the player: Space plays or pauses, the arrows skip 5 s.
    fn key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        match key {
            "space" => self.toggle(cx),
            "left" => self.skip(false, cx),
            "right" => self.skip(true, cx),
            _ => return false,
        }
        true
    }

    fn button(
        &self,
        name: &str,
        class: &str,
        title: &str,
        glyph: &str,
        cx: &mut Context<Self>,
        act: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        div()
            .id(SharedString::from(format!("{name}-{}", self.id)))
            .size(px(28.0))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded(px(4.0))
            .cursor_pointer()
            .hover(|d| d.bg(rgba(0xffffff26)))
            .child(icon(glyph, 18.0).text_color(rgb(0xffffff)))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                cx.stop_propagation();
                act(this, window, cx);
            }))
            .e2e("button", class)
            .attr("title", title)
            .into_any_element()
    }

    /// A thin bar filled to `fraction`, the seek bar or the volume slider.
    fn slider(
        &self,
        name: &str,
        drag: Drag,
        cell: Rc<Cell<Bounds<Pixels>>>,
        layers: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(SharedString::from(format!("{name}-{}", self.id)))
            .relative()
            .h(px(14.0))
            .flex()
            .items_center()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.dragging = Some(drag);
                    this.last_drag_seek = None;
                    this.drag_to(event.position.x, cx);
                }),
            )
            .child(
                canvas(move |bounds, _, _| cell.set(bounds), |_, _, _, _| {})
                    .absolute()
                    .inset_0(),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(4.0))
                    .rounded_full()
                    .bg(rgba(0xffffff40))
                    .children(layers),
            )
    }

    fn fill(fraction: f32, color: u32, class: &str) -> AnyElement {
        let shown = (fraction * 1000.0).round() / 1000.0;
        div()
            .absolute()
            .top_0()
            .left_0()
            .h_full()
            .w(relative(fraction.clamp(0.0, 1.0)))
            .rounded_full()
            .bg(rgba(color))
            .e2e("div", class)
            .attr("data-fraction", format!("{shown}"))
            .into_any_element()
    }

    /// The bar at the bottom: seek bar over play/pause, time, mute, volume,
    /// download and full window.
    fn controls(&mut self, big: bool, cx: &mut Context<Self>) -> AnyElement {
        let duration = self.status.duration;
        let played = if duration > Duration::ZERO {
            self.status.position.as_secs_f32() / duration.as_secs_f32()
        } else {
            0.0
        };
        // The whole file is in memory once it plays.
        let buffered = if self.player.is_some() { 1.0 } else { 0.0 };
        let seek = self
            .slider(
                "video-seek",
                Drag::Seek,
                self.seek_bar.clone(),
                vec![
                    Self::fill(buffered, 0xffffff59, "video-seek-buffered"),
                    Self::fill(played, theme::alpha(theme::accent(), 0xff), "video-seek-played"),
                ],
                cx,
            )
            .w_full()
            .e2e("div", "video-seek");
        let time = format!("{} / {}", human_duration(self.status.position), human_duration(duration));
        let playing = self.status.playing;
        let (mute_glyph, mute_title) = if self.muted || self.volume <= 0.0 {
            ("volume-x", "Unmute")
        } else {
            ("volume-2", "Mute")
        };
        let volume = self
            .slider(
                "video-volume",
                Drag::Volume,
                self.volume_bar.clone(),
                vec![Self::fill(if self.muted { 0.0 } else { self.volume }, 0xffffffff, "video-volume-level")],
                cx,
            )
            .w(px(56.0))
            .flex_none()
            .e2e("div", "video-volume")
            .attr("title", "Volume");
        let mut row = div()
            .flex()
            .items_center()
            .gap(px(2.0))
            .child(self.button(
                "video-toggle",
                "video-toggle",
                if playing { "Pause" } else { "Play" },
                if playing { "pause" } else { "play" },
                cx,
                |this, _, cx| this.toggle(cx),
            ))
            .child(
                div()
                    .px(px(4.0))
                    .text_size(px(0.75 * theme::REM))
                    .text_color(rgb(0xffffff))
                    .whitespace_nowrap()
                    .child(time.clone())
                    .e2e("span", "video-time")
                    .text(time),
            )
            .child(div().flex_1());
        if self.status.has_audio {
            row = row
                .child(self.button("video-mute", "video-mute", mute_title, mute_glyph, cx, |this, _, cx| {
                    this.toggle_mute(cx)
                }))
                .child(volume);
        }
        let download_title = self.download.clone().unwrap_or_else(|| "Download".into());
        row = row
            .child(self.button("video-download", "video-download", &download_title, "download", cx, |this, _, cx| {
                this.start_download(cx)
            }))
            .child(if big {
                self.button("video-exit", "video-fullscreen", "Exit full window", "minimize-2", cx, |_, _, cx| {
                    cx.emit(VideoEvent::ExitFullscreen)
                })
            } else {
                self.button("video-full", "video-fullscreen", "Full window", "maximize-2", cx, |_, _, cx| {
                    cx.emit(VideoEvent::Fullscreen)
                })
            });
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .px(px(8.0))
            .pt(px(16.0))
            .pb(px(4.0))
            .flex()
            .flex_col()
            .bg(rgba(0x0000008c))
            // Not `occlude`: the card under it would stop counting as hovered and
            // the bar would vanish under the pointer. Presses stop here instead.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(seek)
            .child(row)
            .e2e("div", "video-controls")
            .into_any_element()
    }

    /// What lies over the picture before it plays: the play button and the
    /// length, a loading mark, or why it cannot play.
    fn overlay(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let mut overlay = div().absolute().inset_0().flex().items_center().justify_center();
        match &self.load {
            Load::Unsupported(why) | Load::Failed(why) => {
                let why = why.clone();
                return overlay
                    .flex_col()
                    .gap(px(8.0))
                    .bg(rgba(0x000000a6))
                    .child(
                        div()
                            .px(px(12.0))
                            .text_size(px(0.85 * theme::REM))
                            .text_color(rgb(0xffffff))
                            .child(why.clone())
                            .e2e("div", "video-unsupported")
                            .text(why),
                    )
                    .child(self.button("video-download", "video-download", "Download", "download", cx, |this, _, cx| {
                        this.start_download(cx)
                    }))
                    .e2e("div", "video-overlay")
                    .into_any_element();
            }
            _ => {}
        }
        overlay = overlay.bg(rgba(0x00000040));
        let probing = self.worth_probing() && matches!(self.load, Load::Idle | Load::Fetching);
        let waiting = self.play_when_ready || self.opening || (self.gif && self.player.is_none());
        if waiting || probing {
            overlay = overlay.child(
                div()
                    .size(px(48.0))
                    .rounded_full()
                    .border_4()
                    .border_color(rgba(0xffffff59))
                    .e2e("div", "video-loading"),
            );
        } else {
            overlay = overlay.child(
                div()
                    .id(SharedString::from(format!("video-play-{}", self.id)))
                    .size(px(48.0))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(rgba(theme::alpha(theme::bg_sidebar(), 0xbf)))
                    .cursor_pointer()
                    .child(icon("play", 20.0).text_color(rgb(theme::text())))
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        window.focus(&this.focus);
                        this.play(cx);
                    }))
                    .e2e("button", "play-button")
                    .attr("title", "Play")
                    .text("▶"),
            );
        }
        let length = self.duration.or_else(|| self.info().map(|i| i.duration));
        if let Some(length) = length
            && !self.gif
        {
            let label = human_duration(length);
            overlay = overlay.child(
                div()
                    .absolute()
                    .bottom(px(8.0))
                    .right(px(10.0))
                    .px(px(6.0))
                    .py(px(2.0))
                    .rounded(px(4.0))
                    .bg(rgba(0x00000099))
                    .text_size(px(0.75 * theme::REM))
                    .text_color(rgb(0xffffff))
                    .child(label.clone())
                    .e2e("span", "video-duration")
                    .text(label),
            );
        }
        overlay.e2e("div", "video-overlay").into_any_element()
    }

    /// Records that the card was drawn (it is in view).
    fn paint_marker(&self) -> AnyElement {
        let painted = self.painted.clone();
        canvas(|_, _, _| (), move |_, _, _, _| painted.set(Instant::now()))
            .absolute()
            .size_0()
            .into_any_element()
    }

    /// The player at `w` x `h` for the full-window viewer.
    pub fn big(&mut self, w: f32, h: f32, focus: &FocusHandle, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        for old in self.stale.drain(..) {
            let _ = window.drop_image(old);
        }
        let mut card = div()
            .id(SharedString::from(format!("video-big-{}", self.id)))
            .relative()
            .w(px(w))
            .h(px(h))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.hovered = *hovered;
                cx.notify();
            }))
            .child(self.paint_marker());
        if self.player.is_some() {
            card = card.child(self.surface(Some(focus), cx));
            card = card.child(self.controls(true, cx));
        }
        Self::drag_listeners(card, cx).into_any_element()
    }

    /// Size of the full-window player: the video's shape, fitted.
    pub fn shape(&self) -> (f32, f32) {
        let (w, h) = match &self.image {
            Some(image) => {
                let s = image.size(0);
                (s.width.0 as f32, s.height.0 as f32)
            }
            None => self.card_size(),
        };
        (w.max(1.0), h.max(1.0))
    }

    /// A drag on the seek bar or volume slider follows the pointer over the
    /// whole card and ends wherever the button is let go.
    fn drag_listeners(card: gpui::Stateful<gpui::Div>, cx: &mut Context<Self>) -> gpui::Stateful<gpui::Div> {
        card.on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
            if this.dragging.is_some() {
                this.drag_to(event.position.x, cx);
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, event: &MouseUpEvent, _, cx| this.end_drag(Some(event.position.x), cx)),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|this, _: &MouseUpEvent, _, cx| this.end_drag(None, cx)),
        )
    }
}

impl Render for VideoPlayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for old in self.stale.drain(..) {
            let _ = window.drop_image(old);
        }
        if self.worth_probing() && matches!(self.load, Load::Idle) {
            self.fetch(cx);
        }
        let (w, h) = self.card_size();
        let mut card = div()
            .id(SharedString::from(format!("video-card-{}", self.id)))
            .relative()
            .mt(px(4.0))
            .min_w_0()
            .rounded(px(theme::RADIUS))
            .overflow_hidden()
            .bg(rgb(theme::bg_input()))
            .child(Shape { w, h })
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.hovered = *hovered;
                cx.notify();
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.dragging.is_some() {
                    this.drag_to(event.position.x, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _, cx| this.end_drag(Some(event.position.x), cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.end_drag(None, cx)),
            )
            .child(self.paint_marker());
        let playing_here = self.player.is_some() && self.image.is_some();
        if playing_here {
            card = card.child(self.surface(None, cx));
            if self.gif {
                card = card.child(
                    div()
                        .absolute()
                        .top(px(8.0))
                        .left(px(8.0))
                        .px(px(6.0))
                        .py(px(1.0))
                        .rounded(px(4.0))
                        .bg(rgba(0x00000099))
                        .text_size(px(0.7 * theme::REM))
                        .text_color(rgb(0xffffff))
                        .child("GIF")
                        .e2e("span", "video-gif")
                        .text("GIF"),
                );
            } else if self.hovered || !self.status.playing || self.dragging.is_some() {
                card = card.child(self.controls(false, cx));
            }
        } else {
            card = match self.thumbnail.clone() {
                Some(image) => card.child(
                    img(crate::media::covering(&image, w, h))
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .rounded(px(theme::RADIUS))
                        .object_fit(ObjectFit::Fill)
                        .e2e("img", "video-thumb")
                        .attr("alt", self.body.clone())
                        .attr("title", self.body.clone()),
                ),
                None => card.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(rgb(theme::text_faint()))
                        .text_size(px(0.85 * theme::REM))
                        .child(self.body.clone())
                        .e2e("div", "image placeholder")
                        .text(self.body.clone()),
                ),
            };
            card = card.child(self.overlay(cx));
        }
        div().flex().child(
            card.e2e("div", "video-card")
                .attr("data-body", self.body.clone())
                .attr("data-state", self.state()),
        )
    }
}

/// The full-window player: the message's video over the dimmed window,
/// playing on where it was. Escape, a click outside or its button closes it.
pub struct VideoViewer {
    player: gpui::Entity<VideoPlayer>,
    focus: FocusHandle,
    take_focus: bool,
}

impl EventEmitter<Closed> for VideoViewer {}

impl VideoViewer {
    pub fn new(player: gpui::Entity<VideoPlayer>, cx: &mut Context<Self>) -> Self {
        cx.observe(&player, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&player, |_, _, event: &VideoEvent, cx| {
            if matches!(event, VideoEvent::ExitFullscreen) {
                cx.emit(Closed);
            }
        })
        .detach();
        Self {
            player,
            focus: cx.focus_handle(),
            take_focus: true,
        }
    }
}

impl Render for VideoViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.take_focus) {
            window.focus(&self.focus);
        }
        let viewport = window.viewport_size();
        let (avail_w, avail_h) = (0.92 * f32::from(viewport.width), 0.88 * f32::from(viewport.height));
        let (w, h) = self.player.read(cx).shape();
        let scale = (avail_w / w).min(avail_h / h);
        let (w, h) = ((w * scale).floor(), (h * scale).floor());
        let focus = self.focus.clone();
        let big = self.player.update(cx, |player, cx| player.big(w, h, &focus, window, cx));
        let player = self.player.clone();
        let layer = div()
            .id("video-viewer")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(move |_, event: &gpui::KeyDownEvent, _, cx| {
                let key = event.keystroke.key.as_str();
                if key == "escape" {
                    cx.emit(Closed);
                    return;
                }
                player.update(cx, |player, cx| {
                    player.key(key, cx);
                });
            }))
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x000000e6))
            .occlude()
            .on_mouse_down(MouseButton::Left, cx.listener(|_, _, _, cx| cx.emit(Closed)))
            .child(
                div()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(big),
            )
            .e2e("div", "overlay video-viewer")
            .focusable(&focus);
        deferred(layer).with_priority(30)
    }
}

impl crate::shell::Shell {
    /// Opens the full-window player on a message's video.
    pub fn open_video(&mut self, player: gpui::Entity<VideoPlayer>, cx: &mut Context<Self>) {
        let viewer = cx.new(|cx| VideoViewer::new(player, cx));
        cx.subscribe(&viewer, |this, _, _: &Closed, cx| this.dialog_close(cx)).detach();
        self.dialog_open(crate::dialogs::Dialog::Video(viewer), cx);
    }
}
