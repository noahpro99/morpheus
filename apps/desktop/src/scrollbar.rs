//! The message list's scrollbar (#144): the state it keeps and what a frame
//! of it is (the numbers are `view_model::scrollbar`'s); `TimelineView`
//! draws it over the list. A thin rounded thumb at the right edge in theme
//! colours: it widens and brightens when the pointer is on its track or drags
//! it, and fades out when nothing has moved for a while. Dragging the thumb
//! scrolls the list; clicking the track goes a page toward the click.

use std::{cell::Cell, rc::Rc, time::Instant};

use gpui::{Bounds, ListState, MouseMoveEvent, Pixels, Window, point, px};

use crate::view_model::scrollbar as vm;

/// What the scrollbar keeps between frames.
pub struct Bar {
    /// The pointer is over the message list.
    pub over_list: bool,
    /// The pointer is over the bar's track.
    pub over_track: bool,
    /// Dragging the thumb: how far below its top it was grabbed.
    pub grab: Option<f32>,
    /// When the list last moved or the pointer last did something here.
    pub active_at: Instant,
    /// The thumb's length as drawn (it follows the range a little per frame).
    pub len: Option<f32>,
    last_offset: f32,
    /// The track's bounds, as last drawn.
    pub bounds: Rc<Cell<Bounds<Pixels>>>,
    /// A repaint is waiting for the thumb to start fading.
    pub timer: bool,
    /// Frames asked for while the list had no range yet.
    pub retries: u8,
    pub seen_rows: usize,
    /// Where the thumb was drawn last frame (for grabbing it).
    pub top: f32,
}

impl Default for Bar {
    fn default() -> Self {
        Self {
            over_list: false,
            over_track: false,
            grab: None,
            active_at: Instant::now(),
            len: None,
            last_offset: 0.0,
            bounds: Rc::default(),
            timer: false,
            retries: 0,
            seen_rows: 0,
            top: 0.0,
        }
    }
}

/// One frame of the bar.
pub struct Frame {
    pub len: f32,
    pub top: f32,
    pub opacity: f32,
    pub wide: bool,
    pub engaged: bool,
    /// Whether another frame is wanted at once (the length easing, a fade).
    pub animating: bool,
    /// Milliseconds until the thumb starts to fade, when it is lingering.
    pub fade_in_ms: Option<u64>,
}

/// The list's scroll numbers: (viewport, most it scrolls, offset from the top).
pub fn numbers(list: &ListState) -> (f32, f32, f32) {
    let max = f32::from(list.max_offset_for_scrollbar().height).max(0.0);
    let offset = (-f32::from(list.scroll_px_offset_for_scrollbar().y)).clamp(0.0, max);
    (f32::from(list.viewport_bounds().size.height), max, offset)
}

/// Scroll the list to `offset` px from the top.
pub fn scroll_to(list: &ListState, offset: f32) {
    list.set_offset_from_scrollbar(point(px(0.0), px(-offset)));
}

impl Bar {
    pub fn engaged(&self) -> bool {
        self.over_list || self.over_track || self.grab.is_some()
    }

    pub fn touch(&mut self) {
        self.active_at = Instant::now();
    }

    /// This frame's bar; `None` where the list does not scroll.
    pub fn frame(&mut self, list: &ListState) -> Option<Frame> {
        let (viewport, max, offset) = numbers(list);
        if (offset - self.last_offset).abs() > 0.5 {
            self.touch();
            self.last_offset = offset;
        }
        let track = {
            let h = f32::from(self.bounds.get().size.height);
            if h > 0.0 { h } else { viewport }
        };
        let target = vm::thumb(viewport, max, offset, track)?;
        let len = vm::ease_len(self.len.unwrap_or(target.len), target.len);
        self.len = Some(len);
        let share = if max > 0.0 { offset / max } else { 0.0 };
        let top = share.clamp(0.0, 1.0) * (track - len).max(0.0);
        self.top = top;
        let engaged = self.engaged();
        let idle = self.active_at.elapsed().as_millis() as u64;
        let opacity = vm::opacity(idle, engaged);
        let animating = (len - target.len).abs() > f32::EPSILON || (opacity > 0.0 && opacity < 1.0);
        let fade_in_ms = (!animating && opacity >= 1.0 && !engaged)
            .then(|| vm::LINGER_MS.saturating_sub(idle) + 30);
        Some(Frame {
            len,
            top,
            opacity,
            wide: self.over_track || self.grab.is_some(),
            engaged,
            animating,
            fade_in_ms,
        })
    }

    /// The thumb is grabbed at window height `y`.
    pub fn grab_at(&mut self, list: &ListState, y: f32) {
        let origin = f32::from(self.bounds.get().origin.y);
        let len = self.len.unwrap_or(vm::MIN_THUMB);
        self.grab = Some((y - origin - self.top).clamp(0.0, len));
        self.touch();
        list.scrollbar_drag_started();
    }

    /// Mouse moved with the thumb held: the list follows.
    pub fn drag_to(&mut self, list: &ListState, event: &MouseMoveEvent) {
        let Some(grab) = self.grab else { return };
        let (_, max, _) = numbers(list);
        let track = self.bounds.get();
        let offset = vm::offset_for_drag(
            f32::from(event.position.y),
            grab,
            f32::from(track.origin.y),
            f32::from(track.size.height),
            self.len.unwrap_or(vm::MIN_THUMB),
            max,
        );
        scroll_to(list, offset);
        self.touch();
    }

    pub fn end_drag(&mut self, list: &ListState) {
        if self.grab.take().is_some() {
            list.scrollbar_drag_ended();
            self.touch();
        }
    }

    /// A click on the track at window height `y`: a page toward it.
    pub fn click_track(&mut self, list: &ListState, y: f32) {
        let (viewport, max, offset) = numbers(list);
        let thumb = vm::Thumb { len: self.len.unwrap_or(vm::MIN_THUMB), top: self.top };
        let rel = y - f32::from(self.bounds.get().origin.y);
        if let Some(to) = vm::page_target(offset, viewport, max, rel, thumb) {
            scroll_to(list, to);
        }
        self.touch();
    }
}

/// Keep the track's bounds (a request for [`Window`] frames goes with it).
pub fn want_frame(window: &mut Window) {
    window.request_animation_frame();
}
