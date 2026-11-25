//! When a timeline offers "Jump to present" (#414): once the view is further than
//! [`JUMP_THRESHOLD_PX`] from the newest message. The desktop measures that distance;
//! the phone's list only knows its first visible row and how far that is scrolled, so
//! [`away_from_present`] estimates it from those.

/// How far from the newest message the view must be to offer the jump, in logical pixels.
pub const JUMP_THRESHOLD_PX: f32 = 240.0;
/// A message row's height where it is not known (rows above the first visible one are not laid out).
pub const ROW_ESTIMATE_PX: f32 = 56.0;

/// Whether the view is far enough from the newest message: `rows_above` rows lie between the
/// newest row and the first visible one, which is scrolled `offset_px` past its start.
pub fn away_from_present(rows_above: u32, offset_px: f32) -> bool {
    rows_above as f32 * ROW_ESTIMATE_PX + offset_px.max(0.0) > JUMP_THRESHOLD_PX
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_messages_in_view_offer_no_jump() {
        assert!(!away_from_present(0, 0.0));
        assert!(!away_from_present(0, 200.0));
        assert!(!away_from_present(2, 0.0));
    }

    #[test]
    fn scrolled_up_by_a_few_rows_offers_the_jump() {
        assert!(away_from_present(5, 0.0));
        assert!(away_from_present(0, 300.0));
        assert!(away_from_present(40, 0.0));
    }
}
