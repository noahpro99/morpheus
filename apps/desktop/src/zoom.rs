//! The zoom and pan of a picture in a box (the picture viewer, a shared
//! screen in a call): 1 is the picture fitted to the box, up to `MAX_ZOOM`;
//! the pan is where the picture's middle is from the box's middle. Zooming
//! keeps the point under the pointer where it is, and the pan keeps the
//! picture covering the box.

use gpui::ScrollDelta;

pub const MAX_ZOOM: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoomPan {
    pub zoom: f32,
    pub offset: (f32, f32),
}

impl Default for ZoomPan {
    fn default() -> Self {
        Self { zoom: 1.0, offset: (0.0, 0.0) }
    }
}

impl ZoomPan {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Whether it is zoomed in at all.
    pub fn zoomed(&self) -> bool {
        self.zoom > 1.001
    }

    /// Keeps the picture from being dragged off the box: along an axis where
    /// it (`fit` times the zoom) is bigger than the box its edges stay at or
    /// past the box's; where it is smaller it stays in the middle.
    pub fn clamp(&mut self, fit: (f32, f32), viewport: (f32, f32)) {
        let max_x = ((fit.0 * self.zoom - viewport.0) / 2.0).max(0.0);
        let max_y = ((fit.1 * self.zoom - viewport.1) / 2.0).max(0.0);
        self.offset.0 = self.offset.0.clamp(-max_x, max_x);
        self.offset.1 = self.offset.1.clamp(-max_y, max_y);
    }

    /// Zooms to `to` (1 to 8) keeping the point of the picture under `at`
    /// (the box's coordinates) where it is.
    pub fn zoom_at(&mut self, to: f32, at: (f32, f32), fit: (f32, f32), viewport: (f32, f32)) {
        let to = to.clamp(1.0, MAX_ZOOM);
        let to = if to < 1.001 { 1.0 } else { to };
        let from = self.zoom;
        let (mx, my) = (viewport.0 / 2.0, viewport.1 / 2.0);
        let (rx, ry) = (at.0 - mx - self.offset.0, at.1 - my - self.offset.1);
        self.offset = (at.0 - mx - rx * to / from, at.1 - my - ry * to / from);
        self.zoom = to;
        if to == 1.0 {
            self.offset = (0.0, 0.0);
        }
        self.clamp(fit, viewport);
    }

    /// What a wheel turn multiplies the zoom by (`None`: it did not turn).
    pub fn wheel_factor(delta: ScrollDelta) -> Option<f32> {
        let amount = match delta {
            ScrollDelta::Lines(lines) => lines.y * 0.15,
            ScrollDelta::Pixels(pixels) => f32::from(pixels.y) * 0.002,
        };
        (amount != 0.0).then(|| amount.exp())
    }
}
