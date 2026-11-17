//! The message list's scrollbar (#144): where the thumb is and how long, what
//! dragging it or clicking the track does, how it looks while idle, hovered
//! and dragged. The list is virtual (rows are measured as they are drawn and
//! more come in at the top), so the range it reports moves under the thumb:
//! the thumb's length follows it only a little per frame ([`ease_len`]) and
//! is never shorter than [`MIN_THUMB`].

/// The shortest the thumb gets, px.
pub const MIN_THUMB: f32 = 32.0;
/// The thumb's width at rest and hovered or dragged, px.
pub const WIDTH: f32 = 6.0;
pub const WIDTH_ACTIVE: f32 = 10.0;
/// How long the thumb stays after the last scroll, move or drag, and how long
/// it then takes to fade out, ms.
pub const LINGER_MS: u64 = 1200;
pub const FADE_MS: u64 = 400;

/// The thumb inside a track of `track` px, for a list whose viewport is
/// `viewport` px and which scrolls `max_offset` px, now at `offset`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thumb {
    pub len: f32,
    pub top: f32,
}

/// `None` where the list does not scroll.
pub fn thumb(viewport: f32, max_offset: f32, offset: f32, track: f32) -> Option<Thumb> {
    if max_offset < 1.0 || viewport <= 0.0 || track <= 0.0 {
        return None;
    }
    let content = max_offset + viewport;
    let len = (viewport / content * track).clamp(MIN_THUMB.min(track), track);
    let share = (offset / max_offset).clamp(0.0, 1.0);
    Some(Thumb { len, top: share * (track - len) })
}

/// The thumb's length one frame on: it moves toward `target` by at most
/// [`MAX_STEP`] px, so a range that is re-estimated as rows are measured does
/// not make it jump.
pub const MAX_STEP: f32 = 4.0;
pub fn ease_len(shown: f32, target: f32) -> f32 {
    if (target - shown).abs() <= 1.0 {
        target
    } else {
        shown + (target - shown).clamp(-MAX_STEP, MAX_STEP)
    }
}

/// The offset a drag puts the list at: the pointer is `pointer_y` in the
/// window, the track starts at `track_top`, and the thumb was grabbed `grab` px
/// below its top.
pub fn offset_for_drag(
    pointer_y: f32,
    grab: f32,
    track_top: f32,
    track: f32,
    thumb_len: f32,
    max_offset: f32,
) -> f32 {
    let room = (track - thumb_len).max(1.0);
    let top = (pointer_y - grab - track_top).clamp(0.0, room);
    top / room * max_offset
}

/// Where a click on the track at `click_y` (from the track's top) goes: a page
/// up when it is above the thumb, a page down when below; `None` on the thumb.
pub fn page_target(offset: f32, viewport: f32, max_offset: f32, click_y: f32, thumb: Thumb) -> Option<f32> {
    let page = viewport * 0.9;
    if click_y < thumb.top {
        Some((offset - page).clamp(0.0, max_offset))
    } else if click_y > thumb.top + thumb.len {
        Some((offset + page).clamp(0.0, max_offset))
    } else {
        None
    }
}

/// The thumb's opacity (0 to 1): full while hovered or dragged and for
/// [`LINGER_MS`] after, then fading out over [`FADE_MS`].
pub fn opacity(idle_ms: u64, engaged: bool) -> f32 {
    if engaged || idle_ms <= LINGER_MS {
        1.0
    } else {
        (1.0 - (idle_ms - LINGER_MS) as f32 / FADE_MS as f32).clamp(0.0, 1.0)
    }
}

/// The thumb's width: wider while hovered or dragged.
pub fn width(engaged: bool) -> f32 {
    if engaged { WIDTH_ACTIVE } else { WIDTH }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_that_does_not_scroll_has_no_thumb() {
        assert_eq!(thumb(500.0, 0.0, 0.0, 500.0), None);
    }

    #[test]
    fn the_thumb_is_the_viewports_share_of_the_content_and_sits_at_the_offset() {
        // 500 px viewport of 2000 px of content: a quarter of the track.
        let top = thumb(500.0, 1500.0, 0.0, 500.0).expect("thumb");
        assert_eq!((top.len, top.top), (125.0, 0.0));
        let bottom = thumb(500.0, 1500.0, 1500.0, 500.0).expect("thumb");
        assert_eq!((bottom.len, bottom.top + bottom.len), (125.0, 500.0));
        let middle = thumb(500.0, 1500.0, 750.0, 500.0).expect("thumb");
        assert_eq!(middle.top, 187.5);
        // An offset out of range (rows still being measured) stays on the track.
        let past = thumb(500.0, 1500.0, 1800.0, 500.0).expect("thumb");
        assert_eq!(past.top + past.len, 500.0);
    }

    #[test]
    fn a_very_long_list_keeps_a_thumb_you_can_grab() {
        let t = thumb(500.0, 500_000.0, 0.0, 500.0).expect("thumb");
        assert_eq!(t.len, MIN_THUMB);
    }

    #[test]
    fn the_length_follows_a_moving_range_a_few_px_at_a_time() {
        let mut shown: f32 = 125.0;
        let mut steps = 0;
        while (shown - 40.0).abs() > f32::EPSILON {
            let next = ease_len(shown, 40.0);
            assert!((next - shown).abs() <= MAX_STEP + f32::EPSILON, "{shown} to {next}");
            shown = next;
            steps += 1;
            assert!(steps < 100);
        }
        assert_eq!(ease_len(40.0, 40.0), 40.0);
        assert_eq!(ease_len(40.0, 41.0), 41.0, "a small change is taken at once");
    }

    #[test]
    fn dragging_puts_the_offset_where_the_thumb_is() {
        // Track 500 px from y 100, thumb 100 px, 1500 px to scroll, grabbed 20 px in.
        let at = |pointer| offset_for_drag(pointer, 20.0, 100.0, 500.0, 100.0, 1500.0);
        assert_eq!(at(120.0), 0.0, "the thumb at the top");
        assert_eq!(at(520.0), 1500.0, "the thumb at the bottom");
        assert_eq!(at(320.0), 750.0, "halfway");
        assert_eq!(at(-900.0), 0.0, "past the top");
        assert_eq!(at(9000.0), 1500.0, "past the bottom");
    }

    #[test]
    fn clicking_the_track_goes_a_page_toward_the_click() {
        let t = Thumb { len: 100.0, top: 200.0 };
        assert_eq!(page_target(600.0, 500.0, 1500.0, 50.0, t), Some(150.0), "above: a page up (90%)");
        assert_eq!(page_target(600.0, 500.0, 1500.0, 450.0, t), Some(1050.0), "below: a page down");
        assert_eq!(page_target(100.0, 500.0, 1500.0, 50.0, t), Some(0.0), "not past the top");
        assert_eq!(page_target(1400.0, 500.0, 1500.0, 450.0, t), Some(1500.0), "not past the end");
        assert_eq!(page_target(600.0, 500.0, 1500.0, 250.0, t), None, "on the thumb");
    }

    #[test]
    fn it_fades_out_when_idle_and_stays_while_engaged() {
        assert_eq!(opacity(0, false), 1.0);
        assert_eq!(opacity(LINGER_MS, false), 1.0);
        let half = opacity(LINGER_MS + FADE_MS / 2, false);
        assert!(half > 0.4 && half < 0.6, "{half}");
        assert_eq!(opacity(LINGER_MS + FADE_MS, false), 0.0);
        assert_eq!(opacity(60_000, false), 0.0);
        assert_eq!(opacity(60_000, true), 1.0, "hovered or dragged");
    }

    #[test]
    fn it_widens_when_engaged() {
        assert!(width(true) > width(false));
        assert_eq!((width(false), width(true)), (WIDTH, WIDTH_ACTIVE));
    }
}
