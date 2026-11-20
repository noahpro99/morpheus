//! The maths of the profile picture crop: a square frame over a picture,
//! panned by dragging and zoomed by the wheel or a slider.
//!
//! Zoom 1.0 is the smallest: the picture just covers the frame (its shorter
//! side fills it). Zooming in shows a smaller part of the picture. The
//! crop's centre is kept inside the picture so the frame never shows empty
//! space.

/// The largest zoom, as a multiple of the smallest.
pub const MAX_ZOOM: f32 = 5.0;

/// Side of the square picture that is uploaded, in pixels.
pub const OUTPUT_SIDE: u32 = 512;

/// Side of a custom emoji cut from a picture.
pub const EMOJI_SIDE: u32 = 128;

/// The square part of a picture to keep, in the picture's own pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceRect {
    pub x: u32,
    pub y: u32,
    pub side: u32,
}

/// Where the frame looks at a picture of `width` x `height` pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    width: f32,
    height: f32,
    zoom: f32,
    /// The picture's pixel at the middle of the frame.
    cx: f32,
    cy: f32,
}

impl Crop {
    /// Centred, at the smallest zoom. A picture with no size crops nothing.
    pub fn new(width: u32, height: u32) -> Self {
        let (width, height) = (width.max(1) as f32, height.max(1) as f32);
        Self {
            width,
            height,
            zoom: 1.0,
            cx: width / 2.0,
            cy: height / 2.0,
        }
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// The zoom as a fraction of the slider's run (0 at the smallest zoom, 1 at the largest).
    pub fn zoom_fraction(&self) -> f32 {
        (self.zoom - 1.0) / (MAX_ZOOM - 1.0)
    }

    /// Zooms to the slider's `fraction` of its run.
    pub fn set_zoom_fraction(&mut self, fraction: f32) {
        self.set_zoom(1.0 + fraction.clamp(0.0, 1.0) * (MAX_ZOOM - 1.0));
    }

    /// Side of the part of the picture the frame shows, in picture pixels.
    pub fn side(&self) -> f32 {
        self.width.min(self.height) / self.zoom
    }

    /// Sets the zoom (clamped to 1.0 ..= [`MAX_ZOOM`]), keeping the middle.
    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = if zoom.is_finite() {
            zoom.clamp(1.0, MAX_ZOOM)
        } else {
            1.0
        };
        self.clamp();
    }

    /// Multiplies the zoom by `factor` (a wheel notch).
    pub fn zoom_by(&mut self, factor: f32) {
        self.set_zoom(self.zoom * factor);
    }

    /// Screen pixels per picture pixel when the frame is `frame` pixels wide.
    pub fn scale(&self, frame: f32) -> f32 {
        frame / self.side()
    }

    /// Drags the picture by `dx`, `dy` screen pixels in a frame `frame` wide.
    pub fn pan_by(&mut self, dx: f32, dy: f32, frame: f32) {
        let scale = self.scale(frame);
        self.cx -= dx / scale;
        self.cy -= dy / scale;
        self.clamp();
    }

    /// Where the picture's top left corner is in a frame `frame` wide, in
    /// screen pixels from the frame's own top left (zero or negative).
    pub fn offset(&self, frame: f32) -> (f32, f32) {
        let scale = self.scale(frame);
        (frame / 2.0 - self.cx * scale, frame / 2.0 - self.cy * scale)
    }

    /// The picture's size on screen in a frame `frame` wide.
    pub fn shown_size(&self, frame: f32) -> (f32, f32) {
        let scale = self.scale(frame);
        (self.width * scale, self.height * scale)
    }

    /// The square to cut out, always inside the picture.
    pub fn source_rect(&self) -> SourceRect {
        let limit = self.width.min(self.height).floor() as u32;
        let side = (self.side().round() as u32).clamp(1, limit.max(1));
        let x = (self.cx - side as f32 / 2.0).round().max(0.0) as u32;
        let y = (self.cy - side as f32 / 2.0).round().max(0.0) as u32;
        SourceRect {
            x: x.min(self.width as u32 - side.min(self.width as u32)),
            y: y.min(self.height as u32 - side.min(self.height as u32)),
            side,
        }
    }

    fn clamp(&mut self) {
        let half = self.side() / 2.0;
        self.cx = self.cx.clamp(half, (self.width - half).max(half));
        self.cy = self.cy.clamp(half, (self.height - half).max(half));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_centred_covering_the_frame() {
        let crop = Crop::new(400, 200);
        assert_eq!(crop.zoom(), 1.0);
        assert_eq!(
            crop.source_rect(),
            SourceRect {
                x: 100,
                y: 0,
                side: 200
            }
        );
        // The shorter side fills the frame; the longer overflows evenly.
        assert_eq!(crop.shown_size(280.0), (560.0, 280.0));
        assert_eq!(crop.offset(280.0), (-140.0, 0.0));
    }

    #[test]
    fn zoom_cannot_go_below_covering_or_above_the_maximum() {
        let mut crop = Crop::new(400, 200);
        crop.set_zoom(0.2);
        assert_eq!(crop.zoom(), 1.0);
        crop.set_zoom(100.0);
        assert_eq!(crop.zoom(), MAX_ZOOM);
        crop.set_zoom(f32::NAN);
        assert_eq!(crop.zoom(), 1.0);
        crop.zoom_by(2.0);
        assert_eq!(crop.zoom(), 2.0);
        crop.zoom_by(0.1);
        assert_eq!(crop.zoom(), 1.0);
    }

    #[test]
    fn zooming_in_shows_a_smaller_square_round_the_same_middle() {
        let mut crop = Crop::new(400, 200);
        crop.set_zoom(2.0);
        assert_eq!(
            crop.source_rect(),
            SourceRect {
                x: 150,
                y: 50,
                side: 100
            }
        );
    }

    #[test]
    fn panning_moves_the_picture_with_the_pointer_and_stops_at_its_edges() {
        let mut crop = Crop::new(400, 200);
        // Dragging the picture left shows more of its right side.
        crop.pan_by(-70.0, 0.0, 280.0);
        assert_eq!(crop.source_rect().x, 150);
        crop.pan_by(-10_000.0, 0.0, 280.0);
        assert_eq!(crop.source_rect().x, 200);
        crop.pan_by(10_000.0, 0.0, 280.0);
        assert_eq!(crop.source_rect().x, 0);
        // The short side has no room to move at the smallest zoom.
        crop.pan_by(0.0, 500.0, 280.0);
        assert_eq!(crop.source_rect().y, 0);
    }

    #[test]
    fn zooming_out_pulls_the_crop_back_inside_the_picture() {
        let mut crop = Crop::new(400, 200);
        crop.set_zoom(4.0);
        crop.pan_by(-10_000.0, -10_000.0, 280.0);
        assert_eq!(
            crop.source_rect(),
            SourceRect {
                x: 350,
                y: 150,
                side: 50
            }
        );
        crop.set_zoom(1.0);
        assert_eq!(
            crop.source_rect(),
            SourceRect {
                x: 200,
                y: 0,
                side: 200
            }
        );
    }

    #[test]
    fn the_frame_is_always_filled_by_the_picture() {
        let mut crop = Crop::new(300, 600);
        for zoom in [1.0, 1.7, 3.0, MAX_ZOOM] {
            crop.set_zoom(zoom);
            for (dx, dy) in [(-9e3, -9e3), (9e3, 9e3), (13.0, -40.0)] {
                crop.pan_by(dx, dy, 280.0);
                let (ox, oy) = crop.offset(280.0);
                let (w, h) = crop.shown_size(280.0);
                assert!(ox <= 0.001 && oy <= 0.001, "{ox} {oy}");
                assert!(ox + w >= 279.99 && oy + h >= 279.99, "{ox} {oy} {w} {h}");
                let rect = crop.source_rect();
                assert!(rect.x + rect.side <= 300 && rect.y + rect.side <= 600);
            }
        }
    }

    #[test]
    fn the_slider_maps_onto_the_zoom_range() {
        let mut crop = Crop::new(100, 100);
        crop.set_zoom_fraction(0.0);
        assert_eq!(crop.zoom(), 1.0);
        crop.set_zoom_fraction(1.0);
        assert_eq!(crop.zoom(), MAX_ZOOM);
        crop.set_zoom_fraction(0.5);
        assert!((crop.zoom_fraction() - 0.5).abs() < 1e-6);
        crop.set_zoom_fraction(7.0);
        assert_eq!(crop.zoom(), MAX_ZOOM);
    }

    #[test]
    fn a_tiny_or_square_picture_still_gives_a_valid_rect() {
        let crop = Crop::new(1, 1);
        assert_eq!(
            crop.source_rect(),
            SourceRect {
                x: 0,
                y: 0,
                side: 1
            }
        );
        let crop = Crop::new(64, 64);
        assert_eq!(
            crop.source_rect(),
            SourceRect {
                x: 0,
                y: 0,
                side: 64
            }
        );
    }
}
