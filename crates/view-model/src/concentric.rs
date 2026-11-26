//! Concentric shapes (#505). Anything inside a rounded container is concentric with it, so
//! the gap between an inner curve and the outer one is the same all the way round.
//!
//! - An icon or button at the end of a pill or rounded field is a circle of radius
//!   `size / 2` (even when its glyph is not round) centred on the centre of the container's
//!   end-cap circle: its inset from the container's edge is `outer - size / 2` on both axes.
//! - A rounded shape inside a rounded shape has the radius `outer - inset`, where the inset
//!   is the padding (plus the border) between them.
//!
//! Pure arithmetic, so the desktop and the phones compute the same numbers.

/// The corner radius of a container `height` high whose ends are half circles.
pub fn pill_radius(height: f32) -> f32 {
    height / 2.0
}

/// How far in from the container's edge, on both axes, a child of `size` (a circle of
/// radius `size / 2`) sits to be centred on the end-cap circle of a container with corner
/// radius `outer`. Zero when the child is wider than the cap (it cannot be concentric).
pub fn end_cap_inset(outer: f32, size: f32) -> f32 {
    (outer - size / 2.0).max(0.0)
}

/// The corner radius of a shape inset `inset` from a rounded container of radius `outer`:
/// never larger than the outer one, zero only when the inset reaches the outer radius.
pub fn inner_radius(outer: f32, inset: f32) -> f32 {
    (outer - inset).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pill_is_half_its_height() {
        assert_eq!(pill_radius(50.0), 25.0);
    }

    #[test]
    fn a_circle_at_the_end_of_a_pill_is_inset_by_the_radius_less_its_own() {
        // A 36 px button in a 50 px pill: 25 - 18 = 7 from the left and from the top.
        assert_eq!(end_cap_inset(25.0, 36.0), 7.0);
        // Vertically centred in the pill, the same 7 (50 - 36) / 2.
        assert_eq!(end_cap_inset(25.0, 36.0), (50.0 - 36.0) / 2.0);
        // A child wider than the cap has no concentric place: it sits at the edge.
        assert_eq!(end_cap_inset(6.0, 36.0), 0.0);
    }

    #[test]
    fn an_inner_shape_is_the_outer_radius_less_its_padding() {
        assert_eq!(inner_radius(12.0, 7.0), 5.0);
        assert_eq!(inner_radius(25.0, 7.0), 18.0);
        assert_eq!(inner_radius(12.0, 20.0), 0.0);
    }
}
