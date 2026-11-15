//! The pictures a live video (a call's tile, a video playing in a message)
//! hands to GPUI, one new `RenderImage` per frame. A picture that was painted
//! sits in the window's sprite atlas until `Window::drop_image` takes it out,
//! which needs the window, so replaced pictures wait for the next render.
//!
//! Only pictures that were painted wait: one replaced before it was ever
//! painted never reached the atlas and is let go at once. A tile that is not
//! drawn (a call out of view, the window hidden, minimised or covered by
//! another one; macOS stops drawing an occluded window altogether) used to
//! keep every frame it was given until it was drawn again: a shared screen
//! at 2560 px wide and 30 frames a second is 440 MB a second (issue #20).
//! Now a tile holds at most its current picture and one painted one.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use gpui::{RenderImage, Window};

/// Pictures held by every [`Pictures`] (shown, waiting, orphaned).
static HELD: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// Painted pictures of tiles that went away, for the next render to
    /// take out of the atlas.
    static ORPHANS: RefCell<Vec<Arc<RenderImage>>> = const { RefCell::new(Vec::new()) };
    static LOGGED: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// How many pictures all live videos hold (for the log and tests).
pub fn held() -> usize {
    HELD.load(Ordering::Relaxed)
}

/// The latest picture of one video and those replaced that were painted.
#[derive(Default)]
pub struct Pictures {
    shown: Option<(Arc<RenderImage>, Rc<Cell<bool>>)>,
    stale: Vec<Arc<RenderImage>>,
}

impl Pictures {
    /// `image` is the picture now: the one before waits to leave the atlas
    /// if it was painted, else goes now.
    pub fn show(&mut self, image: Arc<RenderImage>) {
        HELD.fetch_add(1, Ordering::Relaxed);
        if let Some((old, painted)) = self.shown.replace((image, Rc::default())) {
            if painted.get() {
                self.stale.push(old);
            } else {
                HELD.fetch_sub(1, Ordering::Relaxed);
            }
        }
        log_now_and_then();
    }

    /// The picture now.
    pub fn image(&self) -> Option<&Arc<RenderImage>> {
        self.shown.as_ref().map(|(image, _)| image)
    }

    /// The picture now, and a flag to set when painting it.
    pub fn to_paint(&self) -> Option<(Arc<RenderImage>, Rc<Cell<bool>>)> {
        self.shown.clone()
    }

    /// Pictures held: the one shown and those waiting.
    pub fn len(&self) -> usize {
        self.stale.len() + usize::from(self.shown.is_some())
    }

    /// Takes the replaced pictures (and orphans) out of `window`'s atlas.
    pub fn drop_stale(&mut self, window: &mut Window) {
        self.take_stale(|image| {
            let _ = window.drop_image(image);
        });
        drop_orphans(window);
    }

    fn take_stale(&mut self, mut each: impl FnMut(Arc<RenderImage>)) {
        for old in self.stale.drain(..) {
            HELD.fetch_sub(1, Ordering::Relaxed);
            each(old);
        }
    }
}

impl Drop for Pictures {
    fn drop(&mut self) {
        let mut painted: Vec<_> = self.stale.drain(..).collect();
        if let Some((image, was_painted)) = self.shown.take() {
            if was_painted.get() {
                painted.push(image);
            } else {
                HELD.fetch_sub(1, Ordering::Relaxed);
            }
        }
        if !painted.is_empty() {
            // Still counted in HELD until a render takes them out.
            let _ = ORPHANS.try_with(|o| o.borrow_mut().extend(painted));
        }
    }
}

/// Takes the pictures of videos that went away out of `window`'s atlas.
pub fn drop_orphans(window: &mut Window) {
    let orphans = ORPHANS
        .try_with(|o| std::mem::take(&mut *o.borrow_mut()))
        .unwrap_or_default();
    for image in orphans {
        HELD.fetch_sub(1, Ordering::Relaxed);
        let _ = window.drop_image(image);
    }
}

/// Every half minute while videos play: how many pictures they hold, so a
/// log shows whether that stays small (a few per video shown).
fn log_now_and_then() {
    let now = Instant::now();
    let due = LOGGED
        .try_with(|at| {
            let due = at
                .get()
                .is_none_or(|t| now.duration_since(t) >= Duration::from_secs(30));
            if due {
                at.set(Some(now));
            }
            due
        })
        .unwrap_or(false);
    if due {
        eprintln!(
            "morpheus-desktop: video: {} pictures held for drawing",
            held()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture() -> Arc<RenderImage> {
        let buffer = image::RgbaImage::from_raw(2, 2, vec![0; 16]).expect("2x2");
        Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
    }

    /// A tile that is never drawn (a call out of view, a hidden window)
    /// keeps only its latest picture however many frames come.
    #[test]
    fn frames_not_drawn_are_not_kept() {
        let mut pictures = Pictures::default();
        for _ in 0..1000 {
            pictures.show(picture());
        }
        assert_eq!(pictures.len(), 1);
    }

    /// Drawn now and then: the painted ones wait for the next render, no
    /// more than one per render.
    #[test]
    fn painted_frames_wait_for_one_render() {
        let mut pictures = Pictures::default();
        let mut dropped = 0;
        for frame in 0..1000 {
            pictures.show(picture());
            // Painted every third frame, rendered (and so cleaned) every tenth.
            if frame % 3 == 0
                && let Some((_, painted)) = pictures.to_paint()
            {
                painted.set(true);
            }
            assert!(
                pictures.len() <= 5,
                "{} pictures held at frame {frame}",
                pictures.len()
            );
            if frame % 10 == 0 {
                pictures.take_stale(|_| dropped += 1);
            }
        }
        assert!(dropped > 0, "painted pictures leave the atlas");
    }

    /// The one painted last waits when it is replaced, until a render.
    #[test]
    fn a_painted_frame_is_taken_out_of_the_atlas() {
        let mut pictures = Pictures::default();
        pictures.show(picture());
        let first = pictures.to_paint().expect("shown");
        first.1.set(true);
        pictures.show(picture());
        let mut out = Vec::new();
        pictures.take_stale(|image| out.push(image.id));
        assert_eq!(out, vec![first.0.id]);
        assert_eq!(pictures.len(), 1);
    }
}
